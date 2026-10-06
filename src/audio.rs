use anyhow::{Context, Result, ensure};
use std::{io::Read, ops::Range};
pub const SAMPLE_RATE: usize = 16_000;
/// transcribe-cpp rejects a chunk shorter than two mel frames (about 16 ms). One second keeps a
/// wide margin and stays far below any useful target length.
const MIN_CHUNK: usize = SAMPLE_RATE;
/// ffmpeg aborts a network read that makes no progress for this long (microseconds).
const NETWORK_TIMEOUT_MICROS: &str = "30000000";
/// Decoded audio may be this much shorter than the reported duration: the larger of
/// `SHORT_SECS` and `SHORT_FRACTION` of it. The slack covers whole-second durations from yt-dlp,
/// encoder padding and an audio track that ends before the video track. A dropped connection
/// or a truncated file loses far more.
const SHORT_SECS: f64 = 5.0;
const SHORT_FRACTION: f64 = 0.01;
/// Split the whole audio into chunks of at most `seconds`, cut at a quiet point.
#[cfg(test)]
pub fn chunks(pcm: &[f32], seconds: std::num::NonZeroU32) -> Vec<Range<usize>> {
    chunks_and_hard_cuts(pcm, seconds).0
}
/// `chunks`, and the count of hard cuts among them.
#[cfg(test)]
pub fn chunks_and_hard_cuts(
    pcm: &[f32],
    seconds: std::num::NonZeroU32,
) -> (Vec<Range<usize>>, usize) {
    let mut chunker = Chunker::new(seconds);
    let mut ranges = chunker.ready(pcm);
    let (rest, hard_cuts) = chunker.finish(pcm);
    ranges.extend(rest);
    (ranges, hard_cuts)
}

/// Cuts chunks from audio that is still growing, with the same cut points as `chunks` on the
/// whole audio. A cut depends only on the audio up to the chunk's hard end, so a chunk is final
/// once more audio than that exists. It is released once `MIN_CHUNK` samples follow it: only the
/// last chunk can be shorter, and it joins the chunk just before it.
pub struct Chunker {
    target: usize,
    /// Start of the first chunk whose end is still unknown.
    next: usize,
    /// Final chunks that are not released yet, each with true when it ends at a hard cut.
    held: std::collections::VecDeque<(Range<usize>, bool)>,
    /// Hard cuts among the released chunks.
    hard_cuts: usize,
}
impl Chunker {
    pub fn new(seconds: std::num::NonZeroU32) -> Self {
        Self {
            target: seconds.get() as usize * SAMPLE_RATE,
            next: 0,
            held: Default::default(),
            hard_cuts: 0,
        }
    }
    /// Cut the chunk that starts at `self.next` and hold it. A hard cut is a cut inside the
    /// audio at the length limit: `cut` found no quiet point. A quiet cut always lies at least
    /// half a window before the limit, so the two cannot be confused.
    fn hold_next(&mut self, pcm: &[f32]) {
        let end = cut(pcm, self.next, self.target);
        let hard = end < pcm.len() && end == self.next + self.target;
        self.held.push_back((self.next..end, hard));
        self.next = end;
    }
    /// Chunks that no later audio can change. `pcm` is the audio so far; it may only grow.
    pub fn ready(&mut self, pcm: &[f32]) -> Vec<Range<usize>> {
        while self.next + self.target < pcm.len() {
            self.hold_next(pcm);
        }
        let mut ranges = Vec::new();
        while let Some((first, _)) = self.held.front()
            && first.end + MIN_CHUNK <= pcm.len()
        {
            let (range, hard) = self.held.pop_front().expect("a held chunk");
            self.hard_cuts += usize::from(hard);
            ranges.push(range);
        }
        ranges
    }
    /// The remaining chunks once `pcm` holds the whole audio, and the count of hard cuts over
    /// all chunks.
    pub fn finish(mut self, pcm: &[f32]) -> (Vec<Range<usize>>, usize) {
        while self.next < pcm.len() {
            self.hold_next(pcm);
        }
        // The engine fails on a tiny chunk, so a short tail joins the previous chunk, and the
        // cut between them disappears. The release rule in `ready` keeps that previous chunk here.
        let mut held = Vec::from(self.held);
        if held.len() > 1 && held.last().is_some_and(|(last, _)| last.len() < MIN_CHUNK) {
            let (tail, hard) = held.pop().expect("a last chunk");
            let previous = held.last_mut().expect("a previous chunk");
            *previous = (previous.0.start..tail.end, hard);
        }
        let hard_cuts = self.hard_cuts + held.iter().filter(|(_, hard)| *hard).count();
        (
            held.into_iter().map(|(range, _)| range).collect(),
            hard_cuts,
        )
    }
}

