# Topic merge rules

A **topic note** merges the lessons on one subject from several sources. The main agent plans the merge and owns `topics/INDEX.md`. One helper per topic note reads the lessons and writes that note, so lesson text stays out of the main agent's context.

## Plan (main agent)

1. Set `lib` to the absolute library root (`$SCRIBE_LIBRARY`, default `~/Knowledge/scribe`).
2. Build the map of topic slugs to lessons files from the frontmatter `topics:` lines. Each output line is one lessons file and its full topic list:

   ```bash
   grep -H '^topics: \[' "$lib"/sources/*/lessons.md
   ```

   PowerShell:

   ```powershell
   Select-String -Path "$lib\sources\*\lessons.md" -Pattern '^topics: \[' | ForEach-Object { "$($_.Path):$($_.Line)" }
   ```

   A slug matches only a complete list element: `durable-execution` does not match `execution`. To list the files for one slug, use an exact match:

   ```bash
   grep -lE '^topics: \[(.*, )?<topic-slug>(,|\])' "$lib"/sources/*/lessons.md
   ```

   PowerShell: `Select-String -List -Path "$lib\sources\*\lessons.md" -Pattern '^topics: \[(.*, )?<topic-slug>(,|\])'`.
3. Read `topics/INDEX.md` (create it when it is missing).
4. **Synonyms**: when two topic slugs name the same subject, keep the one in `topics/INDEX.md` (or the clearer one) and rename the other in the frontmatter `topics:` line and the lesson `topics:` lines of each lessons file that uses it.
5. Decide per topic slug on the new or re-extracted lessons:
   - **A topic note exists**: merge.
   - **No note, and two or more sources use the slug**: create `topics/<topic-slug>.md` and merge the lessons from all of those sources.
   - **No note, and only one source uses the slug**: no note. List it under `## Single-source` in `topics/INDEX.md`.
   - **Re-extracted source**: also merge every topic note that links to its source folder (`grep -l '<source-folder>' "$lib"/topics/*.md`), even when its new lessons no longer use that slug.

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

- <Speaker> (<source title>) argues/explains/recommends ... [L3](../sources/<source-folder>/lessons.md#l3-...) at [hh:mm:ss](../sources/<source-folder>/parts/NN-....md)
- <Another speaker> (<source title>) disagrees: ... [L5](...)

## Open questions

- <conflicts between sources, unverified facts carried as `verify:`, gaps>

## Excluded lessons

- [L7](../sources/<source-folder>/lessons.md#l7-...): <reason, for example "covered in topics/idempotency.md" or "off the subject of this note">
```

- Group lessons that say the same thing under one heading, not by source. When sources agree, list each one under the same heading.
- Call two claims a conflict only when they address the same scope under incompatible conditions; otherwise describe the difference. Keep each lesson's conditions and exceptions.
- Keep each speaker's attribution and each `verify:` note. An opinion from one speaker stays an opinion.
- Link every bullet to its lesson and its timestamp, so each merged lesson traces back to the source.
- Update the paragraph under the title when new lessons change the overall picture.

Done when every lesson tagged with the slug is cited in a bullet or listed under Excluded lessons with a reason, and every link names a file that exists.

## Update `topics/INDEX.md` (main agent)

Write it from the helper replies. One line per topic note: `- [<topic-slug>](<topic-slug>.md): <one-line scope> (<N> sources)`, sorted by slug. Then the `## Single-source` list of topic slugs, each with the lessons file that uses it. Remove a slug from `## Single-source` when it gains a topic note.

## Done

Every topic slug on the new lessons is merged into a topic note or listed under Single-source, every helper reply accounts for all tagged lessons, `topics/INDEX.md` lists every file in `topics/`, and no slug is in both lists.
