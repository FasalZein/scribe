use std::ops::Range;
pub const SAMPLE_RATE: usize = 16_000;
pub fn chunks(pcm: &[f32], seconds: std::num::NonZeroU32) -> Vec<Range<usize>> {
    let target = seconds.get() as usize * SAMPLE_RATE;
    let window = SAMPLE_RATE * 30 / 1000;
    let mut ranges = Vec::new();
    let mut start = 0;
    while start < pcm.len() {
        let hard_end = (start + target).min(pcm.len());
        let mut end = hard_end;
        if hard_end < pcm.len() {
            // Search at most the last ten seconds, but never produce an empty chunk.
            let search = hard_end.saturating_sub(10 * SAMPLE_RATE).max(start);
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
    ranges
}
pub fn decode(path: &std::path::Path) -> anyhow::Result<(Vec<f32>, Vec<u8>)> {
    use anyhow::ensure;
    let output = crate::fetch::command_output(
        std::process::Command::new("ffmpeg")
            .args(["-nostdin", "-hide_banner", "-loglevel", "error", "-i"])
            .arg(path)
            .args(["-vn", "-f", "f32le", "-ac", "1", "-ar", "16000", "-"]),
        "ffmpeg",
    )?;
    ensure!(
        output.stdout.len() % 4 == 0,
        "ffmpeg returned incomplete f32 samples"
    );
    let pcm: Vec<f32> = output
        .stdout
        .chunks_exact(4)
        .map(|b| f32::from_le_bytes(b.try_into().expect("four-byte sample")))
        .collect();
    ensure!(!pcm.is_empty(), "media has no audio samples");
    ensure!(
        pcm.iter().all(|v| v.is_finite()),
        "ffmpeg returned non-finite samples"
    );
    Ok((pcm, output.stdout))
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
    fn short_audio_is_one_chunk() {
        assert_eq!(
            chunks(&vec![0.0; SAMPLE_RATE], 60.try_into().unwrap()),
            vec![0..16_000]
        );
        assert!(chunks(&[], 60.try_into().unwrap()).is_empty());
    }
}
