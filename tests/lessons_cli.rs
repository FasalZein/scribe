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
        let dir = std::env::temp_dir().join(format!(
            "scribe-lessons-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(dir.join("parts")).unwrap();
        fs::write(
            dir.join("parts/01.md"),
            "# Transcript\n\n[00:00:05] A reusable point.\n",
        )
        .unwrap();
        fs::write(dir.join("lessons.md"), valid()).unwrap();
        Self(dir)
    }
    fn run(&self, action: &str) -> Output {
        Command::new(env!("CARGO_BIN_EXE_scribe"))
            .args(["lessons", action])
            .arg(self.0.join("lessons.md"))
            .env("SCRIBE_LIBRARY", self.0.join("unused-library"))
            .output()
            .unwrap()
    }
    fn replace(&self, from: &str, to: &str) {
        let path = self.0.join("lessons.md");
        let text = fs::read_to_string(&path).unwrap();
        assert!(text.contains(from));
        fs::write(path, text.replace(from, to)).unwrap();
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}
fn valid() -> &'static str {
    "---\nsource: https://example.com/source\ntitle: Example\ntopics: [learning]\nlessons: 1\n---\n\n# Lessons: Example\n\n## Lessons\n\n### L1\nKeep the condition with the claim\n\n- kind: claim\n- who: Speaker\n- at: [00:00:05](parts/01.md)\n- topics: learning\n\nThe speaker argues that conditions matter.\n"
}
fn assert_success(output: Output) {
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}
fn assert_error(output: Output, reason: &str) {
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains(reason),
        "expected {reason:?}, got {stderr:?}"
    );
}

#[test]
fn check_accepts_format_two_and_does_not_write() {
    let fixture = Fixture::new();
    assert_success(fixture.run("check"));
    assert_eq!(
        fs::read_to_string(fixture.0.join("lessons.md")).unwrap(),
        valid()
    );
    assert!(!fixture.0.join("unused-library").exists());
}

#[test]
fn check_names_count_anchor_part_timestamp_and_kind_errors() {
    for (from, to, reason) in [
        ("lessons: 1", "lessons: 2", "lesson count"),
        ("### L1", "### L2", "bad anchor"),
        ("### L1\n", "### L1. Old title\n", "bad anchor"),
        ("parts/01.md", "parts/02.md", "missing part file"),
        ("00:00:05", "00:00:06", "timestamp absent from part"),
        ("kind: claim", "kind: mechanism", "unknown kind"),
    ] {
        let fixture = Fixture::new();
        fixture.replace(from, to);
        assert_error(fixture.run("check"), reason);
    }
}

#[test]
fn check_rejects_bad_at_links_and_invalid_timestamps() {
    for (link, reason) in [
        ("[00:00:05](../parts/01.md)", "bad at link"),
        ("[00:00:05](parts/01.md#l1)", "bad at link"),
        ("00:00:05", "bad at link"),
        ("[00:60:05](parts/01.md)", "bad timestamp"),
        ("[0:00:05](parts/01.md)", "bad timestamp"),
        ("[00:00:05](parts/01-not/a-part.md)", "bad at link"),
    ] {
        let fixture = Fixture::new();
        fixture.replace("[00:00:05](parts/01.md)", link);
        assert_error(fixture.run("check"), reason);
    }
}

#[test]
fn check_requires_a_paragraph_timestamp_not_a_mention_in_prose() {
    let fixture = Fixture::new();
    fs::write(
        fixture.0.join("parts/01.md"),
        "# Transcript\n\nThe time was [00:00:05] but this is not a paragraph timestamp.\n",
    )
    .unwrap();
    assert_error(fixture.run("check"), "timestamp absent from part");
}

#[test]
fn check_validates_required_fields_titles_and_topic_union() {
    for (from, to, reason) in [
        ("Keep the condition with the claim", "", "missing title"),
        ("- who: Speaker", "- who:", "who:"),
        (
            "- topics: learning",
            "- topics: Learning",
            "invalid topic slug",
        ),
        (
            "topics: [learning]",
            "topics: [other]",
            "frontmatter topics",
        ),
        ("- kind: claim", "- kind: claim\n- kind: example", "kind:"),
        ("lessons: 1", "lessons: 1\nlessons: 1", "frontmatter"),
    ] {
        let fixture = Fixture::new();
        fixture.replace(from, to);
        assert_error(fixture.run("check"), reason);
    }
}

