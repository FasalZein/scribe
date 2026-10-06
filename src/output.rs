use crate::{cli::Cli, engine::Word, fetch::Metadata};
use anyhow::{Context, Result};
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
};
use time::{OffsetDateTime, format_description::well_known::Rfc3339};

pub fn today() -> String {
    let date = OffsetDateTime::now_utc().date();
    format!(
        "{:04}{:02}{:02}",
        date.year(),
        u8::from(date.month()),
        date.day()
    )
}
fn slug(title: &str) -> String {
    let mut slug = String::new();
    for ch in title.chars().flat_map(char::to_lowercase) {
        if ch.is_alphanumeric() {
            slug.push(ch);
        } else if !slug.is_empty() && !slug.ends_with('-') {
            slug.push('-');
        }
        if slug.chars().count() >= 60 {
            break;
        }
    }
    let slug = slug.trim_end_matches('-');
    if slug.is_empty() {
        "media".to_owned()
    } else {
        slug.to_owned()
    }
}
/// The folder suffix that ties a folder name to its source ID.
fn id_suffix(id: &str) -> String {
    format!(
        "-{}",
        &crate::fetch::sha256_hex(id.as_bytes())[..ID_SUFFIX_HEX]
    )
}
/// Hex digits of the folder suffix. 32 bits keep names short; when two sources with the same
/// date and slug still share a suffix, the stored ID check fails the second instead of mixing them.
const ID_SUFFIX_HEX: usize = 8;
/// The `id` and `source` fields of a folder's meta.json, when it has one.
fn stored(dir: &Path) -> Option<serde_json::Value> {
    serde_json::from_slice(&fs::read(dir.join("meta.json")).ok()?).ok()
}

/// The folder of a source: `<date>-<title-slug>-<id-suffix>`. An existing folder with the same
/// source ID wins, because the date and title of one source can change between runs.
pub fn directory(root: &Path, meta: &Metadata) -> Result<PathBuf> {
    fs::create_dir_all(root)?;
    let suffix = id_suffix(&meta.id);
    for entry in fs::read_dir(root)? {
        let path = entry?.path();
        if path.is_dir()
            && path
                .file_name()
                .is_some_and(|name| name.to_string_lossy().ends_with(&suffix))
            && stored(&path).is_some_and(|m| m["id"] == meta.id.as_str())
        {
            return Ok(std::path::absolute(path)?);
        }
    }
    let date = meta
        .upload_date
        .as_deref()
        .filter(|d| d.len() == 8 && d.bytes().all(|b| b.is_ascii_digit()))
        .map(str::to_owned)
        .unwrap_or_else(today);
    let name = format!("{date}-{}", slug(&meta.title));
    // Legacy folders have no ID or suffix. Match the source string independently of the
    // current title: a new X title cut must not orphan the earlier transcript and lessons.
    for entry in fs::read_dir(root)? {
        let legacy = entry?.path();
        if legacy.join("index.md").is_file()
            && stored(&legacy)
                .is_some_and(|m| m["id"].is_null() && m["source"] == meta.source.as_str())
        {
            return Ok(std::path::absolute(legacy)?);
        }
    }
    let dir = root.join(format!("{name}{suffix}"));
    if let Some(other) = stored(&dir).filter(|m| m["id"] != meta.id.as_str()) {
        anyhow::bail!(
            "{} belongs to another source ({})",
            dir.display(),
            other["id"]
        );
    }
    fs::create_dir_all(&dir)?;
    // `absolute` avoids the `\\?\` prefix that `canonicalize` adds on Windows.
    Ok(std::path::absolute(dir)?)
}

/// Write the file through a temporary sibling and a rename, so a reader never sees a partial file.
fn write_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    let name = path.file_name().context("missing file name")?;
    let temp = path.with_file_name(format!(
        ".{}.{}.tmp",
        name.to_string_lossy(),
        std::process::id()
    ));
    let result = (|| {
        let mut file = fs::File::create(&temp)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        fs::rename(&temp, path)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temp);
    }
    result.with_context(|| format!("cannot write {}", path.display()))
}

