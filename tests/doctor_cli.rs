#![cfg(unix)]
use std::{fs, os::unix::fs::PermissionsExt, process::Command};

fn doctor(args: &[&str]) -> std::process::Output {
    doctor_with_broken_tool(args, None)
}

fn doctor_with_broken_tool(args: &[&str], broken_tool: Option<&str>) -> std::process::Output {
    let root = std::env::temp_dir().join(format!(
        "scribe-doctor-{}-{}-{}",
        std::process::id(),
        args.join("-").replace('/', "_"),
        broken_tool.unwrap_or("none")
    ));
    fs::create_dir_all(&root).unwrap();
    for tool in ["ffmpeg", "ffprobe", "uvx"] {
        let path = root.join(tool);
        let script = if broken_tool == Some(tool) {
            "#!/bin/sh\nexit 1\n".to_owned()
        } else {
            format!("#!/bin/sh\necho '{tool} test-version'\n")
        };
        fs::write(&path, script).unwrap();
        fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
    }
    let output = Command::new(env!("CARGO_BIN_EXE_scribe"))
        .arg("doctor")
        .args(args)
        .env("PATH", &root)
        .env("HOME", &root)
        .env("XDG_CACHE_HOME", root.join("cache"))
        .output()
        .unwrap();
    fs::remove_dir_all(root).unwrap();
    output
}

#[test]
fn working_backend_reports_machine_without_self_test() {
    let output = doctor(&["--backend", "cpu", "--no-self-test"]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report = String::from_utf8(output.stdout).unwrap();
    for expected in [
        "backend: cpu",
        "device:",
        "memory:",
        "ffmpeg test-version",
        "ffprobe test-version",
        "uvx test-version",
        "model cache:",
        "self-test: skipped (--no-self-test)",
    ] {
        assert!(report.contains(expected), "missing {expected}: {report}");
    }
}

#[test]
fn unavailable_backend_fails_without_self_test() {
    let backend = if cfg!(target_os = "macos") {
        "cuda"
    } else {
        "metal"
    };
    let output = doctor(&["--backend", backend, "--no-self-test"]);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("unavailable or broken"));
}

#[test]
fn missing_model_skips_self_test_without_downloading() {
    let output = doctor(&["--backend", "cpu"]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report = String::from_utf8_lossy(&output.stdout);
    assert!(report.contains("model not cached"), "{report}");
    assert!(
        report.contains("self-test: skipped (model not cached"),
        "{report}"
    );
}

#[test]
fn invalid_local_model_fails_self_test() {
    let output = doctor(&["--backend", "cpu", "--model", "/no-such-scribe-model.gguf"]);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("self-test model load failed"));
}

#[test]
fn broken_runtime_tool_is_not_ready() {
    let output = doctor_with_broken_tool(&["--backend", "cpu", "--no-self-test"], Some("ffprobe"));
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stdout).contains("ffprobe: missing or broken"));
    assert!(
        String::from_utf8_lossy(&output.stderr)
            .contains("required runtime tools are missing or broken")
    );
}
