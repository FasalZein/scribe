//! Deterministic validation of the flat Markdown format in reference/lessons.md.
use anyhow::{Context, Result, bail, ensure};
use std::{collections::BTreeSet, fs, path::Path};

struct Document<'a> {
    frontmatter: Vec<&'a str>,
    body: &'a str,
}
impl<'a> Document<'a> {
    fn parse(text: &'a str) -> Result<Self> {
        let mut lines = text.split_inclusive('\n');
        let first = lines.next().unwrap_or_default();
        if first.trim() != "---" {
            return Ok(Self {
                frontmatter: Vec::new(),
                body: text,
            });
        }
        let mut frontmatter = Vec::new();
        let mut offset = first.len();
        for line in lines {
            offset += line.len();
            if line.trim() == "---" {
                return Ok(Self {
                    frontmatter,
                    body: &text[offset..],
                });
            }
            frontmatter.push(line.trim_end());
        }
        bail!("frontmatter: missing closing ---")
    }
    fn field(&self, key: &str) -> Result<&str> {
        let values: Vec<_> = self
            .frontmatter
            .iter()
            .filter_map(|line| line.strip_prefix(key))
            .collect();
        ensure!(values.len() == 1, "frontmatter: expected one {key} field");
        Ok(values[0].trim())
    }
}

struct Summary {
    count: usize,
    topics: BTreeSet<String>,
}

pub fn check(path: &Path) -> Result<()> {
    let text =
        fs::read_to_string(path).with_context(|| format!("cannot read {}", path.display()))?;
    let document = Document::parse(&text)?;
    let summary = validate_body(document.body, path)?;
    validate_frontmatter(&document, &summary)
}

fn validate_frontmatter(document: &Document<'_>, summary: &Summary) -> Result<()> {
    let count = document
        .field("lessons:")?
        .parse::<usize>()
        .context("lesson count: expected a nonnegative integer")?;
    ensure!(
        count == summary.count,
        "lesson count: frontmatter says {count}, found {}",
        summary.count
    );
    let list = document.field("topics:")?;
    let list = list
        .strip_prefix('[')
        .and_then(|s| s.strip_suffix(']'))
        .context("frontmatter topics: expected [slug, ...]")?;
    let topics = if list.trim().is_empty() {
        BTreeSet::new()
    } else {
        parse_topics(list, "frontmatter topics")?
    };
    ensure!(
        topics == summary.topics,
        "frontmatter topics: list does not match lesson topics"
    );
    Ok(())
}

fn validate_body(body: &str, path: &Path) -> Result<Summary> {
    let mut summary = Summary {
        count: 0,
        topics: BTreeSet::new(),
    };
    let mut lesson = Vec::new();
    let mut fence: Option<(char, usize)> = None;
    for line in body.lines() {
        let trimmed = line.trim_start();
        let marker = trimmed.chars().next();
        if matches!(marker, Some('`' | '~')) {
            let marker = marker.unwrap();
            let length = trimmed.chars().take_while(|c| *c == marker).count();
            if let Some((open, size)) = fence {
                if marker == open && length >= size && trimmed[length..].trim().is_empty() {
                    fence = None;
                }
                continue;
            } else if length >= 3 {
                fence = Some((marker, length));
                continue;
            }
        }
        if fence.is_some() {
            continue;
        }
        if line.starts_with("### ") {
            if !lesson.is_empty() {
                validate_lesson(&lesson, path, &mut summary)?;
                lesson.clear();
            }
            let expected = format!("### L{}", summary.count + 1);
            ensure!(
                line == expected,
                "bad anchor: expected {expected}, got {line}"
            );
            lesson.push(line);
        } else if line.starts_with("## ") {
            if !lesson.is_empty() {
                validate_lesson(&lesson, path, &mut summary)?;
                lesson.clear();
            }
        } else if !lesson.is_empty() {
            lesson.push(line);
        }
    }
    ensure!(fence.is_none(), "Markdown: unclosed code fence");
    if !lesson.is_empty() {
        validate_lesson(&lesson, path, &mut summary)?;
    }
    Ok(summary)
}

