use crate::cli::DEFAULT_MODEL;
use crate::sources::{Chapter, Sources};
use anyhow::{Context, Result, bail, ensure};
use serde::Serialize;
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

#[derive(Serialize)]
pub struct Metadata {
    pub title: String,
    pub source: String,
    pub uploader: Option<String>,
    pub upload_date: Option<String>,
    pub duration: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file_size: Option<u64>,
    pub sources: Sources,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub chapters: Vec<Chapter>,
}

pub fn is_url(input: &str) -> bool {
    input.starts_with("https://") || input.starts_with("http://")
}

pub enum Media {
    File(PathBuf),
    Stream(String),
}
/// Media known after the metadata fetch. A yt-dlp page still needs a download.
pub enum Pending {
    Ready(Media),
    YtDlp(String),
}
impl Media {
    pub fn input(&self) -> &std::ffi::OsStr {
        match self {
            Self::File(path) => path.as_os_str(),
            Self::Stream(url) => url.as_ref(),
        }
    }
}

pub fn media(input: &str) -> Result<(Metadata, Pending)> {
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
                sources: Sources::Local {
                    path: path.to_string_lossy().into_owned(),
                },
                chapters: Vec::new(),
            },
            Pending::Ready(Media::File(path)),
        ));
    }
    if crate::sources::is_x_post(input) {
        eprintln!("Fetching X API metadata: {input}");
        match x_media(input) {
            Ok(result) => return Ok(result),
            Err(XError::Fatal(error)) => return Err(error),
            Err(XError::Fallback(error)) => {
                eprintln!("X API failed; falling back to yt-dlp: {error:#}")
            }
        }
    }
    eprintln!("Fetching metadata: {input}");
    let output = command_output(
        yt_dlp()?.args(["--dump-single-json", "--no-playlist", "--", input]),
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
        sources: Sources::Web {
            url: text("webpage_url").unwrap_or_else(|| input.to_owned()),
            channel_url: text("channel_url").or_else(|| text("uploader_url")),
            links: crate::sources::description_links(raw["description"].as_str().unwrap_or("")),
        },
        chapters: raw["chapters"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|c| {
                Some(Chapter {
                    start_time: c["start_time"].as_f64()?,
                    title: c["title"].as_str()?.to_owned(),
                })
            })
            .collect(),
    };
    Ok((meta, Pending::YtDlp(input.to_owned())))
}

