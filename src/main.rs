mod audio;
mod cli;
mod doctor;
mod engine;
mod fetch;
mod known_terms;
mod lessons;
mod logging;
mod low_confidence;
mod output;
mod parts;
mod sources;
mod timings;
mod topics;

use anyhow::{Context, Result};
use clap::Parser;
use cli::Cli;
use std::{fs, time::Instant};

/// Marks errors that stop the run, because every later input needs the same model.
#[derive(Debug)]
struct ModelLoadFailed;
impl std::fmt::Display for ModelLoadFailed {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("model load failed")
    }
}

fn process(
    input: &str,
    cli: &Cli,
    engine: &mut Option<engine::Engine>,
    timings: &mut timings::Timings,
) -> Result<std::path::PathBuf> {
    let (meta, media) = timings.time("metadata", || fetch::media(input, cli.audio_stream))?;
    let dir = output::directory(&cli.output_root()?, &meta)?;
    let index = dir.join("index.md");
    if index.exists() && !cli.force {
        eprintln!("skip: {} exists (use --force)", index.display());
        return Ok(index);
    }
    let workspace = fetch::Workspace::new(&meta.id)?;
    let (media, pcm, transcription) =
        transcribe(media, &workspace.path, &meta, cli, engine, timings)?;
    let duration = pcm.len() as f64 / audio::SAMPLE_RATE as f64;
    let engine_secs = timings.get("engine");
    eprintln!("Transcribed {duration:.1}s of audio in {engine_secs:.2}s of engine time");
    let write_start = Instant::now();
    let index = output::replace(&dir, |stage| {
        if cli.keep_media {
            use std::io::Write;
            let mut audio = std::io::BufWriter::new(fs::File::create(stage.join("audio.f32le"))?);
            for sample in &pcm {
                audio.write_all(&sample.to_le_bytes())?;
            }
            audio.flush()?;
            if fetch::is_url(input)
                && let fetch::Media::File(media) = &media
            {
                let name = media.file_name().context("download has no filename")?;
                fs::copy(media, stage.join(name))?;
            }
        }
        output::write(
            stage,
            &meta,
            cli,
            duration,
            timings,
            &transcription.words,
            transcription.hard_cuts,
        )
    })?;
    timings.add("write", write_start.elapsed().as_secs_f64());
    workspace.complete();
    Ok(index)
}
type Loader<'scope> = std::thread::ScopedJoinHandle<'scope, (Result<engine::Engine>, f64)>;

/// The loaded engine. Waits for the loader thread on first use.
fn ready<'a>(
    engine: &'a mut Option<engine::Engine>,
    loader: &mut Option<Loader<'_>>,
    timings: &mut timings::Timings,
) -> Result<&'a mut engine::Engine> {
    if let Some(loader) = loader.take() {
        let (loaded, secs) = loader.join().expect("model loader panicked");
        timings.add("model", secs);
        *engine = Some(loaded.context(ModelLoadFailed)?);
    }
    engine.as_mut().context("engine was not loaded")
}

