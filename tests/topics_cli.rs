use std::{
    fs,
    path::PathBuf,
    process::{Command, Output},
    sync::atomic::{AtomicUsize, Ordering},
};

static NEXT: AtomicUsize = AtomicUsize::new(0);
struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "scribe-topics-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(root.join("topics")).unwrap();
        Self(root)
    }
    fn source(&self, name: &str, topics: &[&str]) {
        let dir = self.0.join("sources").join(name);
        fs::create_dir_all(dir.join("parts")).unwrap();
        fs::write(dir.join("parts/01.md"), "[00:00:05] A point.\n").unwrap();
        let mut text = format!(
            "---\nlessons: {}\ntopics: [{}]\n---\n\n## Lessons\n",
            topics.len(),
            topics.join(", ")
        );
        for (i, topic) in topics.iter().enumerate() {
            text.push_str(&format!("\n### L{}\nLesson {}\n\n- kind: claim\n- who: Speaker\n- at: [00:00:05](parts/01.md)\n- topics: {topic}\n\nA point.\n", i+1, i+1));
        }
        fs::write(dir.join("lessons.md"), text).unwrap();
    }
    fn run(&self, action: &str) -> Output {
        Command::new(env!("CARGO_BIN_EXE_scribe"))
            .args(["topics", action])
            .env("SCRIBE_LIBRARY", &self.0)
            .output()
            .unwrap()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}
fn success(output: Output) -> String {
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}

#[test]
fn plan_lists_lessons_flags_new_and_single_source_and_reports_prefix_pairs_without_merging() {
    let f = Fixture::new();
    f.source("b", &["agent-trustworthiness", "shared"]);
    f.source("a", &["agent-trust", "shared"]);
    fs::write(
        f.0.join("topics/shared.md"),
        "# Shared\n\nShared knowledge.\n",
    )
    .unwrap();
    let output = success(f.run("plan"));
    assert_eq!(
        output,
        "# Topic plan\n\n## agent-trust (new note, single-source)\n\n- [L1](sources/a/lessons.md#l1): Lesson 1\n\n## agent-trustworthiness (new note, single-source)\n\n- [L1](sources/b/lessons.md#l1): Lesson 1\n\n## shared (existing note, 2 sources)\n\n- [L2](sources/a/lessons.md#l2): Lesson 2\n- [L2](sources/b/lessons.md#l2): Lesson 2\n\n## Near-duplicate slugs\n\n- agent-trust / agent-trustworthiness\n"
    );
    assert!(!f.0.join("topics/INDEX.md").exists());
    assert!(!f.0.join("topics/agent-trust.md").exists());
    assert!(
        fs::read_to_string(f.0.join("sources/b/lessons.md"))
            .unwrap()
            .contains("topics: agent-trustworthiness")
    );
}

#[test]
fn index_matches_handwritten_library_index_and_is_repeatable() {
    let f = Fixture::new();
    f.source("b", &["shared", "pending"]);
    f.source("a", &["shared", "pending", "solo"]);
    fs::write(f.0.join("topics/shared.md"), "---\ntopic: shared\nsources: 99\n---\n\n# Shared\n\nKnowledge across\ntwo sources.\n\n## Lessons\n\nContent.\n").unwrap();
    fs::write(
        f.0.join("topics/archived.md"),
        "# Archived\n\nEarlier knowledge.\n",
    )
    .unwrap();
    fs::write(f.0.join("topics/INDEX.md"), "stale index").unwrap();
    let expected = "# Topics\n\n- [archived](archived.md): Earlier knowledge. (0 sources)\n- [shared](shared.md): Knowledge across two sources. (2 sources)\n\n## Single-source\n\n- solo: [L3](../sources/a/lessons.md#l3)\n\n## Pending notes\n\n- pending (2 sources): [L2](../sources/a/lessons.md#l2), [L2](../sources/b/lessons.md#l2)\n";
    for _ in 0..2 {
        assert_eq!(
            success(f.run("index")).trim(),
            f.0.join("topics/INDEX.md").to_str().unwrap()
        );
        assert_eq!(
            fs::read_to_string(f.0.join("topics/INDEX.md")).unwrap(),
            expected
        );
    }
    assert!(
        fs::read_to_string(f.0.join("topics/shared.md"))
            .unwrap()
            .contains("sources: 99")
    );
}

