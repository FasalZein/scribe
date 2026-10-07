//! Deterministic topic planning. The agent, not this module, writes topic notes.
use crate::lessons;
use anyhow::{Context, Result, ensure};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
};

struct Citation {
    source: String,
    lesson: lessons::Lesson,
}
#[derive(Default)]
struct Topic {
    citations: Vec<Citation>,
    note: Option<PathBuf>,
}
impl Topic {
    fn sources(&self) -> BTreeSet<&str> {
        self.citations.iter().map(|c| c.source.as_str()).collect()
    }
}

fn entries(path: &Path) -> Result<Vec<PathBuf>> {
    if !path.exists() {
        return Ok(Vec::new());
    }
    let mut entries = fs::read_dir(path)
        .with_context(|| format!("cannot read {}", path.display()))?
        .map(|entry| entry.map(|entry| entry.path()))
        .collect::<std::io::Result<Vec<_>>>()?;
    entries.sort();
    Ok(entries)
}

fn collect(library: &Path) -> Result<BTreeMap<String, Topic>> {
    ensure!(
        library.is_dir(),
        "library directory does not exist: {}",
        library.display()
    );
    let mut topics: BTreeMap<String, Topic> = BTreeMap::new();
    for source in entries(&library.join("sources"))? {
        if !source.is_dir() {
            continue;
        }
        let path = source.join("lessons.md");
        if !path.exists() {
            continue;
        }
        let name = source
            .file_name()
            .and_then(|s| s.to_str())
            .context("source folder name is not UTF-8")?;
        for lesson in lessons::read(&path).with_context(|| format!("{}", path.display()))? {
            for slug in &lesson.topics {
                topics
                    .entry(slug.clone())
                    .or_default()
                    .citations
                    .push(Citation {
                        source: name.to_owned(),
                        lesson: lessons::Lesson {
                            id: lesson.id,
                            title: lesson.title.clone(),
                            topics: lesson.topics.clone(),
                        },
                    });
            }
        }
    }
    for note in entries(&library.join("topics"))? {
        if !note.is_file() || note.extension().is_none_or(|ext| ext != "md") {
            continue;
        }
        let slug = note
            .file_stem()
            .and_then(|s| s.to_str())
            .context("topic filename is not UTF-8")?
            .to_owned();
        if slug == "INDEX" {
            continue;
        }
        ensure!(
            !slug.is_empty()
                && slug.split('-').all(|word| !word.is_empty()
                    && word
                        .bytes()
                        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit())),
            "invalid topic slug: {slug}"
        );
        topics.entry(slug).or_default().note = Some(note);
    }
    Ok(topics)
}

// Library folder names can contain spaces or Markdown link delimiters.
fn link_path(path: &str) -> String {
    path.replace('%', "%25")
        .replace(' ', "%20")
        .replace('#', "%23")
        .replace('(', "%28")
        .replace(')', "%29")
}

pub fn plan(library: &Path) -> Result<String> {
    let topics = collect(library)?;
    let mut output = String::from("# Topic plan\n");
    for (slug, topic) in &topics {
        let status = if topic.note.is_some() {
            "existing note"
        } else {
            "new note"
        };
        let count = topic.sources().len();
        let sources = if count == 1 {
            "single-source".to_owned()
        } else {
            format!("{count} sources")
        };
        output.push_str(&format!("\n## {slug} ({status}, {sources})\n\n"));
        for citation in &topic.citations {
            let path = link_path(&format!("sources/{}/lessons.md", citation.source));
            output.push_str(&format!(
                "- [L{}]({path}#l{}): {}\n",
                citation.lesson.id, citation.lesson.id, citation.lesson.title
            ));
        }
    }
    output.push_str("\n## Near-duplicate slugs\n\n");
    let slugs: Vec<_> = topics.keys().collect();
    let mut found = false;
    for (i, a) in slugs.iter().enumerate() {
        for b in &slugs[i + 1..] {
            // A complete slug prefix catches trust/trustworthiness without a guessed distance cutoff.
            if b.starts_with(a.as_str()) {
                output.push_str(&format!("- {a} / {b}\n"));
                found = true;
            }
        }
    }
    if !found {
        output.push_str("None.\n");
    }
    Ok(output)
}

fn scope(note: &Path) -> Result<String> {
    let text =
        fs::read_to_string(note).with_context(|| format!("cannot read {}", note.display()))?;
    let document = lessons::Document::parse(&text)?;
    let mut lines = document
        .body
        .lines()
        .skip_while(|line| !line.starts_with("# "));
    ensure!(
        lines.next().is_some(),
        "{}: missing topic title",
        note.display()
    );
    let paragraph = lines
        .skip_while(|line| line.trim().is_empty())
        .take_while(|line| !line.trim().is_empty() && !line.starts_with('#'))
        .map(str::trim)
        .collect::<Vec<_>>()
        .join(" ");
    ensure!(
        !paragraph.is_empty(),
        "{}: missing topic scope paragraph",
        note.display()
    );
    Ok(paragraph)
}

fn citation_links(topic: &Topic) -> String {
    topic
        .citations
        .iter()
        .map(|citation| {
            let path = link_path(&format!("../sources/{}/lessons.md", citation.source));
            format!("[L{}]({path}#l{})", citation.lesson.id, citation.lesson.id)
        })
        .collect::<Vec<_>>()
        .join(", ")
}