/// Remove the files of an earlier transcript before a new one is published: first the
/// completion marker, then parts and kept media. Agent-written files such as lessons.md stay.
pub fn clear(dir: &Path) -> Result<()> {
    let index = dir.join("index.md");
    if index.exists() {
        fs::remove_file(&index)?;
    }
    let parts_dir = dir.join("parts");
    if parts_dir.exists() {
        fs::remove_dir_all(&parts_dir)?;
    }
    for entry in fs::read_dir(dir)? {
        let path = entry?.path();
        let name = path.file_name().unwrap_or_default().to_string_lossy();
        if path.is_file() && (name == "audio.f32le" || name.starts_with("media.")) {
            fs::remove_file(&path)?;
        } else if name.starts_with('.') && name.ends_with(".tmp") {
            // Left by an interrupted write.
            if path.is_dir() {
                fs::remove_dir_all(&path)?;
            } else {
                fs::remove_file(&path)?;
            }
        }
    }
    Ok(())
}

/// Write the transcript files of a cleared folder. Every file is written atomically, and
/// index.md last: it is the completion marker that the skip check trusts.
pub fn write(
    dir: &Path,
    meta: &Metadata,
    cli: &Cli,
    duration: f64,
    timings: &crate::timings::Timings,
    words: &[Word],
    hard_cuts: usize,
) -> Result<PathBuf> {
    let index = dir.join("index.md");
    // Build parts in a temporary folder and rename it into place.
    let parts_temp = dir.join(format!(".parts.{}.tmp", std::process::id()));
    if parts_temp.exists() {
        fs::remove_dir_all(&parts_temp)?;
    }
    fs::create_dir(&parts_temp)?;
    let segments = crate::parts::segments(words);
    let parts = crate::parts::split(&segments, &meta.chapters);
    let mut frontmatter = serde_json::json!({
        "title": meta.title, "source": meta.source, "source_id": meta.id, "uploader": meta.uploader,
        "upload_date": meta.upload_date, "duration_secs": duration, "model": cli.model,
        "transcribed_at": OffsetDateTime::now_utc().format(&Rfc3339)?,
        "chunk_secs": cli.chunk_secs.get(), "engine_secs": timings.get("engine"), "hard_cuts": hard_cuts,
    });
    if let Some(language) = &cli.language {
        frontmatter["language"] = language.clone().into();
    }
    let mut markdown = Vec::new();
    writeln!(markdown, "---")?;
    for (key, value) in frontmatter
        .as_object()
        .context("frontmatter is not an object")?
    {
        // JSON scalars are also YAML scalars. Quote strings to preserve punctuation and newlines.
        writeln!(markdown, "{key}: {value}")?;
    }
    writeln!(
        markdown,
        "---\n\n# {}\n",
        meta.title.replace(['\r', '\n'], " ")
    )?;
    write!(
        markdown,
        "{}",
        crate::sources::render(&meta.sources, &meta.chapters)
    )?;
    let mut jsonl = Vec::new();
    for segment in &segments {
        writeln!(
            markdown,
            "[{}] {}\n",
            crate::sources::timestamp(segment.start),
            segment.text.replace(['\r', '\n'], " ")
        )?;
        serde_json::to_writer(&mut jsonl, segment)?;
        writeln!(jsonl)?;
    }
    write_atomic(&dir.join("transcript.md"), &markdown)?;
    write_atomic(&dir.join("segments.jsonl"), &jsonl)?;
    write_atomic(&dir.join("meta.json"), &serde_json::to_vec_pretty(meta)?)?;
    frontmatter["parts"] = parts.len().into();
    let words: usize = parts.iter().map(crate::parts::Part::words).sum();
    frontmatter["words"] = words.into();
    frontmatter["tokens_estimate"] = crate::parts::tokens_estimate(words).into();
    let mut entry = format!("# {}\n\n", meta.title.replace(['\r', '\n'], " "));
    entry.push_str(&crate::sources::render(&meta.sources, &meta.chapters));
    entry.push_str("## Parts\n\n| part | time range | words | tokens_estimate | first words |\n| --- | --- | ---: | ---: | --- |\n");
    for (i, part) in parts.iter().enumerate() {
        // A Part always holds at least one segment, as constructed by split.
        let first = part.segments.first().context("empty part")?;
        let last = part.segments.last().context("empty part")?;
        let time_range = format!(
            "[{}–{}]",
            crate::sources::timestamp(first.start),
            crate::sources::timestamp(last.end)
        );
        let preview = part
            .segments
            .iter()
            .flat_map(|s| s.text.split_whitespace())
            .take(12)
            .collect::<Vec<_>>()
            .join(" ");
        let filename = part_filename(i + 1, part);
        let mut body = format!(
            "# {}\n\nPart {} of {} · {}\n\n",
            meta.title.replace(['\r', '\n'], " "),
            i + 1,
            parts.len(),
            time_range
        );
        if let Some(chapter) = part.chapter {
            body.push_str(&format!(
                "Chapter: {}\n\n",
                chapter.title.replace(['\r', '\n'], " ")
            ));
        }
        for segment in part.segments {
            body.push_str(&format!(
                "[{}] {}\n\n",
                crate::sources::timestamp(segment.start),
                segment.text.replace(['\r', '\n'], " ")
            ));
        }
        fs::write(parts_temp.join(&filename), body)?;
        entry.push_str(&format!(
            "| [{:02}](parts/{filename}) | {time_range} | {} | {} | {} |\n",
            i + 1,
            part.words(),
            crate::parts::tokens_estimate(part.words()),
            preview.replace('|', "\\|")
        ));
    }
    fs::rename(&parts_temp, dir.join("parts"))?;
    frontmatter["fetch_secs"] = (timings.get("metadata") + timings.get("download")).into();
    frontmatter["decode_secs"] = timings.get("decode").into();
    // A model reused from an earlier source has no load time for this source.
    frontmatter["model_load_secs"] = timings.get("model").into();
    // Includes publication up to the completion marker; excludes its final atomic write.
    // Stages overlap (ADR 0005), so total is elapsed wall time, not their sum.
    frontmatter["total_secs"] = timings.elapsed().into();
    let mut header = String::from("---\n");
    for (key, value) in frontmatter
        .as_object()
        .context("frontmatter is not an object")?
    {
        header.push_str(&format!("{key}: {value}\n"));
    }
    header.push_str("---\n\n");
    header.push_str(&entry);
    // Write the index last so an incomplete run cannot be mistaken for a completed transcript.
    write_atomic(&index, header.as_bytes())?;
    Ok(index)
}

