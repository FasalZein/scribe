---
name: scribe
description: Transcribe spoken media into a knowledge library and extract its lessons. Use when given a video, podcast, X post, playlist, channel or local media file to learn from; when the user wants only a transcript; when merging lessons into topic notes; or when answering a question from saved lessons and topic notes.
metadata:
  version: "0.2.1"
allowed-tools:
  - Bash(sh *scripts/install.sh)
  - Bash(powershell -ExecutionPolicy Bypass -File *scripts\install.ps1)
  - Bash(scribe *)
  - Bash(*/scribe *)
  - Bash(uvx yt-dlp@latest *)
  - Bash(yt-dlp *)
  - Bash(grep *)
---

# scribe

`scribe` turns a **source** (URL or local media file) into a **transcript** in the **library**. Helpers then do the **extraction**: they read the transcript and write its **lessons**. Lessons then merge into **topic notes**. Use these terms as written.

The library root is `$SCRIBE_LIBRARY`, default `~/Knowledge/scribe`. Resolve it once to an absolute path and use that path in every command and helper brief. Each source has one folder, named `<date>-<title>-<id>` (`<source-folder>` below):

```
sources/<source-folder>/index.md      metadata, Sources, Chapters, Parts table (read this)
sources/<source-folder>/parts/NN*.md  transcript slices, cut at chapters, at most about 2,500 words each (helpers read these)
sources/<source-folder>/transcript.md full transcript (for grep)
sources/<source-folder>/lessons.md    written by extraction
topics/<topic-slug>.md                merged lessons on one subject
topics/INDEX.md                       one line per topic note, then the single-source topic slugs
```

## Branches

