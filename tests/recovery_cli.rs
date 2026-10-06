use sha2::{Digest, Sha256};
use std::{fs, process::Command};

#[test]
fn interrupted_force_is_restored_and_skipped_without_loading_a_model() {
    let root = std::env::temp_dir().join(format!(
        "scribe-recovery-cli-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&root).unwrap();
    let source = root.join("changed-title.wav");
    let content = b"local source identity fixture";
    fs::write(&source, content).unwrap();
    // ADR 0004: size, first MiB, last MiB. Both samples cover this small file in full.
    let mut hash = Sha256::new();
    hash.update((content.len() as u64).to_le_bytes());
    hash.update(content);
    hash.update(content);
    let content_hash = format!("{:x}", hash.finalize());
    let id = format!("file:{}", &content_hash[..32]);
    let suffix = format!("{:x}", Sha256::digest(id.as_bytes()));
    let library = root.join("library");
    let dir = library.join(format!("20260101-original-title-{}", &suffix[..8]));
    fs::create_dir_all(dir.join("parts")).unwrap();
    fs::write(dir.join("index.md"), "good index").unwrap();
    fs::write(dir.join("parts/01.md"), "good part").unwrap();
    fs::write(dir.join("lessons.md"), "good lessons").unwrap();
    fs::write(dir.join("media.webm"), "good media").unwrap();
    fs::write(
        dir.join("meta.json"),
        serde_json::to_vec(&serde_json::json!({"id": id, "source": source})).unwrap(),
    )
    .unwrap();
    let backup = library.join(".scribe-backup-123-456");
    fs::write(
        backup.with_extension("target"),
        serde_json::to_vec(dir.file_name().unwrap().to_str().unwrap()).unwrap(),
    )
    .unwrap();
    fs::rename(&dir, &backup).unwrap();
    let stage = library.join(".scribe-stage-123-456");
    fs::create_dir(&stage).unwrap();
    fs::write(stage.join("index.md"), "partial replacement").unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_scribe"))
        .arg(&source)
        .arg("--out")
        .arg(&library)
        .arg("--model")
        .arg(root.join("missing-model.gguf"))
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    assert_eq!(
        String::from_utf8(output.stdout).unwrap().trim(),
        dir.join("index.md").to_str().unwrap()
    );
    assert!(String::from_utf8(output.stderr).unwrap().contains("skip:"));
    assert_eq!(
        fs::read_to_string(dir.join("index.md")).unwrap(),
        "good index"
    );
    assert_eq!(
        fs::read_to_string(dir.join("parts/01.md")).unwrap(),
        "good part"
    );
    assert_eq!(
        fs::read_to_string(dir.join("lessons.md")).unwrap(),
        "good lessons"
    );
    assert_eq!(
        fs::read_to_string(dir.join("media.webm")).unwrap(),
        "good media"
    );
    assert!(!backup.exists());
    assert!(!backup.with_extension("target").exists());
    assert!(!stage.exists());
    fs::remove_dir_all(root).unwrap();
}
