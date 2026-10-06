mod audio;
mod cli;
mod engine;
mod fetch;
mod output;
mod parts;
mod sources;

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
) -> Result<std::path::PathBuf> {
    let (meta, media) = fetch::media(input)?;
    let dir = output::directory(&cli.output_root()?, &meta)?;
    let index = dir.join("index.md");
    if index.exists() && !cli.force {
        eprintln!("skip: {} exists (use --force)", index.display());
        return Ok(index);
    }
    let workspace = fetch::Workspace::new()?;
    let media = fetch::resolve(media, &workspace.0)?;
    if let fetch::Media::Stream(_) = &media {
        eprintln!("Streaming X API video directly through ffmpeg (lowest-bitrate mp4)");
    }
    if engine.is_none() {
        // Load lazily so that skipped inputs never pay for the model.
        let loaded = fetch::model(&cli.model)
            .and_then(|model| engine::Engine::load(&model, cli))
            .context(ModelLoadFailed)?;
        *engine = Some(loaded);
    }
    eprintln!("Decoding: {}", meta.title);
    let (pcm, bytes) = audio::decode(media.input())?;
    let duration = pcm.len() as f64 / audio::SAMPLE_RATE as f64;
    let start = Instant::now();
    let segments = engine
        .as_mut()
        .context("engine was not loaded")?
        .transcribe(&pcm, cli.chunk_secs)?;
    let engine_secs = start.elapsed().as_secs_f64();
    eprintln!("Transcribed {:.1}s of audio in {engine_secs:.2}s", duration);
    if cli.keep_media {
        fs::write(dir.join("audio.f32le"), bytes)?;
        if fetch::is_url(input)
            && let fetch::Media::File(media) = &media
        {
            let name = media.file_name().context("download has no filename")?;
            fs::copy(media, dir.join(name))?;
        }
    }
    output::write(&dir, &meta, cli, duration, engine_secs, &segments)
}
fn run(cli: Cli) -> Result<bool> {
    let mut engine = None;
    let mut failed = false;
    for input in &cli.inputs {
        match process(input, &cli, &mut engine) {
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
