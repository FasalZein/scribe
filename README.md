# scribe

scribe turns a video URL (X, YouTube, or any site yt-dlp supports) or a local media file into a timestamped transcript in a Markdown library. It runs the Parakeet speech model locally, so no transcription service or API key is needed. The repository is also an agent skill: the agent runs scribe, splits the reading across helpers that write lessons, and merges the lessons into topic notes.

## Quick start

Install the skill for all your agents:

```sh
npx skills add FasalZein/scribe -g
```

The first time the agent uses the skill, it runs `scripts/install.sh` (`scripts/install.ps1` on Windows). You can run it yourself from the installed skill directory:

```sh
sh ~/.agents/skills/scribe/scripts/install.sh
```

The installer:

- downloads the prebuilt `scribe` for your OS and CPU from the [latest release](https://github.com/FasalZein/scribe/releases/latest), verifies its SHA-256, and installs it to `${SCRIBE_INSTALL_DIR:-$HOME/.local/bin}`;
- builds from source with `cargo install` when no prebuilt binary matches and cargo, cmake and a C++ compiler exist;
- checks ffmpeg, ffprobe, and `uvx` or `yt-dlp`, and prints the install command (brew, apt, dnf, pacman or winget) for each missing tool. It never runs a package manager or sudo.

It exits 0 when everything is ready. Run it again at any time; it changes nothing when scribe is current.

The first transcription downloads the speech model (740 MB) into the platform cache directory, under `scribe/models/` (`~/Library/Caches/scribe/models/` on macOS, `~/.cache/scribe/models/` on Linux).

### Optional companion: browse-x

[browse-x](https://github.com/pc-style/x-md) reads public X posts, threads, profiles and search results. With it, the agent can find X posts with video and read their replies for context:

```sh
npx skills add pc-style/x-md --skill browse-x -g
```

scribe does not need browse-x. It calls the same API (`https://x.pcstyle.dev/api/convert`) itself for X status URLs.

## Platform support

| Platform | Prebuilt binary | Backend | Measured speed |
|---|---|---|---|
| macOS, Apple Silicon | `aarch64-apple-darwin` | Metal | 27:26 video in 13.7 s (M4 Pro) |
| Linux x86_64 (glibc 2.35+) | `x86_64-unknown-linux-gnu` | CPU | not measured |
| Linux arm64 (glibc 2.35+) | `aarch64-unknown-linux-gnu` | CPU | 27:26 video in 135.6 s (OrbStack VM on the M4 Pro, 6 CPUs) |
| Windows x86_64 | `x86_64-pc-windows-msvc`, when its release build succeeds | CPU | not tested |
| macOS Intel, other | none; build from source | CPU | not tested |

Speeds are wall times for one local file, including model load and decoding. Linux runs vary with host load: a 3:00 clip took 16.4 s and 16.9 s, and 162 s on a busier host. On the CPU, memory peaks at 2.2 GB for the 3:00 clip, 3.3 GB for 10:00 and 3.4 GB for 27:26 (Metal: 1.4 GB for 27:26). The Linux binaries use the CPU backend; Vulkan and CUDA need a build from source.

## Use

```sh
scribe 'https://www.youtube.com/watch?v=UNP03fDSj1U'
scribe talk.mp4 another-talk.m4a -o ./transcripts -l en
scribe talk.mp4 -m /path/to/model.gguf --backend cpu --timings
scribe 'https://x.com/pidotdev/status/2107033061905104941' --keep-media
```

```text
scribe [OPTIONS] <INPUT>...
-o, --out DIR          Output root (default $SCRIBE_LIBRARY/sources)
    --force            Redo an existing transcript; keeps lessons.md
-m, --model PATH|URL   GGUF model path or URL
-l, --language CODE    Language hint passed to the engine
    --chunk-secs N     Positive target chunk length in seconds (default 30)
    --keep-media       Keep downloaded media and decoded audio
    --backend NAME     auto|cpu|metal|vulkan|cuda (default auto)
    --timings          Print per-stage wall times on stderr
-h, --help
-V, --version
```

- Inputs run in order. stdout prints one absolute `index.md` path per input that succeeded or was skipped. Progress and errors go to stderr.
- A failed input does not stop the others; scribe exits non-zero at the end. A model-load failure stops the run.
- An input that already has an `index.md` is skipped and its path is still printed. scribe finds the folder by a stable **source ID** (X status ID, yt-dlp extractor and ID, or a content hash of a local file), so re-running a list is safe. `--force` redoes the transcript and keeps `lessons.md`.
- `--backend auto` lets the engine choose. An explicit backend does not fall back; it must be compiled in and available.

Environment variables:

| Variable | Effect |
|---|---|
| `SCRIBE_LIBRARY` | Library root (default `~/Knowledge/scribe`) |
| `SCRIBE_INSTALL_DIR` | Install directory for `scripts/install.sh` (default `~/.local/bin`) |
| `X_API_BASE` | X API origin (default `https://x.pcstyle.dev`) |
| `X_MD_API_KEY` | Optional X API token, sent as `Authorization: Bearer <key>` and never logged |

scribe runs `uvx yt-dlp@latest` when `uvx` is on PATH, and plain `yt-dlp` otherwise. It inherits yt-dlp's configuration, so put cookies for sites that need a login there.

## Library layout

```text
$SCRIBE_LIBRARY/
  sources/<YYYYMMDD>-<title-slug>-<8 hex>/
    index.md        frontmatter, Sources, Chapters, Parts table; written last
    parts/NN.md     transcript slices of about 2,500 words (NN-<chapter>.md with chapters)
    transcript.md   full transcript with [hh:mm:ss] paragraphs
    segments.jsonl  {"start":0.0,"end":60.0,"text":"..."} per engine segment
    meta.json       source ID, Sources, Chapters, duration and engine data
    lessons.md      written by the agent, kept by --force
  topics/<topic>.md merged lessons on one subject
  topics/INDEX.md   one line per topic
```

- `index.md` is the completion marker. Every file is written to a temporary file and renamed, and `index.md` comes last, so an interrupted run leaves no index and the next run redoes the source.
- Each row of the Parts table gives the time range, the word count, `tokens_estimate` (`round(words × 1.33)`) and the first 12 words.
- Sources list the post or page URL, the author or channel, and links from the post or description. Chapters come from yt-dlp.
- scribe fails a source when ffmpeg reports an error or the decoded audio is shorter than the reported duration by more than 5 s or 1 %. This catches truncated downloads.

## Agent workflow

[`SKILL.md`](SKILL.md) is the procedure; [`GLOSSARY.md`](GLOSSARY.md) defines the terms.

1. **Index.** scribe writes the transcript; the agent reads each `index.md` only, so its own context stays free for many sources.
2. **Helper.** One helper per source reads the parts and writes `lessons.md`, following [`reference/lessons.md`](reference/lessons.md).
3. **Lessons.** Each lesson carries topic tags and timestamps that link back to the source.
4. **Topic notes.** The agent merges lessons into `topics/<topic>.md`, following [`reference/topics.md`](reference/topics.md).

## Build from source

All platforms need Rust (stable, edition 2024), CMake and a C++ compiler. At run time, scribe needs ffmpeg with ffprobe, and uv (for `uvx`) or a current yt-dlp.

| Platform | Prerequisites |
|---|---|
| macOS | `xcode-select --install`, then `brew install cmake ffmpeg uv` |
| Debian, Ubuntu | `sudo apt-get install -y build-essential cmake ffmpeg`, plus uv: `curl -LsSf https://astral.sh/uv/install.sh \| sh` |
| Fedora | `sudo dnf install -y gcc-c++ make cmake ffmpeg uv` |
| Arch | `sudo pacman -S --needed base-devel cmake ffmpeg uv` |
| Windows | Visual Studio Build Tools with the C++ workload, `winget install Kitware.CMake Gyan.FFmpeg astral-sh.uv` |

```sh
cargo install --locked --path .
```

The build compiles the speech engine ([transcribe-cpp](https://crates.io/crates/transcribe-cpp), with ggml) and takes a few minutes. macOS builds include Metal. Other platforms default to the CPU backend; add a GPU backend with a feature:

```sh
# Vulkan: Vulkan SDK (headers, loader, glslc) and a GPU driver
cargo install --locked --path . --features vulkan
scribe --backend vulkan talk.mp4

# CUDA: NVIDIA CUDA toolkit (nvcc) and a matching driver
cargo install --locked --path . --features cuda
scribe --backend cuda talk.mp4
```

A local build tunes ggml for the build machine's CPU. The release binaries and `scripts/linux-test.sh` build with `TRANSCRIBE_CMAKE_ARGS=-DGGML_NATIVE=OFF`, so they run on other machines of the same architecture. Use the same setting when a native build fails, for example with GCC 12 in an arm64 VM (`inlining failed in call to 'always_inline' 'vfmaq_f16'`).

## Troubleshooting

| Symptom | Cause and fix |
|---|---|
| YouTube download fails with `HTTP Error 403` | YouTube blocks some requests and old yt-dlp extractors. Install uv so scribe runs `uvx yt-dlp@latest`, or update yt-dlp. Then run scribe again on that source; a retry often succeeds. Some videos also need a JavaScript runtime such as Deno; scribe enables `~/.deno/bin/deno` when it exists. |
| The first run on a Mac spends 15-20 s in model load | macOS rebuilds its Metal shader cache (`$(getconf DARWIN_USER_CACHE_DIR)com.apple.metal`). Later loads take about 0.2 s. `--timings` shows the `model` stage. |
| `ffprobe` not found, or a warning that the duration is unknown | Install ffmpeg with ffprobe. scribe reads local file durations with ffprobe to detect truncated files. |
| `scribe: command not found` after install | Add the install directory to PATH: `export PATH="$HOME/.local/bin:$PATH"`. The installer prints this hint. |
| X post fails with HTTP 429 | The X API rate-limits requests. Wait for the reported `Retry-After`; scribe does not retry. |

## Design decisions

The decisions and their measurements are in [`docs/adr/`](docs/adr/): the agent extracts while the binary transcribes (0001), transcribe-cpp over sherpa-onnx (0002), 30-second chunks (0003), source IDs (0004), and overlapping stages within a source (0005).

## License

The code is MIT ([`LICENSE`](LICENSE)). The default model, [Parakeet TDT 0.6B v3 Q8_0 GGUF](https://huggingface.co/handy-computer/parakeet-tdt-0.6b-v3-gguf), is licensed under CC-BY-4.0, separately from the code. scribe pins it to a Hugging Face commit and verifies its size and SHA-256 after download.
