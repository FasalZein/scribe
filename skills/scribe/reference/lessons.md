# Extraction rules: lesson format 2

A **lesson** is one reusable piece of knowledge from a source. Restate it so it makes sense without the transcript.

## Read and append

1. Read `index.md`: frontmatter, Sources, Chapters and the Parts table.
2. Read the topics index named under "Existing topics" in your brief, if it exists. Reuse an existing topic slug when it fits.
3. Read every part in order. Use the file names in the Parts table. Sources without chapters use `parts/NN.md`. Chapter parts use `parts/NN-<part>.md`, where `<part>` is the chapter slug. A chapter split into several parts adds a piece number, for example `parts/02-long-chapter-1.md`.
4. Create the frontmatter below, then append each part's lessons after reading that part. Continue numbering across parts. Leave `lessons` and frontmatter `topics` for `scribe lessons finalize` to compute.
5. After reading every part, write the Focus answer when a focus is set. Then finalize and check the file as described under Done.

The transcript has no speaker labels. Identify speakers from Sources, the title, self-introductions and cues such as "as Armin said". You may name the person behind a public handle from your own knowledge; add `(inferred)` in `speakers`. Add `(inferred)` in `who` when attribution is a guess. Write `speaker` when there is no cue.

Speech recognition can mishear names and terms. Use known terms from `index.md` and source context to correct them in lessons. Preserve the transcript. Record unresolved spellings under Open questions. Check low-confidence passages before quoting them.

Write lessons in English. Keep an optional quote in the source language. If the transcript has no readable speech, write no lessons and report `no usable speech: <reason>`.

## Select

Keep knowledge that will help months later: an assertion with its reasons, an explanation, complete steps, a rule of thumb, a comparison, or an example. Leave out greetings, banter, sponsor reads, repetition and promises without content.

When a **focus** is set, answer it from the lessons. Also capture the source's other lasting knowledge.

## Write `lessons.md`

Use flat frontmatter fields. Write frontmatter `topics` as a comma-separated list in brackets; the finalizer writes this list for you.

```markdown
---
source: <source URL or path from index.md>
index: index.md
title: <title from index.md, on one line>
speakers: [<name>, ...]
focus: <focus or null>
extracted_at: <RFC 3339 time>
---

# Lessons: <title>

## Focus answer

<Only when a focus is set: 2-6 sentences, each pointing to lesson IDs, for example (L3, L7).>

## Lessons

### L1
<short title that states the point>

- kind: claim
- who: <speaker>
- at: [00:00:05](parts/01.md)
- topics: <slug>, <slug>
- verify: <what to check, and where; omit when not needed>

<Restate the lesson with its reasons, conditions, exceptions and uncertainty.>

> "<At most one exact sentence, when the wording itself matters.>"

## Open questions

- <Unclear terms, possible misrecognitions, conflicting claims or unresolved questions.>
```

### Anchors and titles

Use `### L<n>` alone, starting at L1 and increasing by one. Put the title on the next line. Link to a lesson with `lessons.md#l<n>`, for example `lessons.md#l1`. A title change leaves this anchor unchanged. Format 1 headings such as `### L1. Title` do not pass the checker.

### Kinds and bodies

Choose one of these six kinds:

| Kind | Use for |
| --- | --- |
| `claim` | An assertion or opinion, with its reasons. |
| `explanation` | How or why something works. |
| `procedure` | Complete numbered steps someone can follow. Include amounts, conditions and stopping criteria where needed. |
| `heuristic` | A rule of thumb, with the conditions where it helps. |
| `trade-off` | Options and their costs or benefits. |
| `example` | A concrete case that illustrates reusable knowledge. |

A procedure body uses numbered steps from 1 in order:

```markdown
### L2
Thicken a wet mixture gradually

- kind: procedure
- who: Cook
- at: [00:10:00](parts/02-mixing.md)
- topics: cooking

1. Add 10 g flour if the mixture runs off the spoon.
2. Stir for 30 seconds, then check the texture again.
3. Repeat until the mixture coats the spoon.
```

Keep one idea per lesson. Split a passage that makes two points. Keep related measurements about one product or benchmark together. Use Mermaid fenced code blocks when a diagram helps. The checker ignores lesson-like text inside code fences.

The checker verifies numbered procedure steps, not whether they are complete or correct. Check completeness against the source yourself.

### Metadata

- **who**: Name the speaker for every lesson. Write `A and B` when speakers build the point together. Attribute opinions in the body, for example "X argues...".
- **at**: Use the exact `[hh:mm:ss]` timestamp at the start of the transcript paragraph. Link to its part file relative to `lessons.md`. Use the first paragraph when a lesson draws on several paragraphs. A time mentioned only in prose is not a citation timestamp.
- **topics**: Write 1-3 comma-separated lower-kebab slugs. Name subjects, not sources: `durable-execution`, not `pi-durable-talk`. Reuse a slug from the existing topics index when one fits.
- **verify**: Add this optional bullet when a fact needs evidence beyond the transcript, such as a benchmark or product capability. Name what to check and where. Omit it otherwise.

## Done

Run these commands after every part is read and the lessons are complete:

```sh
scribe lessons finalize <path/to/lessons.md>
scribe lessons check <path/to/lessons.md>
```

`finalize` validates lesson bodies and citations before replacing the file. It writes the `lessons:` count and the sorted, unique `topics:` list. It preserves other frontmatter fields and the body. A body-only draft also works, but you must add source metadata yourself.

`check` reads without writing. It validates the count, sequential anchors, title lines, required metadata, kinds, numbered procedure steps, topic list, part links and exact paragraph timestamps. Both commands exit 0 on success. They exit nonzero and print `scribe: <reason>` on failure. Neither command loads a model or calls an LLM.

Fix every reported error, then rerun both commands. Completion requires a successful check, every part read, and source conditions preserved in the lessons and Focus answer.
