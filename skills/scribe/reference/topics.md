# Topic merge rules

A **topic note** merges the lessons on one subject from several sources. The main agent runs `topics plan` and `topics index`. One helper per topic note reads the lessons and writes that note, so lesson text stays out of the main agent's context.

`<scribe>` is the absolute path from step 1. `topics plan` and `topics index` read `$SCRIBE_LIBRARY` and take no path argument. Export `SCRIBE_LIBRARY` to the absolute library root before either command. Both commands exit 0 on success. On failure they print `scribe: <error>` on stderr and exit nonzero. Neither command loads a model.

## Plan (main agent)

Run `<scribe> topics plan`. It reads format 2 lessons and topic-note filenames and changes nothing.

Each slug is a section:

```markdown
## <slug> (existing note|new note, single-source|<n> sources)

- [L8](sources/<source-folder>/lessons.md#l8): <lesson title>
```

- `existing note` means `topics/<slug>.md` is already there. `new note` means it is not.
- `single-source` means one source uses the slug. `<n> sources` is the source count. A note that nothing currently uses is `existing note, 0 sources`. Leave that note in place on a routine run.
- The links are relative to the library root. In a topic note, prefix `../`.
- Near-duplicates are listed under `## Near-duplicate slugs` as `- <slug> / <other>`. One slug's full text is a prefix of the other, such as `agent-trust` / `agent-trustworthiness`. A shared word is not enough: `agent-trust` and `agent-verification` are not a pair. `None.` means there is no pair.
- Read each listed pair against the lesson titles in the plan. Then apply one case. These cases cover two slugs that earlier sources already use. A new source that is about to add a slug follows **Reuse** instead.
  - **Parent and child.** The longer slug is a narrower subject. A lesson under the longer slug does not always belong under the shorter slug. Example: `claude-code` / `claude-code-plugins`. Keep both slugs.
  - **Same subject.** The titles under both slugs state one subject, so one note could cite every one of those lessons. Example: `agent-trust` / `agent-trustworthiness`. Keep both slugs until the user asks for a merge.
- The list leaves out synonyms that use different words. Example: `context-management` / `context-window`.
- The command reads every `lessons.md` in the library. A validation error names that file. An older heading such as `### L1. Title` fails here. When that source still has parts, fix the named file with [`lessons.md`](lessons.md), publish it with the step 5 commands, and rerun the plan. When the error is a missing part and the index says `parts: 0`, re-extraction cannot repair the file. Ask the user to move that `lessons.md` aside, as step 4 describes, and rerun the plan.

**Reuse**: For a lesson on a new or re-extracted source, keep an existing slug when the subject is the same. Compare that slug with every slug heading, not only the listed pairs. The words may differ. A parent-and-child pair is not a reuse: add the narrower slug. Reuse does not merge two slugs that earlier sources already use.

**Synonyms**: Merge synonym slugs only when the user asks. A routine run leaves the slugs as they are, whether or not the plan lists the pair. On request, compare the slug headings yourself. The near-duplicate list misses different words. Keep the existing slug that fits best, or the clearer slug when neither note exists. Apply that slug in the affected lessons by editing a `lessons.draft.md` copy, then use the publish steps in `SKILL.md` step 5 so the current `lessons.md` stays in place until `lessons check` exits 0. Merge the affected topic notes, keep one note for the chosen slug, and rerun `<scribe> topics plan`.

Decide per topic slug on the new or re-extracted lessons:

- **existing note**: merge.
- **new note, two or more sources**: create `topics/<topic-slug>.md` and merge every lesson that uses the slug.
- **new note, single-source**: do not create a note.
- **Re-extracted source**: also merge every topic note that links to its source folder, including a note whose slug the new lessons dropped. Search the topic notes for that folder.

The plan is complete when every affected slug has its lessons and its re-extracted sources identified.

## Merge (one helper per topic note)

Start one helper for each topic note to create or update, at most 6 at a time. Give each this brief:

