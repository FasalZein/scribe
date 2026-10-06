use crate::{audio::SAMPLE_RATE, cli::Cli};
use anyhow::{Context, Result};
use std::ops::Range;
use transcribe_cpp::{
    DeviceType, Model, ModelOptions, RunOptions, Session, SessionOptions, TimestampKind, Token,
    Transcript,
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

/// One word of the transcript, with its time in the source in seconds.
pub struct Word {
    pub start: f64,
    pub end: f64,
    pub text: String,
    /// The lowest engine confidence `p` (0 to 1) of the word's tokens, or None when the model
    /// gives no token scores. Parakeet computes `p` as 1 - entropy / max entropy per token.
    pub confidence: Option<f32>,
}

/// Engine stage times summed over all chunks. transcribe-cpp spreads the shared batch encode
/// over its utterances, so the sums equal the real batch times.
#[derive(Default)]
pub struct EngineTimings {
    pub mel: f64,
    pub encode: f64,
    pub decode: f64,
}

/// Words and engine times, filled one batch at a time.
#[derive(Default)]
pub struct Transcription {
    pub words: Vec<Word>,
    pub timings: EngineTimings,
    /// Chunk cuts made at the length limit (see `audio::Chunker`), set by the caller.
    pub hard_cuts: usize,
    chunks: usize,
}

impl Transcription {
    /// Append the words of the engine result for the chunk `range`. Blank words are dropped,
    /// so a chunk without speech adds nothing, and audio without speech has no words and no
    /// parts.
    fn add(&mut self, range: &Range<usize>, result: Transcript) {
        let timings = &mut self.timings;
        timings.mel += f64::from(result.timings.mel_ms) / 1000.0;
        timings.encode += f64::from(result.timings.encode_ms) / 1000.0;
        timings.decode += f64::from(result.timings.decode_ms) / 1000.0;
        let offset = range.start as f64 / SAMPLE_RATE as f64;
        let words = &mut self.words;
        let seconds = |ms: i64| offset + ms as f64 / 1000.0;
        if !result.words.is_empty() {
            let tokens = &result.tokens;
            words.extend(result.words.iter().filter_map(|w| {
                let text = w.text.trim();
                (!text.is_empty()).then(|| Word {
                    start: seconds(w.t0_ms),
                    end: seconds(w.t1_ms),
                    text: text.to_owned(),
                    confidence: word_confidence(tokens, w.first_token, w.n_tokens),
                })
            }));
        } else {
            // A model without word times: every word gets the times of its segment, or of
            // the whole chunk without segment times. A paragraph timestamp can then be up to
            // one segment early, as it was before word timestamps.
            let spans: Vec<(f64, f64, String)> =
                if result.timestamp_kind != TimestampKind::None && !result.segments.is_empty() {
                    result
                        .segments
                        .into_iter()
                        .map(|s| (seconds(s.t0_ms), seconds(s.t1_ms), s.text))
                        .collect()
                } else {
                    let end = range.end as f64 / SAMPLE_RATE as f64;
                    vec![(offset, end, result.text)]
                };
            for (start, end, text) in spans {
                words.extend(text.split_whitespace().map(|text| Word {
                    start,
                    end,
                    text: text.to_owned(),
                    confidence: None,
                }));
            }
        }
    }
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
        crate::progress::line!(
            "Model loaded; backend: {}; threads: {}",
            model.backend(),
            if n_threads == 0 {
                "default".to_owned()
            } else {
                n_threads.to_string()
            }
        );
        // Request the finest timestamps the model has. Word times let every paragraph start at
        // the word it cites (ADR 0013). For Parakeet the finest kind is Token: its token rows
        // also carry the engine's per-token confidence `p`, at no extra engine cost.
        let timestamps = match model.capabilities().max_timestamp_kind {
            kind @ (TimestampKind::Token | TimestampKind::Word) => kind,
            TimestampKind::None => TimestampKind::None,
            _ => TimestampKind::Segment,
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
            self.run(pcm, batch, 0, &mut transcription)?;
        }
        Ok(transcription)
    }
    /// Transcribe the retained PCM ranges and append words at their absolute source times.
    pub fn run(
        &mut self,
        pcm: &[f32],
        ranges: &[Range<usize>],
        sample_offset: usize,
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
            let absolute_range = sample_offset + range.start..sample_offset + range.end;
            transcription.add(&absolute_range, result);
        }
        Ok(())
    }
}