- **New sources** (the default): steps 1-7.
- **Transcript only**, when the user asks for nothing more: steps 1-3, then report the `index.md` paths.
- **Merge only**, for lessons already in the library: step 6, then step 7.
- **Question** about what the sources said: [Answer from the library](#answer-from-the-library). No tools need installing.

## Steps

### 1. Check the tools

Run `sh <this skill's directory>/scripts/install.sh` (on Windows, `powershell -ExecutionPolicy Bypass -File <this skill's directory>\scripts\install.ps1`) on every run. It installs or updates scribe to this skill's version, then checks ffmpeg, ffprobe, and uvx or yt-dlp. It is fast when everything is ready. For each missing tool it prints an install command; run that command only with the user's approval, then run the script again. Build prerequisites per platform are in the [README](https://github.com/FasalZein/scribe#build-from-source).

When the only remaining problem is PATH, call scribe by the absolute path the script printed for the rest of this run.

Done when the script prints `ready: scribe <version>` and that version is at least this skill's `metadata.version`, or when PATH is the only problem and you use the absolute path.

### 2. Collect the sources and the focus

Gather every URL and file path the user gave. Expand a playlist or channel into video URLs (use `yt-dlp` in place of `uvx yt-dlp@latest` when uvx is missing):

```bash
uvx yt-dlp@latest --flat-playlist --print url "<playlist-or-channel-url>"
```

For a channel, use its `/videos` tab URL (`https://www.youtube.com/@name/videos`); the bare channel URL can list tabs instead of videos. When a list holds more than about 20 videos, tell the user the count and ask how many to take; pass `--playlist-end N` to take the first N. Remove duplicate URLs. Report an empty or failed expansion to the user.

For X, use the [browse-x](https://github.com/pc-style/x-md) skill when it is installed: search posts or read a profile to find posts with video, and read the replies or thread of a post for context. scribe reads the post text itself, so browse-x is optional.

Take the **focus** from the user's request when it asks a question of the sources ("how can X help me"). Ask only when the request hints at a focus but does not make it clear. Otherwise the focus is none.

Done when you hold a list of sources and the focus (or none).

### 3. Transcribe

```bash
scribe <source>...
```

For each source that succeeds or is skipped, stdout prints one absolute `index.md` path. A failed source prints no stdout line; stderr prints `scribe: <input>: <error>` for it. Pair each printed path with its source by the `source:` line in that index's frontmatter, not by line position. Progress goes to stderr.

- **Skip**: a source that already has an `index.md` is skipped and still printed. scribe finds the folder by a stable source ID, not by title or date, so re-running a list is safe. The skip check runs after the metadata fetch, so a network error such as HTTP 403 can fail a source that is already transcribed; run scribe again later.
- **HTTP 403**: when a YouTube source fails with HTTP 403, run scribe once more on that source. A second 403 is a failure.
- **Model load**: when stderr shows `model load failed`, the whole run stopped and the remaining sources were not attempted. Report the error and stop; a rerun fails the same way until its cause (disk space, network, backend) is fixed.
- **`--force`** redoes a transcript and keeps its `lessons.md`. Part boundaries and file names can move, so re-extract that source in step 5.
- **Other languages**: the default model knows English and other European languages. `--language <code>` passes a language hint to the model.

Time per 30 minutes of audio: about 15-20 s on Apple Silicon with Metal on an idle Mac, and 30-90 s when the Mac is busy. On the CPU (Linux, Windows), about 2.5 minutes on 6 cores, and up to 10 times more on a busy host. Add the download time. The first run also downloads the 740 MB model. A long batch can run past your shell timeout: run scribe in the background and poll, or transcribe in smaller groups. Re-runs skip finished sources, so a group can start again safely.

Done when every source has an `index.md` path, a reported failure, or a "not attempted" status after a model-load failure.

### 4. Read the indexes

For each index path, read the frontmatter `title`, `parts` and `words`, and check whether `lessons.md` exists beside it. Leave the Sources, Chapters and Parts table to the helpers.

Mark each source:

- **extract**: no `lessons.md`; or the user asked to redo it; or it was transcribed again with `--force`; or its `lessons.md` has no frontmatter (an interrupted extraction).
- **already extracted**: `lessons.md` has frontmatter.
- **no speech**: `parts: 0`. It gets no helper; name it in the report.

Done when each source has one mark.

### 5. Extract lessons, one helper per source

Start one helper per source marked **extract**, and at most one helper per index path. Run at most 6 helpers at a time; start the next when one finishes. Give each one this brief, with the values filled in:

```
Extract lessons from one transcript.
- Index: <absolute path to index.md>
- Focus: <focus or "none">
- Write: <same directory>/lessons.md
- Rules: read <absolute path to this skill>/reference/lessons.md first and follow it exactly.
- Existing topics: <absolute library path>/topics/INDEX.md, or "none yet"
Reply with the lessons.md path, the lesson count, and the focus answer in at most 3 sentences.
```

When you cannot start helpers, extract the sources yourself, one at a time: follow `reference/lessons.md` for one source, finish its `lessons.md`, then start the next.

Check each written `lessons.md` yourself:

- its frontmatter `lessons:` equals the number of `### L` headings (`grep -c '^### L' lessons.md`);
- every `at` link names a part file that exists in `parts/`.

Run a helper again once when a check fails; report the source when it fails again.

Done when every source marked **extract** has a `lessons.md` that passes both checks, or a reported failure.

### 6. Merge lessons into topic notes

Follow [`reference/topics.md`](reference/topics.md). One helper merges each topic note; you are the only writer of `topics/INDEX.md`.

Done when every topic slug on the new lessons is merged into a topic note or listed as single-source, and every helper's reply shows no lesson unaccounted for.

### 7. Report

Give the user, per source: the title, the lesson count, and the focus answer when a focus was set. Then list the topic notes you created or updated, with paths. Name each failed source with its error, each source not attempted, and each source with no speech.

## Answer from the library

1. Read `topics/INDEX.md`. Pick the topic notes and single-source slugs that bear on the question.
2. Read those topic notes.
3. Read the lessons the notes cite, in their `lessons.md` files. For a single-source slug, read the lessons tagged with it. Open a part file only to check an exact wording.

Answer from those lessons. Cite each point with the speaker, the source title, the `[hh:mm:ss]` timestamp linked to its part file, and the source URL from the lessons frontmatter. Say so when the library does not cover the question.
