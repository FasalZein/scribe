# Chunk cuts stay, without overlap, until a measurement says otherwise

Parakeet Ultra uses full self-attention, so encoder memory grows with the square of the input length. One pass over a 27:26 talk took 120 s against 19.4 s in chunks, and 38 minutes would need an estimated 13-26 GB for one attention matrix. Upstream transcribe.cpp HEAD cuts Ultra audio the same way scribe does: at a pause, at most 30 s, without overlap.

Hard cuts are rare: 0 of 204 cuts in five recordings found no quiet point. But the quietest 30 ms window is often a pause inside a sentence: in the 38-minute field run, 56 of 90 chunks started with a lowercase word, and 14 of those followed a false full stop.

Decision:

- Keep 30 s non-overlapping chunks (ADR 0003).
- Request word timestamps from the engine. Parts cut at a true sentence end, and a full stop followed by a lowercase start is not a sentence end. Lesson timestamps point at the word, not at the chunk start.
- `index.md` records the count of hard cuts.
- Change the cut rule (for example, prefer longer pauses) only when it lowers WER on the human-subtitle reference set. Add chunk overlap only when a check against human subtitles shows words lost at cuts.

## Longer-pause rule measured (#17, 2026-10-07)

The quietest-window rule stays the default. A longer-pause rule raised WER beyond noise.

Both rules search the same region: the last 10 s before the 30 s hard end, and never closer than 1 s to the chunk start. Both use 30 ms windows of mean energy. Both fall back to the hard end when no window is 6 dB (energy ratio 0.25) below the loudest window.

- Quietest window (default): cut at the centre of the window with the lowest energy. A tie goes to the later window.
- Longest pause: a window is quiet when its energy is below 0.25 of the loudest window, the same 6 dB bound. Cut at the centre of the longest run of consecutive quiet windows. A tie goes to the later run. The rule adds no new parameter: the region, window and bound are the current rule's.

Setup: the Parakeet Ultra Q8_0 default model, the native Metal backend (the default on this Mac; neither this ADR nor #17 asks for CPU), and the 4 TED talks with human subtitles from the model benchmark (9,273 reference words). Scoring used `scripts/wer-compare.py`. Runs went one at a time and started only at a 1-minute load average of 7 or less (14 cores). Start loads were 4.0-6.8, and one run ended at 8.7. A second full pass gave the same transcripts word for word.

| Rule | WER % | Sub | Del | Ins | Errors | Hard cuts |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| Quietest window | 3.15 | 127 | 58 | 107 | 292 | 0 |
| Longest pause | 3.38 | 126 | 67 | 120 | 313 | 0 |

The longest-pause rule changed 92 transcript words and added 21 errors: +0.226 points against a noise bound of 0.207 points (2*sqrt(c)/N). It was worse on 3 of 4 talks; on arj7oStGLkU it was 0.04 points better. No cut point was the same between the two rules. The 38-minute field case was not measured: its audio is not stored locally, and it has no human reference.
