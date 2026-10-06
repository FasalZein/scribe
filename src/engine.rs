use crate::{
    audio::{SAMPLE_RATE, chunks},
    cli::Cli,
};
use anyhow::{Context, Result};
use serde::Serialize;
use transcribe_cpp::{Model, ModelOptions, RunOptions, Session, TimestampKind};

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
        let inputs: Vec<&[f32]> = ranges.iter().map(|range| &pcm[range.clone()]).collect();
        eprintln!("Transcribing {} chunks", ranges.len());
        let results = self
            .session
            .run_batch(&inputs, &self.options)
            .context("batch transcription failed")?;
        anyhow::ensure!(
            results.len() == ranges.len(),
            "engine returned an incomplete batch"
        );
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
