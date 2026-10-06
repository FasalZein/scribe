use crate::{
    audio::{SAMPLE_RATE, chunks},
    cli::Cli,
};
use anyhow::{Context, Result};
use serde::Serialize;
use transcribe_cpp::{Model, ModelOptions, RunOptions, Session, TimestampKind};

/// Chunks per engine batch. transcribe-cpp holds the features and encoder output of a whole
/// batch, so one batch over all chunks grows memory with the source length. On the 27:26 talk
/// (M4 Pro, Metal, busy host) peak memory was 1.28 GB at 8, 1.42 GB at 16 and 1.53 GB at 32
/// chunks against 1.95 GB for one batch; engine time stayed within run-to-run noise.
const BATCH_CHUNKS: usize = 16;

#[derive(Serialize)]
pub struct Segment {
    pub start: f64,
    pub end: f64,
    pub text: String,
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
        eprintln!("Model loaded; backend: {}", model.backend());
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
            session: model.session()?,
            options,
        })
    }
    pub fn transcribe(
        &mut self,
        pcm: &[f32],
        seconds: std::num::NonZeroU32,
    ) -> Result<Vec<Segment>> {
        let ranges = chunks(pcm, seconds);
        eprintln!("Transcribing {} chunks", ranges.len());
        let mut results = Vec::with_capacity(ranges.len());
        for batch in ranges.chunks(BATCH_CHUNKS) {
            let inputs: Vec<&[f32]> = batch.iter().map(|range| &pcm[range.clone()]).collect();
            let batch_results = self
                .session
                .run_batch(&inputs, &self.options)
                .context("batch transcription failed")?;
            anyhow::ensure!(
                batch_results.len() == batch.len(),
                "engine returned an incomplete batch"
            );
            results.extend(batch_results);
        }
        let mut segments = Vec::new();
        for (index, (range, result)) in ranges.iter().zip(results).enumerate() {
            let result =
                result.with_context(|| format!("transcription failed for chunk {}", index + 1))?;
            let offset = range.start as f64 / SAMPLE_RATE as f64;
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
        Ok(segments)
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
    #[ignore = "needs a Parakeet GGUF and the Pi durable-sessions talk; set SCRIBE_REGRESSION_MODEL and SCRIBE_REGRESSION_MEDIA"]
    fn default_chunks_keep_the_sentence_long_chunks_dropped() {
        let (Ok(model), Ok(media)) = (
            std::env::var("SCRIBE_REGRESSION_MODEL"),
            std::env::var("SCRIBE_REGRESSION_MEDIA"),
        ) else {
            eprintln!("skip: SCRIBE_REGRESSION_MODEL or SCRIBE_REGRESSION_MEDIA is not set");
            return;
        };
        let cli = Cli::parse_from(["scribe", &media, "--model", &model]);
        let pcm = crate::audio::decode(media.as_ref(), false, None).unwrap();
        let pcm = &pcm[..70 * SAMPLE_RATE];
        let mut engine = Engine::load(model.as_ref(), &cli).unwrap();
        let text = engine
            .transcribe(pcm, cli.chunk_secs)
            .unwrap()
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
