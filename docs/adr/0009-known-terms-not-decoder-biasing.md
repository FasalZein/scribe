# Known terms and low-confidence passages, not decoder biasing

Parakeet mishears names: one 38-minute talk spelled one product name 7 ways and turned the handle "poteto" into "Potato". transcribe-cpp 0.3.1 has no name biasing for Parakeet (its vocabulary option works only for Whisper and Qwen3-ASR, and Parakeet ignores it with a warning), and upstream transcribe.cpp issue #126 asking for it is open. The only off-the-shelf alternative, sherpa-onnx hotwords, needs beam search and a different engine (ADR 0002).

Decision: the transcript stays as the engine heard it. `index.md` lists **known terms** taken from the source's own metadata (post text, title, uploader, description) and **low-confidence passages** taken from the engine's per-token scores. The extraction helper uses both to spell names right in the lessons and to flag doubtful quotes.

## Considered options

- **Rewrite the transcript with fuzzy matches to known terms.** Rejected: a wrong match ("Potato" is a real word) silently corrupts the source text that every lesson cites.
- **Patch the transcribe.cpp greedy decoder to boost terms.** Rejected for now: boosting must run inside the decode loop, so it is a C++ fork until upstream merges it. Revisit when upstream adds biasing for Parakeet.
