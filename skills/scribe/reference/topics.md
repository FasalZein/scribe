# Topic merge rules

A **topic note** merges the lessons on one subject from several sources. The main agent plans the merge and owns `topics/INDEX.md`. One helper per topic note reads the lessons and writes that note, so lesson text stays out of the main agent's context.

Both topic commands exit 0 on success. Errors print `scribe: <error>` on stderr and return a nonzero exit code.

## Plan (main agent)

1. Set `SCRIBE_LIBRARY` to the absolute library root, or use the default `~/Knowledge/scribe`.
2. Run `scribe topics plan`. It reads format 2 lessons and existing topic note filenames without changing files.
   - Each slug lists its lessons as `sources/<source-folder>/lessons.md#l<n>`, relative to the library root.
   - `new note` means no topic note exists. `single-source` means only one source uses the slug, regardless of lesson count.
   - Near-duplicates are distinct slugs where one complete slug is a prefix of the other, such as `agent-trust` and `agent-trustworthiness`.
   - A shared word alone does not match: `agent-trust` and `agent-verification` are not flagged.
   - These flags suggest candidates for inspection. They do not establish that two subjects are synonyms.
   - A validation error names the lessons file. Correct it with `reference/lessons.md`, then rerun the plan.
3. **Reuse**: choose an existing slug when it fits the lesson's subject. Inspect the flagged pairs before adding a slug.
4. **Synonyms**: merge synonym slugs only when the user requests it. Routine runs keep the slugs unchanged.
   On request, keep the existing slug that fits best, or the clearer slug when neither exists.
   Update the frontmatter and lesson `topics:` lines in every affected lessons file.
   Run `scribe lessons finalize <lessons.md>` and `scribe lessons check <lessons.md>` for each changed file.
   Merge the affected topic notes, update their links, and retain one note for the chosen slug.
   Rerun `scribe topics plan` after the requested merge.
5. Decide per topic slug on the new or re-extracted lessons:
   - **A topic note exists**: merge.
   - **No note, and two or more sources use the slug**: create `topics/<topic-slug>.md` and merge all tagged lessons.
   - **No note, and only one source uses the slug**: keep its lessons without creating a note.
   - **Re-extracted source**: also merge every topic note that links to its source folder, even when its new lessons no longer use that slug.
     Search the existing topic notes for the source folder to find those notes.

The plan is complete when every affected topic note has its tagged lessons and re-extracted sources identified.

## Merge (one helper per topic note)

Start one helper for each topic note to create or update, at most 6 at a time. Give each one this brief:

```
Merge lessons into one topic note.
- Topic slug: <topic-slug>
- Topic note: <absolute path to topics/<topic-slug>.md> (exists | new)
- Lessons files: <absolute paths of every lessons.md that uses the slug>
- Re-extracted sources: <absolute paths of their lessons.md, or "none">
- Rules: read <absolute path to this skill>/reference/topics.md, section "Topic note rules", and follow it exactly.
Reply with one line: <topic-slug> | <one-line scope> | <source count> | <lessons cited> | <lessons excluded>
```

When you cannot start helpers, merge the notes yourself, one at a time.

Check each reply: `lessons cited` plus `lessons excluded` equals the number of lessons tagged with the slug. Run a helper again once when it does not.

## Topic note rules

Read only the lessons tagged with your topic slug: the `### L` sections whose `topics:` line holds the slug.

**Re-extracted source**: before you merge, remove every bullet that links to that source's `lessons.md`, then merge its new lessons as for a new source.

Write `topics/<topic-slug>.md`:

```markdown
---
topic: <topic-slug>
sources: <count>
updated_at: <RFC 3339 time>
---

# <Topic title>

<2-4 sentences: what this subject is and the current state of knowledge across sources.>

## Lessons

### <merged lesson stated as a sentence>

- <Speaker> (<source title>) argues/explains/recommends ... [L3](../sources/<source-folder>/lessons.md#l3) at [hh:mm:ss](../sources/<source-folder>/parts/NN.md)
- <Another speaker> (<source title>) disagrees: ... [L5](../sources/<another-source-folder>/lessons.md#l5) at [hh:mm:ss](../sources/<another-source-folder>/parts/NN.md)

## Open questions

- <conflicts between sources, unverified facts carried as `verify:`, gaps>

## Excluded lessons

- [L7](../sources/<source-folder>/lessons.md#l7): <reason, for example "covered in topics/idempotency.md" or "off the subject of this note">
```

- Group lessons that say the same thing under one heading, not by source. When sources agree, list each one under the same heading.
- Call two claims a conflict only when they address the same scope under incompatible conditions; otherwise describe the difference. Keep each lesson's conditions and exceptions.
- Keep each speaker's attribution and each `verify:` note. An opinion from one speaker stays an opinion.
- Link every bullet to its lesson and its timestamp, so each merged lesson traces back to the source.
- Update the paragraph under the title when new lessons change the overall picture.

Done when every lesson tagged with the slug is cited in a bullet or listed under Excluded lessons with a reason, and every link names a file that exists.

## Update `topics/INDEX.md` (main agent)

Run `scribe topics index` after all topic notes are written. It rebuilds `topics/INDEX.md` and prints its absolute path when the library root is absolute.

- Topic notes appear in slug order with their first scope paragraph and the number of sources whose lessons use the slug.
- Every topic note is listed, including notes with no currently tagged sources (`0 sources`).
- Slugs with one source and no note appear under `## Single-source`, with links to every tagged lesson.
- Slugs with multiple sources and no note appear under `## Pending notes`. Create those notes, then rerun the command.
- An existing topic note stays out of both lists, even when it has one source.
- The command validates lessons and requires a title and scope paragraph in every topic note before replacing the index.
- On validation failure, correct the named file and rerun the command. The existing index stays unchanged.

Index rebuilding is complete when the command exits 0 and no pending notes remain for the affected slugs.

## Done

Every topic slug on the new lessons is merged into a topic note or listed under Single-source, every helper reply accounts for all tagged lessons, `topics/INDEX.md` lists every file in `topics/`, and no slug is in both lists.
