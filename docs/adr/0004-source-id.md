# Folders are keyed on a source ID, not on date and title

A transcript folder is `<date>-<title-slug>-<id-suffix>`, and the skip check matches the source ID stored in `meta.json`, not the folder name. Date and title are not an identity: a local file has no upload date and got today's date on every run, two sources can share a date and title, and one X post gets different titles from the X API and from the yt-dlp fallback. Each case either re-transcribed a done source (orphaning its `lessons.md`) or skipped a new source and printed another source's transcript.

The source ID is:

- X post: `x:<status id>` from the URL, so the X API and the yt-dlp fallback agree.
- yt-dlp: `<extractor_key>:<id>`, lower-case extractor.
- Local file: `file:` plus a SHA-256 over the size and the first and last MiB. Hashing a multi-GB file in full costs seconds per run; media containers keep headers and indexes at the ends, so two different recordings with the same size and ends do not occur in practice. A path or mtime would change on a copy or a touch.

The folder suffix is the first 8 hex digits of SHA-256(ID). It keeps names readable and lets scribe find a folder by suffix when the title or date changed. A suffix shared by two IDs fails the second source instead of mixing them.

## Consequences

- Folders written before this change have no suffix. scribe reuses such a folder when its `meta.json` has no `id` and the same `source`; otherwise it creates a suffixed folder.
- Changing the ID scheme renames every folder in a library. Treat it as a migration.
