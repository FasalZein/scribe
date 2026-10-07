# Extraction rules

A **lesson** is one reusable piece of knowledge from a source. Restate it so it makes sense without the transcript.

Write `lessons.draft.md` in the source folder. Leave `lessons.md` unchanged. The main agent publishes it after `lessons check` exits 0. Until that check passes, the existing `lessons.md` stays as it is.

`<scribe>` is the absolute path in your brief. Use it for every scribe command. A different `scribe` on PATH can be older.

## Read

1. Read `index.md`: frontmatter, Sources, Chapters, the Parts table, Known terms, and Low-confidence passages.
2. Read the topics index from your brief when it exists. Reuse an existing topic slug when it fits the subject. Add a new slug only when none fits.
3. Read every part in order. Use the file names in the Parts table. A source with no chapters uses `parts/NN.md`. A chapter part uses `parts/NN-<chapter>.md`. A chapter split into several parts adds a piece number, for example `parts/02-long-chapter-1.md`.

The transcript has no speaker labels. Name speakers from Sources, the title, self-introductions, and cues such as "as Armin said". You may name the person behind a public handle from your own knowledge. Add `(inferred)` in `speakers`, and in `who` when the attribution is a guess. Write `speaker` when there is no cue.

Speech recognition mishears names. The transcript keeps the words the engine heard. Do not edit the transcript. Use two sections of `index.md` when you write a name in a lesson:

- **Known terms** lists handles and names from the source metadata. When a transcript word sounds like a known term, write the known term. Example: the transcript says "Potato" and the known terms include `poteto`.
- **Low-confidence passages** lists spans the engine scored as doubtful. Each span has a timestamp link to its part, and the doubtful words are in bold. Before you quote a passage or take a term from it, check the context and the known terms. When you cannot resolve a passage, restate it and skip the quote.

A mishearing can also score high. Record each unresolved spelling under Open questions.

Write lessons in English. Keep an optional quote in the source language. When the transcript has no usable speech, write no lessons. Put `no usable speech: <reason>` under Open questions and report that line.

## Resume

A long source can outlast one context. The draft is the resume point. A draft that is present belongs to the current transcript, and every later pass resumes from it.

After you finish reading one part, append that part's lessons to `lessons.draft.md`. Then set the last section to the next part, as below. Repeat that after every part, including a short part. One part is one update. `tokens_estimate` does not change the unit.

```markdown
## Resume

next: parts/04.md
```

On a later run, read the lessons already in the draft, then read parts from `next:` onward. Keep the existing lesson numbers. When every part in the Parts table has been read, remove the `## Resume` section.

When you cannot continue, leave the draft in place, leave `lessons.md` unchanged, and reply `partial: <next part path>`. Run `lessons finalize` only after every part has been read and `## Resume` is gone.

## Select

Keep knowledge that will still help months later: an assertion and its reasons, an explanation, complete steps, a rule of thumb, a comparison, or an example. Leave out greetings, banter, sponsor reads, repetition, and promises that have no content.

When a **focus** is set, answer it from the lessons. Also keep the source's other lasting knowledge.

## Write the draft

Create this frontmatter first, then append lessons. Omit `lessons:` and the frontmatter `topics:` list. `lessons finalize` writes both.

```markdown
---
source: <source URL or path from index.md>
index: index.md
title: <the title: line from index.md, copied exactly, quotes included>
speakers: [<name>, ...]
focus: <focus or null>
extracted_at: <RFC 3339 time>
---

# Lessons: <title>

## Focus answer

<Only when a focus is set: 2-6 sentences. Point each sentence at lesson ids, for example (L3, L7).>

## Lessons

### L1
<short title that states the point>

- kind: claim
- who: <speaker>
- at: [00:00:05](parts/01.md)
- topics: <slug>, <slug>
- verify: <what to check, and where; omit when not needed>

<Restate the lesson with its reasons, conditions, exceptions, and uncertainty.>

> "<At most one exact sentence, when the wording itself matters.>"

## Open questions

- <Unclear terms, possible misrecognitions, conflicting claims, or unresolved questions.>
```

`lessons check` and `lessons finalize` reject frontmatter that is not valid YAML. Quote a value that contains `: ` or that starts with a special character. `index.md` already quotes those values. stderr says `frontmatter: not valid YAML` and names the line.