/// The end of the chunk that starts at `start`: the quietest 30 ms window in the last ten
/// seconds before `start + target`, or that hard end. Reads only `pcm[..start + target]`.
fn cut(pcm: &[f32], start: usize, target: usize) -> usize {
    let window = SAMPLE_RATE * 30 / 1000;
    let hard_end = (start + target).min(pcm.len());
    // Search at most the last ten seconds, but never cut a chunk shorter than MIN_CHUNK.
    let search = hard_end
        .saturating_sub(10 * SAMPLE_RATE)
        .max(start + MIN_CHUNK);
    if hard_end == pcm.len() || search + window > hard_end {
        return hard_end;
    }
    let mut quietest = f64::INFINITY;
    let mut loudest: f64 = 0.0;
    let mut cut = hard_end;
    // A rolling sum checks every possible 30 ms window in linear time.
    let mut energy = pcm[search..search + window]
        .iter()
        .map(|&x| f64::from(x).powi(2))
        .sum::<f64>();
    for offset in search..=hard_end - window {
        if offset > search {
            energy +=
                f64::from(pcm[offset + window - 1]).powi(2) - f64::from(pcm[offset - 1]).powi(2);
        }
        let energy = energy.max(0.0) / window as f64;
        loudest = loudest.max(energy);
        if energy <= quietest {
            quietest = energy;
            cut = offset + window / 2;
        }
    }
    // A flat signal has no useful quiet boundary. Require a 6 dB RMS drop.
    if quietest < loudest * 0.25 {
        cut
    } else {
        hard_end
    }
}
/// Decode audio stream `stream` of the input to 16 kHz mono f32 samples, with the checks of
/// `decode_blocks`.
#[cfg(test)]
pub fn decode(
    input: &std::ffi::OsStr,
    network: bool,
    stream: usize,
    expected: Option<f64>,
) -> Result<Vec<f32>> {
    let mut pcm =
        Vec::with_capacity(expected.map_or(0, |secs| (secs * SAMPLE_RATE as f64) as usize));
    decode_blocks(input, network, stream, expected, |block| {
        pcm.extend_from_slice(&block);
        true
    })?;
    Ok(pcm)
}

