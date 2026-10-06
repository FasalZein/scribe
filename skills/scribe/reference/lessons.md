# Extraction rules

You are extracting lessons from one transcript. A **lesson** is one reusable piece of knowledge from the source, restated so it makes sense to someone who never saw the video.

## Read

1. Read `index.md`: frontmatter, Sources (post text, links, description), Chapters and the Parts table.
2. Read the topics index named under "Existing topics" in your brief, if it exists, so you reuse existing topic slugs.
3. Read every part in order; the last part often holds the conclusion or the call to action. After each part, append its lessons to `lessons.md`, so a long source never holds all of its lessons only in your context. Write the frontmatter at the top of the file and the Focus answer last, when every part is read. A `lessons.md` without frontmatter marks an unfinished extraction.

The transcript has no speaker labels. Identify speakers from the Sources section, the title, self-introductions and cues such as "as Armin said". You may use your own knowledge to name the person behind a public handle; add `(inferred)` after the name in `speakers`. Attribute each turn from such cues; when a turn's attribution is a guess, add `(inferred)` after the name in `who`. When there is no cue at all, write `speaker`.

Transcripts come from speech recognition. Names and terms are often misheard (for example "PyDurable" for "Pi Durable", "Burcell" for "Vercel"). Write the correct form in lessons when the context makes it clear. Note it under Open questions when the context does not make it clear.

Write lessons in English for every source. Keep the optional quote in the source language. When the transcript holds no readable speech (noise, music, or text that makes no sense), write no lessons and reply `no usable speech: <reason>`.

## Select

Keep a lesson when it would still help someone months later, without the video: a claim with its reasons, how something works, a practice to follow, or a trade-off between options. Leave out greetings, banter, sponsor reads, repetition and promises with no content.

When a **focus** is set, answer it first from the lessons, then capture everything else of lasting value.

## Write `lessons.md`

```markdown
---
source: <source URL or path, from index.md>
index: <relative path: index.md>
title: <title from index.md, on one line, cut at a word boundary if very long>
speakers: [<name>, ...]
focus: <focus or null>
extracted_at: <RFC 3339 time>
topics: [<every topic slug used below>]
lessons: <count>
---

# Lessons: <title>

## Focus answer

<Only when a focus is set: 2-6 sentences, each pointing to lesson IDs, for example (L3, L7).>

## Lessons

### L1. <short title that states the point>

- kind: claim | mechanism | practice | trade-off
- who: <speaker>
- at: [hh:mm:ss](parts/NN-<part>.md)
- topics: <slug>, <slug>
- verify: <what to check, and where>

<1-4 sentences that restate the lesson in your own words, with the reason or the condition it depends on.>

> "<at most one exact sentence from the transcript, when the wording itself matters>"

## Open questions

- <unclear terms, likely misrecognitions, claims that conflict, things the speakers left open>
```

## Rules for each lesson

- **kind**: a *claim* is an assertion or opinion; a *mechanism* explains how something works; a *practice* is a recommended way of acting; a *trade-off* compares options and their costs.
- **who**: every lesson names its speaker. When two speakers build the point together, write `A and B`. In the body, write "X argues..." or "X recommends...", so an opinion stays an opinion when it reaches a topic note.
- **at**: the `[hh:mm:ss]` timestamp of the paragraph the lesson comes from, linked to its part file. When a lesson draws on several paragraphs or parts, use the first one.
- **topics**: 1-3 lower-kebab slugs that name subjects, not sources (`durable-execution`, not `pi-durable-talk`). Reuse a slug from `topics/INDEX.md` when one fits.
- **verify**: the last bullet, present only when the lesson states a fact (a number, a version, a benchmark, a product capability) that the transcript alone cannot prove. Name what to check. Omit the bullet otherwise.
- One lesson holds one idea. Split a passage that makes two points. A run of numbers about one thing (one product, one benchmark) is one idea.
- Keep the speaker's conditions, exceptions and uncertainty in the body and in the Focus answer. A deliberate trade-off is not unfinished work.

## Done

Every part is read, every lesson has kind, who, at and topics, every `at` link names a part file that exists and holds that `[hh:mm:ss]` timestamp, the frontmatter `topics` list holds every topic slug used, and the frontmatter `lessons` count matches the number of `### L` headings.
