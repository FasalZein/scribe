use crate::cli::DEFAULT_MODEL;
use crate::sources::{Chapter, Sources};
use anyhow::{Context, Result, bail, ensure};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::{Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
    process::{Command, Output},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

const DEFAULT_MODEL_BYTES: u64 = 740_363_168;
/// SHA-256 of the default model at the pinned revision in `DEFAULT_MODEL`.
const DEFAULT_MODEL_SHA256: &str =
    "007a59761e9258779f189df396b50d12b83bb5b79e66df4b955c230d2f2a0a59";
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

/// Failed runs keep media for seven days since last use. Active workspaces are never swept.
const MEDIA_STALE_AFTER: Duration = Duration::from_secs(7 * 24 * 60 * 60);

pub struct Workspace {
    pub path: PathBuf,
    // The lock lives beside the directory so Windows can remove the completed directory.
    _lock: fs::File,
}
impl Workspace {
    pub fn new(source_id: &str) -> Result<Self> {
        let cache = dirs::cache_dir()
            .context("platform cache directory is unavailable")?
            .join("scribe/media");
        Self::in_cache(&cache, source_id, SystemTime::now())
    }

    fn in_cache(cache: &Path, source_id: &str, now: SystemTime) -> Result<Self> {
        fs::create_dir_all(cache)?;
        Self::sweep(cache, now)?;
        let key = sha256_hex(source_id.as_bytes());
        let lock = Self::lock(cache, &key)?;
        lock.try_lock()
            .with_context(|| format!("media download already in use for {source_id}"))?;
        let path = cache.join(key);
        fs::create_dir_all(&path)?;
        // Explicitly refresh on reuse, even when yt-dlp has not written any new bytes yet.
        let marker = fs::File::create(path.join("last-used"))?;
        marker.set_modified(now)?;
        Ok(Self { path, _lock: lock })
    }

    fn lock(cache: &Path, key: &str) -> Result<fs::File> {
        Ok(fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(cache.join(format!("{key}.lock")))?)
    }

    fn sweep(cache: &Path, now: SystemTime) -> Result<()> {
        for entry in fs::read_dir(cache)? {
            let entry = entry?;
            let key = entry.file_name();
            let key = key.to_string_lossy();
            // Own only full source-ID hashes, never arbitrary user files or symlinks.
            if key.len() != 64
                || !key.bytes().all(|b| b.is_ascii_hexdigit())
                || !entry.file_type()?.is_dir()
            {
                continue;
            }
            // Lock before reading the marker: a concurrent run may have refreshed it since
            // read_dir. Another run may also have removed its completed directory already.
            let lock = Self::lock(cache, &key)?;
            if lock.try_lock().is_err() {
                continue;
            }
            let result = Self::sweep_one(&entry.path(), now);
            // Unlock explicitly: a child forked meanwhile shares this open file until it execs,
            // and closing our copy alone would not release the lock.
            let _ = lock.unlock();
            result?;
        }
        Ok(())
    }

    fn sweep_one(path: &Path, now: SystemTime) -> Result<()> {
        let info = match fs::metadata(path.join("last-used")).or_else(|_| fs::metadata(path)) {
            Ok(info) => info,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(error) => return Err(error.into()),
        };
        if now
            .duration_since(info.modified()?)
            .is_ok_and(|age| age >= MEDIA_STALE_AFTER)
        {
            fs::remove_dir_all(path)?;
        }
        Ok(())
    }

    /// Only a successful publication retires resumable media. Lock files are tiny and retained
    /// to avoid a race where another process locks an unlinked inode for the same source ID.
    pub fn complete(self) {
        if let Err(error) = fs::remove_dir_all(&self.path) {
            crate::progress::line!(
                "warning: cannot remove media cache {}: {error}",
                self.path.display()
            );
        }
    }
}
impl Drop for Workspace {
    // A child process forked by another thread shares the lock's open file until it execs
    // (Rust forks instead of using posix_spawn when a command changes PATH). Closing our copy
    // alone would keep the lock for that window, so release it explicitly.
    fn drop(&mut self) {
        let _ = self._lock.unlock();
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
    /// The yt-dlp description, read for known terms. meta.json does not store it.
    #[serde(skip)]
    pub description: Option<String>,
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

/// `audio_stream` counts audio streams from 0; a local file reports the duration of that stream.
pub fn media(input: &str, audio_stream: usize) -> Result<(Metadata, Pending)> {
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
                duration: probe_duration(&path, audio_stream),
                file_size: Some(info.len()),
                description: None,
                sources: Sources::Local {
                    path: path.to_string_lossy().into_owned(),
                },
                chapters: Vec::new(),
            },
            Pending::Ready(Media::File(path)),
        ));
    }
    if crate::sources::is_x_post(input) {
        crate::progress::line!("Fetching X API metadata: {input}");
        match x_media(input) {
            Ok(result) => return Ok(result),
            Err(XError::Fatal(error)) => return Err(error),
            Err(XError::Fallback(error)) => {
                crate::progress::line!("X API failed; falling back to yt-dlp: {error:#}")
            }
        }
    }
    crate::progress::line!("Fetching metadata: {input}");
    let output = yt_dlp(|command| {
        command.args(["--dump-single-json", "--no-playlist", "--", input]);
    })?;
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
        description: text("description"),
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
    crate::progress::line!("Downloading media");
    let output_path = yt_dlp(|command| media_download_args(command, workspace, &info_path))?;
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

fn media_download_args(command: &mut Command, workspace: &Path, info_path: &Path) {
    command
        .args([
            "--no-playlist",
            "--no-progress",
            "--continue",
            // yt-dlp skips a fragment it cannot fetch and still succeeds. The decoded audio
            // then misses that stretch, and the duration check allows up to 1 % of slack.
            "--abort-on-unavailable-fragments",
            // Keep the best audio-only format: a lower audio bitrate needs a WER comparison
            // first (ADR 0005). Without one, take the smallest format that carries audio, so a
            // site with only progressive video does not download its largest file. `!=?` keeps
            // formats whose audio codec is unknown, as for plain HLS streams.
            "-f",
            "bestaudio/worst[acodec!=?none]",
            // Print the final media path: a user config can add subtitles or thumbnails.
            "--print",
            "after_move:filepath",
            "-o",
        ])
        .arg(workspace.join("media.%(ext)s"))
        .arg("--load-info-json")
        .arg(info_path);
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
    model_in_cache(input, &cache)
}

