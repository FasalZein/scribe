# scribe

A small Rust CLI that turns video URLs or local media into timestamped transcripts.
It supports YouTube, X, and other sites supported by yt-dlp. An agent can later use
the transcript to write notes and skills. This CLI does not summarize the transcript.

## Install

Install Rust, CMake, a C++ compiler, **yt-dlp**, and **ffmpeg**, then run:

```sh
cargo install --path .
```

On macOS, install Xcode Command Line Tools and the external tools:

```sh
xcode-select --install
brew install cmake yt-dlp ffmpeg
```

Keep yt-dlp current. Old extractors can fail with HTTP 403 errors on YouTube.
Some sites also require authentication or cookies. scribe inherits yt-dlp's normal
configuration, so configure cookies there if needed. YouTube extraction may also
need a JavaScript runtime supported by your yt-dlp version, such as Deno.

### Linux CPU, Vulkan, and CUDA

CPU is the default on non-macOS platforms. For example, on Ubuntu:

```sh
sudo apt-get install build-essential cmake ffmpeg
# Install a current yt-dlp using its official instructions.
cargo install --path .
```

For Vulkan, install the Vulkan SDK (headers, loader, and `glslc` shader compiler),
plus a working GPU driver, then build with:

```sh
cargo install --path . --features vulkan
scribe --backend vulkan talk.mp4
```

For CUDA, install NVIDIA's CUDA toolkit (including `nvcc`) and a matching driver:

```sh
cargo install --path . --features cuda
scribe --backend cuda talk.mp4
```

Metal is enabled only on macOS. `--backend auto` lets the engine select a backend;
`cpu`, `metal`, `vulkan`, or `cuda` explicitly requests that backend without fallback.
A backend must be included at build time and available on the machine.

## Use

```sh
scribe 'https://www.youtube.com/watch?v=UNP03fDSj1U'
scribe talk.mp4 another-talk.m4a -o ./transcripts -l en
scribe talk.mp4 -m /path/to/model.gguf --chunk-secs 60 --backend cpu
scribe 'https://x.com/pidotdev/status/2107033061905104941' --keep-media
```

Options:

```text
scribe [OPTIONS] <INPUT>...
-o, --out DIR          Output root (default ./scribe-out)
-m, --model PATH|URL   GGUF model path or URL
-l, --language CODE    Language hint passed to the engine
    --chunk-secs N     Positive target chunk length in seconds (default 60)
    --keep-media       Keep downloaded media and decoded audio
    --backend NAME     auto|cpu|metal|vulkan|cuda (default auto)
-h, --help
-V, --version
```

The default model is
[Parakeet TDT 0.6B v3 Q8_0 GGUF](https://huggingface.co/handy-computer/parakeet-tdt-0.6b-v3-gguf).
It downloads 739,508,576 bytes on first use into the platform cache directory,
under `scribe/models/`. On macOS this is `~/Library/Caches/scribe/models/`.
Downloads stream into a `.part` file, verify the expected byte count, then rename
into place. A correctly sized cached model skips the download. Custom model URLs
use their HTTP content length when available. Without a known size, scribe must
download again to avoid trusting an unverified cache entry.

The **model is licensed under CC-BY-4.0**, separately from this MIT-licensed CLI.
See the linked model card for attribution and license terms. Alternative models
have their own licenses and supported languages. A language hint does not add
language support to a model.

## Output and processing

Inputs run in order. Standard output contains only absolute `transcript.md` paths,
one per successful input. Progress and errors go to standard error. If any input
fails, scribe continues with the remaining inputs and exits non-zero. A model-load
failure stops the run because all inputs depend on that model.

Each input creates `<out>/<YYYYMMDD>-<title-slug>/`:

- `transcript.md`: YAML frontmatter and timestamped paragraphs.
- `segments.jsonl`: `{"start":0.0,"end":60.0,"text":"..."}` rows, in seconds.
- `meta.json`: the metadata subset used, or local file information.
- With `--keep-media`: `audio.f32le` (16 kHz, mono, little-endian float32) and,
  for URL inputs, the downloaded `media.<extension>`.

The date comes from the upload date, or today's UTC date when absent. The title
slug has at most 60 characters. Existing output directories cause a clear error
instead of overwriting transcripts. Choose another output root for repeated runs
or colliding titles. A failed transcription can leave an incomplete output
directory. Temporary downloads are removed on normal success or error; an abrupt
process termination can leave a temporary directory.

scribe calls yt-dlp for metadata and media, then ffmpeg to decode audio into memory.
It cuts at the quietest 30 ms window within the ten seconds before each target
boundary. A cut requires an RMS level below half the loudest window's RMS in that
search interval; flat signals use the hard boundary. Chunks are contiguous with no
overlap. Shorter target lengths search only within that chunk. The engine loads
one model per run and uses its batch API for chunks, reusing the model for later
inputs. Timestamps use engine segments when available, or chunk offsets otherwise.
Audio and batch state remain in memory, so very long recordings need more RAM.

`engine_secs` measures chunk selection and transcription, not model loading,
downloading, decoding, or output writing. `duration_secs` measures decoded audio.
Local sources are absolute paths. `model` records the requested path or URL.

## Development

```sh
cargo build --release
cargo test
cargo clippy --all-targets -- -D warnings
```

Unit tests exercise silence boundaries, pure-tone hard boundaries, shorter final
chunks, and short audio. CI builds and tests on macOS (Metal) and Ubuntu (CPU).
The unit tests do not require a model or external media tools.