/// Undo `link_path` and any other percent-encoding in a relative link path.
fn percent_decode(path: &str) -> String {
    let bytes = path.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && let Some(byte) = path
                .get(i + 1..i + 3)
                .and_then(|hex| u8::from_str_radix(hex, 16).ok())
        {
            decoded.push(byte);
            i += 3;
        } else {
            decoded.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8_lossy(&decoded).into_owned()
}

/// The `[text](target)` links on one line.
fn line_links(line: &str) -> Vec<(&str, &str)> {
    let mut links = Vec::new();
    let mut from = 0;
    while let Some(offset) = line[from..].find("](") {
        let close = from + offset;
        let start = close + 2;
        let Some(length) = line[start..].find(')') else {
            break;
        };
        if let Some(open) = line[..close].rfind('[') {
            links.push((&line[open + 1..close], &line[start..start + length]));
        }
        from = start + length + 1;
    }
    links
}

/// Why a relative link in a topic note does not resolve, or None when it does. A `#l<n>`
/// fragment needs a `### L<n>` heading, and link text `hh:mm:ss` needs a paragraph that starts
/// with `[hh:mm:ss]`, as in a lesson's `at:` link. Other fragments are not checked: GitHub and
/// Obsidian compute heading anchors differently.
fn broken(dir: &Path, text: &str, path: &str, fragment: Option<&str>) -> Option<String> {
    let file = dir.join(percent_decode(path));
    if !file.is_file() {
        return Some("missing file".to_owned());
    }
    let lesson = fragment
        .and_then(|f| f.strip_prefix('l'))
        .filter(|n| !n.is_empty() && n.bytes().all(|c| c.is_ascii_digit()));
    let time = text.len() == 8
        && text.bytes().enumerate().all(|(i, c)| {
            if i % 3 == 2 {
                c == b':'
            } else {
                c.is_ascii_digit()
            }
        });
    if lesson.is_none() && !time {
        return None;
    }
    let Ok(content) = fs::read_to_string(&file) else {
        return Some("unreadable file".to_owned());
    };
    if let Some(n) = lesson {
        let heading = format!("### L{n}");
        if !content.lines().any(|line| line.trim_end() == heading) {
            return Some(format!("no {heading} heading"));
        }
    }
    let prefix = format!("[{text}] ");
    if time && !content.lines().any(|line| line.starts_with(&prefix)) {
        return Some(format!("no paragraph starts with [{text}]"));
    }
    None
}

/// Every relative link in the note that does not resolve, one message each. Links inside code
/// fences and links with a URL scheme are skipped.
fn broken_links(note: &Path) -> Result<Vec<String>> {
    let text =
        fs::read_to_string(note).with_context(|| format!("cannot read {}", note.display()))?;
    let dir = note.parent().unwrap_or_else(|| Path::new("."));
    let mut errors = Vec::new();
    let mut fence: Option<&str> = None;
    for line in text.lines() {
        let trimmed = line.trim_start();
        let marker = ["```", "~~~"].into_iter().find(|m| trimmed.starts_with(m));
        if let Some(marker) = marker {
            match fence {
                None => fence = Some(marker),
                Some(open) if open == marker => fence = None,
                Some(_) => {}
            }
            continue;
        }
        if fence.is_some() {
            continue;
        }
        for (label, target) in line_links(line) {
            let target = target.trim();
            let target = target
                .strip_prefix('<')
                .and_then(|t| t.split_once('>'))
                .map_or_else(|| target.split(' ').next().unwrap_or(""), |(t, _)| t);
            if target.is_empty() || target.starts_with('#') || target.contains(':') {
                continue;
            }
            let (path, fragment) = target
                .split_once('#')
                .map_or((target, None), |(p, f)| (p, Some(f)));
            if let Some(reason) = broken(dir, label, path, fragment) {
                errors.push(format!(
                    "{}: broken link {target}: {reason}",
                    note.display()
                ));
            }
        }
    }
    Ok(errors)
}

pub fn index(library: &Path) -> Result<PathBuf> {
    let topics = collect(library)?;
    let mut errors = Vec::new();
    for note in topics.values().filter_map(|topic| topic.note.as_ref()) {
        errors.extend(broken_links(note)?);
    }
    ensure!(errors.is_empty(), "{}", errors.join("\n"));
    let mut output = String::from("# Topics\n\n");
    for (slug, topic) in &topics {
        if let Some(note) = &topic.note {
            output.push_str(&format!(
                "- [{slug}]({slug}.md): {} ({} sources)\n",
                scope(note)?,
                topic.sources().len()
            ));
        }
    }
    output.push_str("\n## Single-source\n\n");
    for (slug, topic) in &topics {
        if topic.note.is_none() && topic.sources().len() == 1 {
            output.push_str(&format!("- {slug}: {}\n", citation_links(topic)));
        }
    }
    let pending: Vec<_> = topics
        .iter()
        .filter(|(_, topic)| topic.note.is_none() && topic.sources().len() > 1)
        .collect();
    if !pending.is_empty() {
        output.push_str("\n## Pending notes\n\n");
        for (slug, topic) in pending {
            output.push_str(&format!(
                "- {slug} ({} sources): {}\n",
                topic.sources().len(),
                citation_links(topic)
            ));
        }
    }
    let dir = library.join("topics");
    fs::create_dir_all(&dir)?;
    let path = dir.join("INDEX.md");
    let temp = dir.join(format!(".INDEX.md.{}.tmp", std::process::id()));
    use std::io::Write;
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temp)?;
    let write = (|| -> Result<()> {
        file.write_all(output.as_bytes())?;
        file.sync_all()?;
        drop(file);
        fs::rename(&temp, &path)?;
        Ok(())
    })();
    if write.is_err() {
        let _ = fs::remove_file(&temp);
    }
    write.with_context(|| format!("cannot write {}", path.display()))?;
    Ok(path)
}
