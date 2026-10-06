# Keep Parakeet TDT v3 as the single default model

scribe ships one default model and no model names or aliases; `-m <path|url>` is the escape hatch for any other GGUF. We tested whether Parakeet Ultra (handy-computer/parakeet-ultra-gguf, Moondream's post-train of v3) should replace v3 as that one default.

The rule: switch only if Ultra's aggregate WER is at least 0.3 points lower than v3's, no talk is more than 0.5 points worse, and the end-to-end time stays within 20% of v3.

## Measurement

Four English TED talks with human-written English subtitles as references (9,273 reference words): Julian Treasure `eIho2S0ZahI` (9:58), Tim Urban `arj7oStGLkU` (14:04, heavy laughter), Yuval Noah Harari `nzj7Wg4DAbs` (17:08, non-native), Esther Perel `sa0RUmGTCYY` (19:10, non-native). Reference and hypothesis were normalized the same way: sound notes such as "(Laughter)" dropped, lowercased, punctuation and fillers dropped, numbers written as words. M4 Pro, Metal, one run per cell, sequential; other apps ran, so the 1-minute load average was 10-74 (the last talk ran at 55-74). Models: v3 Q8_0 (740 MB), Ultra Q8_0 (740 MB, Hugging Face revision `39eeb55`). scribe ran with the ADR 0006 thread default.

| Candidate | WER % | Sub | Del | Ins | Wall, 4 talks | Real-time factor | Peak memory |
| --- | --- | --- | --- | --- | --- | --- | --- |
| scribe, v3, 30 s chunks (transcribe-cpp 0.3.1) | 3.21 | 121 | 67 | 110 | 64.0 s | 57x | 1.17 GB |
| scribe, Ultra, 30 s chunks (transcribe-cpp 0.3.1) | 3.15 | 129 | 56 | 107 | 59.4 s | 61x | 1.18 GB |
| transcribe-cli at transcribe.cpp `3727340`, Ultra, whole file (its own VAD cuts) | 3.31 | 118 | 69 | 120 | 49.5 s | 73x | 1.10 GB |
| transcribe-cli at `3727340`, v3, whole file | 4.35 | 123 | 178 | 102 | 164.4 s | 22x | 1.87 GB |
| Moondream Parakeet Redux (PyTorch, MPS) | 4.00 | 183 | 65 | 123 | 91.4 s | 40x | 1.08 GB |

Per talk, scribe v3 against scribe Ultra: 3.19 / 3.13, 3.91 / 4.09, 2.90 / 2.85, 2.95 / 2.69.

- Ultra is 0.06 points better in aggregate, far below the 0.3-point bar, and one talk is 0.18 points worse. The rule fails; v3 stays the default.
- Ultra's own VAD segmentation was not better than scribe's 30 s chunks (3.31% against 3.15%).
- transcribe-cpp 0.3.1 already loads and runs Ultra; no newer crate was needed to test it.
- Ultra's decoder steps cost the same as v3's (median 0.94 ms against 1.09 ms per step). The 45-118 ms per step seen earlier came from contention and 8 decoder threads (ADR 0006), not from the model.
- Without chunking, v3 deleted 114 words on the longest talk, which agrees with ADR 0003.

## Consequences

- No new dependency and no model download change. Users who want Ultra pass `-m` with its GGUF URL.
- Revisit when a candidate claims a clear long-form gain; rerun the same four talks with `wer.py` from the benchmark (kept outside the repository).
