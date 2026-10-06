use crate::{audio::SAMPLE_RATE, cli::Cli};
use anyhow::{Context, Result};
use serde::Serialize;
use std::ops::Range;
use transcribe_cpp::{
    DeviceType, Model, ModelOptions, RunOptions, Session, SessionOptions, TimestampKind,
};

/// Chunks per engine batch. transcribe-cpp holds the features and encoder output of a whole
/// batch, so one batch over all chunks grows memory with the source length. On the 27:26 talk
/// (M4 Pro, Metal, busy host) peak memory was 1.28 GB at 8, 1.42 GB at 16 and 1.53 GB at 32
/// chunks against 1.95 GB for one batch; engine time stayed within run-to-run noise.
pub const BATCH_CHUNKS: usize = 16;

/// CPU threads for a GPU backend, where only the TDT decoder uses them. The decoder runs
/// two tiny graphs per step, and its workers sync at a barrier after every node, so on a
/// loaded host one parked worker stalls every step. Measurements: docs/adr/0006-decoder-threads.md.
const GPU_DECODER_THREADS: i32 = 1;

#[derive(Serialize)]
pub struct Segment {
    pub start: f64,
    pub end: f64,
    pub text: String,
}

/// Engine stage times summed over all chunks. transcribe-cpp spreads the shared batch encode
/// over its utterances, so the sums equal the real batch times.
#[derive(Default)]
pub struct EngineTimings {
    pub mel: f64,
    pub encode: f64,
    pub decode: f64,
}

/// Segments and engine times, filled one batch at a time.
#[derive(Default)]
pub struct Transcription {
    pub segments: Vec<Segment>,
    pub timings: EngineTimings,
    chunks: usize,
}

pub struct Engine {
    session: Session,
    options: RunOptions,
}
impl Engine {
    pub fn load(path: &std::path::Path, cli: &Cli) -> Result<Self> {
        transcribe_cpp::init_backends_default()?;
        let model = Model::load_with(
            path,
            &ModelOptions {
                backend: cli.backend.into(),
                ..Default::default()
            },
        )?;
        // 0 keeps the library default (min(8, CPUs)), which the CPU backend needs for its encoder.
        let on_gpu = model
            .device()
            .is_ok_and(|device| matches!(device.device_type, DeviceType::Gpu | DeviceType::Igpu));
        let n_threads = match cli.threads {
            0 if on_gpu => GPU_DECODER_THREADS,
            0 => 0,
            threads => i32::from(threads),
        };
        eprintln!(
            "Model loaded; backend: {}; threads: {}",
            model.backend(),
            if n_threads == 0 {
                "default".to_owned()
            } else {
                n_threads.to_string()
            }
        );
        let capabilities = model.capabilities();
        let timestamps = if capabilities.max_timestamp_kind == TimestampKind::None {
            TimestampKind::None
        } else {
            TimestampKind::Segment
        };
        let options = RunOptions {
            language: cli.language.clone(),
            timestamps,
            ..Default::default()
        };
        Ok(Self {
            session: model.session_with(&SessionOptions {
                n_threads,
                ..Default::default()
            })?,
            options,
        })
    }
    /// Transcribe whole audio. The pipeline in main.rs uses `run` on growing audio instead.
    #[cfg(test)]
    pub fn transcribe(
        &mut self,
        pcm: &[f32],
        seconds: std::num::NonZeroU32,
    ) -> Result<Transcription> {
        let mut transcription = Transcription::default();
        for batch in crate::audio::chunks(pcm, seconds).chunks(BATCH_CHUNKS) {
            self.run(pcm, batch, &mut transcription)?;
        }
        Ok(transcription)
    }
    /// Transcribe the `ranges` of `pcm` as one batch and append their segments.
    pub fn run(
        &mut self,
        pcm: &[f32],
        ranges: &[Range<usize>],
        transcription: &mut Transcription,
    ) -> Result<()> {
        let inputs: Vec<&[f32]> = ranges.iter().map(|range| &pcm[range.clone()]).collect();
        let results = self
            .session
            .run_batch(&inputs, &self.options)
            .context("batch transcription failed")?;
        anyhow::ensure!(
            results.len() == ranges.len(),
            "engine returned an incomplete batch"
        );
        for (range, result) in ranges.iter().zip(results) {
            transcription.chunks += 1;
            let result = result.with_context(|| {
                format!("transcription failed for chunk {}", transcription.chunks)
            })?;
            let timings = &mut transcription.timings;
            timings.mel += f64::from(result.timings.mel_ms) / 1000.0;
            timings.encode += f64::from(result.timings.encode_ms) / 1000.0;
            timings.decode += f64::from(result.timings.decode_ms) / 1000.0;
            let offset = range.start as f64 / SAMPLE_RATE as f64;
            let segments = &mut transcription.segments;
            if result.timestamp_kind != TimestampKind::None && !result.segments.is_empty() {
                segments.extend(result.segments.into_iter().map(|s| Segment {
                    start: offset + s.t0_ms as f64 / 1000.0,
                    end: offset + s.t1_ms as f64 / 1000.0,
                    text: s.text.trim().to_owned(),
                }));
            } else {
                segments.push(Segment {
                    start: offset,
                    end: range.end as f64 / SAMPLE_RATE as f64,
                    text: result.text.trim().to_owned(),
                });
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    /// Regression for a sentence that transcribe-cpp dropped from a 52.6 s chunk.
    /// The first 70 s of the source still get that chunk boundary at the old
    /// 60 s default, so this short slice is enough to reproduce the loss.
    #[test]
    #[ignore = "needs the Pi durable-sessions talk in SCRIBE_REGRESSION_MEDIA; SCRIBE_REGRESSION_MODEL overrides the default model"]
    fn default_chunks_keep_the_sentence_long_chunks_dropped() {
        let Ok(media) = std::env::var("SCRIBE_REGRESSION_MEDIA") else {
            eprintln!("skip: SCRIBE_REGRESSION_MEDIA is not set");
            return;
        };
        let model = std::env::var("SCRIBE_REGRESSION_MODEL")
            .unwrap_or_else(|_| crate::cli::DEFAULT_MODEL.to_owned());
        let cli = Cli::parse_from(["scribe", &media, "--model", &model]);
        let pcm = crate::audio::decode(media.as_ref(), false, None).unwrap();
        let pcm = &pcm[..70 * SAMPLE_RATE];
        // The default model comes from the cache, or downloads once and is verified.
        let model = crate::fetch::model(&model).unwrap();
        let mut engine = Engine::load(&model, &cli).unwrap();
        let text = engine
            .transcribe(pcm, cli.chunk_secs)
            .unwrap()
            .segments
            .into_iter()
            .map(|segment| segment.text)
            .collect::<Vec<_>>()
            .join(" ");
        assert!(
            text.contains("fits snugly into memory"),
            "sentence near 0:25 is missing: {text}"
        );
    }
}
