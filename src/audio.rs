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
pub fn chunks(pcm: &[f32], seconds: std::num::NonZeroU32) -> Vec<Range<usize>> {
    let target = seconds.get() as usize * SAMPLE_RATE;
    let window = SAMPLE_RATE * 30 / 1000;
    let mut ranges = Vec::new();
    let mut start = 0;
    while start < pcm.len() {
        let hard_end = (start + target).min(pcm.len());
        let mut end = hard_end;
        // Search at most the last ten seconds, but never cut a chunk shorter than MIN_CHUNK.
        let search = hard_end
            .saturating_sub(10 * SAMPLE_RATE)
            .max(start + MIN_CHUNK);
        if hard_end < pcm.len() && search + window <= hard_end {
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
            // A flat signal has no useful quiet boundary. Require a 6 dB RMS drop.
            if quietest < loudest * 0.25 {
                end = cut;
            }
        }
        ranges.push(start..end);
        start = end;
    }
    // The engine fails on a tiny chunk, so a short tail joins the previous chunk.
    if ranges.len() > 1 && ranges.last().is_some_and(|last| last.len() < MIN_CHUNK) {
        let tail = ranges.pop().expect("a last chunk");
        ranges.last_mut().expect("a previous chunk").end = tail.end;
    }
    ranges
}
/// Decode the input to 16 kHz mono f32 samples. Any ffmpeg error output fails the source:
/// ffmpeg exits 0 on a truncated file or a dropped connection and only reports it on stderr.
/// `network` adds a stall timeout and allows only HTTPS for third-party media URLs.
pub fn decode(input: &std::ffi::OsStr, network: bool, expected: Option<f64>) -> Result<Vec<f32>> {
    let mut command = std::process::Command::new("ffmpeg");
    command.args(["-nostdin", "-hide_banner", "-loglevel", "error", "-xerror"]);
    if network {
        command.args(["-rw_timeout", NETWORK_TIMEOUT_MICROS]);
        command.args(["-protocol_whitelist", "https,tls,tcp"]);
    }
    let sample_rate = SAMPLE_RATE.to_string();
    command
        .arg("-i")
        .arg(input)
        .args(["-vn", "-f", "f32le", "-ac", "1", "-ar", &sample_rate, "-"])
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
    let mut pcm =
        Vec::with_capacity(expected.map_or(0, |secs| (secs * SAMPLE_RATE as f64) as usize));
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
            pcm.extend(
                buffer[..whole]
                    .chunks_exact(4)
                    .map(|b| f32::from_le_bytes(b.try_into().expect("four-byte sample"))),
            );
            buffer.copy_within(whole..filled, 0);
            filled -= whole;
        }
    })();
    let status = child.wait()?;
    let errors = errors.join().unwrap_or_default();
    let errors = errors.trim();
    ensure!(
        status.success() && errors.is_empty(),
        "ffmpeg failed ({status}): {}",
        if errors.is_empty() {
            "no error output"
        } else {
            errors
        }
    );
    ensure!(read? == 0, "ffmpeg returned incomplete f32 samples");
    ensure!(!pcm.is_empty(), "media has no audio samples");
    ensure!(
        pcm.iter().all(|v| v.is_finite()),
        "ffmpeg returned non-finite samples"
    );
    check_complete(pcm.len(), expected)?;
    Ok(pcm)
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
    #[test]
    fn chooses_silence_before_target() {
        let mut pcm = vec![0.5; 70 * SAMPLE_RATE];
        pcm[55 * SAMPLE_RATE..56 * SAMPLE_RATE].fill(0.0);
        let cuts = chunks(&pcm, 60.try_into().unwrap());
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
        assert_eq!(
            chunks(&pcm, 60.try_into().unwrap()),
            vec![0..960_000, 960_000..1_920_000, 1_920_000..2_000_000]
        );
    }
    #[test]
    fn a_tail_shorter_than_the_engine_minimum_joins_the_previous_chunk() {
        // 30 s plus five samples of a tone: the hard boundary left a five-sample chunk.
        let pcm: Vec<f32> = (0..30 * SAMPLE_RATE + 5)
            .map(|i| (i as f32 * std::f32::consts::TAU * 440.0 / SAMPLE_RATE as f32).sin())
            .collect();
        assert_eq!(chunks(&pcm, 30.try_into().unwrap()), vec![0..pcm.len()]);
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
    #[test]
    fn short_audio_is_one_chunk() {
        assert_eq!(
            chunks(&vec![0.0; SAMPLE_RATE], 60.try_into().unwrap()),
            vec![0..16_000]
        );
        assert!(chunks(&[], 60.try_into().unwrap()).is_empty());
    }
}
