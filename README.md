# scribe

A small Rust CLI that turns video URLs or local media into timestamped transcripts.
It supports YouTube, X, and other sites supported by yt-dlp. X status URLs use
the x.pcstyle.dev API first. An agent can later use
the transcript to write notes and skills. This CLI does not summarize the transcript.

## Install

Install Rust, CMake, a C++ compiler, **uv** (for `uvx`) or **yt-dlp**, and **ffmpeg**, then run:

```sh
cargo install --path .
```

On macOS, install Xcode Command Line Tools and the external tools:

```sh
xcode-select --install
brew install cmake yt-dlp ffmpeg
```

scribe selects `uvx yt-dlp@latest` when `uvx` is on PATH. Otherwise, it uses
plain `yt-dlp`. It logs the selected command once when a run needs yt-dlp.
Install [uv](https://docs.astral.sh/uv/getting-started/installation/) for automatic
latest-version selection. Keep plain yt-dlp current if you use the fallback.
Old extractors can fail with HTTP 403 errors on YouTube.
Some sites also require authentication or cookies. scribe inherits yt-dlp's normal
configuration, so configure cookies there if needed. YouTube extraction may also
need a JavaScript runtime supported by your yt-dlp version, such as Deno.
scribe explicitly enables Deno at `~/.deno/bin/deno` when that file exists.

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

The default model is
[Parakeet TDT 0.6B v3 Q8_0 GGUF](https://huggingface.co/handy-computer/parakeet-tdt-0.6b-v3-gguf).
The URL is pinned to Hugging Face commit `90f0824`, so the file cannot change
under scribe. It downloads 739,508,576 bytes on first use into the platform cache directory,
under `scribe/models/`. On macOS this is `~/Library/Caches/scribe/models/`.
Downloads stream into a `.part` file with a unique name per process, verify the
expected byte count (and, for the default model, its SHA-256), then rename into
place. A correctly sized cached model skips the download. Custom model URLs
use their HTTP content length when available, and their cache name carries a
SHA-256 prefix of the URL. Without a known size, scribe must
download again to avoid trusting an unverified cache entry.

The **model is licensed under CC-BY-4.0**, separately from this MIT-licensed CLI.
See the linked model card for attribution and license terms. Alternative models
have their own licenses and supported languages. A language hint does not add
language support to a model.

## Output and processing

Inputs run in order. Standard output contains only absolute `index.md` paths,
one per successful or skipped input. Progress and errors go to standard error. If
any input fails, scribe continues with the remaining inputs and exits non-zero. A
model-load failure stops the run because all later inputs depend on that model.
scribe loads the model only when the first input needs transcription.

### Library

Without `-o`, scribe writes into the library at `$SCRIBE_LIBRARY/sources`.
`SCRIBE_LIBRARY` defaults to `~/Knowledge/scribe`. With `-o DIR`, `DIR` is the
root that receives the transcript folders. scribe creates missing directories.

Each input creates `<root>/<YYYYMMDD>-<title-slug>-<id-suffix>/`. The suffix is
the first 8 hex digits of the SHA-256 of the source ID (see below), for example
`20260223-pi-durable-sessions-28bda125`.

- `index.md`: the entry file and the completion marker. YAML frontmatter (the `transcript.md` fields plus
  `parts`, `words` and `tokens_estimate`), title, Sources, optional Chapters, and
  a `## Parts` table. Each row has the part link, the time range, words,
  `tokens_estimate`, and the first 12 words of the part.
- `parts/NN.md`, or `parts/NN-<chapter-slug>.md` for a source with chapters:
  the transcript in parts, numbered from `01`. Part names never depend on the
  transcript words, so links to a part survive `--force`. Each
  part has the source title, `Part N of M`, the time range `[hh:mm:ss–hh:mm:ss]`,
  the chapter title if any, and then its `[hh:mm:ss] text` paragraphs.
- `transcript.md`: YAML frontmatter, title, Sources, optional Chapters, and all
  timestamped paragraphs.
- `segments.jsonl`: `{"start":0.0,"end":60.0,"text":"..."}` rows, in seconds.
- `meta.json`: the metadata subset used, including the source `id`, structured `sources` and
  `chapters` when present, or local file information.
- With `--keep-media`: `audio.f32le` (16 kHz, mono, little-endian float32) and,
  for yt-dlp URL inputs, the downloaded `media.<extension>`. X API streams do
  not create a downloaded media file, even with `--keep-media`.

Parts follow the source's chapters when it has any. A part holds whole engine
segments, assigned to the chapter that holds the segment midpoint. A chapter that holds no segment midpoint, such as a short silent intro, gets no part. A chapter longer
than about 2,500 words continues in more parts, named `NN-<chapter>-1`,
`NN-<chapter>-2`, and so on. Without chapters, a part ends at the first segment
boundary after it reaches 2,500 words, so a part can be slightly longer. `tokens_estimate` is
`round(words × 1.33)`, a rough size estimate for English text, not a tokenizer
count. `words` counts whitespace-separated words of the transcript text.

The date comes from the upload date, or today's UTC date when absent. The title
slug has at most 60 characters.

The source ID identifies a source independently of its title and date
(`docs/adr/0004-source-id.md`):

- X post: `x:<status id>`, also when the X API fails and yt-dlp is used.
- yt-dlp source: `<extractor_key>:<id>` in lower case, for example `youtube:UNP03fDSj1U`.
- Local file: `file:` and a SHA-256 over the file size and its first and last MiB.
  The same file at another path or on another day has the same ID.

scribe stores the ID in `meta.json` (`id`) and in the frontmatter (`source_id`).
Before it downloads or transcribes media, scribe looks for a folder whose suffix and
stored ID match. If that folder has `index.md`, scribe skips the input, logs
`skip: <path> exists (use --force)` and still prints the index path. For URLs,
the skip check still fetches metadata. Two sources with the same date and title
slug get different folders. A folder from before source IDs (no suffix, no `id`)
is reused when its `meta.json` has the same `source`.

Every output file is written to a temporary file and renamed into place.
`index.md` is renamed last, so an interrupted or failed run leaves no index and
the next run redoes the source. `--force` keeps the old transcript until the new
one is transcribed. Then it removes the index, parts, `audio.f32le` and `media.*`,
writes the new files, and keeps `lessons.md` and other files in the folder.
Temporary downloads are removed on normal success or error; an abrupt process
termination can leave a temporary directory.

### Truncation checks and timeouts

ffmpeg exits 0 for a truncated file or a dropped connection and reports the
problem only on standard error. scribe fails the source on any ffmpeg error
output (`-loglevel error -xerror`). It also fails the source when the decoded audio
is shorter than the reported duration by more than 5 s or 1 %, whichever is larger.
The slack covers whole-second durations from yt-dlp and audio tracks that end
slightly before the video. The reported duration comes from yt-dlp, the X API, or
`ffprobe` for local files. Without a duration, scribe warns and relies on the
ffmpeg error check.

Network waits are bounded, so one stalled source fails and the run continues:

- HTTP requests (X API, model download): 15-30 s connect and 60-120 s read timeouts.
- ffmpeg reading an X video URL: `-rw_timeout` of 30 s, and only the `https`,
  `tls` and `tcp` protocols, because a third-party API supplies the URL.
- yt-dlp: `--socket-timeout 30`.

### X API and source references

For `/<handle>/status/<id>` URLs on `x.com` or `twitter.com` (including `www.`
and `mobile.`), scribe requests JSON from `https://x.pcstyle.dev/api/convert`.
It selects the first video or GIF and streams its lowest-bitrate MP4 directly
through ffmpeg. It does not separately download that video or invoke yt-dlp.
A post without video or GIF fails with a clear error. HTTP 429 fails that input
and reports `Retry-After`; scribe does not retry. Network errors, other HTTP
errors, malformed JSON, and unusable video formats fall back to yt-dlp with a
reason on stderr.

Environment variables:

- `X_API_BASE`: override the API origin (default `https://x.pcstyle.dev`).
- `X_MD_API_KEY`: optional API token sent as `Authorization: Bearer <key>`.
  scribe does not log the token. Do not put tokens in command-line arguments.

X Sources include the post URL, author, date, full text as a blockquote,
expanded outbound links, mentioned profile URLs, and quoted post URLs and text
when supplied by the API. Links to the same post's media are omitted. Thread
replies are not treated as quoted posts. The title uses the author and the first
80 Unicode characters of post text. The directory date comes from `created_at`.

For yt-dlp inputs, Sources include `webpage_url`, the channel or uploader URL,
and HTTP(S) links from the description, deduplicated in order. Available chapters
appear as a separate `## Chapters` list with `[hh:mm:ss] title` entries. Local
Sources contain only the absolute file path. `meta.json` stores the same source
data under `sources`, tagged with `kind` (`x`, `web`, or `local`). If the X API
falls back to yt-dlp, source data follows the yt-dlp format.

### Audio processing

For other URL inputs, scribe calls yt-dlp once for metadata and downloads the
media from that saved JSON (`--load-info-json`), so the extractor runs once per
source. ffmpeg decodes audio into memory for all inputs.
It cuts at the quietest 30 ms window within the ten seconds before each target
boundary. A cut requires an RMS level below half the loudest window's RMS in that
search interval; flat signals use the hard boundary. Chunks are contiguous with no
overlap. Shorter target lengths search only within that chunk. The engine loads
one model per run and uses its batch API for chunks, reusing the model for later
inputs. Timestamps use engine segments when available, or chunk offsets otherwise.
The engine runs 16 chunks per batch, so its memory does not grow with the
recording. The decoded audio stays in memory (about 230 MB per hour). On the
27:26 talk, peak memory fell from 1.95 GB to 1.42 GB; on a 1:49:46 recording
it fell from 4.12 GB to 1.81 GB. A chunk shorter than 1 s joins the previous
chunk, because the engine rejects tiny chunks.
The default target is 30 s because transcribe-cpp's Parakeet drops whole sentences
from chunks near 60 s (see `docs/adr/0003-30-second-chunks.md`).

### Performance

The stages of one source overlap (see `docs/adr/0005-overlap-stages-within-a-source.md`):

- The model loads on its own thread while the media downloads and decodes. A
  skipped input never loads the model.
- ffmpeg streams samples to the main thread while it decodes. An engine batch
  starts as soon as 16 chunks are final, so most of the decode time hides behind
  transcription. The chunk cuts are the same as on the whole audio.
- The truncation check runs on the whole decoded audio, before the last batches
  and before any output.

`--timings` prints one line per input on stderr, for example:

```text
timings: metadata 0.20s, model 0.22s, download 0.00s, decode 5.23s, engine 12.47s, after-decode 0.14s, mel 0.04s, encode 9.15s, tdt-decode 3.01s, write 0.02s; total 16.83s
```

`model`, `download` and `decode` run in parallel with each other and with
`engine`, so the stages do not add up to `total`. `engine` is the time inside
engine batches; `after-decode` is the wall time from the end of decoding to the
last batch, which is the transcription that did not overlap. `mel`, `encode` and
`tdt-decode` are transcribe-cpp's own stage sums. Median wall time of three or four runs
on an M4 Pro (Metal, load average 5-12):

| source | before | after |
|---|---:|---:|
| local 27:26 mp4 | 14.36 s | 13.67 s |
| X post, 27:26 video | 24.55 s | 16.83 s |
| YouTube, 39:49 talk | 33.38 s | 27.31 s |
| YouTube, 3:27 talk | 9.29 s | 6.54 s |

One cold model load took 17 s instead of 0.2 s, while macOS rebuilt its Metal
shader cache (`$(getconf DARWIN_USER_CACHE_DIR)com.apple.metal`). The overlap
hides a cold load only as far as the download and decode last.

`engine_secs` is the time inside engine batches, not model loading,
downloading, decoding, chunk selection, or output writing. `duration_secs` measures decoded audio.
Local sources are absolute paths. `model` records the requested path or URL.

## Development

```sh
cargo build --release
cargo test
cargo clippy --all-targets -- -D warnings
cargo fmt --check
```

Unit tests exercise silence boundaries, pure-tone hard boundaries, shorter final
chunks, short audio, X URL detection, MP4 selection, source link extraction,
metadata, Sources/Chapters rendering, and splitting into parts (chapters,
oversized chapters, word limit, short and empty transcripts, no lost segments). Trimmed real API and yt-dlp JSON lives
under `tests/fixtures/`; tests also construct small edge cases. CI builds and tests on macOS (Metal) and Ubuntu (CPU).
The unit tests do not require a model or external media tools.
One ignored test guards against lost speech in long chunks. It needs a model and
the Pi durable-sessions talk (27:26), and runs in about 3 s on Metal:

```sh
SCRIBE_REGRESSION_MODEL=/path/to/parakeet-tdt-0.6b-v3-Q8_0.gguf \
SCRIBE_REGRESSION_MEDIA=/path/to/video.mp4 cargo test --release -- --ignored
```
