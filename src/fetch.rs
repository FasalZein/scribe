use crate::cli::DEFAULT_MODEL;
use crate::sources::{Chapter, Sources};
use anyhow::{Context, Result, bail, ensure};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::{Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
    process::{Command, Output},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

const DEFAULT_MODEL_BYTES: u64 = 739_508_576;
/// SHA-256 of the default model at the pinned revision in `DEFAULT_MODEL`.
const DEFAULT_MODEL_SHA256: &str =
    "5859f77944efcd8eafa23a6350731960b2b55b2203df51f319665c807d802cc7";
/// yt-dlp gives up on a socket that makes no progress for this many seconds.
const SOCKET_TIMEOUT_SECS: &str = "30";
/// A local source ID hashes this many bytes from each end of the file.
const ID_SAMPLE_BYTES: u64 = 1024 * 1024;

pub fn sha256_hex(bytes: &[u8]) -> String {
    hex(&Sha256::digest(bytes))
}
fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

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
    /// The source ID: stable across runs, titles and dates (see docs/adr/0004-source-id.md).
    pub id: String,
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
    /// The `--dump-single-json` output, so the download reuses the extraction.
    YtDlp(Vec<u8>),
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
        // `absolute` avoids the `\\?\` prefix that `canonicalize` adds on Windows.
        let path = std::path::absolute(input)?;
        let info =
            fs::metadata(&path).with_context(|| format!("cannot open local media {input}"))?;
        ensure!(info.is_file(), "local input is not a file: {input}");
        let title = path
            .file_stem()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned();
        return Ok((
            Metadata {
                id: file_id(&path, info.len())?,
                title,
                source: path.to_string_lossy().into_owned(),
                uploader: None,
                upload_date: None,
                duration: probe_duration(&path),
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
        id: yt_dlp_id(input, &raw)?,
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
    Ok((meta, Pending::YtDlp(output.stdout)))
}

/// Resolve media only after the existing index check.
pub fn resolve(media: Pending, workspace: &Path) -> Result<Media> {
    let info = match media {
        Pending::Ready(media) => return Ok(media),
        Pending::YtDlp(info) => info,
    };
    // A second extraction costs seconds on YouTube (page fetch and JS challenge). The format
    // URLs in the saved JSON stay valid for hours, far longer than this run needs.
    let info_path = workspace.join("info.json");
    fs::write(&info_path, info)?;
    eprintln!("Downloading media");
    let output_path = command_output(
        yt_dlp()?
            .args([
                "--no-playlist",
                "--no-progress",
                "-f",
                "bestaudio/best",
                // Print the final media path: a user config can add subtitles or thumbnails.
                "--print",
                "after_move:filepath",
                "-o",
            ])
            .arg(workspace.join("media.%(ext)s"))
            .arg("--load-info-json")
            .arg(&info_path),
        "yt-dlp",
    )?;
    let path = String::from_utf8_lossy(&output_path.stdout)
        .lines()
        .rev()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .map(PathBuf::from)
        .filter(|path| path.is_file())
        .context("yt-dlp did not produce a media file")?;
    Ok(Media::File(path))
}

pub fn model(input: &str) -> Result<PathBuf> {
    if !is_url(input) {
        let path = std::path::absolute(input)?;
        fs::metadata(&path).with_context(|| format!("cannot open model {input}"))?;
        return Ok(path);
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
        // A fixed hash keeps the cache name across Rust releases, unlike DefaultHasher.
        cache.join(format!("{}-{name}", &sha256_hex(input.as_bytes())[..16]))
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
    let digest = (input == DEFAULT_MODEL).then_some(DEFAULT_MODEL_SHA256);
    download(response.into_reader(), &path, expected, digest)?;
    Ok(path)
}

fn download(
    mut reader: impl Read,
    path: &Path,
    expected: Option<u64>,
    sha256: Option<&str>,
) -> Result<()> {
    // A unique name per download: two processes must not write into the same file.
    let nonce = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
    let part = path.with_file_name(format!(
        "{}.{}-{nonce}.part",
        path.file_name()
            .context("missing cache filename")?
            .to_string_lossy(),
        std::process::id()
    ));
    let result = (|| {
        let mut file = fs::File::create(&part)?;
        let mut hasher = Sha256::new();
        let mut buffer = [0u8; 64 * 1024];
        let mut total = 0u64;
        let mut last_report = 0u64;
        loop {
            let count = reader.read(&mut buffer)?;
            if count == 0 {
                break;
            }
            file.write_all(&buffer[..count])?;
            hasher.update(&buffer[..count]);
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
        if let Some(sha256) = sha256 {
            let actual = hex(&hasher.finalize());
            ensure!(
                actual == sha256,
                "model download has SHA-256 {actual}, expected {sha256}"
            );
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
    command.args(["--socket-timeout", SOCKET_TIMEOUT_SECS]);
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

/// The source ID of a yt-dlp source. An X post keeps its status ID when the X API fails.
fn yt_dlp_id(input: &str, raw: &serde_json::Value) -> Result<String> {
    if let Some(status) = crate::sources::x_status_id(input) {
        return Ok(format!("x:{status}"));
    }
    let extractor = raw["extractor_key"]
        .as_str()
        .context("yt-dlp metadata has no extractor_key")?;
    let id = raw["id"].as_str().context("yt-dlp metadata has no id")?;
    Ok(format!("{}:{id}", extractor.to_lowercase()))
}

/// The source ID of a local file: its size and a SHA-256 over the first and last MiB. Media
/// containers keep their headers and index at the ends, so two different recordings with equal
/// sizes and equal ends do not occur in practice, and a multi-GB file is not read in full.
fn file_id(path: &Path, size: u64) -> Result<String> {
    let mut file = fs::File::open(path)?;
    let mut hasher = Sha256::new();
    hasher.update(size.to_le_bytes());
    let mut sample = Vec::new();
    (&mut file).take(ID_SAMPLE_BYTES).read_to_end(&mut sample)?;
    hasher.update(&sample);
    sample.clear();
    file.seek(SeekFrom::Start(size.saturating_sub(ID_SAMPLE_BYTES)))?;
    file.take(ID_SAMPLE_BYTES).read_to_end(&mut sample)?;
    hasher.update(&sample);
    Ok(format!("file:{}", &hex(&hasher.finalize())[..32]))
}

/// The audio duration a local file reports, for the truncation check. None when unknown.
fn probe_duration(path: &Path) -> Option<f64> {
    let output = command_output(
        Command::new("ffprobe")
            .args(["-v", "error", "-select_streams", "a:0"])
            .args(["-show_entries", "stream=duration:format=duration"])
            .args(["-of", "default=noprint_wrappers=1:nokey=1"])
            .arg(path),
        "ffprobe",
    )
    .ok()?;
    // The audio stream duration comes first; containers without one report N/A there.
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .find_map(|line| line.trim().parse::<f64>().ok())
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
        id: format!(
            "x:{}",
            crate::sources::x_status_id(url).context("X post URL has no status ID")?
        ),
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
    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("scribe-test-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }
    #[test]
    fn local_source_id_follows_content_not_path() {
        let dir = temp_dir("file-id");
        let content: Vec<u8> = (0..3 * ID_SAMPLE_BYTES).map(|i| (i % 251) as u8).collect();
        let (a, b, c) = (dir.join("a.mp4"), dir.join("b.mp4"), dir.join("c.mp4"));
        fs::write(&a, &content).unwrap();
        fs::write(&b, &content).unwrap();
        let mut changed = content.clone();
        *changed.last_mut().unwrap() ^= 1;
        fs::write(&c, &changed).unwrap();
        let id = |p: &Path| file_id(p, fs::metadata(p).unwrap().len()).unwrap();
        assert_eq!(id(&a), id(&b));
        assert_ne!(id(&a), id(&c));
        assert!(id(&a).starts_with("file:"));
        fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn yt_dlp_id_uses_extractor_and_id_and_keeps_x_status_ids() {
        let raw = serde_json::json!({"extractor_key": "Youtube", "id": "UNP03fDSj1U"});
        assert_eq!(
            yt_dlp_id("https://www.youtube.com/watch?v=UNP03fDSj1U", &raw).unwrap(),
            "youtube:UNP03fDSj1U"
        );
        assert_eq!(
            yt_dlp_id("https://x.com/pidotdev/status/2107033061905104941", &raw).unwrap(),
            "x:2107033061905104941"
        );
    }
    #[test]
    fn download_checks_size_and_sha256_and_leaves_no_part_file() {
        let dir = temp_dir("download");
        let path = dir.join("model.gguf");
        let data = b"model bytes";
        let digest = sha256_hex(data);
        assert!(download(&data[..5], &path, Some(11), None).is_err());
        assert!(download(&data[..], &path, Some(11), Some(&"0".repeat(64))).is_err());
        assert!(!path.exists());
        download(&data[..], &path, Some(11), Some(&digest)).unwrap();
        assert_eq!(fs::read(&path).unwrap(), data);
        assert_eq!(
            fs::read_dir(&dir).unwrap().count(),
            1,
            "no .part file is left"
        );
        fs::remove_dir_all(dir).unwrap();
    }
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
        assert_eq!(meta.id, "x:2107033061905104941");
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