/// Download, decode and transcribe one source, with three overlapping stages:
/// - a loader thread loads the model when no engine exists yet;
/// - a decoder thread downloads the media and streams ffmpeg samples over a channel;
/// - this thread collects the samples and runs an engine batch as soon as enough final chunks
///   exist (see `audio::Chunker`).
///
/// The channel is unbounded on purpose: a slow engine batch (minutes on a CPU) must not stall
/// a network read past ffmpeg's timeout. It holds at most the audio, which is kept anyway.
/// The truncation check runs on the whole audio before the last batches and before any output.
fn transcribe(
    media: fetch::Pending,
    workspace: &std::path::Path,
    meta: &fetch::Metadata,
    cli: &Cli,
    engine: &mut Option<engine::Engine>,
    timings: &mut timings::Timings,
) -> Result<(fetch::Media, Vec<f32>, engine::Transcription)> {
    std::thread::scope(|scope| {
        let mut loader = engine.is_none().then(|| {
            scope.spawn(|| {
                let start = Instant::now();
                let loaded =
                    fetch::model(&cli.model).and_then(|model| engine::Engine::load(&model, cli));
                (loaded, start.elapsed().as_secs_f64())
            })
        });
        let (sender, blocks) = std::sync::mpsc::channel::<Vec<f32>>();
        let decoder = scope.spawn(move || {
            let start = Instant::now();
            let media = fetch::resolve(media, workspace)?;
            let download = start.elapsed().as_secs_f64();
            if let fetch::Media::Stream(_) = &media {
                eprintln!("Streaming X API video directly through ffmpeg (lowest-bitrate mp4)");
            }
            eprintln!("Decoding: {}", meta.title);
            let network = matches!(media, fetch::Media::Stream(_));
            let samples = audio::decode_blocks(
                media.input(),
                network,
                cli.audio_stream,
                meta.duration,
                |block| sender.send(block).is_ok(),
            )?;
            let decode = start.elapsed().as_secs_f64() - download;
            anyhow::Ok((media, samples, download, decode))
        });
        let capacity = meta
            .duration
            .map_or(0, |secs| (secs * audio::SAMPLE_RATE as f64) as usize);
        let mut pcm = Vec::with_capacity(capacity);
        let mut chunker = audio::Chunker::new(cli.chunk_secs);
        let mut pending = Vec::new();
        let mut transcription = engine::Transcription::default();
        let mut engine_secs = 0.0;
        // Ends when the decoder thread finishes and drops the sender.
        for block in blocks {
            pcm.extend_from_slice(&block);
            pending.extend(chunker.ready(&pcm));
            if pending.len() >= engine::BATCH_CHUNKS {
                let batch: Vec<_> = pending.drain(..engine::BATCH_CHUNKS).collect();
                let engine = ready(engine, &mut loader, timings)?;
                let start = Instant::now();
                engine.run(&pcm, &batch, &mut transcription)?;
                engine_secs += start.elapsed().as_secs_f64();
            }
        }
        let decoded = decoder.join().expect("decoder panicked");
        let tail_start = Instant::now();
        // Keep a loaded engine for the next source even when this source failed.
        let engine = ready(engine, &mut loader, timings)?;
        let (media, samples, download, decode) = decoded?;
        timings.add("download", download);
        timings.add("decode", decode);
        anyhow::ensure!(samples == pcm.len(), "decoded samples were lost");
        let (rest, hard_cuts) = chunker.finish(&pcm);
        pending.extend(rest);
        transcription.hard_cuts = hard_cuts;
        for batch in pending.chunks(engine::BATCH_CHUNKS) {
            let start = Instant::now();
            engine.run(&pcm, batch, &mut transcription)?;
            engine_secs += start.elapsed().as_secs_f64();
        }
        timings.add("engine", engine_secs);
        timings.add("after-decode", tail_start.elapsed().as_secs_f64());
        let stages = &transcription.timings;
        timings.add("mel", stages.mel);
        timings.add("encode", stages.encode);
        timings.add("tdt-decode", stages.decode);
        Ok((media, pcm, transcription))
    })
}

fn run(cli: Cli) -> Result<bool> {
    if let Some(command) = &cli.command {
        match command {
            cli::Command::Doctor(args) => doctor::run(args)?,
            cli::Command::Lessons { action } => match action {
                cli::LessonsAction::Check { file } => lessons::check(file)?,
                cli::LessonsAction::Finalize { file } => lessons::finalize(file)?,
            },
            cli::Command::Topics { action } => {
                let library = Cli::library_root()?;
                match action {
                    cli::TopicsAction::Plan => print!("{}", topics::plan(&library)?),
                    cli::TopicsAction::Index => println!("{}", topics::index(&library)?.display()),
                }
            }
        }
        return Ok(false);
    }
    let mut engine = None;
    let mut failed = false;
    for input in &cli.inputs {
        let mut timings = timings::Timings::new(cli.timings);
        let result = process(input, &cli, &mut engine, &mut timings);
        timings.report();
        match result {
            Ok(path) => println!("{}", path.display()),
            Err(error) if error.downcast_ref::<ModelLoadFailed>().is_some() => return Err(error),
            Err(error) => {
                eprintln!("scribe: {input}: {error:#}");
                failed = true;
            }
        }
    }
    Ok(failed)
}
fn main() -> std::process::ExitCode {
    let cli = Cli::parse();
    logging::init(cli.verbose);
    match run(cli) {
        Ok(false) => std::process::ExitCode::SUCCESS,
        Ok(true) => std::process::ExitCode::FAILURE,
        Err(error) => {
            eprintln!("scribe: {error:#}");
            std::process::ExitCode::FAILURE
        }
    }
}
