# Extraction rules

You are extracting lessons from one transcript. A **lesson** is one reusable piece of knowledge from the source, restated so it makes sense to someone who never saw the video.

## Read

1. Read `index.md`: frontmatter, Sources (post text, links, description), Chapters and the Parts table.
2. Read `topics/INDEX.md` if it exists, so you reuse existing topic slugs.
3. Read every part in order. Take notes as you go; the last part often holds the conclusion or the call to action.

Identify speakers from the Sources section, the title and the way speakers introduce themselves. The transcript has no speaker labels. When a turn cannot be attributed, write `speaker`.

Transcripts come from speech recognition. Names and terms are often misheard (for example "PyDurable" for "Pi Durable", "Burcell" for "Vercel"). Write the correct form in lessons when the context makes it clear. Note it under Open questions when the context does not make it clear.

## Select

Keep a lesson when it would still help someone months later, without the video: a claim with its reasons, how something works, a practice to follow, or a trade-off between options. Leave out greetings, banter, sponsor reads, repetition and promises with no content.

When a **focus** is set, answer it first from the lessons, then capture everything else of lasting value.

## Write `lessons.md`

```markdown
---
source: <source URL or path, from index.md>
index: <relative path: index.md>
title: <title>
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
- verify: <what to check, and where>   (only when it applies)

<1-4 sentences that restate the lesson in your own words, with the reason or the condition it depends on.>

> "<at most one exact sentence from the transcript, when the wording itself matters>"

## Open questions

- <unclear terms, likely misrecognitions, claims that conflict, things the speakers left open>
```

## Rules for each lesson

- **kind**: a *claim* is an assertion or opinion; a *mechanism* explains how something works; a *practice* is a recommended way of acting; a *trade-off* compares options and their costs.
- **who**: every lesson names its speaker. Write "X argues..." or "X recommends...", so an opinion stays an opinion when it reaches a topic note.
- **at**: the `[hh:mm:ss]` timestamp of the paragraph the lesson comes from, linked to its part file.
- **topics**: 1-3 lower-kebab slugs that name subjects, not sources (`durable-execution`, not `pi-durable-talk`). Reuse a slug from `topics/INDEX.md` when one fits.
- **verify**: add it when the lesson states a fact (a number, a version, a benchmark, a product capability) that the transcript alone cannot prove. Name what to check.
- One lesson holds one idea. Split a passage that makes two points.

## Done

Every part is read, every lesson has kind, who, at and topics, and the frontmatter `lessons` count matches the number of `### L` headings.