fn model_in_cache(input: &str, cache: &Path) -> Result<PathBuf> {
    fs::create_dir_all(cache)?;
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
    // The OS releases this lock even after SIGKILL. Keep the lock file itself stable:
    // unlinking it could let a third process lock a different inode during a download.
    let lock = fs::File::options()
        .create(true)
        .truncate(false)
        .write(true)
        .open(cache.join(".download.lock"))?;
    lock.lock()?;
    sweep_partials(cache)?;
    let agent = ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(30))
        .timeout_read(Duration::from_secs(120))
        .build();
    if input != DEFAULT_MODEL {
        let completed = fs::read(sidecar(&path, ".complete.json"))
            .ok()
            .and_then(|bytes| serde_json::from_slice::<CompletedRecord>(&bytes).ok());
        if let Some(record) = completed.filter(|r| r.url == input && r.size > 0)
            && is_cached(&path, Some(record.size), Some(&record.sha256))
        {
            return Ok(path);
        }
    }
    let digest = (input == DEFAULT_MODEL).then_some(DEFAULT_MODEL_SHA256);
    let expected = if input == DEFAULT_MODEL {
        Some(DEFAULT_MODEL_BYTES)
    } else {
        agent.head(input).call().ok().and_then(|r| {
            r.header("content-length")
                .and_then(|s| s.parse::<u64>().ok())
        })
    };
    // A failed completion-record hash must not fall back to trusting HEAD's size.
    // A legacy cache has no record yet; migrate it after the old size check succeeds.
    if (digest.is_some() || !sidecar(&path, ".complete.json").exists())
        && is_cached(&path, expected, digest)
    {
        if digest.is_none() {
            let mut hasher = Sha256::new();
            std::io::copy(&mut fs::File::open(&path)?, &mut hasher)?;
            write_completed(input, &path, hex(&hasher.finalize()))?;
        }
        return Ok(path);
    }
    download_model(&agent, input, &path, expected, digest)?;
    Ok(path)
}

fn sidecar(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(suffix);
    PathBuf::from(name)
}

#[derive(Serialize, Deserialize)]
struct CompletedRecord {
    url: String,
    size: u64,
    sha256: String,
}

#[derive(Serialize, Deserialize)]
struct PartialRecord {
    url: String,
    validator: Option<String>,
}

/// Seven days without a write makes a partial stale. Recent partials remain resumable.
/// The cache lock prevents the sweep from deleting another scribe download's files.
const PARTIAL_MAX_AGE: Duration = Duration::from_secs(7 * 24 * 60 * 60);
fn sweep_partials(cache: &Path) -> Result<()> {
    for entry in fs::read_dir(cache)? {
        let entry = entry?;
        let path = entry.path();
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name.ends_with(".part")
            && entry
                .metadata()?
                .modified()?
                .elapsed()
                .is_ok_and(|age| age > PARTIAL_MAX_AGE)
        {
            fs::remove_file(&path)?;
            let _ = fs::remove_file(sidecar(&path, ".json"));
        } else if name.ends_with(".part.json") {
            let part = cache.join(name.trim_end_matches(".json"));
            if !part.exists() {
                // read_dir can still yield a sidecar removed with its stale partial above.
                match fs::remove_file(path) {
                    Ok(()) => (),
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => (),
                    Err(error) => return Err(error.into()),
                }
            }
        }
    }
    Ok(())
}

fn download_model(
    agent: &ureq::Agent,
    url: &str,
    path: &Path,
    expected: Option<u64>,
    sha256: Option<&str>,
) -> Result<()> {
    let part = sidecar(path, ".part");
    let record_path = sidecar(&part, ".json");
    let record = fs::read(&record_path)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<PartialRecord>(&bytes).ok())
        .filter(|record| record.url == url);
    // If-Range guards against remote replacements when the server supplies a validator.
    // Without one, range resume assumes the URL's contents remain stable, as with curl -C.
    // The pinned default's SHA-256 still rejects any mixed or damaged content.
    let offset = if record.is_some() || sha256.is_some() {
        fs::metadata(&part).map(|m| m.len()).unwrap_or(0)
    } else {
        0
    };
    crate::progress::line!("Downloading model: {url}");
    let mut request = agent.get(url).set("Accept-Encoding", "identity");
    if offset > 0 {
        request = request.set("Range", &format!("bytes={offset}-"));
        if let Some(validator) = record.as_ref().and_then(|r| r.validator.as_deref()) {
            request = request.set("If-Range", validator);
        }
    }
    let response = match request.call() {
        // The remote object shrank, or the partial already has all bytes. Start over safely.
        Err(ureq::Error::Status(416, _)) if offset > 0 => {
            fs::remove_file(&part)?;
            let _ = fs::remove_file(&record_path);
            return download_model(agent, url, path, expected, sha256);
        }
        response => response.context("model download failed")?,
    };
    let length = response
        .header("content-length")
        .and_then(|s| s.parse::<u64>().ok());
    let (resume, total) = if response.status() == 206 {
        ensure!(offset > 0, "unexpected partial model response");
        let (start, end, total) = response
            .header("content-range")
            .and_then(parse_content_range)
            .context("invalid model Content-Range")?;
        ensure!(
            start == offset && end + 1 == total && length.is_none_or(|n| n == total - start),
            "model Content-Range does not match requested range"
        );
        ensure!(
            expected.is_none_or(|n| n == total),
            "model size changed during resume"
        );
        (true, Some(total))
    } else {
        ensure!(
            response.status() == 200,
            "unexpected model HTTP status {}",
            response.status()
        );
        (false, expected.or(length))
    };
    let validator = response
        .header("etag")
        .filter(|s| !s.starts_with("W/"))
        .or_else(|| response.header("last-modified"))
        .map(str::to_owned);
    if resume
        && let (Some(previous), Some(current)) = (
            record.as_ref().and_then(|r| r.validator.as_deref()),
            validator.as_deref(),
        )
    {
        ensure!(
            previous == current,
            "model validator changed during range resume"
        );
    }
    let validator = validator.or_else(|| {
        if resume {
            record.and_then(|r| r.validator)
        } else {
            None
        }
    });
    // Clear old bytes before publishing a validator for a replacement response. A kill
    // between these writes must not associate the old prefix with a new remote object.
    if !resume {
        fs::File::create(&part)?.sync_all()?;
    }
    fs::write(
        &record_path,
        serde_json::to_vec(&PartialRecord {
            url: url.to_owned(),
            validator,
        })?,
    )?;
    let result = download(response.into_reader(), path, total, sha256, resume);
    if result.is_ok() || !part.exists() {
        let _ = fs::remove_file(record_path);
    }
    let actual = result?;
    if sha256.is_none() {
        write_completed(url, path, actual)?;
    }
    Ok(())
}

