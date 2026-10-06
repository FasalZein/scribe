mod audio;
mod cli;
mod engine;
mod fetch;
mod output;
mod sources;

use anyhow::{Context, Result};
use clap::Parser;
use cli::Cli;
use std::{fs, time::Instant};

fn process(input: &str, cli: &Cli, engine: &mut engine::Engine) -> Result<std::path::PathBuf> {
    let workspace = fetch::Workspace::new()?;
    let (meta, media) = fetch::media(input, &workspace.0)?;
    eprintln!("Decoding: {}", meta.title);
    let (pcm, bytes) = audio::decode(media.input())?;
    let duration = pcm.len() as f64 / audio::SAMPLE_RATE as f64;
    let dir = output::directory(&cli.out, &meta)?;
    let start = Instant::now();
    let segments = engine.transcribe(&pcm, cli.chunk_secs)?;
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
    let model = fetch::model(&cli.model)?;
    let mut engine = engine::Engine::load(&model, &cli)?;
    let mut failed = false;
    for input in &cli.inputs {
        match process(input, &cli, &mut engine) {
            Ok(path) => println!("{}", path.display()),
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