/// Decode audio stream `stream` of the input, counted from 0 among its audio streams, to 16 kHz
/// mono f32 samples and pass them to `sink` in blocks while ffmpeg runs. `sink` returns false
/// to stop ffmpeg. Then check the whole audio and return its sample count:
/// - The audio must be about as long as `expected`, the duration the source reported.
/// - ffmpeg exits 0 on a truncated file or a dropped connection and only reports it on stderr.
///   So any ffmpeg error output fails a network stream or a source without a duration. A local
///   file with a duration keeps a corrupt frame as a warning: the duration check still catches
///   truncation.
/// - A near-silent mix whose first channel is loud means the channels cancel out (phase-inverted
///   stereo). That fails instead of producing an empty transcript.
///
/// `network` adds a stall timeout and allows only HTTPS for third-party media URLs.
pub fn decode_blocks(
    input: &std::ffi::OsStr,
    network: bool,
    stream: usize,
    expected: Option<f64>,
    mut sink: impl FnMut(Vec<f32>) -> bool,
) -> Result<usize> {
    let expected = expected.filter(|secs| secs.is_finite() && *secs > 0.0);
    let strict = network || expected.is_none();
    let mut energy = 0.0;
    let (samples, errors) = ffmpeg(input, network, stream, strict, MIX, |block| {
        energy += squares(&block);
        sink(block)
    })
    .with_context(|| format!("cannot decode audio stream {stream}"))?;
    check_complete(samples, expected).map_err(|error| {
        if errors.is_empty() {
            error
        } else {
            anyhow::anyhow!("{error:#}; ffmpeg reported: {errors}")
        }
    })?;
    if let Some(first) = errors.lines().next() {
        eprintln!(
            "warning: ffmpeg reported {} error lines, but the decoded audio covers the reported \
             duration; a corrupt frame can lose a moment of audio. First: {first}",
            errors.lines().count()
        );
    }
    let mix = (energy / samples as f64).sqrt();
    if mix < QUIET_MIX_RMS {
        let mut energy = 0.0;
        let (samples, _) = ffmpeg(input, network, stream, strict, FIRST_CHANNEL, |block| {
            energy += squares(&block);
            true
        })?;
        let first = (energy / samples as f64).sqrt();
        let dbfs = |rms: f64| 20.0 * rms.log10();
        ensure!(
            first < QUIET_MIX_RMS || first <= CANCEL_RATIO * mix,
            "the channels of audio stream {stream} cancel out in the mono mix (phase-inverted \
             stereo): first channel {:.1} dBFS, mix {:.1} dBFS. Transcribe one channel instead, \
             for example: ffmpeg -i INPUT -map 0:a:{stream} -af pan=mono|c0=c0 channel.wav",
            dbfs(first),
            dbfs(mix)
        );
    }
    Ok(samples)
}

/// The ffmpeg options that mix all channels to mono, as ffmpeg's channel downmix does.
const MIX: &[&str] = &["-ac", "1"];
/// The ffmpeg options that keep only the first channel.
const FIRST_CHANNEL: &[&str] = &["-af", "pan=mono|c0=c0"];
/// A mix below this RMS (-50 dBFS) gets the cancellation check. Speech sits far above it, so
/// the extra decode pass is rare.
const QUIET_MIX_RMS: f64 = 0.003_16;
/// A first channel more than this many times the mix RMS (20 dB) means the channels cancel out.
/// A mix of uncorrelated channels, or of one silent channel, loses at most 6 dB.
const CANCEL_RATIO: f64 = 10.0;

fn squares(block: &[f32]) -> f64 {
    block.iter().map(|&x| f64::from(x).powi(2)).sum()
}