fn write_completed(url: &str, path: &Path, sha256: String) -> Result<()> {
    let record = CompletedRecord {
        url: url.to_owned(),
        size: fs::metadata(path)?.len(),
        sha256,
    };
    fs::write(
        sidecar(path, ".complete.json"),
        serde_json::to_vec(&record)?,
    )?;
    write_marker(path, &record.sha256);
    Ok(())
}

fn parse_content_range(value: &str) -> Option<(u64, u64, u64)> {
    let (range, total) = value.strip_prefix("bytes ")?.split_once('/')?;
    let (start, end) = range.split_once('-')?;
    let (start, end, total) = (start.parse().ok()?, end.parse().ok()?, total.parse().ok()?);
    (start <= end && end < total).then_some((start, end, total))
}

/// A cached model is reused when its size matches and, for the default model, its SHA-256.
/// The hash keeps a different or damaged file at the cache path from loading as the default
/// model. Hashing 740 MB costs about 0.8-1.5 s on an M4 Pro (sha2 without its `asm` feature
/// uses no ARM SHA instructions), so a verified hash is recorded in a marker file keyed by the
/// file's size and modification time, and later runs check only the marker. Accepted limit: a
/// change that keeps both size and modification time goes unnoticed; delete the marker (or the
/// model) to force a full check.
fn is_cached(path: &Path, expected: Option<u64>, sha256: Option<&str>) -> bool {
    let (Some(size), Ok(info)) = (expected, fs::metadata(path)) else {
        return false;
    };
    if info.len() != size {
        return false;
    }
    let Some(sha256) = sha256 else {
        return true;
    };
    let marker = marker_line(sha256, &info);
    if marker.is_some() && fs::read_to_string(marker_path(path)).ok() == marker {
        return true;
    }
    let mut hasher = Sha256::new();
    let verified = fs::File::open(path)
        .and_then(|mut file| std::io::copy(&mut file, &mut hasher))
        .is_ok_and(|_| hex(&hasher.finalize()) == sha256);
    if verified {
        write_marker(path, sha256);
    }
    verified
}

fn marker_path(path: &Path) -> PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(".verified");
    PathBuf::from(name)
}

/// `<sha256> <size> <mtime ns>`; `None` when the file system reports no modification time.
fn marker_line(sha256: &str, info: &fs::Metadata) -> Option<String> {
    let mtime = info.modified().ok()?.duration_since(UNIX_EPOCH).ok()?;
    Some(format!("{sha256} {} {}\n", info.len(), mtime.as_nanos()))
}

/// Best effort: without a marker the next run hashes the file again.
fn write_marker(path: &Path, sha256: &str) {
    if let Some(line) = fs::metadata(path)
        .ok()
        .and_then(|info| marker_line(sha256, &info))
    {
        let _ = fs::write(marker_path(path), line);
    }
}

