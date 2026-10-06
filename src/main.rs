mod audio;
mod cli;
mod engine;
mod fetch;
mod output;
mod parts;
mod sources;
mod timings;

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
    let (meta, media) = timings.time("metadata", || fetch::media(input))?;
    let dir = output::directory(&cli.output_root()?, &meta)?;
    let index = dir.join("index.md");
    if index.exists() && !cli.force {
        eprintln!("skip: {} exists (use --force)", index.display());
        return Ok(index);
    }
    let workspace = fetch::Workspace::new()?;
    let media = timings.time("download", || fetch::resolve(media, &workspace.0))?;
    if let fetch::Media::Stream(_) = &media {
        eprintln!("Streaming X API video directly through ffmpeg (lowest-bitrate mp4)");
    }
    if engine.is_none() {
        // Load lazily so that skipped inputs never pay for the model.
        let loaded = timings
            .time("model", || {
                fetch::model(&cli.model).and_then(|model| engine::Engine::load(&model, cli))
            })
            .context(ModelLoadFailed)?;
        *engine = Some(loaded);
    }
    eprintln!("Decoding: {}", meta.title);
    let network = matches!(media, fetch::Media::Stream(_));
    let pcm = timings.time("decode", || {
        audio::decode(media.input(), network, meta.duration)
    })?;
    let duration = pcm.len() as f64 / audio::SAMPLE_RATE as f64;
    let start = Instant::now();
    let (segments, stages) = engine
        .as_mut()
        .context("engine was not loaded")?
        .transcribe(&pcm, cli.chunk_secs)?;
    let engine_secs = start.elapsed().as_secs_f64();
    timings.add("engine", engine_secs);
    timings.add("mel", stages.mel);
    timings.add("encode", stages.encode);
    timings.add("tdt-decode", stages.decode);
    let write_start = Instant::now();
    eprintln!("Transcribed {:.1}s of audio in {engine_secs:.2}s", duration);
    output::clear(&dir)?;
    if cli.keep_media {
        use std::io::Write;
        let mut audio = std::io::BufWriter::new(fs::File::create(dir.join("audio.f32le"))?);
        for sample in &pcm {
            audio.write_all(&sample.to_le_bytes())?;
        }
        audio.flush()?;
        if fetch::is_url(input)
            && let fetch::Media::File(media) = &media
        {
            let name = media.file_name().context("download has no filename")?;
            fs::copy(media, dir.join(name))?;
        }
    }
    let index = output::write(&dir, &meta, cli, duration, engine_secs, &segments)?;
    timings.add("write", write_start.elapsed().as_secs_f64());
    Ok(index)
}
fn run(cli: Cli) -> Result<bool> {
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
    match run(Cli::parse()) {
        Ok(false) => std::process::ExitCode::SUCCESS,
        Ok(true) => std::process::ExitCode::FAILURE,
        Err(error) => {
            eprintln!("scribe: {error:#}");
            std::process::ExitCode::FAILURE
        }
    }
}
