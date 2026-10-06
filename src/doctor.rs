use crate::cli::{Backend, DEFAULT_MODEL};
use anyhow::{Context, Result, ensure};
use clap::{Args, ValueEnum};
use std::{fs, path::PathBuf, process::Command};
use transcribe_cpp::{Model, ModelOptions, RunOptions, SessionOptions};

#[derive(Args)]
pub struct Doctor {
    /// Probe the backend and tools without loading a model
    #[arg(long)]
    no_self_test: bool,
    /// Compute backend (explicit selections do not fall back)
    #[arg(long, value_enum, default_value = "auto")]
    backend: Backend,
    /// Use a local GGUF instead of the cached default model (never downloads)
    #[arg(short, long, value_name = "PATH")]
    model: Option<PathBuf>,
}

fn tool_version(tool: &str, flag: &str) -> Option<String> {
    let output = Command::new(tool).arg(flag).output().ok()?;
    if !output.status.success() {
        return None;
    }
    let text = if output.stdout.is_empty() {
        &output.stderr
    } else {
        &output.stdout
    };
    String::from_utf8_lossy(text)
        .lines()
        .next()
        .filter(|line| !line.is_empty())
        .map(str::to_owned)
}

pub fn run(args: &Doctor) -> Result<()> {
    let backend = args.backend.to_possible_value().unwrap();
    println!("backend: {} (requested)", backend.get_name());
    transcribe_cpp::init_backends_default().context("backend initialization failed")?;
    let available = transcribe_cpp::backend_available(args.backend.into());
    for device in transcribe_cpp::devices() {
        println!(
            "device: {} / {} ({})",
            device.name, device.description, device.kind
        );
        if device.memory_total == 0 {
            println!("memory: not reported for {}", device.name);
        } else {
            println!(
                "memory: {} bytes total, {} bytes free ({})",
                device.memory_total, device.memory_free, device.name
            );
        }
    }
    let mut tools_ready = true;
    for tool in ["ffmpeg", "ffprobe"] {
        match tool_version(tool, "-version") {
            Some(version) => println!("{tool}: {version}"),
            None => {
                println!("{tool}: missing or broken");
                tools_ready = false;
            }
        }
    }
    if let Some(version) = tool_version("uvx", "--version") {
        println!("uvx: {version}");
    } else if let Some(version) = tool_version("yt-dlp", "--version") {
        println!("yt-dlp: {version}");
    } else {
        println!("uvx or yt-dlp: missing or broken");
        tools_ready = false;
    }
    // Presence is not integrity verification. The engine self-test checks that the model loads.
    // Do not call fetch::model here: a damaged or missing cache must never trigger a download.
    let model = if let Some(path) = &args.model {
        println!("model cache: local override {}", path.display());
        Some(path.clone())
    } else {
        let cache = dirs::cache_dir()
            .context("platform cache directory is unavailable")?
            .join("scribe/models");
        let name = DEFAULT_MODEL
            .rsplit('/')
            .next()
            .context("default model URL has no filename")?;
        let path = cache.join(name);
        if path.is_file() {
            let bytes = fs::metadata(&path)?.len();
            println!("model cache: present {} ({bytes} bytes)", path.display());
            for suffix in [".verified", ".complete.json"] {
                println!(
                    "cache record {suffix}: {}",
                    if cache.join(format!("{name}{suffix}")).is_file() {
                        "present (not revalidated)"
                    } else {
                        "absent"
                    }
                );
            }
            Some(path)
        } else {
            println!("model cache: model not cached ({})", path.display());
            None
        }
    };
    ensure!(
        available,
        "backend {} is unavailable or broken",
        backend.get_name()
    );
    ensure!(tools_ready, "required runtime tools are missing or broken");
    if args.no_self_test {
        println!("self-test: skipped (--no-self-test)");
    } else if let Some(path) = model {
        let model = Model::load_with(
            &path,
            &ModelOptions {
                backend: args.backend.into(),
                ..Default::default()
            },
        )
        .context("self-test model load failed")?;
        println!("backend: {} (loaded)", model.backend());
        // One second of generated 440 Hz audio exercises inference, not speech accuracy.
        let pcm: Vec<f32> = (0..crate::audio::SAMPLE_RATE)
            .map(|i| {
                0.01 * (std::f32::consts::TAU * 440.0 * i as f32 / crate::audio::SAMPLE_RATE as f32)
                    .sin()
            })
            .collect();
        let mut session = model.session_with(&SessionOptions {
            n_threads: 1,
            ..Default::default()
        })?;
        session
            .run(&pcm, &RunOptions::default())
            .context("backend self-test failed")?;
        println!("self-test: passed (1 s generated audio)");
    } else {
        println!(
            "self-test: skipped (model not cached; transcribe a source or use doctor --model PATH)"
        );
    }
    Ok(())
}