fn download(
    mut reader: impl Read,
    path: &Path,
    expected: Option<u64>,
    sha256: Option<&str>,
    resume: bool,
) -> Result<String> {
    let part = sidecar(path, ".part");
    let mut hasher = Sha256::new();
    let mut total = 0;
    if resume {
        total = std::io::copy(&mut fs::File::open(&part)?, &mut hasher)?;
    }
    let mut file = fs::File::options()
        .create(true)
        .write(true)
        .truncate(!resume)
        .append(resume)
        .open(&part)?;
    let mut buffer = [0u8; 64 * 1024];
    let mut last_report = total;
    loop {
        // Keep the stable partial on transport failure or premature EOF for the next run.
        let count = reader.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        file.write_all(&buffer[..count])?;
        hasher.update(&buffer[..count]);
        total += count as u64;
        if expected.is_some_and(|size| total > size) {
            drop(file);
            fs::remove_file(&part)?;
            bail!("model download exceeds expected size");
        }
        if total - last_report >= 16 * 1024 * 1024 {
            crate::progress::line!(
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
    ensure!(total > 0, "model download is empty");
    let actual = hex(&hasher.finalize());
    if let Some(sha256) = sha256
        && actual != sha256
    {
        drop(file);
        fs::remove_file(&part)?;
        bail!("model download has SHA-256 {actual}, expected {sha256}");
    }
    file.sync_all()?;
    drop(file);
    fs::rename(&part, path)?;
    if let Some(sha256) = sha256 {
        write_marker(path, sha256);
    }
    crate::progress::line!("Model cached: {} ({total} bytes)", path.display());
    Ok(actual)
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
/// Run yt-dlp with the arguments that `args` adds. Through uvx, the cached yt-dlp runs first,
/// because `@latest` checks the package index on every call (about 0.5 s each). Only a failed
/// call retries with `@latest`, which covers a cached yt-dlp that YouTube has broken.
fn yt_dlp(args: impl Fn(&mut Command)) -> Result<Output> {
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
                crate::progress::line!(
                    "Using yt-dlp command: {}",
                    if uvx { "uvx yt-dlp" } else { "yt-dlp" }
                );
            }
            selected
        })
        .context("cannot run yt-dlp: neither uvx nor yt-dlp is on PATH; install one of them")?;
    let run = |package: Option<&str>| {
        let mut command = Command::new(package.map_or("yt-dlp", |_| "uvx"));
        command.args(package);
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
        args(&mut command);
        command_output(&mut command, "yt-dlp")
    };
    if !uvx {
        return run(None);
    }
    run(Some("yt-dlp")).or_else(|error| {
        crate::progress::line!("yt-dlp failed; retrying with uvx yt-dlp@latest: {error:#}");
        run(Some("yt-dlp@latest"))
    })
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

/// The duration a local file reports for audio stream `stream`, the stream that
/// `audio::decode_blocks` decodes, for the truncation check. None when unknown.
fn probe_duration(path: &Path, stream: usize) -> Option<f64> {
    let output = command_output(
        Command::new("ffprobe")
            .args(["-v", "error", "-select_streams", &format!("a:{stream}")])
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
        title: format!("{author} (@{handle}): {}", x_title_text(text)),
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
        description: None,
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
// The limit applies to post text, not the author prefix. Keep an oversized first word intact
// rather than producing an empty title or splitting a word.
const X_TITLE_CHARS: usize = 80;
fn x_title_text(text: &str) -> String {
    let mut title = String::new();
    let mut chars = 0;
    for word in text
        .split(['\r', '\n'])
        .next()
        .unwrap_or("")
        .split_whitespace()
    {
        let word_chars = word.chars().count();
        if !title.is_empty() {
            if chars + 1 + word_chars > X_TITLE_CHARS {
                break;
            }
            title.push(' ');
            chars += 1;
        }
        title.push_str(word);
        chars += word_chars;
    }
    title
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
    // A finite server script records real requests and never waits indefinitely on a failure.
    fn server(responses: Vec<&'static str>) -> (String, std::thread::JoinHandle<Vec<String>>) {
        use std::net::TcpListener;
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/model.gguf", listener.local_addr().unwrap());
        listener.set_nonblocking(true).unwrap();
        let thread = std::thread::spawn(move || {
            let mut requests = Vec::new();
            for response in responses {
                let deadline = std::time::Instant::now() + Duration::from_secs(10);
                let mut stream = loop {
                    match listener.accept() {
                        Ok((stream, _)) => break stream,
                        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                            if std::time::Instant::now() > deadline {
                                return requests;
                            }
                            std::thread::sleep(Duration::from_millis(10));
                        }
                        Err(error) => panic!("{error}"),
                    }
                };
                // macOS inherits the listener's nonblocking mode on accepted sockets.
                stream.set_nonblocking(false).unwrap();
                stream
                    .set_read_timeout(Some(Duration::from_secs(3)))
                    .unwrap();
                let mut request = Vec::new();
                while !request.ends_with(b"\r\n\r\n") {
                    let mut byte = [0];
                    if stream.read(&mut byte).unwrap() == 0 {
                        break;
                    }
                    request.push(byte[0]);
                }
                requests.push(String::from_utf8(request).unwrap());
                stream.write_all(response.as_bytes()).unwrap();
            }
            requests
        });
        (url, thread)
    }

    #[test]
    fn interrupted_model_resumes_on_the_next_run() {
        let dir = temp_dir("http-resume");
        let (url, server) = server(vec![
            "HTTP/1.1 200 OK\r\nContent-Length: 11\r\nConnection: close\r\n\r\n",
            "HTTP/1.1 200 OK\r\nContent-Length: 11\r\nETag: \"v1\"\r\nConnection: close\r\n\r\nmodel",
            "HTTP/1.1 200 OK\r\nContent-Length: 11\r\nConnection: close\r\n\r\n",
            "HTTP/1.1 206 Partial Content\r\nContent-Length: 6\r\nContent-Range: bytes 5-10/11\r\nETag: \"v1\"\r\nConnection: close\r\n\r\n bytes",
        ]);
        assert!(model_in_cache(&url, &dir).is_err());
        let partials: Vec<_> = fs::read_dir(&dir)
            .unwrap()
            .flatten()
            .filter(|e| e.file_name().to_string_lossy().ends_with(".part"))
            .collect();
        assert_eq!(partials.len(), 1, "one resumable partial remains");
        assert!(
            partials[0]
                .file_name()
                .to_string_lossy()
                .ends_with("model.gguf.part")
        );
        assert_eq!(fs::read(partials[0].path()).unwrap(), b"model");
        let path = model_in_cache(&url, &dir).unwrap();
        assert_eq!(fs::read(path).unwrap(), b"model bytes");
        let requests = server.join().unwrap();
        assert!(
            requests
                .last()
                .unwrap()
                .to_ascii_lowercase()
                .contains("range: bytes=5-")
        );
        assert!(
            requests
                .last()
                .unwrap()
                .to_ascii_lowercase()
                .contains("if-range: \"v1\"")
        );
        fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn interrupted_model_resumes_without_a_server_validator() {
        let dir = temp_dir("http-resume-no-validator");
        let (url, server) = server(vec![
            "HTTP/1.1 405 Method Not Allowed\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
            "HTTP/1.1 200 OK\r\nContent-Length: 11\r\nConnection: close\r\n\r\nmodel",
            "HTTP/1.1 405 Method Not Allowed\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
            "HTTP/1.1 206 Partial Content\r\nContent-Length: 6\r\nContent-Range: bytes 5-10/11\r\nConnection: close\r\n\r\n bytes",
        ]);
        assert!(model_in_cache(&url, &dir).is_err());
        let result = model_in_cache(&url, &dir);
        let requests = server.join().unwrap();
        let path = result.unwrap_or_else(|error| panic!("{error:#}; requests: {requests:#?}"));
        assert_eq!(fs::read(path).unwrap(), b"model bytes");
        assert!(
            requests
                .last()
                .unwrap()
                .to_ascii_lowercase()
                .contains("range: bytes=5-")
        );
        fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn a_server_ignoring_range_replaces_the_partial_instead_of_appending() {
        let dir = temp_dir("http-range-ignored");
        let (url, server) = server(vec![
            "HTTP/1.1 200 OK\r\nContent-Length: 11\r\nConnection: close\r\n\r\n",
            "HTTP/1.1 200 OK\r\nContent-Length: 11\r\nETag: \"v1\"\r\nConnection: close\r\n\r\nmodel",
            "HTTP/1.1 200 OK\r\nContent-Length: 11\r\nConnection: close\r\n\r\n",
            "HTTP/1.1 200 OK\r\nContent-Length: 11\r\nETag: \"v2\"\r\nConnection: close\r\n\r\nother bytes",
        ]);
        assert!(model_in_cache(&url, &dir).is_err());
        let path = model_in_cache(&url, &dir).unwrap();
        assert_eq!(fs::read(path).unwrap(), b"other bytes");
        assert!(
            server
                .join()
                .unwrap()
                .last()
                .unwrap()
                .to_ascii_lowercase()
                .contains("range: bytes=5-")
        );
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn invalid_content_range_cannot_publish_a_model() {
        let dir = temp_dir("http-invalid-range");
        let (url, server) = server(vec![
            "HTTP/1.1 200 OK\r\nContent-Length: 11\r\nConnection: close\r\n\r\n",
            "HTTP/1.1 200 OK\r\nContent-Length: 11\r\nETag: \"v1\"\r\nConnection: close\r\n\r\nmodel",
            "HTTP/1.1 200 OK\r\nContent-Length: 11\r\nConnection: close\r\n\r\n",
            "HTTP/1.1 206 Partial Content\r\nContent-Length: 6\r\nContent-Range: bytes 4-9/11\r\nConnection: close\r\n\r\n bytes",
        ]);
        assert!(model_in_cache(&url, &dir).is_err());
        let error = model_in_cache(&url, &dir).unwrap_err();
        assert!(error.to_string().contains("Content-Range"), "{error}");
        server.join().unwrap();
        let partial = fs::read_dir(&dir)
            .unwrap()
            .flatten()
            .find(|e| e.file_name().to_string_lossy().ends_with(".part"))
            .unwrap();
        assert_eq!(fs::read(partial.path()).unwrap(), b"model");
        assert!(
            !fs::read_dir(&dir)
                .unwrap()
                .flatten()
                .any(|e| e.path().extension().is_some_and(|s| s == "gguf"))
        );
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn unsatisfiable_range_restarts_the_download() {
        let dir = temp_dir("http-range-416");
        let (url, server) = server(vec![
            "HTTP/1.1 200 OK\r\nContent-Length: 11\r\nConnection: close\r\n\r\n",
            "HTTP/1.1 200 OK\r\nContent-Length: 11\r\nETag: \"v1\"\r\nConnection: close\r\n\r\nmodel",
            "HTTP/1.1 200 OK\r\nContent-Length: 11\r\nConnection: close\r\n\r\n",
            "HTTP/1.1 416 Range Not Satisfiable\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
            "HTTP/1.1 200 OK\r\nContent-Length: 11\r\nConnection: close\r\n\r\nmodel bytes",
        ]);
        assert!(model_in_cache(&url, &dir).is_err());
        let path = model_in_cache(&url, &dir).unwrap();
        assert_eq!(fs::read(path).unwrap(), b"model bytes");
        let requests = server.join().unwrap();
        assert!(requests[3].to_ascii_lowercase().contains("range: bytes=5-"));
        assert!(!requests[4].to_ascii_lowercase().contains("range:"));
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn simultaneous_model_requests_publish_one_completed_download() {
        let dir = temp_dir("http-lock");
        let (url, server) = server(vec![
            "HTTP/1.1 200 OK\r\nContent-Length: 11\r\nConnection: close\r\n\r\n",
            "HTTP/1.1 200 OK\r\nContent-Length: 11\r\nConnection: close\r\n\r\nmodel bytes",
        ]);
        let paths = std::thread::scope(|scope| {
            let a = scope.spawn(|| model_in_cache(&url, &dir).unwrap());
            let b = scope.spawn(|| model_in_cache(&url, &dir).unwrap());
            (a.join().unwrap(), b.join().unwrap())
        });
        assert_eq!(paths.0, paths.1);
        assert_eq!(fs::read(paths.0).unwrap(), b"model bytes");
        assert_eq!(server.join().unwrap().len(), 2, "only one HEAD and one GET");
        fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn a_damaged_completed_model_is_downloaded_again_not_trusted_by_size() {
        let dir = temp_dir("http-damaged-cache");
        let (url, server) = server(vec![
            "HTTP/1.1 200 OK\r\nContent-Length: 11\r\nConnection: close\r\n\r\n",
            "HTTP/1.1 200 OK\r\nContent-Length: 11\r\nConnection: close\r\n\r\nmodel bytes",
            "HTTP/1.1 200 OK\r\nContent-Length: 11\r\nConnection: close\r\n\r\n",
            "HTTP/1.1 200 OK\r\nContent-Length: 11\r\nConnection: close\r\n\r\nmodel bytes",
        ]);
        let path = model_in_cache(&url, &dir).unwrap();
        let mtime = fs::metadata(&path).unwrap().modified().unwrap();
        fs::write(&path, b"wrong bytes").unwrap();
        fs::File::options()
            .write(true)
            .open(&path)
            .unwrap()
            .set_modified(mtime + Duration::from_secs(5))
            .unwrap();
        assert_eq!(model_in_cache(&url, &dir).unwrap(), path);
        let requests = server.join().unwrap();
        assert_eq!(fs::read(path).unwrap(), b"model bytes");
        assert_eq!(
            requests.len(),
            4,
            "a hash failure must fetch, not trust HEAD's size"
        );
        fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn legacy_custom_cache_gets_a_completion_record_for_offline_reuse() {
        let dir = temp_dir("http-legacy-cache");
        let (url, server) = server(vec![
            "HTTP/1.1 200 OK\r\nContent-Length: 11\r\nConnection: close\r\n\r\n",
            "HTTP/1.1 200 OK\r\nContent-Length: 11\r\nConnection: close\r\n\r\nmodel bytes",
            "HTTP/1.1 200 OK\r\nContent-Length: 11\r\nConnection: close\r\n\r\n",
        ]);
        let path = model_in_cache(&url, &dir).unwrap();
        fs::remove_file(sidecar(&path, ".complete.json")).unwrap();
        assert_eq!(model_in_cache(&url, &dir).unwrap(), path);
        assert_eq!(
            server.join().unwrap().len(),
            3,
            "migration uses HEAD, not another download"
        );
        assert_eq!(model_in_cache(&url, &dir).unwrap(), path);
        assert_eq!(fs::read(path).unwrap(), b"model bytes");
        fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn completed_custom_model_is_reused_offline_without_head() {
        let dir = temp_dir("http-offline");
        let (url, server) = server(vec![
            "HTTP/1.1 405 Method Not Allowed\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
            "HTTP/1.1 200 OK\r\nContent-Length: 11\r\nConnection: close\r\n\r\nmodel bytes",
        ]);
        let path = model_in_cache(&url, &dir).unwrap();
        assert_eq!(server.join().unwrap().len(), 2);
        assert_eq!(model_in_cache(&url, &dir).unwrap(), path);
        assert_eq!(fs::read(path).unwrap(), b"model bytes");
        fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn proxy_client_child() {
        let Ok(cache) = std::env::var("SCRIBE_PROXY_TEST_CACHE") else {
            return;
        };
        let url = std::env::var("SCRIBE_PROXY_TEST_URL").unwrap();
        assert!(model_in_cache(&url, Path::new(&cache)).unwrap().is_file());
        if std::env::var_os("SCRIBE_PROXY_TEST_API").is_some() {
            match x_media("https://x.com/example/status/1") {
                Err(XError::Fallback(error)) => {
                    assert!(error.to_string().contains("invalid X API JSON"), "{error}")
                }
                _ => panic!("expected the TLS server's HTML, not an API response"),
            }
        }
    }

    fn proxy_child(cache: &Path, proxy: &str, url: &str) -> Command {
        let mut child = Command::new(std::env::current_exe().unwrap());
        child
            .args(["--exact", "fetch::tests::proxy_client_child", "--nocapture"])
            .env("SCRIBE_PROXY_TEST_CACHE", cache)
            .env("SCRIBE_PROXY_TEST_URL", url)
            .env("HTTPS_PROXY", proxy);
        // Isolate environment changes in a subprocess, not Rust's parallel test process.
        for name in [
            "ALL_PROXY",
            "all_proxy",
            "https_proxy",
            "HTTP_PROXY",
            "http_proxy",
            "NO_PROXY",
            "no_proxy",
        ] {
            child.env_remove(name);
        }
        child
    }

    #[test]
    fn https_proxy_routes_model_requests_through_loopback_proxy() {
        let dir = temp_dir("proxy");
        let (proxy_url, server) = server(vec![
            "HTTP/1.1 405 Method Not Allowed\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
            "HTTP/1.1 200 OK\r\nContent-Length: 11\r\nConnection: close\r\n\r\nmodel bytes",
        ]);
        let output = proxy_child(
            &dir,
            proxy_url.trim_end_matches("/model.gguf"),
            "http://model.invalid/model.gguf",
        )
        .output()
        .unwrap();
        let requests = server.join().unwrap();
        assert!(
            output.status.success(),
            "{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(requests.len(), 2);
        assert!(requests[1].starts_with("GET http://model.invalid/model.gguf HTTP/1.1"));
        fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn model_start_sweeps_stale_partials_and_preserves_recent_partials() {
        let dir = temp_dir("stale-parts");
        let stale = dir.join("old.gguf.123-456.part");
        fs::write(&stale, b"old orphan").unwrap();
        fs::File::options()
            .write(true)
            .open(&stale)
            .unwrap()
            .set_modified(SystemTime::now() - Duration::from_secs(8 * 24 * 60 * 60))
            .unwrap();
        let stale_stable = dir.join("stale.gguf.part");
        fs::write(&stale_stable, b"stale partial").unwrap();
        fs::write(sidecar(&stale_stable, ".json"), b"{}").unwrap();
        fs::File::options()
            .write(true)
            .open(&stale_stable)
            .unwrap()
            .set_modified(SystemTime::now() - Duration::from_secs(8 * 24 * 60 * 60))
            .unwrap();
        let recent = dir.join("recent.gguf.part");
        fs::write(&recent, b"resumable").unwrap();
        let orphan = dir.join("missing.gguf.part.json");
        fs::write(&orphan, b"{}").unwrap();
        let (url, server) = server(vec![
            "HTTP/1.1 405 Method Not Allowed\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
            "HTTP/1.1 200 OK\r\nContent-Length: 11\r\nConnection: close\r\n\r\nmodel bytes",
        ]);
        model_in_cache(&url, &dir).unwrap();
        server.join().unwrap();
        assert!(
            !stale.exists(),
            "the startup sweep removes stale legacy partials"
        );
        assert!(!orphan.exists(), "orphan resume records are removed");
        assert!(!stale_stable.exists());
        assert!(!sidecar(&stale_stable, ".json").exists());
        assert_eq!(fs::read(recent).unwrap(), b"resumable");
        fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn interrupted_media_workspace_reuses_partial_bytes_for_the_same_source_id() {
        let cache = temp_dir("media-resume");
        let first = Workspace::in_cache(&cache, "youtube:a", SystemTime::now()).unwrap();
        let path = first.path.clone();
        fs::write(path.join("media.webm.part"), "download prefix").unwrap();
        drop(first); // A failed run must retain the checkpoint, just like a killed run.
        let next = Workspace::in_cache(&cache, "youtube:a", SystemTime::now()).unwrap();
        assert_eq!(next.path, path);
        assert_eq!(
            fs::read_to_string(next.path.join("media.webm.part")).unwrap(),
            "download prefix"
        );
        next.complete();
        assert!(!path.exists());
        fs::remove_dir_all(cache).unwrap();
    }

    #[test]
    fn media_workspace_uses_the_platform_cache_and_separates_source_ids() {
        let id = format!("scribe-workspace-test:{}", std::process::id());
        let first = Workspace::new(&id).unwrap();
        assert!(
            first
                .path
                .starts_with(dirs::cache_dir().unwrap().join("scribe/media"))
        );
        assert!(
            Workspace::new(&id).is_err(),
            "concurrent downloads of one source must not overwrite each other"
        );
        let other = Workspace::new(&format!("{id}:other")).unwrap();
        assert_ne!(first.path, other.path);
        first.complete();
        other.complete();
    }

    #[test]
    fn stale_media_is_swept_but_recent_active_and_unrelated_files_are_kept() {
        let cache = temp_dir("media-sweep");
        let now = SystemTime::now();
        let old = now - MEDIA_STALE_AFTER - Duration::from_secs(1);
        let stale = Workspace::in_cache(&cache, "youtube:stale", old).unwrap();
        let stale_path = stale.path.clone();
        fs::write(stale.path.join("media.mp4.part"), "stale prefix").unwrap();
        drop(stale);
        let active = Workspace::in_cache(&cache, "youtube:active", old).unwrap();
        let active_path = active.path.clone();
        fs::write(active.path.join("media.mp4.part"), "active prefix").unwrap();
        let recent = Workspace::in_cache(&cache, "youtube:recent", now).unwrap();
        let recent_path = recent.path.clone();
        drop(recent);
        fs::create_dir(cache.join("unrelated")).unwrap();
        Workspace::sweep(&cache, now).unwrap();
        assert!(!stale_path.exists());
        assert_eq!(
            fs::read_to_string(active_path.join("media.mp4.part")).unwrap(),
            "active prefix"
        );
        assert!(recent_path.exists());
        assert!(cache.join("unrelated").exists());
        drop(active);
        Workspace::sweep(&cache, now).unwrap();
        assert!(!active_path.exists());
        fs::remove_dir_all(cache).unwrap();
    }

    #[test]
    fn media_arguments_resume_and_prefer_audio_then_the_smallest_audio_bearing_video() {
        let mut command = Command::new("yt-dlp");
        media_download_args(
            &mut command,
            Path::new("/disk/cache/source"),
            Path::new("/disk/cache/source/info.json"),
        );
        let args: Vec<_> = command
            .get_args()
            .map(|s| s.to_string_lossy().into_owned())
            .collect();
        assert!(args.iter().any(|arg| arg == "--continue"), "{args:?}");
        assert!(
            args.iter()
                .any(|arg| arg == "--abort-on-unavailable-fragments"),
            "{args:?}"
        );
        assert!(
            args.windows(2)
                .any(|pair| pair == ["-f", "bestaudio/worst[acodec!=?none]"]),
            "{args:?}"
        );
        assert!(
            args.windows(2)
                .any(|pair| pair == ["-o", "/disk/cache/source/media.%(ext)s"]),
            "{args:?}"
        );
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
        assert!(download(&data[..5], &path, Some(11), None, false).is_err());
        assert_eq!(fs::read(sidecar(&path, ".part")).unwrap(), &data[..5]);
        assert!(download(&data[..], &path, Some(11), Some(&"0".repeat(64)), false).is_err());
        assert!(!path.exists());
        assert!(
            !sidecar(&path, ".part").exists(),
            "a bad hash cannot be resumed"
        );
        download(&data[..], &path, Some(11), Some(&digest), false).unwrap();
        assert_eq!(fs::read(&path).unwrap(), data);
        assert!(
            fs::read_dir(&dir).unwrap().all(|entry| !entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .ends_with(".part")),
            "no .part file is left"
        );
        fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn cached_model_needs_matching_size_and_sha256() {
        let dir = temp_dir("cached");
        let path = dir.join("model.gguf");
        let data = b"model bytes";
        let digest = sha256_hex(data);
        assert!(!is_cached(&path, Some(11), Some(&digest)), "missing file");
        fs::write(&path, data).unwrap();
        assert!(is_cached(&path, Some(11), Some(&digest)));
        assert!(is_cached(&path, Some(11), None));
        assert!(!is_cached(&path, Some(12), None), "size differs");
        assert!(!is_cached(&path, None, None), "size unknown");
        let mtime = fs::metadata(&path).unwrap().modified().unwrap();
        fs::write(&path, b"other bytes").unwrap();
        // Coarse file-system clocks can give the rewrite the same mtime; move it explicitly so
        // the verified marker no longer matches and the hash runs.
        fs::File::options()
            .write(true)
            .open(&path)
            .unwrap()
            .set_modified(mtime + Duration::from_secs(5))
            .unwrap();
        assert!(
            !is_cached(&path, Some(11), Some(&digest)),
            "same size, other content"
        );
        fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn verified_marker_skips_rehash_until_size_or_mtime_changes() {
        let dir = temp_dir("marker");
        let path = dir.join("model.gguf");
        let data = b"model bytes";
        let digest = sha256_hex(data);
        fs::write(&path, data).unwrap();
        assert!(is_cached(&path, Some(11), Some(&digest)));
        assert!(
            marker_path(&path).exists(),
            "a verified hash writes a marker"
        );
        let mtime = fs::metadata(&path).unwrap().modified().unwrap();

        // Same size and restored mtime: the marker is trusted (the documented limit).
        fs::write(&path, b"other bytes").unwrap();
        fs::File::options()
            .write(true)
            .open(&path)
            .unwrap()
            .set_modified(mtime)
            .unwrap();
        assert!(is_cached(&path, Some(11), Some(&digest)));

        // A new mtime invalidates the marker, so the hash runs and rejects the file.
        fs::File::options()
            .write(true)
            .open(&path)
            .unwrap()
            .set_modified(mtime + Duration::from_secs(5))
            .unwrap();
        assert!(!is_cached(&path, Some(11), Some(&digest)));
        fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn download_writes_a_verified_marker() {
        let dir = temp_dir("download-marker");
        let path = dir.join("model.gguf");
        let data = b"model bytes";
        let digest = sha256_hex(data);
        download(&data[..], &path, Some(11), Some(&digest), false).unwrap();
        let info = fs::metadata(&path).unwrap();
        assert_eq!(
            fs::read_to_string(marker_path(&path)).ok(),
            marker_line(&digest, &info)
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
    fn x_title_ends_at_a_whole_word_and_sources_keep_full_text() {
        let mut raw: serde_json::Value =
            serde_json::from_str(include_str!("../tests/fixtures/x.json")).unwrap();
        let text = "here's how i shipped 2,500 PRs last month to production this was originally supposed to be a talk";
        raw["posts"][0]["text"] = text.into();
        let meta = x_metadata(&raw["posts"][0], &raw).unwrap();
        assert_eq!(
            meta.title,
            "Pi (@pidotdev): here's how i shipped 2,500 PRs last month to production this was originally"
        );
        assert!(crate::sources::render(&meta.sources, &[]).contains(&format!("> {text}\n")));
    }
    #[test]
    fn x_titles_preserve_character_and_line_boundaries() {
        let mut raw: serde_json::Value =
            serde_json::from_str(include_str!("../tests/fixtures/x.json")).unwrap();
        for (text, expected) in [
            (
                "  First\t line  \r\nSecond line".to_owned(),
                "First line".to_owned(),
            ),
            (format!("{} next", "🌞".repeat(80)), "🌞".repeat(80)),
            (format!("{} next", "a".repeat(81)), "a".repeat(81)),
        ] {
            raw["posts"][0]["text"] = text.clone().into();
            let meta = x_metadata(&raw["posts"][0], &raw).unwrap();
            assert_eq!(meta.title, format!("Pi (@pidotdev): {expected}"));
            let Sources::X {
                text: full_text, ..
            } = meta.sources
            else {
                panic!("expected X sources")
            };
            assert_eq!(full_text, text);
        }
    }
    #[test]
    fn field_post_title_stops_at_first_line_without_losing_sources() {
        let raw: serde_json::Value =
            serde_json::from_str(include_str!("../tests/fixtures/x-poteto.json")).unwrap();
        let meta = x_metadata(&raw["posts"][0], &raw).unwrap();
        assert_eq!(
            meta.title,
            "lauren (@poteto): here's how i shipped 2,500 PRs last month to production"
        );
        let markdown = crate::sources::render(&meta.sources, &[]);
        assert!(
            markdown.contains("> this was originally supposed to be for Cursor Compile in London.")
        );
        assert!(markdown.contains("Grok @Bot Galaxy"));
        assert!(markdown.contains("- <https://x.com/Bot>"));
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
            "Pi (@pidotdev): Welcome to our Monday Meditations! 🌞"
        );
    }
    #[test]
    fn a_local_file_reports_the_duration_of_the_chosen_audio_stream() {
        let dir = temp_dir("stream-duration");
        let path = dir.join("two.mp4");
        let status = Command::new("ffmpeg")
            .args(["-nostdin", "-v", "error", "-y", "-f", "lavfi"])
            .args(["-i", "sine=f=440:d=2:r=16000", "-f", "lavfi"])
            .args(["-i", "anullsrc=cl=stereo:r=16000:d=4"])
            .args(["-map", "0", "-map", "1", "-c:a", "aac"])
            .arg(&path)
            .status()
            .unwrap();
        assert!(status.success());
        let duration = |stream| {
            media(path.to_str().unwrap(), stream)
                .unwrap()
                .0
                .duration
                .unwrap()
        };
        // AAC adds up to one 1024-sample frame of padding.
        assert!((2.0..2.1).contains(&duration(0)), "{}", duration(0));
        assert!((4.0..4.1).contains(&duration(1)), "{}", duration(1));
        fs::remove_dir_all(dir).unwrap();
    }
    /// Serve the files of `dir` over HTTP on a local port, answering 404 for `missing`.
    fn serve(dir: PathBuf, missing: &'static str) -> String {
        use std::io::{BufRead, BufReader};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { continue };
                let mut request = String::new();
                if BufReader::new(&stream).read_line(&mut request).is_err() {
                    continue;
                }
                let name = request.split_whitespace().nth(1).unwrap_or("/");
                let name = name.trim_start_matches('/');
                let body = (name != missing)
                    .then(|| fs::read(dir.join(name)).ok())
                    .flatten();
                let head = match &body {
                    Some(body) => format!(
                        "HTTP/1.0 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                        body.len()
                    ),
                    None => {
                        "HTTP/1.0 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                            .into()
                    }
                };
                let _ = stream.write_all(head.as_bytes());
                if !request.starts_with("HEAD")
                    && let Some(body) = body
                {
                    let _ = stream.write_all(&body);
                }
            }
        });
        base
    }
    #[test]
    fn a_missing_middle_fragment_fails_the_download() {
        let dir = temp_dir("fragments");
        // A 3 s HLS stream in 1 s fragments: seg0.ts to seg3.ts.
        let status = Command::new("ffmpeg")
            .args(["-nostdin", "-v", "error", "-y", "-f", "lavfi"])
            .args(["-i", "sine=f=440:d=3:r=16000", "-c:a", "aac", "-f", "hls"])
            .args([
                "-hls_time",
                "1",
                "-hls_list_size",
                "0",
                "-hls_segment_filename",
            ])
            .arg(dir.join("seg%d.ts"))
            .arg(dir.join("list.m3u8"))
            .status()
            .unwrap();
        assert!(status.success());
        let download = |base: String| {
            let workspace = temp_dir(&format!("fragments-{}", base.rsplit(':').next().unwrap()));
            let (_, pending) = media(&format!("{base}/list.m3u8"), 0)?;
            let result = resolve(pending, &workspace);
            fs::remove_dir_all(workspace).unwrap();
            result
        };
        // The complete stream downloads; the same stream without seg1.ts fails.
        assert!(download(serve(dir.clone(), "")).is_ok());
        let error = download(serve(dir.clone(), "seg1.ts"))
            .map(|_| ())
            .unwrap_err();
        assert!(format!("{error:#}").contains("fragment"), "{error:#}");
        fs::remove_dir_all(dir).unwrap();
    }
}
