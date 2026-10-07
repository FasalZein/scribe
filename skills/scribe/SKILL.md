---
name: scribe
description: Transcribe spoken media into a knowledge library and extract its lessons. Use when given a video, podcast, X post, playlist, channel or local media file to learn from; when the user wants only a transcript; when merging lessons into topic notes; or when answering a question from saved lessons and topic notes.
metadata:
  version: "0.3.0"
allowed-tools:
  - Bash(sh *scripts/install.sh)
  - Bash(PATH=* sh *scripts/install.sh)
  - Bash(powershell -ExecutionPolicy Bypass -File *scripts\install.ps1)
  - Bash(powershell -ExecutionPolicy Bypass -Command *\scripts\install.ps1'")
  - Bash(scribe *)
  - Bash(*/scribe *)
  - Bash(uvx yt-dlp@latest *)
  - Bash(yt-dlp *)
---

# scribe

`scribe` turns a **source** (URL or local media file) into a **transcript** in the **library**. **Extraction** reads that transcript and writes its **lessons**. Lessons then merge into **topic notes**. Use these terms as written.

The library root is `$SCRIBE_LIBRARY`, default `~/Knowledge/scribe`. Resolve it once to an absolute path and use that path for the rest of the run. Export `SCRIBE_LIBRARY` to that path before `topics plan` and `topics index`. Each source has one folder, `<source-folder>`:

```
sources/<source-folder>/index.md          metadata, Sources, Chapters, Parts, Known terms, Low-confidence passages
sources/<source-folder>/parts/            transcript parts; the Parts table names each file
sources/<source-folder>/transcript.md     full transcript, for search
sources/<source-folder>/lessons.md        published lessons
sources/<source-folder>/lessons.draft.md  extraction in progress
topics/<topic-slug>.md                    merged lessons on one subject
topics/INDEX.md                           topic notes, then single-source slugs
```

`<scribe>` below means the absolute path from step 1. Use it for every scribe command, including commands inside a helper brief.

## Branches

- **New sources** (the default): steps 1-7.
- **Transcript only**, when the user wants nothing more: steps 1-3, then report the `index.md` paths.
- **Merge only**, for lessons already in the library: steps 1, 6, and 7.
- **Question** about what the sources said: [Answer from the library](#answer-from-the-library). Skip the install.

## Steps

### 1. Check the tools

On every run that calls scribe, run `sh <this skill's directory>/scripts/install.sh`. On Windows, run `powershell -ExecutionPolicy Bypass -File <this skill's directory>\scripts\install.ps1`.

The script installs or updates scribe and checks ffmpeg, ffprobe, and uvx or yt-dlp. It also runs `doctor` on the binary it checked. `doctor` reports the backend, devices, memory, tool versions, the model cache, and a short self-test. When the script prints a PATH export, run that assignment and the script as one command with no `;`. Use the install directory the script printed. A later command does not keep the assignment. On macOS and Linux:

```bash
PATH="<install-dir>:$PATH" sh <this skill's directory>/scripts/install.sh
```

On Windows, from the Bash tool, the backslashes keep Bash from expanding `$env`:

```bash
powershell -ExecutionPolicy Bypass -Command "\$env:PATH = '<install-dir>;' + \$env:PATH; & '<skill-dir>\scripts\install.ps1'"
```

From a PowerShell shell, write `` `$env:PATH `` in place of `\$env:PATH`.

When the script prints a package install command, run that command only with the user's approval, then run the script again. Build prerequisites are in the [README](https://github.com/FasalZein/scribe#build-from-source).

Take `<scribe>` from the last `ok: scribe <version> (<path>)` line. That path is the binary the script checked. An older `scribe` earlier on PATH is a different binary. Call the printed path, not the other one.

Done when the script exits 0. Exit 0 means ready.

### 2. Collect the sources and the focus

Gather every URL and file path the user gave. Expand a playlist or channel into video URLs. Use `yt-dlp` when `uvx` is missing:

```bash
uvx yt-dlp@latest --flat-playlist --print url "<playlist-or-channel-url>"
```

For a channel, use its `/videos` tab URL (`https://www.youtube.com/@name/videos`). The bare channel URL can list tabs instead of videos. When a list holds more than about 20 videos, tell the user the count and ask how many to take. Pass `--playlist-end N` to take the first N. Remove duplicate URLs. Report an empty or failed expansion to the user.

For X, use the [browse-x](https://github.com/pc-style/x-md) skill when it is installed. scribe reads the post text itself, so browse-x is optional.

Take the **focus** from the user's request when it asks a question of the sources ("how can X help me"). Ask only when the request hints at a focus and does not state it. Otherwise the focus is none.

Done when you hold the source list and the focus, or none.

### 3. Transcribe

```bash
<scribe> <source>...
```

For each source that succeeds or is skipped, stdout prints one absolute `index.md` path. A failed source prints no stdout line. stderr prints `scribe: <input>: <error>` for it. Pair each path with its source by the `source:` line in that index, not by line position. Other stderr lines are progress or a short summary. They are not a failure by themselves.

- **Skip**: an existing `index.md` is skipped and still printed. scribe finds the folder by source ID, not by title or date. The skip check follows the metadata fetch, so a network error such as HTTP 403 can fail a source that is already transcribed. Run that source again later.
- **HTTP 403**: run scribe once more on that YouTube source. A second 403 is a failure.
- **Model load**: when stderr contains `model load failed`, the run stopped. Later sources were not attempted. Report the error and stop.
- **`--force`** redoes a transcript and keeps the existing `lessons.md`. Before you run `--force` on a source, remove its `lessons.draft.md`. Part boundaries can move, so compare the part files. Before the command, read each `sources/*/index.md` until its `source:` line matches that input. Copy that folder's `parts/` directory outside the library. After the command, compare base names in the copy and in the new `parts/` directory. For each shared name, run `cmp -s` on the two files. The parts are unchanged when the two name sets are equal and every `cmp -s` exits 0. Stage times in `index.md` and `transcribed_at` in `transcript.md` change on every run. Ignore them. When no `parts/` directory existed, you kept no copy, or `cmp` cannot be run, the parts count as changed. Re-extract that source in step 5 only when the parts changed.
- **`--language <code>`** passes a language hint. The default model covers English and other European languages. Lessons stay in English.

A long batch can outlast a shell timeout. Measured examples for 30 minutes of audio are about 15-20 s of engine time on an idle Apple Silicon Mac, about 1-2 minutes on 6 Linux arm64 cores, and about 5 minutes on an x86_64 CPU with AVX2. An x86_64 CPU without AVX2 takes about as long as the audio. A busy host can take several times longer. The first run also downloads the model (about 740 MB). Run scribe in the background and poll, or transcribe in smaller groups. A rerun skips finished sources.

When a failure names the backend, a tool, or the model, run `<scribe> doctor` and include its output in the report.

Done when every source has an `index.md` path, a reported failure, or a not-attempted status after a model-load failure.

### 4. Mark each source

For each index, read frontmatter `title`, `parts`, and `words`. Give the source the first mark that fits:

1. **no speech**: `parts: 0`. Do not extract it. When `lessons.md` exists, run `<scribe> lessons check` on it. When that check exits 0, or when `lessons.md` is absent, name the source in the report. When the check fails, name the source and the `scribe:` error. Before step 6, ask the user whether to move `lessons.md` aside, for example to `lessons.stale.md`, and to remove topic-note bullets that link to it. Until that file is moved aside, `topics plan` and `topics index` fail for the whole library.
2. **extract**: `lessons.draft.md` exists, or the user asked to redo the source, or this run used `--force` and step 3 counted the parts as changed. When the user asks to redo a source, remove its `lessons.draft.md` once, before step 5. Leave a draft that step 5 has already written.
3. **extract**: `lessons.md` is absent, or `<scribe> lessons check <lessons.md>` exits non-zero.
4. **already extracted**: `lessons check` exits 0.

Done when each source has one mark.

### 5. Extract lessons

One source: extract it inline. Follow [`reference/lessons.md`](reference/lessons.md).

Several sources, when the harness can start helpers: start one general-purpose helper per source marked **extract**. The brief is the whole task. Run at most 6 helpers at a time, and at most one helper per index path. Give each this brief:

```
Extract lessons from one transcript.
- scribe: <absolute path from step 1>
- Index: <absolute path to index.md>
- Focus: <focus or "none">
- Draft: <same directory>/lessons.draft.md
- Leave <same directory>/lessons.md unchanged.
- Rules: read <absolute path to this skill>/reference/lessons.md first and follow it exactly.
- Existing topics: <absolute library path>/topics/INDEX.md, or "none yet"
Reply with either "ready" or "partial: <next part path>". On ready, add the lesson count, and the focus answer in at most 3 sentences when a focus is set.
```

No helpers: extract inline, one source at a time. Finish or pause that source before the next one. Drop its part text from your notes before you start the next source.

Publish a draft only when it has no `## Resume` section. When that section is present, resume extraction from its `next:` part and do not publish.

When the section is absent, publish the draft yourself:

1. Run `<scribe> lessons finalize <lessons.draft.md>`, then `<scribe> lessons check <lessons.draft.md>`.
2. When `lessons check` exits 0, replace the published file: `mv -f <lessons.draft.md> <lessons.md>`. Until that check exits 0, leave the existing `lessons.md` in place.
3. Run `<scribe> lessons check <lessons.md>`.

When the draft check fails, leave `lessons.md` unchanged and run the extraction once more with the `scribe:` error. Report the source when it fails again.

Done when every source marked **extract** has a `lessons.md` that passes `lessons check`, or a reported failure.

### 6. Merge lessons into topic notes

Follow [`reference/topics.md`](reference/topics.md).

When a source has `parts: 0` and its `lessons.md` still fails `lessons check`, wait for the user's answer before the commands below. Move that file aside only when the user agrees, and remove topic-note bullets that link to it. When the user does not agree, skip the commands below and report in step 7 that topic notes were not updated, naming the blocking file.

Run `<scribe> topics plan`. Start one helper per topic note to create or update, at most 6 at a time. When you cannot start helpers, merge the notes yourself, one at a time.

You publish `topics/INDEX.md` by running `<scribe> topics index`. That command is the only writer of the index.

Done when every topic slug on the new lessons is a topic note or a single-source line, every merged note accounts for every tagged lesson as described in the reference, `topics index` exits 0, and the new index has no `## Pending notes` entry for those slugs. When the user left the blocking file in place, this step is done by skipping it.

### 7. Report

Per source, give the title, the lesson count, and the focus answer when a focus was set. Then list the topic notes you created or updated, with paths. When step 6 was skipped, say that topic notes were not updated and name the blocking `lessons.md`. Name each failed source and its error, each source not attempted, and each source with no speech.

## Answer from the library

1. Read `topics/INDEX.md`. Pick the topic notes and single-source slugs that bear on the question.
2. Read those topic notes.
3. Read the lessons the notes cite. For a single-source slug, read the lessons tagged with it. Open a part only to check an exact wording.

Answer from those lessons. Cite each point with the speaker, the source title, the `[hh:mm:ss]` timestamp linked to its part, and the source URL from the lessons frontmatter. Say when the library does not cover the question.