#[test]
fn plan_counts_sources_not_lessons_and_ignores_code_fence_fields() {
    let f = Fixture::new();
    f.source("a", &["learning"]);
    let path = f.0.join("sources/a/lessons.md");
    let mut text = fs::read_to_string(&path)
        .unwrap()
        .replace("lessons: 1", "lessons: 2");
    text.push_str("\n### L2\nAnother lesson\n\n- kind: example\n- who: Speaker\n- at: [00:00:05](parts/01.md)\n- topics: learning\n\n```text\n### L99\n- topics: fake-topic\n```\n");
    fs::write(path, text).unwrap();
    let output = success(f.run("plan"));
    assert!(output.contains("## learning (new note, single-source)"));
    assert!(output.contains("[L1](sources/a/lessons.md#l1)"));
    assert!(output.contains("[L2](sources/a/lessons.md#l2)"));
    assert!(!output.contains("fake-topic"));
    assert!(output.ends_with("## Near-duplicate slugs\n\nNone.\n"));
}

#[test]
fn invalid_lessons_or_notes_fail_without_replacing_the_index() {
    let f = Fixture::new();
    f.source("a", &["learning"]);
    let index = f.0.join("topics/INDEX.md");
    fs::write(&index, "keep this index").unwrap();
    let lessons = f.0.join("sources/a/lessons.md");
    let valid = fs::read_to_string(&lessons).unwrap();
    fs::write(&lessons, valid.replace("### L1", "### L1. Old title")).unwrap();
    for action in ["plan", "index"] {
        let output = f.run(action);
        assert!(!output.status.success());
        let error = String::from_utf8_lossy(&output.stderr);
        assert!(
            error.contains("bad anchor") && error.contains("sources/a/lessons.md"),
            "{error}"
        );
        assert_eq!(fs::read_to_string(&index).unwrap(), "keep this index");
    }
    fs::write(lessons, valid).unwrap();
    fs::write(f.0.join("topics/learning.md"), "# Learning\n\n## Lessons\n").unwrap();
    let output = f.run("index");
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("missing topic scope paragraph"));
    assert_eq!(fs::read_to_string(index).unwrap(), "keep this index");
    assert_eq!(fs::read_dir(f.0.join("topics")).unwrap().count(), 2);
}

#[test]
fn empty_library_can_create_an_index_but_a_missing_library_fails() {
    let f = Fixture::new();
    fs::remove_dir(f.0.join("topics")).unwrap();
    assert_eq!(
        success(f.run("plan")),
        "# Topic plan\n\n## Near-duplicate slugs\n\nNone.\n"
    );
    success(f.run("index"));
    assert_eq!(
        fs::read_to_string(f.0.join("topics/INDEX.md")).unwrap(),
        "# Topics\n\n\n## Single-source\n\n"
    );
    let output = Command::new(env!("CARGO_BIN_EXE_scribe"))
        .args(["topics", "index"])
        .env("SCRIBE_LIBRARY", f.0.join("missing"))
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("library directory does not exist"));
    assert!(!f.0.join("missing").exists());
}

#[test]
fn near_duplicates_include_existing_notes_but_not_just_a_shared_word() {
    let f = Fixture::new();
    f.source("a", &["agent-trustworthiness", "agent-verification"]);
    fs::write(
        f.0.join("topics/agent-trust.md"),
        "# Trust\n\nTrust in agents.\n",
    )
    .unwrap();
    let output = success(f.run("plan"));
    assert!(output.contains("## agent-trust (existing note, 0 sources)"));
    assert!(output.ends_with("## Near-duplicate slugs\n\n- agent-trust / agent-trustworthiness\n"));
}

#[test]
fn lesson_links_encode_source_folder_delimiters() {
    let f = Fixture::new();
    f.source("source (one)#1%", &["learning"]);
    assert!(
        success(f.run("plan")).contains("[L1](sources/source%20%28one%29%231%25/lessons.md#l1)")
    );
    success(f.run("index"));
    assert!(
        fs::read_to_string(f.0.join("topics/INDEX.md"))
            .unwrap()
            .contains("[L1](../sources/source%20%28one%29%231%25/lessons.md#l1)")
    );
}
