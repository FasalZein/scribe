use clap::{Parser, ValueEnum};
use std::{num::NonZeroU32, path::PathBuf};

pub const DEFAULT_MODEL: &str = "https://huggingface.co/handy-computer/parakeet-tdt-0.6b-v3-gguf/resolve/main/parakeet-tdt-0.6b-v3-Q8_0.gguf";

#[derive(Parser)]
#[command(
    version,
    about = "Turn video URLs or local media into timestamped transcripts"
)]
pub struct Cli {
    /// URL or local media file path; processed in order
    #[arg(required = true, value_name = "INPUT")]
    pub inputs: Vec<String>,
    /// Output root
    #[arg(short, long, default_value = "./scribe-out", value_name = "DIR")]
    pub out: PathBuf,
    /// GGUF model path or URL
    #[arg(short, long, default_value = DEFAULT_MODEL, value_name = "PATH|URL")]
    pub model: String,
    /// Language hint passed to the engine
    #[arg(short, long, value_name = "CODE")]
    pub language: Option<String>,
    /// Target chunk length in seconds
    #[arg(long, default_value = "60", value_name = "N")]
    pub chunk_secs: NonZeroU32,
    /// Keep downloaded media and decoded 16 kHz mono f32 audio
    #[arg(long)]
    pub keep_media: bool,
    /// Compute backend (explicit selections do not fall back)
    #[arg(long, value_enum, default_value = "auto", value_name = "NAME")]
    pub backend: Backend,
}

#[derive(Clone, Copy, ValueEnum)]
pub enum Backend {
    Auto,
    Cpu,
    Metal,
    Vulkan,
    Cuda,
}
impl From<Backend> for transcribe_cpp::Backend {
    fn from(value: Backend) -> Self {
        match value {
            Backend::Auto => Self::Auto,
            Backend::Cpu => Self::Cpu,
            Backend::Metal => Self::Metal,
            Backend::Vulkan => Self::Vulkan,
            Backend::Cuda => Self::Cuda,
        }
    }
}