fn validate_lesson(lines: &[&str], path: &Path, summary: &mut Summary) -> Result<()> {
    let id = lines[0];
    ensure!(
        lines
            .get(1)
            .is_some_and(|title| !title.trim().is_empty() && !title.starts_with(['#', '-'])),
        "{id}: missing title on the next line"
    );
    let field = |key: &str| -> Result<&str> {
        let values: Vec<_> = lines
            .iter()
            .filter_map(|line| line.strip_prefix(key))
            .collect();
        ensure!(
            values.len() == 1 && !values[0].trim().is_empty(),
            "{id}: expected one nonempty {key} field"
        );
        Ok(values[0].trim())
    };
    let kind = field("- kind:")?;
    ensure!(
        matches!(
            kind,
            "claim" | "explanation" | "procedure" | "heuristic" | "trade-off" | "example"
        ),
        "{id}: unknown kind: {kind}"
    );
    field("- who:")?;
    let at = field("- at:")?;
    validate_citation(at, path).with_context(|| format!("{id}: at link"))?;
    summary
        .topics
        .extend(parse_topics(field("- topics:")?, id)?);
    if kind == "procedure" {
        let mut steps = 0;
        for line in lines {
            if let Some((number, text)) = line.split_once(". ")
                && let Ok(number) = number.parse::<usize>()
            {
                ensure!(
                    number == steps + 1 && !text.trim().is_empty(),
                    "{id}: procedure steps must be numbered from 1 in order"
                );
                steps += 1;
            }
        }
        ensure!(steps > 0, "{id}: procedure requires numbered steps");
    }
    summary.count += 1;
    Ok(())
}

fn parse_topics(list: &str, context: &str) -> Result<BTreeSet<String>> {
    let mut topics = BTreeSet::new();
    for topic in list.split(',').map(str::trim) {
        ensure!(
            !topic.is_empty()
                && topic.split('-').all(|word| !word.is_empty()
                    && word
                        .bytes()
                        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit())),
            "{context}: invalid topic slug: {topic}"
        );
        ensure!(
            topics.insert(topic.to_owned()),
            "{context}: duplicate topic slug: {topic}"
        );
    }
    Ok(topics)
}

fn validate_citation(at: &str, lessons: &Path) -> Result<()> {
    let (timestamp, target) = at
        .strip_prefix('[')
        .and_then(|s| s.split_once("]("))
        .context("bad at link: expected [hh:mm:ss](parts/NN.md)")?;
    let target = target
        .strip_suffix(')')
        .context("bad at link: missing closing )")?;
    let time: Vec<_> = timestamp.split(':').collect();
    ensure!(
        time.len() == 3
            && time
                .iter()
                .all(|s| s.len() == 2 && s.bytes().all(|c| c.is_ascii_digit()))
            && time[1].parse::<u8>()? < 60
            && time[2].parse::<u8>()? < 60,
        "bad timestamp: expected hh:mm:ss"
    );
    let name = target
        .strip_prefix("parts/")
        .and_then(|s| s.strip_suffix(".md"))
        .context("bad at link: expected a parts Markdown file")?;
    let (number, suffix) = name
        .split_once('-')
        .map_or((name, None), |(a, b)| (a, Some(b)));
    ensure!(
        number.len() >= 2
            && number.bytes().all(|c| c.is_ascii_digit())
            && number.parse::<usize>()? > 0
            && suffix.is_none_or(|s| !s.is_empty()
                && s.split('-').all(|word| !word.is_empty()
                    && word
                        .bytes()
                        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit()))),
        "bad at link: invalid part name: {target}"
    );
    let part = lessons
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join(target);
    let text = fs::read_to_string(&part)
        .with_context(|| format!("missing part file or unreadable part: {}", part.display()))?;
    let prefix = format!("[{timestamp}] ");
    ensure!(
        text.lines().any(|line| line.starts_with(&prefix)),
        "timestamp absent from part: [{timestamp}] in {target}"
    );
    Ok(())
}

pub fn finalize(path: &Path) -> Result<()> {
    let text =
        fs::read_to_string(path).with_context(|| format!("cannot read {}", path.display()))?;
    let document = Document::parse(&text)?;
    let summary = validate_body(document.body, path)?;
    let mut result = String::from("---\n");
    for line in &document.frontmatter {
        if !line.starts_with("lessons:") && !line.starts_with("topics:") {
            result.push_str(line);
            result.push('\n');
        }
    }
    result.push_str(&format!(
        "lessons: {}\ntopics: [{}]\n---\n{}",
        summary.count,
        summary
            .topics
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>()
            .join(", "),
        document.body
    ));
    // Validate before replacing the draft. A failed finalization must leave it intact.
    let finalized = Document::parse(&result)?;
    validate_frontmatter(&finalized, &summary)?;
    let name = path
        .file_name()
        .context("lessons file has no filename")?
        .to_string_lossy();
    let temp = path.with_file_name(format!(".{name}.{}.tmp", std::process::id()));
    use std::io::Write;
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temp)?;
    let write = (|| -> Result<()> {
        file.write_all(result.as_bytes())?;
        file.sync_all()?;
        drop(file);
        fs::rename(&temp, path)?;
        Ok(())
    })();
    if write.is_err() {
        let _ = fs::remove_file(&temp);
    }
    write.with_context(|| format!("cannot finalize {}", path.display()))
}
