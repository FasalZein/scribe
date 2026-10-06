# Chunk cuts stay, without overlap, until a measurement says otherwise

Parakeet Ultra uses full self-attention, so encoder memory grows with the square of the input length. One pass over a 27:26 talk took 120 s against 19.4 s in chunks, and 38 minutes would need an estimated 13-26 GB for one attention matrix. Upstream transcribe.cpp HEAD cuts Ultra audio the same way scribe does: at a pause, at most 30 s, without overlap.

Hard cuts are rare: 0 of 204 cuts in five recordings found no quiet point. But the quietest 30 ms window is often a pause inside a sentence: in the 38-minute field run, 56 of 90 chunks started with a lowercase word, and 14 of those followed a false full stop.

Decision:

- Keep 30 s non-overlapping chunks (ADR 0003).
- Request word timestamps from the engine. Parts cut at a true sentence end, and a full stop followed by a lowercase start is not a sentence end. Lesson timestamps point at the word, not at the chunk start.
- `index.md` records the count of hard cuts.
- Change the cut rule (for example, prefer longer pauses) only when it lowers WER on the human-subtitle reference set. Add chunk overlap only when a check against human subtitles shows words lost at cuts.