/// Run ffmpeg on audio stream `stream` with the channel options `channels`, and pass the
/// samples to `sink`. Returns the sample count and ffmpeg's error output, which is empty when
/// `strict`: then any error output fails.
fn ffmpeg(
    input: &std::ffi::OsStr,
    network: bool,
    stream: usize,
    strict: bool,
    channels: &[&str],
    mut sink: impl FnMut(Vec<f32>) -> bool,
) -> Result<(usize, String)> {
    let mut command = std::process::Command::new("ffmpeg");
    command.args(["-nostdin", "-hide_banner", "-loglevel", "error"]);
    if strict {
        command.arg("-xerror");
    }
    if network {
        command.args(["-rw_timeout", NETWORK_TIMEOUT_MICROS]);
        // These options also work on ffmpeg versions before 4.4. Do not reconnect at
        // normal EOF: an MP4 stream has a finite end, unlike a live broadcast.
        command.args([
            "-reconnect",
            "1",
            "-reconnect_streamed",
            "1",
            "-reconnect_delay_max",
            "10",
        ]);
        command.args(["-protocol_whitelist", "https,tls,tcp"]);
    }
    let sample_rate = SAMPLE_RATE.to_string();
    command
        .arg("-i")
        .arg(input)
        .args(["-map", &format!("0:a:{stream}"), "-f", "f32le"])
        .args(channels)
        .args(["-ar", &sample_rate, "-"])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    let mut child = command
        .spawn()
        .context("cannot run ffmpeg; install ffmpeg and ensure it is on PATH")?;
    let mut stderr = child.stderr.take().context("ffmpeg stderr")?;
    // Drain stderr on its own thread so a full pipe cannot block ffmpeg.
    let errors = std::thread::spawn(move || {
        let mut text = String::new();
        let _ = stderr.read_to_string(&mut text);
        text
    });
    // Convert while reading, so the raw bytes never exist next to the samples.
    let mut total = 0;
    let mut finite = true;
    let mut stopped = false;
    let read = (|| -> std::io::Result<usize> {
        let mut stdout = child.stdout.take().expect("piped stdout");
        let mut buffer = vec![0u8; 64 * 1024];
        let mut filled = 0;
        loop {
            let count = stdout.read(&mut buffer[filled..])?;
            if count == 0 {
                return Ok(filled);
            }
            filled += count;
            let whole = filled - filled % 4;
            let block: Vec<f32> = buffer[..whole]
                .chunks_exact(4)
                .map(|b| f32::from_le_bytes(b.try_into().expect("four-byte sample")))
                .collect();
            buffer.copy_within(whole..filled, 0);
            filled -= whole;
            total += block.len();
            finite &= block.iter().all(|v| v.is_finite());
            if !block.is_empty() && !sink(block) {
                stopped = true;
                return Ok(0);
            }
        }
    })();
    if stopped {
        let _ = child.kill();
    }
    let status = child.wait()?;
    let errors = errors.join().unwrap_or_default();
    ensure!(!stopped, "decoding stopped");
    let errors = errors.trim();
    ensure!(
        status.success() && (errors.is_empty() || !strict),
        "ffmpeg failed ({status}): {}",
        if errors.is_empty() {
            "no error output"
        } else {
            errors
        }
    );
    ensure!(read? == 0, "ffmpeg returned incomplete f32 samples");
    ensure!(total > 0, "media has no audio samples");
    ensure!(finite, "ffmpeg returned non-finite samples");
    Ok((total, errors.to_owned()))
}