#[test]
fn check_accepts_all_kinds_chapter_parts_and_mermaid() {
    let fixture = Fixture::new();
    fs::write(
        fixture.0.join("parts/02-cooking-2.md"),
        "[00:10:00] Cooking advice.\n",
    )
    .unwrap();
    let mut text = valid().replace("lessons: 1", "lessons: 6");
    for (id, kind) in [
        (2, "explanation"),
        (3, "procedure"),
        (4, "heuristic"),
        (5, "trade-off"),
        (6, "example"),
    ] {
        text.push_str(&format!("\n### L{id}\nA reusable lesson\n\n- kind: {kind}\n- who: Cook\n- at: [00:10:00](parts/02-cooking-2.md)\n- topics: learning\n\n1. Add 100 g flour if the mixture is wet.\n2. Stir for 30 seconds.\n"));
    }
    text.push_str("\n```mermaid\nflowchart LR\nA --> B\n### L99\n- kind: imaginary\n```\n");
    fs::write(fixture.0.join("lessons.md"), text).unwrap();
    assert_success(fixture.run("check"));
}

#[test]
fn check_requires_numbered_procedure_steps_outside_code_fences() {
    let fixture = Fixture::new();
    fixture.replace("kind: claim", "kind: procedure");
    assert_error(fixture.run("check"), "procedure requires numbered steps");
    fixture.replace(
        "The speaker argues that conditions matter.",
        "```text\n1. Not an actual step.\n```\n",
    );
    assert_error(fixture.run("check"), "procedure requires numbered steps");
    fixture.replace("```text", "2. Add 100 g flour.\n\n```text");
    assert_error(fixture.run("check"), "procedure steps must be numbered");
}

#[test]
fn finalize_updates_appended_lessons_and_preserves_other_metadata_and_body() {
    let fixture = Fixture::new();
    let second = "\n### L2\nName the conditions\n\n- kind: heuristic\n- who: Speaker\n- at: [00:00:05](parts/01.md)\n- topics: learning, careful-reading\n\nKeep the conditions with the advice.\n";
    let original = format!("{}{second}", valid());
    fs::write(fixture.0.join("lessons.md"), &original).unwrap();
    assert_error(fixture.run("check"), "lesson count");
    assert_success(fixture.run("finalize"));
    let finalized = fs::read_to_string(fixture.0.join("lessons.md")).unwrap();
    assert!(finalized.contains("lessons: 2\ntopics: [careful-reading, learning]\n"));
    assert!(finalized.contains("source: https://example.com/source\ntitle: Example\n"));
    assert_eq!(
        finalized.split_once("\n---\n").unwrap().1,
        original.split_once("\n---\n").unwrap().1
    );
    assert_success(fixture.run("check"));
    assert_success(fixture.run("finalize"));
    assert_eq!(
        fs::read_to_string(fixture.0.join("lessons.md")).unwrap(),
        finalized
    );
}

#[test]
fn finalize_creates_missing_count_and_topics_in_draft_frontmatter() {
    let fixture = Fixture::new();
    fixture.replace("topics: [learning]\nlessons: 1\n", "");
    assert_success(fixture.run("finalize"));
    assert_success(fixture.run("check"));
    assert!(
        fs::read_to_string(fixture.0.join("lessons.md"))
            .unwrap()
            .contains("lessons: 1\ntopics: [learning]")
    );
}

#[test]
fn finalize_can_add_frontmatter_to_a_body_only_draft_and_handle_zero_lessons() {
    let fixture = Fixture::new();
    fs::write(
        fixture.0.join("lessons.md"),
        "# Lessons: No usable speech\n\n## Lessons\n",
    )
    .unwrap();
    assert_success(fixture.run("finalize"));
    assert_eq!(
        fs::read_to_string(fixture.0.join("lessons.md")).unwrap(),
        "---\nlessons: 0\ntopics: []\n---\n# Lessons: No usable speech\n\n## Lessons\n"
    );
    assert_success(fixture.run("check"));
}

#[test]
fn failed_finalize_preserves_the_original_file() {
    let fixture = Fixture::new();
    fixture.replace("kind: claim", "kind: unknown");
    let original = fs::read_to_string(fixture.0.join("lessons.md")).unwrap();
    assert_error(fixture.run("finalize"), "unknown kind");
    assert_eq!(
        fs::read_to_string(fixture.0.join("lessons.md")).unwrap(),
        original
    );
    assert_eq!(fs::read_dir(&fixture.0).unwrap().count(), 2);
}

