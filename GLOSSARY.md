# Scribe

Scribe turns spoken media into transcripts, and an agent extracts lessons from them for later reuse.

## Language

### Media and transcripts

**Source**:
The video or audio a transcript comes from: a URL (X post, YouTube, other yt-dlp site) or a local media file.
_Avoid_: input, media, video (when the origin is meant)

**Transcript**:
The timestamped text of one source, with its metadata and links back to the source.
_Avoid_: captions, subtitles

**Index**:
The entry file of a transcript that lists its metadata, its source links and its parts, with size estimates for each part.
_Avoid_: table of contents, manifest

**Part**:
One contiguous slice of a transcript, cut at a chapter or at a size limit, small enough to read in one step.
_Avoid_: chunk (a chunk is the audio slice the speech engine processes), section, page

### Knowledge

**Extraction**:
The act of reading a transcript and writing down its lessons. One helper does the extraction for one source.
_Avoid_: distillation, digest, summary, mining

**Lesson**:
One reusable piece of knowledge from a source, restated so it makes sense without the transcript. It names who said it, its kind (claim, mechanism, practice or trade-off) and the timestamp in the source.
_Avoid_: insight, takeaway, finding, distillate

**Focus**:
An optional question an extraction answers first, before it captures the rest of the source's lasting value.
_Avoid_: goal, prompt, lens

**Topic note**:
A note that merges the lessons on one subject from several sources, with links back to each lesson.
_Avoid_: synthesis, wiki page, summary

**Library**:
The one root directory where every transcript, lessons file and topic note lives across runs.
_Avoid_: vault, output dir, knowledge base
