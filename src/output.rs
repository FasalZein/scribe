use crate::{cli::Cli, engine::Segment, fetch::Metadata};
use anyhow::{Context, Result, ensure};
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
fn timestamp(seconds: f64) -> String {
    let seconds = seconds.max(0.0) as u64;
    format!(
        "{:02}:{:02}:{:02}",
        seconds / 3600,
        seconds / 60 % 60,
        seconds % 60
    )
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
    ensure!(
        !dir.exists(),
        "output directory already exists: {}; choose another --out to avoid overwriting",
        dir.display()
    );
    fs::create_dir(&dir)?;
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
    let mut jsonl = BufWriter::new(fs::File::create(dir.join("segments.jsonl"))?);
    for segment in segments {
        writeln!(
            markdown,
            "[{}] {}\n",
            timestamp(segment.start),
            segment.text.replace(['\r', '\n'], " ")
        )?;
        serde_json::to_writer(&mut jsonl, segment)?;
        writeln!(jsonl)?;
    }
    markdown.flush()?;
    jsonl.flush()?;
    fs::write(dir.join("meta.json"), serde_json::to_vec_pretty(meta)?)?;
    Ok(path)
}