/// `NN.md`, or `NN-<chapter-slug>[-<piece>].md`. The name never depends on transcript words,
/// so links from lessons survive a `--force` run.
fn part_filename(number: usize, part: &crate::parts::Part) -> String {
    let chapter = part
        .chapter
        .map(|c| format!("-{}", slug(&c.title)))
        .unwrap_or_default();
    let piece = part.piece.map(|n| format!("-{n}")).unwrap_or_default();
    format!("{number:02}{chapter}{piece}.md")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sources::{Chapter, Sources};
    use clap::Parser;

    fn temp_root(name: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!("scribe-test-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        root
    }
    fn meta(id: &str, title: &str, date: Option<&str>) -> Metadata {
        Metadata {
            id: id.into(),
            title: title.into(),
            source: format!("https://example.com/{id}"),
            uploader: None,
            upload_date: date.map(Into::into),
            duration: Some(60.0),
            file_size: None,
            sources: Sources::Local {
                path: "/media.mp4".into(),
            },
            chapters: Vec::new(),
        }
    }
    /// Words spoken one per `step` seconds from `start`.
    fn timed(text: &str, start: f64, step: f64) -> Vec<Word> {
        text.split_whitespace()
            .enumerate()
            .map(|(i, text)| Word {
                start: start + i as f64 * step,
                end: start + (i + 1) as f64 * step,
                text: text.to_owned(),
            })
            .collect()
    }
    /// Three 20 s sentences "Say <word> <i>.", at 0, 20 and 40 s.
    fn words(word: &str) -> Vec<Word> {
        (0..3)
            .flat_map(|i| timed(&format!("Say {word} {i}."), i as f64 * 20.0, 20.0 / 3.0))
            .collect()
    }
    fn recorded_timings() -> crate::timings::Timings {
        let mut timings = crate::timings::Timings::new(false);
        timings.add("metadata", 2.0);
        timings.add("download", 3.0);
        timings.add("decode", 4.0);
        timings.add("model", 6.0);
        timings.add("engine", 1.0);
        timings
    }
    fn publish(dir: &Path, meta: &Metadata, word: &str) -> PathBuf {
        let cli = Cli::parse_from(["scribe", "x"]);
        clear(dir).unwrap();
        write(dir, meta, &cli, 60.0, &recorded_timings(), &words(word), 0).unwrap()
    }
    fn names(dir: &Path) -> Vec<String> {
        let mut names: Vec<_> = fs::read_dir(dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    }

    #[test]
    fn index_always_records_all_five_stage_times() {
        for enabled in [false, true] {
            let root = temp_root(if enabled { "times-on" } else { "times-off" });
            let m = meta("file:times", "Stage times", None);
            let dir = directory(&root, &m).unwrap();
            let cli = if enabled {
                Cli::parse_from(["scribe", "x", "--timings"])
            } else {
                Cli::parse_from(["scribe", "x"])
            };
            let timings = recorded_timings();
            let before = timings.elapsed();
            write(&dir, &m, &cli, 60.0, &timings, &words("hello"), 0).unwrap();
            let after = timings.elapsed();
            let index = fs::read_to_string(dir.join("index.md")).unwrap();
            let frontmatter = index.split("---").nth(1).unwrap();
            for key in [
                "fetch_secs",
                "decode_secs",
                "model_load_secs",
                "engine_secs",
                "total_secs",
            ] {
                assert!(
                    frontmatter
                        .lines()
                        .any(|line| line.starts_with(&format!("{key}: "))),
                    "missing {key}: {index}"
                );
            }
            for line in [
                "fetch_secs: 5.0",
                "decode_secs: 4.0",
                "model_load_secs: 6.0",
                "engine_secs: 1.0",
            ] {
                assert!(frontmatter.lines().any(|actual| actual == line), "{index}");
            }
            let total: f64 = frontmatter
                .lines()
                .find_map(|line| line.strip_prefix("total_secs: "))
                .unwrap()
                .parse()
                .unwrap();
            assert!(
                total.is_finite() && total >= before && total <= after,
                "total must be elapsed time, not the sum: {total}"
            );
            fs::remove_dir_all(root).unwrap();
        }
    }

    #[test]
    fn reused_model_records_zero_load_time() {
        let root = temp_root("times-reused");
        let m = meta("file:reused", "Reused model", None);
        let dir = directory(&root, &m).unwrap();
        let cli = Cli::parse_from(["scribe", "x"]);
        let mut timings = crate::timings::Timings::new(false);
        timings.add("engine", 1.0);
        write(&dir, &m, &cli, 60.0, &timings, &words("hello"), 0).unwrap();
        let index = fs::read_to_string(dir.join("index.md")).unwrap();
        assert!(
            index.lines().any(|line| line == "model_load_secs: 0.0"),
            "{index}"
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn same_source_id_reuses_its_folder_across_dates_and_titles() {
        let root = temp_root("same-id");
        let first = directory(&root, &meta("file:abc", "talk", None)).unwrap();
        publish(&first, &meta("file:abc", "talk", None), "hello");
        // Another day (a local file is dated today) or another title (X API against yt-dlp).
        let later = meta("file:abc", "other title", Some("20300101"));
        assert_eq!(directory(&root, &later).unwrap(), first);
        assert!(
            first.join("index.md").is_file(),
            "the skip check finds the index"
        );
        let stored = stored(&first).unwrap();
        assert_eq!(stored["id"], "file:abc");
        let index = fs::read_to_string(first.join("index.md")).unwrap();
        assert!(index.contains("source_id: \"file:abc\""));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn same_slug_and_date_with_another_id_gets_its_own_folder() {
        let root = temp_root("collision");
        let a = meta("youtube:a", "Video", Some("20260101"));
        let b = meta("youtube:b", "Video", Some("20260101"));
        let dir_a = directory(&root, &a).unwrap();
        publish(&dir_a, &a, "first");
        let dir_b = directory(&root, &b).unwrap();
        assert_ne!(dir_a, dir_b);
        assert!(!dir_b.join("index.md").exists(), "b must not skip");
        let name = dir_b.file_name().unwrap().to_string_lossy().into_owned();
        assert!(name.starts_with("20260101-video-") && name.len() == "20260101-video-".len() + 8);
        // A folder that holds another source's meta.json is never reused.
        fs::copy(dir_a.join("meta.json"), dir_b.join("meta.json")).unwrap();
        assert!(directory(&root, &b).is_err());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn legacy_folder_without_id_is_reused_for_the_same_source() {
        let root = temp_root("legacy");
        let m = meta("youtube:a", "Video", Some("20260101"));
        let legacy = root.join("20260101-video");
        fs::create_dir_all(&legacy).unwrap();
        fs::write(legacy.join("index.md"), "done").unwrap();
        fs::write(
            legacy.join("meta.json"),
            r#"{"source":"https://example.com/youtube:a"}"#,
        )
        .unwrap();
        assert_eq!(
            directory(&root, &m).unwrap(),
            std::path::absolute(&legacy).unwrap()
        );
        let renamed = meta("youtube:a", "Whole word title", Some("20261006"));
        assert_eq!(
            directory(&root, &renamed).unwrap(),
            std::path::absolute(&legacy).unwrap()
        );
        let other = meta("youtube:b", "Video", Some("20260101"));
        assert_ne!(
            directory(&root, &other).unwrap(),
            std::path::absolute(&legacy).unwrap()
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn force_keeps_lessons_and_removes_old_media_and_parts() {
        let root = temp_root("force");
        let m = meta("youtube:a", "Video", Some("20260101"));
        let dir = directory(&root, &m).unwrap();
        publish(&dir, &m, "old");
        fs::write(dir.join("lessons.md"), "lesson").unwrap();
        fs::write(dir.join("audio.f32le"), "pcm").unwrap();
        fs::write(dir.join("media.webm"), "media").unwrap();
        publish(&dir, &m, "new");
        assert_eq!(
            fs::read_to_string(dir.join("lessons.md")).unwrap(),
            "lesson"
        );
        assert!(!dir.join("audio.f32le").exists() && !dir.join("media.webm").exists());
        let transcript = fs::read_to_string(dir.join("transcript.md")).unwrap();
        assert!(transcript.contains("new 0") && !transcript.contains("old 0"));
        assert_eq!(
            names(&dir),
            [
                "index.md",
                "lessons.md",
                "meta.json",
                "parts",
                "segments.jsonl",
                "transcript.md"
            ]
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn a_folder_without_index_is_redone_and_interrupted_writes_are_cleaned() {
        let root = temp_root("atomic");
        let m = meta("youtube:a", "Video", Some("20260101"));
        let dir = directory(&root, &m).unwrap();
        publish(&dir, &m, "old");
        // An interruption before the index rename: every file but the completion marker.
        fs::remove_file(dir.join("index.md")).unwrap();
        fs::write(dir.join(".index.md.1.tmp"), "half an ind").unwrap();
        fs::create_dir(dir.join(".parts.1.tmp")).unwrap();
        assert_eq!(directory(&root, &m).unwrap(), dir);
        assert!(
            !dir.join("index.md").exists(),
            "no index, so the source is redone"
        );
        publish(&dir, &m, "new");
        assert!(names(&dir).iter().all(|n| !n.ends_with(".tmp")));
        assert!(
            fs::read_to_string(dir.join("index.md"))
                .unwrap()
                .contains("new 0")
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn part_names_do_not_depend_on_transcript_words() {
        let root = temp_root("parts");
        let mut m = meta("youtube:a", "Video", Some("20260101"));
        let dir = directory(&root, &m).unwrap();
        publish(&dir, &m, "first");
        assert_eq!(names(&dir.join("parts")), ["01.md"]);
        publish(&dir, &m, "second");
        assert_eq!(names(&dir.join("parts")), ["01.md"]);
        m.chapters = vec![
            Chapter {
                start_time: 0.0,
                title: "Intro".into(),
            },
            Chapter {
                start_time: 20.0,
                title: "Main Part".into(),
            },
        ];
        publish(&dir, &m, "third");
        assert_eq!(
            names(&dir.join("parts")),
            ["01-intro.md", "02-main-part.md"]
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn timestamps_mark_the_first_word_of_their_text() {
        let root = temp_root("word-times");
        let m = meta("youtube:a", "Video", Some("20260101"));
        let dir = directory(&root, &m).unwrap();
        // A 30 s chunk cut falls at 30 s, inside the second sentence. The first sentence starts
        // after 2 s of silence and ends at 24.5 s; the second starts at 25.7 s.
        let first = "First we measure the build on a quiet machine and we write the number down.";
        let second = "Then we change one flag and we measure the same build again on that machine.";
        let mut words = timed(first, 2.0, 1.5);
        words.extend(timed(second, 25.7, 1.5));
        let cli = Cli::parse_from(["scribe", "x"]);
        write(&dir, &m, &cli, 60.0, &recorded_timings(), &words, 2).unwrap();
        let expected = format!("[00:00:02] {first}\n\n[00:00:25] {second}\n\n");
        let transcript = fs::read_to_string(dir.join("transcript.md")).unwrap();
        assert!(transcript.ends_with(&expected), "{transcript}");
        let part = fs::read_to_string(dir.join("parts/01.md")).unwrap();
        assert!(part.ends_with(&expected), "{part}");
        let starts: Vec<f64> = fs::read_to_string(dir.join("segments.jsonl"))
            .unwrap()
            .lines()
            .map(|line| {
                serde_json::from_str::<serde_json::Value>(line).unwrap()["start"]
                    .as_f64()
                    .unwrap()
            })
            .collect();
        assert_eq!(starts, [2.0, 25.7]);
        let index = fs::read_to_string(dir.join("index.md")).unwrap();
        assert!(index.lines().any(|line| line == "hard_cuts: 2"), "{index}");
        fs::remove_dir_all(root).unwrap();
    }
}
