use crate::cli::DEFAULT_MODEL;
use anyhow::{Context, Result, bail, ensure};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
    process::{Command, Output},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

const DEFAULT_MODEL_BYTES: u64 = 739_508_576;

pub fn command_output(command: &mut Command, tool: &str) -> Result<Output> {
    let output = command
        .output()
        .with_context(|| format!("cannot run {tool}; install {tool} and ensure it is on PATH"))?;
    ensure!(
        output.status.success(),
        "{tool} failed ({}): {}",
        output.status,
        String::from_utf8_lossy(&output.stderr).trim()
    );
    Ok(output)
}

pub struct Workspace(pub PathBuf);
impl Workspace {
    pub fn new() -> Result<Self> {
        let nonce = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
        let path = std::env::temp_dir().join(format!("scribe-{}-{nonce}", std::process::id()));
        fs::create_dir(&path)?;
        Ok(Self(path))
    }
}
impl Drop for Workspace {
    fn drop(&mut self) {
        if let Err(error) = fs::remove_dir_all(&self.0) {
            eprintln!(
                "warning: cannot remove temporary directory {}: {error}",
                self.0.display()
            );
        }
    }
}

#[derive(Serialize, Deserialize)]
pub struct Metadata {
    pub title: String,
    pub source: String,
    pub uploader: Option<String>,
    pub upload_date: Option<String>,
    pub duration: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file_size: Option<u64>,
}

pub fn is_url(input: &str) -> bool {
    input.starts_with("https://") || input.starts_with("http://")
}

pub fn media(input: &str, workspace: &Path) -> Result<(Metadata, PathBuf)> {
    if !is_url(input) {
        let path =
            fs::canonicalize(input).with_context(|| format!("cannot open local media {input}"))?;
        let info = fs::metadata(&path)?;
        ensure!(info.is_file(), "local input is not a file: {input}");
        let title = path
            .file_stem()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned();
        return Ok((
            Metadata {
                title,
                source: path.to_string_lossy().into_owned(),
                uploader: None,
                upload_date: None,
                duration: None,
                file_size: Some(info.len()),
            },
            path,
        ));
    }
    eprintln!("Fetching metadata: {input}");
    let output = command_output(
        Command::new("yt-dlp").args(["--dump-single-json", "--no-playlist", "--", input]),
        "yt-dlp",
    )?;
    let raw: serde_json::Value =
        serde_json::from_slice(&output.stdout).context("invalid yt-dlp metadata")?;
    let text = |key: &str| raw[key].as_str().map(str::to_owned);
    let meta = Metadata {
        title: text("title").context("yt-dlp metadata has no title")?,
        source: input.to_owned(),
        uploader: text("uploader"),
        upload_date: text("upload_date"),
        duration: raw["duration"].as_f64(),
        file_size: None,
    };
    eprintln!("Downloading media: {}", meta.title);
    command_output(
        Command::new("yt-dlp")
            .args([
                "--no-playlist",
                "--no-progress",
                "-f",
                "bestaudio/best",
                "-o",
            ])
            .arg(workspace.join("media.%(ext)s"))
            .args(["--", input]),
        "yt-dlp",
    )?;
    let files: Vec<_> = fs::read_dir(workspace)?.collect::<std::io::Result<Vec<_>>>()?;
    let path = files
        .into_iter()
        .map(|entry| entry.path())
        .find(|p| {
            p.is_file()
                && p.extension()
                    .is_some_and(|ext| ext != "part" && ext != "ytdl")
        })
        .context("yt-dlp did not produce a media file")?;
    Ok((meta, path))
}

pub fn model(input: &str) -> Result<PathBuf> {
    if !is_url(input) {
        return fs::canonicalize(input).with_context(|| format!("cannot open model {input}"));
    }
    let cache = dirs::cache_dir()
        .context("platform cache directory is unavailable")?
        .join("scribe/models");
    fs::create_dir_all(&cache)?;
    let name = input
        .split('?')
        .next()
        .unwrap_or(input)
        .rsplit('/')
        .next()
        .filter(|s| !s.is_empty())
        .context("model URL has no filename")?;
    ensure!(name != "." && name != "..", "invalid model filename");
    // Separate arbitrary URLs with the same filename. Keep the default cache name predictable.
    let path = if input == DEFAULT_MODEL {
        cache.join(name)
    } else {
        use std::hash::{Hash, Hasher};
        let mut hash = std::collections::hash_map::DefaultHasher::new();
        input.hash(&mut hash);
        cache.join(format!("{:016x}-{name}", hash.finish()))
    };
    let agent = ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(30))
        .timeout_read(Duration::from_secs(120))
        .build();
    let expected = if input == DEFAULT_MODEL {
        Some(DEFAULT_MODEL_BYTES)
    } else {
        agent.head(input).call().ok().and_then(|r| {
            r.header("content-length")
                .and_then(|s| s.parse::<u64>().ok())
        })
    };
    if let (Some(size), Ok(info)) = (expected, fs::metadata(&path))
        && info.len() == size
    {
        return Ok(path);
    }
    eprintln!("Downloading model: {input}");
    let response = agent.get(input).call().context("model download failed")?;
    let expected = expected.or_else(|| {
        response
            .header("content-length")
            .and_then(|s| s.parse().ok())
    });
    download(response.into_reader(), &path, expected)?;
    Ok(path)
}

fn download(mut reader: impl Read, path: &Path, expected: Option<u64>) -> Result<()> {
    let part = path.with_file_name(format!(
        "{}.part",
        path.file_name()
            .context("missing cache filename")?
            .to_string_lossy()
    ));
    let result = (|| {
        let mut file = fs::File::create(&part)?;
        let mut buffer = [0u8; 64 * 1024];
        let mut total = 0u64;
        let mut last_report = 0u64;
        loop {
            let count = reader.read(&mut buffer)?;
            if count == 0 {
                break;
            }
            file.write_all(&buffer[..count])?;
            total += count as u64;
            if total - last_report >= 16 * 1024 * 1024 {
                eprintln!(
                    "Model: {} MiB{}",
                    total / (1024 * 1024),
                    expected
                        .map(|n| format!(" / {} MiB", n / (1024 * 1024)))
                        .unwrap_or_default()
                );
                last_report = total;
            }
        }
        if let Some(size) = expected {
            ensure!(
                total == size,
                "model download is incomplete: expected {size} bytes, got {total}"
            );
        }
        if total == 0 {
            bail!("model download is empty");
        }
        file.sync_all()?;
        fs::rename(&part, path)?;
        eprintln!("Model cached: {} ({total} bytes)", path.display());
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(part);
    }
    result
}
