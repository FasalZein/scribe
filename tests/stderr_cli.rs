use std::{fs, process::Command};

#[test]
fn source_failure_keeps_the_skill_error_line() {
    let source = std::env::temp_dir().join(format!("scribe-missing-{}.mp4", std::process::id()));
    assert!(!source.exists());
    let output = Command::new(env!("CARGO_BIN_EXE_scribe"))
        .arg(&source)
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(!output.stderr.contains(&b'\r'));
    assert!(!output.stderr.contains(&0x1b));
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert_eq!(stderr.lines().count(), 1, "{stderr}");
    assert!(
        stderr.starts_with(&format!("scribe: {}: ", source.display())),
        "{stderr}"
    );
    assert!(stderr.contains("cannot open local media"), "{stderr}");
}

#[test]
#[ignore = "needs SCRIBE_REGRESSION_MEDIA and the cached default model; checks real engine stderr"]
fn local_source_is_quiet_by_default_and_verbose_restores_engine_logs() {
    let media = std::env::var("SCRIBE_REGRESSION_MEDIA").expect("set SCRIBE_REGRESSION_MEDIA");
    let root = std::env::temp_dir().join(format!("scribe-stderr-{}", std::process::id()));
    fs::create_dir_all(&root).unwrap();
    let fixture = root.join("fixture.wav");
    let generated = Command::new("ffmpeg")
        .args([
            "-nostdin", "-v", "error", "-y", "-i", &media, "-t", "5", "-ac", "1", "-ar", "16000",
        ])
        .arg(&fixture)
        .output()
        .unwrap();
    assert!(
        generated.status.success(),
        "{}",
        String::from_utf8_lossy(&generated.stderr)
    );
    let model = std::env::var("SCRIBE_REGRESSION_MODEL").ok();
    for (name, flags) in [
        ("quiet", vec![]),
        ("verbose", vec!["--verbose", "--timings"]),
    ] {
        let mut command = Command::new(env!("CARGO_BIN_EXE_scribe"));
        command
            .arg(&fixture)
            .arg("--out")
            .arg(root.join(name))
            .args(flags);
        if let Some(model) = &model {
            command.args(["--model", model]);
        }
        let output = command.output().unwrap();
        let stderr = String::from_utf8(output.stderr).unwrap();
        assert!(output.status.success(), "{stderr}");
        assert!(!stderr.contains('\r'), "{stderr}");
        assert!(!stderr.contains('\x1b'), "{stderr}");
        let engine_lines = stderr.lines().any(|line| {
            line.contains("ggml_") || line.contains("parakeet:") || line.contains("decoder:")
        });
        assert_eq!(engine_lines, name == "verbose", "{stderr}");
        assert!(stderr.contains("Model loaded; backend:"), "{stderr}");
        assert!(stderr.contains("Transcribed 5.0s of audio in"), "{stderr}");
        assert_eq!(
            stderr
                .lines()
                .filter(|line| line.starts_with("timings:"))
                .count(),
            usize::from(name == "verbose"),
            "{stderr}"
        );
        let index_path = String::from_utf8(output.stdout).unwrap();
        let index = fs::read_to_string(index_path.trim()).unwrap();
        let frontmatter = index.split("---").nth(1).unwrap();
        for key in [
            "fetch_secs",
            "decode_secs",
            "model_load_secs",
            "engine_secs",
            "total_secs",
        ] {
            let value = frontmatter
                .lines()
                .find_map(|line| line.strip_prefix(&format!("{key}: ")))
                .unwrap_or_else(|| panic!("missing {key}: {index}"));
            let seconds: f64 = value.parse().unwrap();
            assert!(seconds.is_finite() && seconds >= 0.0, "{key}: {value}");
        }
    }
    fs::remove_dir_all(root).unwrap();
}
