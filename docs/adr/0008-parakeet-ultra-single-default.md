# Parakeet Ultra as the single default model

Supersedes [ADR 0007](0007-keep-parakeet-v3-single-default.md).

scribe still ships one default model and no model names or aliases; `-m <path|url>` remains the escape hatch. The default changes from Parakeet TDT 0.6B v3 Q8_0 to Parakeet Ultra Q8_0: Moondream's post-trained NVIDIA Parakeet TDT 0.6B v3 (`moondream/parakeet-ultra`), as the GGUF `handy-computer/parakeet-ultra-gguf`, file `parakeet-ultra-Q8_0.gguf`, Hugging Face revision `39eeb55181f0d354fd934f06e92fd8d5037fed8e`, 740,363,168 bytes, SHA-256 `007a59761e9258779f189df396b50d12b83bb5b79e66df4b955c230d2f2a0a59`, CC-BY-4.0.

## Decision

The user chose Ultra after the ADR 0007 benchmark (four English TED talks, 9,273 reference words, human subtitles as references, M4 Pro, Metal, transcribe-cpp 0.3.1, 30 s chunks, one run per cell):

| Model | WER % | Sub | Del | Ins | Wall, 4 talks | Peak memory |
| --- | --- | --- | --- | --- | --- | --- |
| v3 Q8_0 | 3.21 | 121 | 67 | 110 | 64.0 s | 1.17 GB |
| Ultra Q8_0 | 3.15 | 129 | 56 | 107 | 59.4 s | 1.18 GB |

Per talk, v3 against Ultra: Treasure 3.19 / 3.13, Urban 3.91 / 4.09 (heavy audience laughter), Harari 2.90 / 2.85, Perel 2.95 / 2.69. Ultra is better on 3 of 4 talks and 0.18 points worse on the talk with laughter.

ADR 0007's bar (at least 0.3 points lower aggregate WER) is not met: Ultra is 0.06 points better. The user waived the bar, because Ultra ties or slightly wins at the same model size, memory and speed. The wall-time difference is within run-to-run noise on a loaded host (1-minute load average 10-74).

## Unchanged

- 30 s chunks (ADR 0003). Ultra with transcribe.cpp's own VAD cuts scored 3.31%, worse than 3.15% with scribe's 30 s chunks. Ultra keeps "fits snugly into memory" in the 27:26 regression talk at 30 s chunks.
- transcribe-cpp 0.3.1 loads and runs Ultra; no new dependency.

## Consequences

- The cache file name changes to `parakeet-ultra-Q8_0.gguf`, so a cached v3 file is never taken for Ultra. scribe reuses a cached default model only when its size and SHA-256 match; hashing costs about 1.5 s per run on an M4 Pro and overlaps media decoding.
- The first run after the upgrade downloads 740 MB. The old v3 file stays in the cache until the user deletes it.
- Existing transcripts stay as they are. Their `model` frontmatter records the URL of the model that wrote them.
- To keep v3, pass `-m https://huggingface.co/handy-computer/parakeet-tdt-0.6b-v3-gguf/resolve/90f082450fcbacdb54e5900c44ef697c9ea59622/parakeet-tdt-0.6b-v3-Q8_0.gguf`.
- The release that ships this change is 0.2.0.