```
Merge lessons into one topic note.
- Topic slug: <topic-slug>
- Topic note: <absolute path to topics/<topic-slug>.md> (exists | new)
- Lessons files: <absolute paths of every lessons.md that uses the slug>
- Lesson links from the plan: <each plan line for this slug>
- Re-extracted sources: <absolute paths of their lessons.md, or "none">
- Rules: read <absolute path to this skill>/reference/topics.md, section "Topic note rules", and follow it exactly.
Reply with one line: <topic-slug> | <one-line scope> | <source count> | <lessons cited> | <lessons excluded>
```

When you cannot start helpers, merge the notes yourself, one at a time.

Check each note against the plan: every listed lesson link must appear under Lessons or Excluded lessons with a reason. For an inline merge, read the written note and check off each plan link. With helpers, also check each reply: `lessons cited` plus `lessons excluded` equals the number of lessons tagged with the slug. Run that helper once more when either check fails.

## Topic note rules

Read each lesson the plan lists for this slug. The helper brief copies those lines. A `topics:` entry counts only when the slug is a whole entry, so `agent` does not match `agent-trust`.

**Re-extracted source**: remove every bullet that links to that source's `lessons.md`, then merge its new lessons as you would for a new source. The published `lessons.md` is already the validated replacement.

Write `topics/<topic-slug>.md`:

```markdown
---
topic: <topic-slug>
sources: <count>
updated_at: <RFC 3339 time>
---

# <Topic title>

<2-4 sentences: what this subject is, and the current state of knowledge across sources.>

## Lessons

### <merged lesson stated as a sentence>

- <Speaker> (<source title>) argues/explains/recommends ... [L3](../sources/<source-folder>/lessons.md#l3) at [hh:mm:ss](../sources/<source-folder>/parts/NN.md)
- <Another speaker> (<source title>) disagrees: ... [L5](../sources/<another-source-folder>/lessons.md#l5) at [hh:mm:ss](../sources/<another-source-folder>/parts/NN.md)

## Open questions

- <conflicts between sources, unverified facts carried as `verify:`, gaps>

## Excluded lessons

- [L7](../sources/<source-folder>/lessons.md#l7): <reason, for example covered by topics/idempotency.md, or off this subject>
```

- When no lessons are excluded, write `None.` under Excluded lessons.
- Group lessons that say the same thing under one heading. When sources agree, list each one under that heading.
- Call two claims a conflict only when they address the same scope under incompatible conditions. Otherwise describe the difference. Keep each lesson's conditions and exceptions.
- Keep each speaker's attribution and each `verify:` note. An opinion from one speaker stays an opinion.
- Link every bullet to its lesson and its timestamp, using the lesson id and source folder from the plan.
- Update the paragraph under the title when new lessons change the overall picture.

Done when every lesson tagged with the slug is cited in a bullet or listed under Excluded lessons with a reason, and every link names a file that exists.

## Update `topics/INDEX.md` (main agent)

Run `<scribe> topics index` after the topic notes are written. It rebuilds `topics/INDEX.md` and prints the absolute index path when the library root is absolute.

- Topic notes appear in slug order. Each line has the note's first scope paragraph and the number of sources whose lessons use the slug, including `0 sources`.
- A single-source slug with no note appears under `## Single-source`, with a link to each tagged lesson.
- A slug with several sources and no note appears under `## Pending notes`. Create those notes, then run `<scribe> topics index` again.
- An existing topic note stays out of both lists.
- The command checks the lessons files, and it requires a title and a scope paragraph in every topic note. It replaces the index only after those checks pass. On failure, correct the named file and run the command again. The existing index stays in place.

The index is complete when the command exits 0 and no affected slug remains under Pending notes.

## Done

Every topic slug on the new lessons is a topic note or a Single-source line. Every merged note accounts for its tagged lessons, checked against the plan. `topics/INDEX.md` lists every file in `topics/`. No slug is in both lists.