### Anchors and titles

Each heading is `### L<n>` alone, from L1, increasing by one. The title is the next line. Link a lesson as `lessons.md#l<n>`, for example `lessons.md#l1`. A title change leaves the anchor in place. A heading such as `### L1. Title` does not pass `lessons check`.

### Kinds and bodies

Use one of these kinds:

| Kind | Use for |
| --- | --- |
| `claim` | An assertion or opinion, with its reasons. |
| `explanation` | How or why something works. |
| `procedure` | Complete numbered steps someone can follow, with amounts, conditions, and a stopping point where the source gives them. |
| `heuristic` | A rule of thumb, with the conditions where it helps. |
| `trade-off` | Options and what each one costs or gains. |
| `example` | A concrete case that shows reusable knowledge. |

A procedure body numbers its steps from 1, in order:

```markdown
### L2
Thicken a wet mixture gradually

- kind: procedure
- who: Cook
- at: [00:10:00](parts/02-mixing.md)
- topics: cooking

1. Add 10 g flour when the mixture runs off the spoon.
2. Stir for 30 seconds, then check the texture again.
3. Repeat until the mixture coats the spoon.
```

Keep one idea in each lesson. Split a passage that makes two points. Keep related measurements of one product together. A Mermaid fence is allowed when a diagram helps. `lessons check` ignores lesson-like text inside a code fence, and it rejects an unclosed fence.

`lessons check` checks that procedure steps are numbered from 1. It does not check that the steps are complete. You check completeness against the source.

### Metadata

Write `- kind:`, `- who:`, `- at:`, `- topics:`, and `- verify:` in the list under the title. `lessons check` fails when one of those lines is in the body. stderr says `belongs in the metadata list under the title, not in the body`.

- **who**: Name the speaker. Write `A and B` when speakers build one point together. Attribute an opinion in the body, for example "X argues...".
- **at**: Use the `[hh:mm:ss]` timestamp on the transcript paragraph where the lesson starts. The part file must contain that timestamp at the start of a line, followed by a space. Link the part relative to the source folder. When a lesson uses several paragraphs, cite the first. A time written only in prose is not a citation.
- **topics**: One to three topic slugs, separated by commas. A slug is lowercase words of letters and digits, joined by single hyphens, for example `durable-execution`. Name the subject, not the source. Reuse a slug from the topics index when one fits. Keep a listed slug even when the title or the uploader names it. Apply the source-name test only to a new slug, one that the topics index does not list yet. A new slug names the source when both checks pass. It would sit on more than half of the lessons in this source. Every word of the slug appears as a whole word in the `title` line or the `uploader` line in `index.md`, ignoring case and punctuation. A word may come from either line. Remove that slug from those lessons. When a lesson then has no slug, give it the narrower subject that lesson teaches. A subject may cover most lessons of one source when one of its words is absent from the title and the uploader.
- **verify**: Add this optional bullet when a fact needs evidence outside the transcript, such as a benchmark or a product limit. Name what to check and where. Omit the bullet otherwise.

## Ready for publish

The draft is ready when every part in the Parts table has been read, `## Resume` is gone, the source frontmatter is present, and the Focus answer is present when a focus was set. Apply the source-name test in **topics** before this point.

Run:

```sh
<scribe> lessons finalize <path/to/lessons.draft.md>
<scribe> lessons check <path/to/lessons.draft.md>
```

`lessons finalize` checks the lesson bodies and citations, then writes `lessons:` and the sorted unique `topics:` list. It keeps your other frontmatter fields and the body. When validation fails, finalize leaves the draft unchanged. `lessons check` only reads. It checks the count, the anchors, the title lines, the required fields, the kinds, the procedure step numbers, the topic list, the part links, the paragraph timestamps, the frontmatter YAML, and the metadata list under the title.

Both commands exit 0 on success. On failure they exit nonzero and print `scribe: <reason>` on stderr. Neither command loads a model.

Fix every reported error and run both commands again. Then return. The main agent replaces `lessons.md` with the draft only after its own `lessons check` exits 0.

Your extraction is complete when every part is accounted for and `lessons check` on the draft exits 0, or when the draft still holds `## Resume` and your reply names that next part.