/// The lowest token score of one word: one doubtful token makes the whole word doubtful.
/// Punctuation tokens do not count: a doubtful full stop does not make the word doubtful.
/// None when the word has no tokens in range or no token carries a score (NaN).
fn word_confidence(tokens: &[Token], first: i32, count: i32) -> Option<f32> {
    let first = usize::try_from(first).ok()?;
    let count = usize::try_from(count).ok()?;
    tokens
        .get(first..first.checked_add(count)?)?
        .iter()
        .filter(|token| token.text.chars().any(char::is_alphanumeric))
        .map(|token| token.p)
        .filter(|p| !p.is_nan())
        .reduce(f32::min)
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    #[test]
    fn whitespace_engine_results_publish_zero_parts() {
        // The three result shapes, each with only whitespace: word times, segment times, text.
        let blank = || transcribe_cpp::Word {
            text: " ".into(),
            ..Default::default()
        };
        let results = [
            Transcript {
                timestamp_kind: TimestampKind::Word,
                words: vec![blank(), blank()],
                ..Default::default()
            },
            Transcript {
                timestamp_kind: TimestampKind::Segment,
                segments: vec![transcribe_cpp::Segment {
                    text: " \n ".into(),
                    ..Default::default()
                }],
                ..Default::default()
            },
            Transcript {
                text: "\t ".into(),
                ..Default::default()
            },
        ];
        let mut transcription = Transcription::default();
        for (i, result) in results.into_iter().enumerate() {
            let start = i * 30 * SAMPLE_RATE;
            transcription.add(&(start..start + 30 * SAMPLE_RATE), result);
        }
        assert!(transcription.words.is_empty());
        let dir = std::env::temp_dir().join(format!("scribe-test-silence-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let meta = crate::fetch::Metadata {
            id: "file:silence".into(),
            title: "Silence".into(),
            source: "/silence.wav".into(),
            uploader: None,
            upload_date: None,
            duration: Some(90.0),
            file_size: None,
            description: None,
            sources: crate::sources::Sources::Local {
                path: "/silence.wav".into(),
            },
            chapters: Vec::new(),
        };
        let cli = Cli::parse_from(["scribe", "x"]);
        let index = crate::output::write(
            &dir,
            &meta,
            &cli,
            90.0,
            &crate::timings::Timings::new(false),
            &transcription.words,
            0,
        )
        .unwrap();
        let index = std::fs::read_to_string(index).unwrap();
        assert!(index.lines().any(|line| line == "parts: 0"), "{index}");
        std::fs::remove_dir_all(dir).unwrap();
    }

    fn token(text: &str, p: f32) -> Token {
        Token {
            p,
            text: text.into(),
            ..Default::default()
        }
    }
    #[test]
    fn a_word_scores_as_its_least_confident_token() {
        let tokens = [
            token("\u{2581}Gro", 0.9),
            token("k", 0.2),
            token("bot", 0.7),
            token(".", 0.1),
            token("\u{2581}so", f32::NAN),
        ];
        assert_eq!(word_confidence(&tokens, 0, 3), Some(0.2));
        // The doubtful full stop does not lower "bot.".
        assert_eq!(word_confidence(&tokens, 2, 2), Some(0.7));
        // Only NaN or punctuation scores, an empty word or an index out of range: no score.
        assert_eq!(word_confidence(&tokens, 3, 2), None);
        assert_eq!(word_confidence(&tokens, 1, 0), None);
        assert_eq!(word_confidence(&tokens, 4, 2), None);
        assert_eq!(word_confidence(&tokens, -1, 1), None);
    }

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
        let pcm = crate::audio::decode(media.as_ref(), false, 0, None).unwrap();
        let pcm = &pcm[..70 * SAMPLE_RATE];
        // The default model comes from the cache, or downloads once and is verified.
        let model = crate::fetch::model(&model).unwrap();
        let mut engine = Engine::load(&model, &cli).unwrap();
        let text = engine
            .transcribe(pcm, cli.chunk_secs)
            .unwrap()
            .words
            .into_iter()
            .map(|word| word.text)
            .collect::<Vec<_>>()
            .join(" ");
        assert!(
            text.contains("fits snugly into memory"),
            "sentence near 0:25 is missing: {text}"
        );
    }
}
