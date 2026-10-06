//! Process-boundary regression: loading starts during metadata, but skipped sources do not wait.
#![cfg(unix)]
use std::{
    fs,
    io::Read,
    net::TcpListener,
    os::unix::fs::PermissionsExt,
    process::{Command, Stdio},
    time::{Duration, Instant},
};

#[test]
fn loader_starts_before_metadata_finishes_and_skip_does_not_wait_for_it() {
    let directory =
        std::env::temp_dir().join(format!("scribe-early-loader-{}", std::process::id()));
    fs::create_dir_all(&directory).unwrap();
    let source = directory.join("source.mp4");
    fs::write(&source, "local source identity").unwrap();
    let root = directory.join("sources");
    let existing = root.join("existing");
    fs::create_dir_all(&existing).unwrap();
    fs::write(existing.join("index.md"), "complete transcript\n").unwrap();
    fs::write(
        existing.join("meta.json"),
        serde_json::to_vec(&serde_json::json!({"source": source})).unwrap(),
    )
    .unwrap();
    let gate = directory.join("metadata-ready");
    let probe = directory.join("ffprobe");
    fs::write(&probe, "#!/bin/sh\nwhile [ ! -f \"$SCRIBE_METADATA_GATE\" ]; do /bin/sleep 0.05; done\nprintf '10\\n'\n").unwrap();
    fs::set_permissions(&probe, fs::Permissions::from_mode(0o755)).unwrap();
    let server = TcpListener::bind("127.0.0.1:0").unwrap();
    server.set_nonblocking(true).unwrap();
    let model = format!("http://{}/early-loader.gguf", server.local_addr().unwrap());
    let mut child = Command::new(env!("CARGO_BIN_EXE_scribe"))
        .arg(&source)
        .arg("--out")
        .arg(&root)
        .arg("--model")
        .arg(&model)
        .env("PATH", &directory)
        .env("SCRIBE_METADATA_GATE", &gate)
        .env("XDG_CACHE_HOME", directory.join("cache"))
        .env_remove("HTTP_PROXY")
        .env_remove("HTTPS_PROXY")
        .env_remove("ALL_PROXY")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let start = Instant::now();
    let request = loop {
        if let Ok((mut stream, _)) = server.accept() {
            stream
                .set_read_timeout(Some(Duration::from_secs(2)))
                .unwrap();
            let mut bytes = [0; 4096];
            let length = stream.read(&mut bytes).unwrap();
            if length > 0 {
                break Some(stream);
            }
        }
        if start.elapsed() > Duration::from_secs(5) {
            break None;
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    // Keep the HTTP response pending. This models an arbitrarily slow cold load.
    fs::write(&gate, "ready").unwrap();
    let released = Instant::now();
    let exited = loop {
        if child.try_wait().unwrap().is_some() {
            break true;
        }
        if released.elapsed() > Duration::from_secs(3) {
            break false;
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    if !exited {
        child.kill().unwrap();
    }
    let output = child.wait_with_output().unwrap();
    assert!(
        request.is_some(),
        "loader did not request the model while metadata was blocked: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(exited, "skipped source waited for model download");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stderr).contains("skip:"));
    assert_eq!(
        fs::read_to_string(existing.join("index.md")).unwrap(),
        "complete transcript\n"
    );
    drop(request);
    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn model_failure_cancels_active_decode_and_removes_the_spool() {
    let directory =
        std::env::temp_dir().join(format!("scribe-cancel-decode-{}", std::process::id()));
    fs::create_dir_all(&directory).unwrap();
    let source = directory.join("source.mp4");
    fs::write(&source, "source identity").unwrap();
    let root = directory.join("sources");
    let pid_file = directory.join("decoder.pid");
    for (name, script) in [
        ("ffprobe", "#!/bin/sh\nprintf '600\\n'\n"),
        (
            "ffmpeg",
            "#!/bin/sh\nprintf '%s' \"$$\" > \"$SCRIBE_DECODER_PID\"\nexec /bin/cat /dev/zero\n",
        ),
    ] {
        let path = directory.join(name);
        fs::write(&path, script).unwrap();
        fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
    }
    let mut child = Command::new(env!("CARGO_BIN_EXE_scribe"))
        .arg(&source)
        .arg("--out")
        .arg(&root)
        .arg("--model")
        .arg(directory.join("missing.gguf"))
        .env("PATH", &directory)
        .env("SCRIBE_DECODER_PID", &pid_file)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let start = Instant::now();
    let exited = loop {
        if child.try_wait().unwrap().is_some() {
            break true;
        }
        if start.elapsed() > Duration::from_secs(15) {
            break false;
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    if !exited {
        child.kill().unwrap();
    }
    let output = child.wait_with_output().unwrap();
    let pid = fs::read_to_string(pid_file).unwrap();
    let decoder_alive = Command::new("/bin/kill")
        .args(["-0", &pid])
        .output()
        .unwrap()
        .status
        .success();
    if decoder_alive {
        Command::new("/bin/kill")
            .args(["-9", &pid])
            .status()
            .unwrap();
    }
    assert!(exited, "model failure waited for an active decoder");
    assert!(!decoder_alive, "ffmpeg remained alive after model failure");
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("model load failed"));
    for entry in fs::read_dir(root).unwrap() {
        let source_directory = entry.unwrap().path();
        if !source_directory.is_dir() {
            continue;
        }
        assert!(!source_directory.join("index.md").exists());
        assert_eq!(
            fs::read_dir(source_directory).unwrap().count(),
            0,
            "failed source retained an audio spool"
        );
    }
    fs::remove_dir_all(directory).unwrap();
}
