# 30 s chunks, not 60 s

The default target chunk length is 30 s. With transcribe-cpp 0.3.1 and Parakeet TDT 0.6b v3, some chunks near 60 s lose whole sentences: the 52.6 s first chunk of a 27:26 talk dropped "And everything fits snugly into memory as well, ...". The loss is deterministic and the same with `run` and `run_batch`, segment or no timestamps, Metal or CPU, and Q8_0 or F16. parakeet-mlx keeps the sentence on the same clip, so the cause is in the engine, not the audio or the model weights.

Word error rate against the parakeet-mlx transcript of the same talk (fillers removed), silence-aware chunks, batch API on an M4 Pro:

| Target | Chunks | WER | Deleted words | Engine time |
| --- | --- | --- | --- | --- |
| 60 s | 30 | 6.16% | 198 | 14.6 s |
| 45 s | 42 | 4.21% | 60 | 15.5 s |
| 30 s | 66 | 3.61% | 24 | 13.4 s |
| 20 s | 112 | 4.17% | 23 | 15.9 s |

Engine time varies by about 2 s between runs (30 s also measured 15.3 s); a full end-to-end run at 30 s took 14.4 s wall time. The same order held with the audio shifted by 7 s (60 s: 6.14%, 30 s: 3.78%), so the result does not depend on one set of cut points.

## Consequences

- Shorter chunks cost no measurable speed on Metal with the batch API.
- The evidence comes from one recording; revisit the default when transcribe-cpp changes its Parakeet path or a second reference recording disagrees.