/// Fail when the decoded audio is materially shorter than the duration the source reported.
pub fn check_complete(samples: usize, expected: Option<f64>) -> Result<()> {
    let decoded = samples as f64 / SAMPLE_RATE as f64;
    let Some(expected) = expected.filter(|secs| secs.is_finite() && *secs > 0.0) else {
        eprintln!("warning: source reports no duration; only ffmpeg errors can reveal truncation");
        return Ok(());
    };
    let slack = SHORT_SECS.max(expected * SHORT_FRACTION);
    ensure!(
        decoded + slack >= expected,
        "decoded audio is incomplete: {decoded:.1}s of {expected:.1}s"
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(unix)]
    #[test]
    fn reconnect_options_child() {
        if std::env::var_os("SCRIBE_FFMPEG_TEST_ARGS").is_none() {
            return;
        }
        let network = std::env::var_os("SCRIBE_FFMPEG_TEST_NETWORK").is_some();
        assert_eq!(
            decode_blocks(
                "https://stream.invalid/video.mp4".as_ref(),
                network,
                0,
                None,
                |_| { true }
            )
            .unwrap(),
            1
        );
    }

    #[cfg(unix)]
    #[test]
    fn network_decode_passes_reconnect_options_before_the_input() {
        use std::{fs, os::unix::fs::PermissionsExt, process::Command};
        let dir =
            std::env::temp_dir().join(format!("scribe-test-reconnect-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let ffmpeg = dir.join("ffmpeg");
        fs::write(&ffmpeg, "#!/bin/sh\nprintf '%s\\n' \"$@\" > \"$SCRIBE_FFMPEG_TEST_ARGS\"\nprintf '\\000\\000\\000\\000'\n").unwrap();
        fs::set_permissions(&ffmpeg, fs::Permissions::from_mode(0o755)).unwrap();
        for network in [true, false] {
            let args_path = dir.join(if network { "stream-args" } else { "local-args" });
            let mut child = Command::new(std::env::current_exe().unwrap());
            child
                .args([
                    "--exact",
                    "audio::tests::reconnect_options_child",
                    "--nocapture",
                ])
                .env("PATH", &dir)
                .env("SCRIBE_FFMPEG_TEST_ARGS", &args_path);
            if network {
                child.env("SCRIBE_FFMPEG_TEST_NETWORK", "1");
            } else {
                child.env_remove("SCRIBE_FFMPEG_TEST_NETWORK");
            }
            let output = child.output().unwrap();
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            let args = fs::read_to_string(args_path).unwrap();
            if network {
                assert!(
                    args.contains(
                        "-reconnect\n1\n-reconnect_streamed\n1\n-reconnect_delay_max\n10\n"
                    ),
                    "{args}"
                );
                assert!(args.find("-reconnect\n").unwrap() < args.find("-i\n").unwrap());
            } else {
                assert!(
                    !args.contains("-reconnect"),
                    "local files must not reconnect"
                );
            }
        }
        fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn chooses_silence_before_target() {
        let mut pcm = vec![0.5; 70 * SAMPLE_RATE];
        pcm[55 * SAMPLE_RATE..56 * SAMPLE_RATE].fill(0.0);
        let (cuts, hard_cuts) = chunks_and_hard_cuts(&pcm, 60.try_into().unwrap());
        assert_eq!(hard_cuts, 0);
        assert!((55 * SAMPLE_RATE..56 * SAMPLE_RATE).contains(&cuts[0].end));
        assert_eq!(cuts[1].start, cuts[0].end);
        assert_eq!(cuts[1].end, pcm.len());
    }
    #[test]
    fn finds_a_thirty_millisecond_gap_between_grid_positions() {
        let mut pcm = vec![0.5; 70 * SAMPLE_RATE];
        let gap_start = 55 * SAMPLE_RATE + 17;
        pcm[gap_start..gap_start + 480].fill(0.0);
        assert_eq!(chunks(&pcm, 60.try_into().unwrap())[0].end, gap_start + 240);
    }
    #[test]
    fn pure_tone_uses_hard_boundary_and_short_last_chunk() {
        let pcm: Vec<f32> = (0..125 * SAMPLE_RATE)
            .map(|i| (i as f32 * std::f32::consts::TAU * 440.0 / SAMPLE_RATE as f32).sin())
            .collect();
        // A pure tone has no quiet point, so both cuts are hard cuts.
        assert_eq!(
            chunks_and_hard_cuts(&pcm, 60.try_into().unwrap()),
            (
                vec![0..960_000, 960_000..1_920_000, 1_920_000..2_000_000],
                2
            )
        );
    }
    #[test]
    fn a_tail_shorter_than_the_engine_minimum_joins_the_previous_chunk() {
        // 30 s plus five samples of a tone: the hard boundary left a five-sample chunk.
        let pcm: Vec<f32> = (0..30 * SAMPLE_RATE + 5)
            .map(|i| (i as f32 * std::f32::consts::TAU * 440.0 / SAMPLE_RATE as f32).sin())
            .collect();
        // The tail join removes the only cut, a hard one.
        let (ranges, hard_cuts) = chunks_and_hard_cuts(&pcm, 30.try_into().unwrap());
        assert_eq!(ranges, vec![0..pcm.len()]);
        assert_eq!(hard_cuts, 0);
        // A silence just after a chunk start must not produce a chunk shorter than MIN_CHUNK.
        let mut pcm = vec![0.5; 3 * SAMPLE_RATE];
        pcm[..480].fill(0.0);
        assert!(
            chunks(&pcm, 1.try_into().unwrap())
                .iter()
                .all(|c| c.len() >= MIN_CHUNK)
        );
    }
    #[test]
    fn truncated_audio_fails_and_rounding_passes() {
        let secs = |s: f64| (s * SAMPLE_RATE as f64) as usize;
        // The Opus review reproduction: 355 s decoded from a 1,646 s X video.
        assert!(check_complete(secs(355.0), Some(1646.416)).is_err());
        assert!(check_complete(secs(1646.48), Some(1646.416)).is_ok());
        // yt-dlp reports whole seconds; 1 % of a long source or 5 s of a short one is slack.
        assert!(check_complete(secs(1630.0), Some(1646.0)).is_ok());
        assert!(check_complete(secs(1620.0), Some(1646.0)).is_err());
        assert!(check_complete(secs(56.0), Some(60.0)).is_ok());
        assert!(check_complete(secs(54.0), Some(60.0)).is_err());
        assert!(check_complete(secs(1.0), None).is_ok());
    }
    /// The whole-buffer chunking before `Chunker` existed (commit 7ee8f2a), kept as the oracle.
    fn reference_chunks(pcm: &[f32], seconds: std::num::NonZeroU32) -> Vec<Range<usize>> {
        let target = seconds.get() as usize * SAMPLE_RATE;
        let window = SAMPLE_RATE * 30 / 1000;
        let mut ranges = Vec::new();
        let mut start = 0;
        while start < pcm.len() {
            let hard_end = (start + target).min(pcm.len());
            let mut end = hard_end;
            let search = hard_end
                .saturating_sub(10 * SAMPLE_RATE)
                .max(start + MIN_CHUNK);
            if hard_end < pcm.len() && search + window <= hard_end {
                let mut quietest = f64::INFINITY;
                let mut loudest: f64 = 0.0;
                let mut cut = hard_end;
                let mut energy = pcm[search..search + window]
                    .iter()
                    .map(|&x| f64::from(x).powi(2))
                    .sum::<f64>();
                for offset in search..=hard_end - window {
                    if offset > search {
                        energy += f64::from(pcm[offset + window - 1]).powi(2)
                            - f64::from(pcm[offset - 1]).powi(2);
                    }
                    let energy = energy.max(0.0) / window as f64;
                    loudest = loudest.max(energy);
                    if energy <= quietest {
                        quietest = energy;
                        cut = offset + window / 2;
                    }
                }
                if quietest < loudest * 0.25 {
                    end = cut;
                }
            }
            ranges.push(start..end);
            start = end;
        }
        if ranges.len() > 1 && ranges.last().is_some_and(|last| last.len() < MIN_CHUNK) {
            let tail = ranges.pop().expect("a last chunk");
            ranges.last_mut().expect("a previous chunk").end = tail.end;
        }
        ranges
    }
    /// Speech-like test audio: noise bursts with short pauses at pseudo-random places.
    fn bursts(samples: usize, seed: u64) -> Vec<f32> {
        let mut state = seed;
        let mut next = move || {
            state = state
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            (state >> 33) as usize
        };
        let mut pcm = Vec::with_capacity(samples);
        while pcm.len() < samples {
            let loud = SAMPLE_RATE / 4 + next() % (3 * SAMPLE_RATE);
            let quiet = next() % (SAMPLE_RATE / 5);
            pcm.extend((0..loud).map(|_| (next() % 2000) as f32 / 2000.0 - 0.5));
            pcm.extend((0..quiet).map(|_| (next() % 20) as f32 / 2000.0));
        }
        pcm.truncate(samples);
        pcm
    }
    #[test]
    fn incremental_chunking_matches_whole_buffer_chunking() {
        let cases = [
            (bursts(200 * SAMPLE_RATE + 123, 1), 30),
            (bursts(95 * SAMPLE_RATE, 2), 30),
            // A tail shorter than MIN_CHUNK after a hard cut.
            (vec![0.5; 60 * SAMPLE_RATE + 7], 30),
            (bursts(30 * SAMPLE_RATE + 9, 3), 30),
            (bursts(13 * SAMPLE_RATE, 4), 1),
            (bursts(20 * SAMPLE_RATE, 5), 60),
        ];
        for (pcm, seconds) in cases {
            let seconds = seconds.try_into().unwrap();
            let expected = reference_chunks(&pcm, seconds);
            // A hard cut ends a chunk, other than the last, at exactly the target length.
            let target = seconds.get() as usize * SAMPLE_RATE;
            let expected_hard_cuts = expected[..expected.len() - 1]
                .iter()
                .filter(|range| range.len() == target)
                .count();
            assert_eq!(
                chunks_and_hard_cuts(&pcm, seconds),
                (expected.clone(), expected_hard_cuts)
            );
            // Grow the audio in uneven steps, like ffmpeg pipe reads.
            for step in [1_000, 16_384, 7 * SAMPLE_RATE + 11] {
                let mut chunker = Chunker::new(seconds);
                let mut ranges = Vec::new();
                let mut len = 0;
                while len < pcm.len() {
                    len = (len + step).min(pcm.len());
                    let ready = chunker.ready(&pcm[..len]);
                    assert!(ready.iter().all(|r| r.end <= len));
                    ranges.extend(ready);
                }
                let (rest, hard_cuts) = chunker.finish(&pcm);
                ranges.extend(rest);
                assert_eq!(ranges, expected, "step {step}");
                assert_eq!(hard_cuts, expected_hard_cuts, "step {step}");
            }
        }
    }
    /// A fresh scratch folder for ffmpeg fixtures.
    fn fixture_dir(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("scribe-test-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }
    /// Write `name` in `dir` with ffmpeg. `args` sit between the global options and the output.
    fn fixture(dir: &std::path::Path, name: &str, args: &[&str]) -> std::path::PathBuf {
        let path = dir.join(name);
        let status = std::process::Command::new("ffmpeg")
            .args(["-nostdin", "-v", "error", "-y"])
            .args(args)
            .arg(&path)
            .status()
            .expect("ffmpeg runs");
        assert!(status.success(), "ffmpeg failed to write {name}");
        path
    }
    /// Two seconds of a 440 Hz tone at 16 kHz. Its RMS is about 0.088 (amplitude 1/8).
    const TONE: &str = "sine=f=440:d=2:r=16000";
    fn rms(pcm: &[f32]) -> f64 {
        (pcm.iter().map(|&x| f64::from(x).powi(2)).sum::<f64>() / pcm.len() as f64).sqrt()
    }
    #[test]
    fn the_first_audio_stream_is_decoded_unless_another_is_chosen() {
        let dir = fixture_dir("two-tracks");
        // Tone first, stereo silence second, neither marked default. ffmpeg's own choice takes
        // the stream with more channels, the silence (Astra review R3).
        let media = fixture(
            &dir,
            "two.mkv",
            &[
                "-f",
                "lavfi",
                "-i",
                TONE,
                "-f",
                "lavfi",
                "-i",
                "anullsrc=cl=stereo:r=16000:d=2",
                "-map",
                "0",
                "-map",
                "1",
                "-c:a",
                "aac",
                "-disposition:a",
                "0",
            ],
        );
        let speech = decode(media.as_os_str(), false, 0, None).unwrap();
        assert!(rms(&speech) > 0.05, "first stream RMS {}", rms(&speech));
        let silence = decode(media.as_os_str(), false, 1, None).unwrap();
        assert_eq!(rms(&silence), 0.0);
        let error = decode(media.as_os_str(), false, 2, None).unwrap_err();
        assert!(
            format!("{error:#}").contains("cannot decode audio stream 2"),
            "{error:#}"
        );
        std::fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn phase_inverted_stereo_is_an_error_not_silence() {
        let dir = fixture_dir("phase");
        let media = fixture(
            &dir,
            "inverted.wav",
            &[
                "-f",
                "lavfi",
                "-i",
                TONE,
                "-af",
                "pan=stereo|c0=c0|c1=-1*c0",
            ],
        );
        let error = decode(media.as_os_str(), false, 0, Some(2.0)).unwrap_err();
        assert!(format!("{error:#}").contains("cancel"), "{error:#}");
        // The same tone in phase on both channels decodes.
        let media = fixture(
            &dir,
            "same.wav",
            &["-f", "lavfi", "-i", TONE, "-af", "pan=stereo|c0=c0|c1=c0"],
        );
        assert!(rms(&decode(media.as_os_str(), false, 0, Some(2.0)).unwrap()) > 0.05);
        // Digital silence is not cancellation: it decodes and yields no speech later.
        let media = fixture(
            &dir,
            "silence.wav",
            &["-f", "lavfi", "-i", "anullsrc=cl=stereo:r=16000:d=2"],
        );
        assert_eq!(
            rms(&decode(media.as_os_str(), false, 0, Some(2.0)).unwrap()),
            0.0
        );
        std::fs::remove_dir_all(dir).unwrap();
    }
    /// A 4 s FLAC tone in Matroska with 40 bytes in the middle overwritten: one bad frame.
    fn corrupt_fixture(dir: &std::path::Path) -> std::path::PathBuf {
        let good = fixture(
            dir,
            "good.mkv",
            &[
                "-f",
                "lavfi",
                "-i",
                "sine=f=440:d=4:r=16000",
                "-c:a",
                "flac",
            ],
        );
        let mut bytes = std::fs::read(good).unwrap();
        let middle = bytes.len() / 2;
        bytes[middle..middle + 40].fill(0xa5);
        let path = dir.join("corrupt.mkv");
        std::fs::write(&path, bytes).unwrap();
        path
    }
    #[test]
    fn one_corrupt_frame_in_a_local_file_is_a_warning() {
        let dir = fixture_dir("corrupt");
        let media = corrupt_fixture(&dir);
        let pcm = decode(media.as_os_str(), false, 0, Some(4.0)).unwrap();
        // At most a frame or two is lost.
        assert!(pcm.len() as f64 / SAMPLE_RATE as f64 > 3.0);
        std::fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn corrupt_frames_still_fail_without_a_known_duration_and_truncation_fails() {
        let dir = fixture_dir("truncated");
        // Without a duration, only ffmpeg errors can reveal lost audio. ffmpeg 5.1 and 6.1
        // (Debian 12, Ubuntu 24.04) conceal this corrupt frame without a message; ffmpeg 8
        // reports it. Assert the strict failure only where this ffmpeg reports the damage.
        let media = corrupt_fixture(&dir);
        let report = std::process::Command::new("ffmpeg")
            .args(["-nostdin", "-hide_banner", "-loglevel", "error", "-i"])
            .arg(&media)
            .args(["-f", "null", "-"])
            .output()
            .unwrap();
        if report.stderr.is_empty() {
            eprintln!(
                "this ffmpeg conceals the corrupt frame silently; strict check not exercised"
            );
        } else {
            assert!(decode(media.as_os_str(), false, 0, None).is_err());
        }
        // ffmpeg exits 0 on a file cut in half; the duration check catches it.
        let whole = fixture(
            &dir,
            "whole.mkv",
            &[
                "-f",
                "lavfi",
                "-i",
                "sine=f=440:d=20:r=16000",
                "-c:a",
                "flac",
            ],
        );
        let bytes = std::fs::read(whole).unwrap();
        let cut = dir.join("cut.mkv");
        std::fs::write(&cut, &bytes[..bytes.len() / 2]).unwrap();
        let error = decode(cut.as_os_str(), false, 0, Some(20.0)).unwrap_err();
        assert!(
            format!("{error:#}").contains("decoded audio is incomplete"),
            "{error:#}"
        );
        std::fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn short_audio_is_one_chunk() {
        assert_eq!(
            chunks(&vec![0.0; SAMPLE_RATE], 60.try_into().unwrap()),
            vec![0..16_000]
        );
        assert!(chunks(&[], 60.try_into().unwrap()).is_empty());
    }
}
