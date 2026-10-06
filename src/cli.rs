use clap::{Parser, Subcommand, ValueEnum};
use std::{num::NonZeroU32, path::PathBuf};

/// Pinned to a Hugging Face commit, so the fixed size and SHA-256 in fetch.rs stay valid.
pub const DEFAULT_MODEL: &str = "https://huggingface.co/handy-computer/parakeet-ultra-gguf/resolve/39eeb55181f0d354fd934f06e92fd8d5037fed8e/parakeet-ultra-Q8_0.gguf";

#[derive(Parser)]
#[command(
    version,
    about = "Turn video URLs or local media into timestamped transcripts",
    subcommand_negates_reqs = true
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Command>,
    /// URL or local media file path; processed in order
    #[arg(required = true, value_name = "INPUT")]
    pub inputs: Vec<String>,
    /// Output root (default: $SCRIBE_LIBRARY/sources or ~/Knowledge/scribe/sources)
    #[arg(short, long, value_name = "DIR")]
    pub out: Option<PathBuf>,
    /// Redo existing transcripts, preserving lessons.md
    #[arg(long)]
    pub force: bool,
    /// GGUF model path or URL
    #[arg(short, long, default_value = DEFAULT_MODEL, value_name = "PATH|URL")]
    pub model: String,
    /// Language hint passed to the engine
    #[arg(short, long, value_name = "CODE")]
    pub language: Option<String>,
    /// Target chunk length in seconds
    // transcribe-cpp Parakeet drops whole sentences from chunks near 60 s;
    // 30 s had the lowest word error rate in docs/adr/0003-30-second-chunks.md.
    #[arg(long, default_value = "30", value_name = "N")]
    pub chunk_secs: NonZeroU32,
    /// Keep downloaded media and decoded 16 kHz mono f32 audio
    #[arg(long)]
    pub keep_media: bool,
    /// Compute backend (explicit selections do not fall back)
    #[arg(long, value_enum, default_value = "auto", value_name = "NAME")]
    pub backend: Backend,
    /// CPU threads for the engine; 0 picks a default per backend
    // On a GPU backend only the TDT decoder runs on the CPU; on the CPU backend this
    // also sets the encoder threads. See docs/adr/0006-decoder-threads.md.
    #[arg(long, default_value = "0", value_name = "N")]
    pub threads: u16,
    /// Print per-stage wall times on stderr
    #[arg(long)]
    pub timings: bool,
}

#[derive(Subcommand)]
pub enum Command {
    /// Plan topic notes or rebuild their index (no model needed)
    Topics {
        #[command(subcommand)]
        action: TopicsAction,
    },
    /// Validate or finalize a format 2 lessons file (no model needed)
    Lessons {
        #[command(subcommand)]
        action: LessonsAction,
    },
}

#[derive(Subcommand)]
pub enum TopicsAction {
    /// List topic slugs, lessons, note status and near-duplicates
    Plan,
    /// Rebuild topics/INDEX.md from topic notes and lessons
    Index,
}

#[derive(Subcommand)]
pub enum LessonsAction {
    /// Validate lesson counts, anchors, kinds, topics and citations
    Check { file: PathBuf },
    /// Write the lesson count and topic list after validating lessons
    Finalize { file: PathBuf },
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

impl Cli {
    pub fn output_root(&self) -> anyhow::Result<PathBuf> {
        if let Some(root) = &self.out {
            return Ok(root.clone());
        }
        Ok(Self::library_root()?.join("sources"))
    }

    pub fn library_root() -> anyhow::Result<PathBuf> {
        let library = match std::env::var_os("SCRIBE_LIBRARY") {
            Some(root) => PathBuf::from(root),
            None => dirs::home_dir()
                .ok_or_else(|| {
                    anyhow::anyhow!("home directory is unavailable; set SCRIBE_LIBRARY or --out")
                })?
                .join("Knowledge/scribe"),
        };
        Ok(library)
    }
}