/// Resolve media only after the existing index check.
pub fn resolve(media: Pending, workspace: &Path) -> Result<Media> {
    let input = match media {
        Pending::Ready(media) => return Ok(media),
        Pending::YtDlp(input) => input,
    };
    eprintln!("Downloading media: {input}");
    command_output(
        yt_dlp()?
            .args([
                "--no-playlist",
                "--no-progress",
                "-f",
                "bestaudio/best",
                "-o",
            ])
            .arg(workspace.join("media.%(ext)s"))
            .args(["--", &input]),
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
    Ok(Media::File(path))
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

fn executable_on_path(name: &str) -> bool {
    #[cfg(windows)]
    let name = &format!("{name}.exe");
    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt;
    std::env::var_os("PATH").is_some_and(|path| {
        std::env::split_paths(&path).any(|dir| {
            fs::metadata(dir.join(name)).is_ok_and(|m| {
                #[cfg(unix)]
                let executable = m.permissions().mode() & 0o111 != 0;
                #[cfg(not(unix))]
                let executable = true;
                m.is_file() && executable
            })
        })
    })
}
fn yt_dlp() -> Result<Command> {
    static TOOL: std::sync::OnceLock<Option<bool>> = std::sync::OnceLock::new();
    let uvx = TOOL
        .get_or_init(|| {
            let selected = if executable_on_path("uvx") {
                Some(true)
            } else if executable_on_path("yt-dlp") {
                Some(false)
            } else {
                None
            };
            if let Some(uvx) = selected {
                eprintln!(
                    "Using yt-dlp command: {}",
                    if uvx { "uvx yt-dlp@latest" } else { "yt-dlp" }
                );
            }
            selected
        })
        .context("cannot run yt-dlp: neither uvx nor yt-dlp is on PATH; install one of them")?;
    let mut command = Command::new(if uvx { "uvx" } else { "yt-dlp" });
    if uvx {
        command.arg("yt-dlp@latest");
    }
    // Deno's standard installer does not add its bin directory to every shell's PATH.
    if let Some(home) = dirs::home_dir() {
        let deno = home.join(".deno/bin/deno");
        if deno.is_file() {
            command
                .arg("--js-runtimes")
                .arg(format!("deno:{}", deno.display()));
        }
    }
    Ok(command)
}

enum XError {
    Fatal(anyhow::Error),
    Fallback(anyhow::Error),
}

fn x_media(input: &str) -> std::result::Result<(Metadata, Pending), XError> {
    let base = std::env::var("X_API_BASE").unwrap_or_else(|_| "https://x.pcstyle.dev".into());
    let agent = ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(15))
        .timeout_read(Duration::from_secs(60))
        .build();
    let endpoint = format!("{}/api/convert", base.trim_end_matches('/'));
    let mut request = agent
        .get(&endpoint)
        .query("url", input)
        .query("format", "json")
        .set("Accept", "application/json");
    if let Ok(key) = std::env::var("X_MD_API_KEY") {
        request = request.set("Authorization", &format!("Bearer {key}"));
    }
    let response = match request.call() {
        Ok(response) => response,
        Err(ureq::Error::Status(429, response)) => {
            return Err(XError::Fatal(anyhow::anyhow!(
                "X API HTTP 429; Retry-After: {}",
                response.header("Retry-After").unwrap_or("not provided")
            )));
        }
        // Do not include request or response headers in diagnostics.
        Err(error) => {
            return Err(XError::Fallback(anyhow::anyhow!(
                "X API request failed: {error}"
            )));
        }
    };
    let raw: serde_json::Value = serde_json::from_reader(response.into_reader())
        .map_err(|e| XError::Fallback(anyhow::anyhow!("invalid X API JSON: {e}")))?;
    let post = raw["posts"]
        .as_array()
        .and_then(|p| p.first())
        .ok_or_else(|| XError::Fallback(anyhow::anyhow!("X API response has no posts[0]")))?;
    let meta = x_metadata(post, &raw).map_err(XError::Fallback)?;
    let variant = crate::sources::mp4_variant(post)
        .map_err(XError::Fallback)?
        .ok_or_else(|| XError::Fatal(anyhow::anyhow!("X post has no video or GIF")))?;
    Ok((meta, Pending::Ready(Media::Stream(variant.to_owned()))))
}

fn x_metadata(post: &serde_json::Value, raw: &serde_json::Value) -> Result<Metadata> {
    let field = |key: &str| {
        post[key]
            .as_str()
            .with_context(|| format!("X post has no {key}"))
    };
    let text = field("text")?;
    let url = field("url")?;
    let date = field("created_at")?;
    let author = post["author"]["name"]
        .as_str()
        .context("X post has no author name")?;
    let handle = post["author"]["screen_name"]
        .as_str()
        .context("X post has no author handle")?;
    let (links, mentions) = crate::sources::x_links(post);
    let mut quoted_posts = Vec::new();
    let quotes = raw["posts"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|p| matches!(p["context"].as_str(), Some("quote" | "quoted")))
        .chain([&post["quote"], &post["quoted_post"], &post["quoted_status"]]);
    for quote in quotes {
        if let (Some(url), Some(text)) = (quote["url"].as_str(), quote["text"].as_str())
            && !quoted_posts
                .iter()
                .any(|q: &crate::sources::QuotedPost| q.url == url)
        {
            quoted_posts.push(crate::sources::QuotedPost {
                url: url.to_owned(),
                text: text.to_owned(),
            });
        }
    }
    Ok(Metadata {
        title: format!(
            "{author} (@{handle}): {}",
            // Post text often has blank lines; titles land in YAML and headings, so keep one line.
            text.split_whitespace()
                .collect::<Vec<_>>()
                .join(" ")
                .chars()
                .take(80)
                .collect::<String>()
        ),
        source: url.to_owned(),
        uploader: Some(handle.to_owned()),
        upload_date: Some(x_date(date)?),
        duration: post["media"]["all"]
            .as_array()
            .and_then(|a| {
                a.iter()
                    .find(|m| matches!(m["type"].as_str(), Some("video" | "gif" | "animated_gif")))
            })
            .and_then(|m| m["duration"].as_f64()),
        file_size: None,
        sources: Sources::X {
            url: url.to_owned(),
            author: format!("{author} (@{handle})"),
            date: date.to_owned(),
            text: text.to_owned(),
            links,
            mentions,
            quoted_posts,
        },
        chapters: Vec::new(),
    })
}
fn x_date(date: &str) -> Result<String> {
    let parts: Vec<_> = date.split_whitespace().collect();
    ensure!(parts.len() == 6, "unexpected X created_at: {date}");
    let month = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ]
    .iter()
    .position(|m| *m == parts[1])
    .context("invalid X date month")?
        + 1;
    let date = time::Date::from_calendar_date(
        parts[5].parse()?,
        time::Month::try_from(month as u8)?,
        parts[2].parse()?,
    )?;
    Ok(format!(
        "{:04}{:02}{:02}",
        date.year(),
        u8::from(date.month()),
        date.day()
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn x_metadata_uses_author_text_and_created_at() {
        let raw: serde_json::Value =
            serde_json::from_str(include_str!("../tests/fixtures/x.json")).unwrap();
        let meta = x_metadata(&raw["posts"][0], &raw).unwrap();
        assert_eq!(meta.uploader.as_deref(), Some("pidotdev"));
        assert_eq!(meta.upload_date.as_deref(), Some("20261005"));
        assert!(
            meta.title
                .starts_with("Pi (@pidotdev): Welcome to our Monday Meditations!")
        );
        assert_eq!(meta.duration, Some(1646.416));
        assert!(x_date("not a date").is_err());
        assert!(x_date("Mon Feb 30 09:00:03 +0000 2026").is_err());
    }
    #[test]
    fn quoted_posts_exclude_thread_and_reply_context() {
        let mut raw: serde_json::Value =
            serde_json::from_str(include_str!("../tests/fixtures/x.json")).unwrap();
        raw["posts"].as_array_mut().unwrap().extend([
            serde_json::json!({"context": "quote", "url": "https://x.com/example/status/1", "text": "Quoted"}),
            serde_json::json!({"context": "thread", "url": "https://x.com/example/status/2", "text": "Thread"}),
            serde_json::json!({"context": "reply", "url": "https://x.com/example/status/3", "text": "Reply"}),
        ]);
        let meta = x_metadata(&raw["posts"][0], &raw).unwrap();
        let Sources::X { quoted_posts, .. } = meta.sources else {
            panic!("expected X sources")
        };
        assert_eq!(quoted_posts.len(), 1);
        assert_eq!(quoted_posts[0].text, "Quoted");
    }
    #[test]
    fn x_title_is_one_line_with_single_spaces() {
        let raw: serde_json::Value =
            serde_json::from_str(include_str!("../tests/fixtures/x.json")).unwrap();
        assert!(raw["posts"][0]["text"].as_str().unwrap().contains("\n\n"));
        let meta = x_metadata(&raw["posts"][0], &raw).unwrap();
        assert_eq!(
            meta.title,
            "Pi (@pidotdev): Welcome to our Monday Meditations! 🌞 Today @badlogicgames and @mitsuhiko are tal"
        );
    }
}
