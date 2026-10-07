# Single-source topic slugs stay

Field problem 13 said one talk added 8 single-source topic slugs. Issue 25 asks whether that growth is a problem after more sources.

A topic slug names a subject, not a source. A lesson carries one to three topic slugs. A **topic note** merges the lessons on one subject from several sources (`GLOSSARY.md`). Until a second source uses the slug, `topics/INDEX.md` lists it under Single-source and links each tagged lesson. The reader opens those lessons from the index. There is no topic note yet, because there is nothing to merge.

Decision D10 already set the growth rule (spec issue 1, topic policy). Reuse an existing topic slug when it fits. Add a new slug only when none fits. `topics plan` reports near-duplicate slugs. Merge synonym slugs only when the user asks. `skills/scribe/reference/lessons.md` and `skills/scribe/reference/topics.md` already say this. This ADR records the measurement. The rule stays. The skill text and the binary do not change.

## Measurement

Measured on 2026-10-07 with `scribe` 0.3.0. `src/topics.rs` is unchanged since tag `v0.3.0`. The commands were `scribe topics plan`, `scribe topics index`, and `scribe lessons check`. No command loads a model.

The real library is a copy of `~/Knowledge/scribe`: 3 sources, migrated in #19. The fresh library is the 0.3.0 poteto re-run from #20. That temp directory was gone. The fresh copy uses the saved `lessons.md`, `index.md`, `meta.json`, and `topics/INDEX.md`. Its part files are stubs that hold only the cited timestamp lines, so `topics plan` can read the lessons. `topics index` on that copy matches the saved field index with no diff.

`lessons check` exits 0 on each of the 3 real lessons files. `topics index` on the real copy rewrites `topics/INDEX.md` with no diff against the original. The original library was not modified.

`topics plan` agrees with a direct count of the `- topics:` lines. The real plan has 20 slug sections. The fresh plan has 9. Both plans end with the heading `Near-duplicate slugs` and the line `None.`

### Real library: 3 sources, 51 lessons

| source | lessons | topic tags | slugs used | new slugs in upload-date order | reused slugs |
| --- | ---: | ---: | ---: | ---: | ---: |
| Eric Allam, 2026-05-10 | 16 | 26 | 7 | 7 | 0 |
| poteto, 2026-09-21 | 18 | 35 | 9 | 9 | 0 |
| Pi Durable, 2026-10-05 | 17 | 34 | 10 | 4 | 6 |

The last two columns sort the current tags by upload date. They are not a log of the #19 re-extraction. A joint re-extraction can assign a shared slug while both sources are in view.

| | count |
| --- | ---: |
| unique topic slugs | 20 |
| shared slugs (2 sources, each with a topic note) | 6 |
| single-source slugs (no topic note) | 14 |
| pending notes | 0 |
| near-duplicate pairs | 0 |
| topic tags | 95 (9 lessons with 1, 40 with 2, 2 with 3) |

Slugs per lesson are 20/51. New slugs per lesson in upload-date order are 7/16, then 9/18, then 4/17.

The six topic notes are `agent-session-storage`, `agent-written-code`, `durable-execution`, `idempotency`, `vm-snapshots`, and `workflow-versioning`. A reader of `topics/INDEX.md` sees those six notes first. Each line has the note's scope paragraph and `(2 sources)`. Then the reader sees 14 Single-source lines. Each line links to the tagged lessons. No slug is in both lists. There is no Pending notes section.

### Fresh poteto library: 1 source, 18 lessons

The fresh run has 9 topic slugs, all single-source, 0 topic notes, and 0 near-duplicate pairs. Slugs per lesson are 9/18. Topic tags are 27 (9 lessons with 1 tag, 9 with 2).

The reader sees no topic notes. The index has nine Single-source lines: `agent-friendly-codebase`, `agent-memory`, `agent-orchestration`, `agent-skills`, `agent-trust`, `agent-verification`, `code-review`, `frontend-performance`, `static-analysis`.

### Same talk, with and without an existing index

The fresh run had no topic slug to reuse, so all 9 slugs are new. In the real library the poteto lessons do not use `agent-memory`. They use `agent-written-code`, which the Pi lessons also use, and that slug has a topic note. The other eight poteto slugs have the same names as the fresh run, and each stays single-source. No existing slug names those subjects. `topics plan` reports no near-duplicate on either library.

The issue title says topic slugs grow by about one per lesson. These files do not show that rate. A source whose subjects are new to the library adds about one slug per two lessons: 9/18 on the fresh run, and 7/16 and 9/18 for the first two real sources in upload-date order. The third source overlaps the other two. It adds 4 slugs on 17 lessons and reuses 6.

## Options already in the design

- Tell the extractor to reuse a slug from the index first. `reference/lessons.md` already says this. The real poteto lessons use the existing `agent-written-code` slug. The fresh run, with an empty index, does not.
- Make `topics plan` report single-source slugs. It already does. The index lists them under Single-source.
- Require more than one source before a topic note. The minimum is already 2. All 6 notes have 2 sources. All 14 single-source slugs have no note.

A higher minimum, or dropping Single-source lines, would hide subjects the index currently links. D10 already keeps a new slug when no existing slug fits, and it merges synonyms only on request.

## One synonym pair the rule leaves alone

`backend-architecture` and `stateful-compute` tag the same two lessons in the Eric Allam source: L2 and L13. Neither slug's full text is a prefix of the other, so `topics plan` does not list them as a near-duplicate. D10 merges that kind of pair only when the user asks. Both index lines link to the same two lessons. The reader does not lose either lesson. The pair is not a failed reuse across sources.

Other overlaps are dual tags, which the one-to-three slug rule allows. For example, the 3 `agent-skills` lessons are also tagged `agent-verification`, and that slug has 2 further lessons.

## Decision

Single-source topic slugs are not a problem. They are the index entry for a subject only one source discusses. A topic note starts when a second source reuses the slug. Growth stays under D10. No skill change. No binary change.

## Consequences

- The Single-source section grows when a source covers a subject the library does not already name.
- A later source that reuses the slug turns that line into a topic note.
- A user who wants `backend-architecture` and `stateful-compute` merged asks for a synonym merge. A routine run does not merge them.
