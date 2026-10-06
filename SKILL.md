---
name: scribe
description: Transcribe videos and extract lessons into the knowledge library. Use when given an X post, YouTube or other video or podcast URL, or a local media file to learn from; when queuing many sources, a playlist or a channel; or when merging lessons into topic notes.
---

# scribe

`scribe` turns a **source** (URL or local media file) into a **transcript** in the **library**. Helpers then do the **extraction**: they read the transcript and write its **lessons**. Lessons then merge into **topic notes**. Terms are defined in [`GLOSSARY.md`](GLOSSARY.md); use them as written.

The library root is `$SCRIBE_LIBRARY`, default `~/Knowledge/scribe`:

```
sources/<slug>/index.md        metadata, Sources, Chapters, Parts table (read this)
sources/<slug>/parts/NN-*.md   transcript slices, about 2,500 words each (helpers read these)
sources/<slug>/transcript.md   full transcript (for grep)
sources/<slug>/lessons.md      written by extraction
topics/<topic>.md              merged lessons on one subject
topics/INDEX.md                one line per topic
```

## Steps

### 1. Check the tools

Run `scribe --version`, `ffmpeg -version` and `uvx --version`. If `scribe` is missing, install it with `cargo install --path <this skill's directory>`; [`README.md`](README.md) covers the prerequisites per platform.

Done when all three commands print a version.

### 2. Collect the sources and the focus

Gather every URL and file path the user gave. Expand a playlist or channel into video URLs:

```bash
uvx yt-dlp@latest --flat-playlist --print url "<playlist-or-channel-url>"
```

Ask for a **focus** only when the user's request implies one ("how can X help me" is a focus). A focus is optional.

Done when you hold a list of sources and the focus (or none).

### 3. Transcribe

```bash
scribe <source>...
```

stdout prints one absolute `index.md` path per source, in order. Progress goes to stderr. A source that already has an `index.md` is skipped and still printed, so re-running a list is safe. `--force` redoes a transcript and keeps its `lessons.md`. When a source fails, scribe continues with the others and exits non-zero; report the stderr line for each failed source.

A 30-minute video takes about 15-20 s to transcribe on Apple Silicon, plus download time. The first run downloads the 740 MB model.

Done when every source has an `index.md` path or a reported failure.

### 4. Read the indexes

Read each `index.md`: its frontmatter, Sources, Chapters and Parts table. The parts belong to the helpers in step 5, so the main context stays free for many sources.

Done when you know each source's title, speakers (from Sources and the title), size in tokens and whether `lessons.md` already exists.

### 5. Extract lessons, one helper per source

Start one helper per source that has no `lessons.md`, or whose lessons the user asked to redo. Launch the helpers together. Give each one this brief, with the values filled in:

```
Extract lessons from one transcript.
- Index: <absolute path to index.md>
- Focus: <focus or "none">
- Write: <same directory>/lessons.md
- Rules: read <absolute path to this skill>/reference/lessons.md first and follow it exactly.
- Existing topics: <absolute path to $SCRIBE_LIBRARY/topics/INDEX.md, or "none yet">
Reply with the lessons.md path, the lesson count, and the focus answer in at most 3 sentences.
```

Done when every source has a `lessons.md` and each helper reported its count.

### 6. Merge lessons into topic notes

Follow [`reference/topics.md`](reference/topics.md).

Done when every topic tag on the new lessons is either merged into a topic note or recorded as single-source.

### 7. Report

Give the user, per source: the title, the lesson count, and the focus answer when a focus was set. Then list the topic notes you created or updated, with paths. Name any failed source and its error.
