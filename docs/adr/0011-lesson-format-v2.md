# Lesson format 2: domain-neutral kinds and short anchors

The library must hold knowledge from any field, from agentic engineering to cooking. The first lesson kinds (claim, mechanism, practice, trade-off) lean toward engineering, and a recipe fits none of them. Topic notes linked lessons by title slugs, which GitHub and Obsidian compute differently and which break when a title changes.

Decision:

- The term stays **lesson**. Kinds become claim, explanation, procedure, heuristic, trade-off and example. A procedure carries complete numbered steps with amounts and conditions, so it can later be followed, or turned into a skill, without the transcript.
- A lesson heading is `### L<n>` alone, with the title on the next line; links use `lessons.md#l<n>`.
- Markdown is the only stored format. Diagrams go in as Mermaid blocks, which GitHub and Obsidian render. Rich HTML views come from the agent (for example the show-me skill) on request; scribe ships no renderer.

## Consequences

- The existing library sources are re-extracted under the new format; `scribe lessons check` validates format 2 only.
- Making skills from lessons remains out of scope; the procedure kind only keeps that door open.