#[test]
fn finalize_preserves_body_bytes_with_windows_line_endings() {
    let fixture = Fixture::new();
    let original = valid().replace('\n', "\r\n");
    fs::write(fixture.0.join("lessons.md"), &original).unwrap();
    assert_success(fixture.run("finalize"));
    let finalized = fs::read_to_string(fixture.0.join("lessons.md")).unwrap();
    assert_eq!(
        finalized.split_once("\n---\n").unwrap().1,
        original.split_once("\r\n---\r\n").unwrap().1
    );
    assert_success(fixture.run("check"));
}

#[test]
fn check_rejects_duplicate_anchors_and_unclosed_fences() {
    let fixture = Fixture::new();
    fixture.replace(
        "The speaker argues that conditions matter.",
        "### L1\nDuplicate anchor\n",
    );
    assert_error(fixture.run("check"), "bad anchor");
    fs::write(
        fixture.0.join("lessons.md"),
        format!("{}\n```mermaid\nflowchart LR\n", valid()),
    )
    .unwrap();
    assert_error(fixture.run("check"), "unclosed code fence");
}

#[test]
fn malformed_frontmatter_is_not_rewritten() {
    let fixture = Fixture::new();
    fixture.replace("lessons: 1\n---", "lessons: 1");
    let original = fs::read_to_string(fixture.0.join("lessons.md")).unwrap();
    assert_error(fixture.run("check"), "frontmatter: missing closing ---");
    assert_error(fixture.run("finalize"), "frontmatter: missing closing ---");
    assert_eq!(
        fs::read_to_string(fixture.0.join("lessons.md")).unwrap(),
        original
    );
}

#[test]
fn missing_lessons_file_reports_a_read_error() {
    let fixture = Fixture::new();
    fs::remove_file(fixture.0.join("lessons.md")).unwrap();
    assert_error(fixture.run("check"), "cannot read");
    assert_error(fixture.run("finalize"), "cannot read");
}

#[test]
fn check_accepts_inline_code_at_the_start_of_a_title() {
    let fixture = Fixture::new();
    fixture.replace(
        "Keep the condition with the claim",
        "`--force` preserves lessons",
    );
    assert_success(fixture.run("check"));
}

#[test]
fn frontmatter_that_is_not_yaml_is_rejected_and_not_finalized() {
    // An X title holds ": "; unquoted, YAML readers such as Obsidian reject the frontmatter (F3).
    let fixture = Fixture::new();
    fixture.replace(
        "title: Example",
        "title: Lydia Hallie (@lydiahallie): A few of you asked",
    );
    let original = fs::read_to_string(fixture.0.join("lessons.md")).unwrap();
    assert_error(fixture.run("check"), "frontmatter: not valid YAML");
    assert_error(fixture.run("finalize"), "frontmatter: not valid YAML");
    assert_eq!(
        fs::read_to_string(fixture.0.join("lessons.md")).unwrap(),
        original
    );
    // The quoted form that index.md writes is valid.
    fixture.replace(
        "title: Lydia Hallie (@lydiahallie): A few of you asked",
        "title: \"Lydia Hallie (@lydiahallie): A few of you asked\"",
    );
    assert_success(fixture.run("check"));
}

#[test]
fn metadata_fields_after_the_lesson_body_are_rejected() {
    // A verify: note written after the body is body text that a merge helper can miss (F11).
    for (field, reason) in [
        (
            "- verify: check the date",
            "L1: - verify: belongs in the metadata list",
        ),
        (
            "- kind: example",
            "L1: - kind: belongs in the metadata list",
        ),
    ] {
        let fixture = Fixture::new();
        fixture.replace(
            "The speaker argues that conditions matter.\n",
            &format!("The speaker argues that conditions matter.\n\n{field}\n"),
        );
        assert_error(fixture.run("check"), reason);
        assert_error(fixture.run("finalize"), reason);
    }
    let fixture = Fixture::new();
    fixture.replace(
        "- topics: learning\n",
        "- topics: learning\n- verify: check the date\n",
    );
    assert_success(fixture.run("check"));
}
