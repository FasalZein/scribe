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

## Word-timestamp engine time and paragraph values measured (#21, 2026-10-07)

### Engine time of the word-timestamp request

The word-timestamp request stays. It has no measurable engine-time cost.

Setup: the 27:26 regression talk, the default model, the native Metal backend, one fresh library per run, `--timings`. A measurement build switched the request between token timestamps (the default, which gives word times) and segment timestamps (the request before #3). Runs went one at a time and started only at a 1-minute load average of 7 or less. Each round ran both requests back to back, and the first request alternated between rounds. 12 rounds (24 runs) were done. The paired difference per round cancels slow load drift; 12 rounds give a 95% interval of about ±1.7 s against a run-to-run spread of 17-37 s.

| Request | Rounds | Engine median | Engine range | Encode median | TDT decode median |
| --- | ---: | ---: | ---: | ---: | ---: |
| Word (token) | 12 | 18.10 s | 17.22-36.75 s | 9.98 s | 7.58 s |
| Segment | 12 | 18.73 s | 16.95-29.99 s | 10.18 s | 8.18 s |

- Paired difference (word - segment), engine: mean -0.51 s, median -0.23 s, 95% interval -2.19 to +1.17 s. The word request was slower in 6 of 12 rounds.
- Start loads were 2.15-6.66. One run ended at load 10.65. Without that round, the interval is -2.31 to -0.04 s, so the result does not depend on it.
- The earlier 27.68 s against 19.46 s came from host load (120 against 7.6), not from the request.
- The CPU backend was not measured. The load rose above 7 again before the run, and the issue asks for CPU only when it is cheap.

### Paragraph and part values

Metric: a forced paragraph end is a paragraph end at the time limit with no true sentence end, so it splits a sentence. A forced part end is a part end at 3,000 words with no true sentence end. Data: word times from the default model on Metal for the 27:26 talk, the 4 TED talks and the 0.3.0 field-run source (the poteto X post, 38:02). The paragraphs of the field run were reproduced exactly (78 of 78), with the forced end after "and" at [00:08:55].

| Target | Limit | Paragraphs | Forced ends | Median | p90 | Longest | Median words |
| ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| 20 s | 45 s | 291 | 5 | 24.0 s | 33.6 s | 44.9 s | 66 |
| 20 s | 60 s (old) | 291 | 1 | 23.9 s | 33.4 s | 59.7 s | 66 |
| 20 s | 75 s | 290 | 0 | 23.9 s | 33.5 s | 62.8 s | 66 |
| 20 s | 80 s (new) | 290 | 0 | 23.9 s | 33.5 s | 62.8 s | 66 |
| 10 s | 60 s | 471 | 1 | 14.0 s | 22.9 s | 59.7 s | 39 |
| 15 s | 60 s | 363 | 0 | 19.2 s | 27.0 s | 56.7 s | 52 |
| 25 s | 60 s | 240 | 2 | 29.6 s | 38.6 s | 59.8 s | 81 |
| 30 s | 60 s | 211 | 1 | 34.5 s | 44.7 s | 59.7 s | 94 |

Decision:

- The paragraph limit changes from 60 s to 80 s. A sentence that starts just before the 20 s target needs the target plus its own length. The longest true sentence in the data is 56.7 s (field run); 20 + 56.7 = 76.7 s, rounded up to 80 s. The TED talks and the 27:26 talk have no sentence over 33.1 s; 4 of 179 field-run sentences are over 40 s. The cost: the longest paragraph grows from 59.7 s to 62.8 s, and a transcript without punctuation gets 80 s paragraphs instead of 60 s.
- The 20 s target stays. Other targets do not lower forced ends in a consistent direction (10 s: 1, 15 s: 0, 25 s: 2, 30 s: 1), because the target only moves where the one long sentence starts. 20 s gives a median paragraph of 24 s and 66 words.
- The 3,000-word part limit stays. It never fired: all 9 part ends in the data are true sentence ends, at most 73 words after 2,500. It acts only on a transcript without punctuation, and the reference set has none.
