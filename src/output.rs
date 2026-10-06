use crate::{cli::Cli, engine::Segment, fetch::Metadata};
use anyhow::{Context, Result};
use std::{
    fs,
    io::{BufWriter, Write},
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
pub fn directory(root: &Path, meta: &Metadata) -> Result<PathBuf> {
    fs::create_dir_all(root)?;
    let date = meta
        .upload_date
        .as_deref()
        .filter(|d| d.len() == 8 && d.bytes().all(|b| b.is_ascii_digit()))
        .map(str::to_owned)
        .unwrap_or_else(today);
    let dir = root.join(format!("{date}-{}", slug(&meta.title)));
    fs::create_dir_all(&dir)?;
    fs::canonicalize(dir).context("cannot resolve output directory")
}

pub fn write(
    dir: &Path,
    meta: &Metadata,
    cli: &Cli,
    duration: f64,
    engine_secs: f64,
    segments: &[Segment],
) -> Result<PathBuf> {
    // Remove the completion marker before replacing transcript files. Keep agent-written lessons.
    let index = dir.join("index.md");
    if index.exists() {
        fs::remove_file(&index)?;
    }
    let parts_dir = dir.join("parts");
    if parts_dir.exists() {
        fs::remove_dir_all(&parts_dir)?;
    }
    fs::create_dir(&parts_dir)?;
    let parts = crate::parts::split(segments, &meta.chapters);
    let mut frontmatter = serde_json::json!({
        "title": meta.title, "source": meta.source, "uploader": meta.uploader,
        "upload_date": meta.upload_date, "duration_secs": duration, "model": cli.model,
        "transcribed_at": OffsetDateTime::now_utc().format(&Rfc3339)?,
        "chunk_secs": cli.chunk_secs.get(), "engine_secs": engine_secs,
    });
    if let Some(language) = &cli.language {
        frontmatter["language"] = language.clone().into();
    }
    let path = dir.join("transcript.md");
    let mut markdown = BufWriter::new(fs::File::create(&path)?);
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
    let mut jsonl = BufWriter::new(fs::File::create(dir.join("segments.jsonl"))?);
    for segment in segments {
        writeln!(
            markdown,
            "[{}] {}\n",
            crate::sources::timestamp(segment.start),
            segment.text.replace(['\r', '\n'], " ")
        )?;
        serde_json::to_writer(&mut jsonl, segment)?;
        writeln!(jsonl)?;
    }
    markdown.flush()?;
    jsonl.flush()?;
    fs::write(dir.join("meta.json"), serde_json::to_vec_pretty(meta)?)?;
    frontmatter["parts"] = parts.len().into();
    let words: usize = parts.iter().map(crate::parts::Part::words).sum();
    frontmatter["words"] = words.into();
    frontmatter["tokens_estimate"] = crate::parts::tokens_estimate(words).into();
    let mut entry = String::from("---\n");
    for (key, value) in frontmatter
        .as_object()
        .context("frontmatter is not an object")?
    {
        entry.push_str(&format!("{key}: {value}\n"));
    }
    entry.push_str(&format!(
        "---\n\n# {}\n\n",
        meta.title.replace(['\r', '\n'], " ")
    ));
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
        let title = part.chapter.map(|c| c.title.as_str()).unwrap_or(&preview);
        let piece = part.piece.map(|c| format!("-{c}")).unwrap_or_default();
        let filename = format!("{:02}-{}{piece}.md", i + 1, slug(title));
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
        fs::write(parts_dir.join(&filename), body)?;
        entry.push_str(&format!(
            "| [{:02}](parts/{filename}) | {time_range} | {} | {} | {} |\n",
            i + 1,
            part.words(),
            crate::parts::tokens_estimate(part.words()),
            preview.replace('|', "\\|")
        ));
    }
    // Write the index last so an incomplete run cannot be mistaken for a completed transcript.
    fs::write(&index, entry)?;
    Ok(index)
}
