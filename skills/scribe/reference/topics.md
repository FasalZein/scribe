# Topic merge rules

A **topic note** merges the lessons on one subject from several sources. Run this after a batch of extractions, in the main agent.

## Gather

1. Read the frontmatter `topics` list of every new `lessons.md`.
2. Read `topics/INDEX.md` (create it when it is missing).
3. For each slug, count the sources that use it across the library:
   `grep -l "topics:.*\b<slug>\b" "$SCRIBE_LIBRARY"/sources/*/lessons.md`

## Decide per slug

- **A topic note exists**: merge the new lessons into it.
- **No note, and two or more sources use the slug**: create `topics/<slug>.md`, then merge the lessons from all of those sources.
- **No note, and only one source uses the slug**: leave it in the lessons file. Record it in the `## Single-source` list at the end of `topics/INDEX.md`.

Read only the lessons tagged with the slug. Find them by their `### L` headings and `topics:` lines.

## Write `topics/<slug>.md`

```markdown
---
topic: <slug>
sources: <count>
updated_at: <RFC 3339 time>
---

# <Topic title>

<2-4 sentences: what this subject is and the current state of knowledge across sources.>

## Lessons

### <merged lesson stated as a sentence>

- <Speaker> (<source title>) argues/explains/recommends ... [L3](../sources/<slug>/lessons.md#l3-...) at [hh:mm:ss](../sources/<slug>/parts/NN-....md)
- <Another speaker> (<source title>) disagrees: ... [L5](...)

## Open questions

- <conflicts between sources, unverified facts carried as `verify:`, gaps>
```

Rules:

- Group lessons that say the same thing under one heading, not by source. When sources agree, list each one under the same heading. When they conflict, keep both and name the conflict.
- Keep each speaker's attribution and each `verify:` note. An opinion from one speaker stays an opinion.
- Link every bullet to its lesson and its timestamp, so each merged lesson traces back to the source.
- Update the paragraph under the title when new lessons change the overall picture.

## Update `topics/INDEX.md`

One line per topic: `- [<slug>](<slug>.md): <one-line scope> (<N> sources)`, sorted by slug, then the `## Single-source` list of slugs with the lessons file that uses each one.

## Done

Every slug on the new lessons is merged into a topic note or listed under Single-source, and `topics/INDEX.md` lists every file in `topics/`.
