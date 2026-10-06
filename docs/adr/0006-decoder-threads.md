# One decoder thread on a GPU backend

transcribe-cpp runs the Parakeet TDT decoder on the CPU, one step at a time. Each step is two tiny ggml graphs (LSTM predictor and joint, about 0.3-0.9 ms of work), and the worker threads meet at a barrier after every graph node. scribe did not set `SessionOptions::n_threads`, so the library default of min(8, CPUs) applied. When the host is busy, the OS parks one of the 8 workers now and then, and every step waits for it.

`--threads N` now sets the engine threads; 0 (the default) picks per backend:

- GPU backend (Metal, Vulkan, CUDA): 1 thread. Only the decoder runs on the CPU.
- CPU backend: the library default. There the same setting also sets the encoder threads, and 1 thread would slow the encoder several times.

Measurement: 27:26 talk, Parakeet TDT v3 Q8_0, M4 Pro (10 P + 4 E cores), Metal, `scribe --timings --force`, transcribe-cpp 0.3.1. Thread counts ran interleaved, three rounds per condition. Load came from 8 or 12 `yes > /dev/null` busy loops; other apps also ran, so the 1-minute load average is given as a range. "idle" means no busy loops.

| Condition | 1-min load (range) | Threads | n | tdt-decode median (s) | tdt-decode max (s) | total median (s) | total max (s) |
| --- | --- | --- | --- | --- | --- | --- | --- |
| idle | 6-11 | 1 | 3 | 8.8 | 9.5 | 20.4 | 21.0 |
| idle | 6-10 | 2 | 3 | 6.1 | 6.2 | 17.4 | 19.4 |
| idle | 7-9 | 4 | 3 | 4.6 | 6.2 | 18.5 | 18.6 |
| idle | 8-12 | 8 | 3 | 6.3 | 6.9 | 18.3 | 20.8 |
| load8 | 12-38 | 1 | 3 | 11.1 | 13.1 | 25.9 | 28.4 |
| load8 | 13-30 | 2 | 3 | 20.8 | 46.2 | 34.6 | 60.9 |
| load8 | 15-78 | 4 | 3 | 35.5 | 82.7 | 49.5 | 97.8 |
| load8 | 15-91 | 8 | 3 | 84.4 | 99.5 | 98.6 | 113.8 |
| load12 | 21-152 | 1 | 3 | 12.8 | 14.0 | 26.9 | 28.4 |
| load12 | 36-168 | 2 | 3 | 26.8 | 33.8 | 40.7 | 46.7 |
| load12 | 32-153 | 4 | 3 | 67.7 | 71.4 | 81.0 | 85.3 |
| load12 | 42-78 | 8 | 3 | 138.3 | 197.1 | 153.0 | 209.2 |

- Under 8 busy loops, 1 thread cut the median total from 98.6 s to 25.9 s; under 12 loops, from 153.0 s to 26.9 s. Its worst run was 28.4 s against 209.2 s.
- Without busy loops, 1 thread cost 2.1 s on the median total (20.4 s against 18.3 s, +12%). 2 threads were the fastest idle (17.4 s) but had a 60.9 s run under 8 loops. We take the small idle cost for a stable worst case.
- Encode stayed at 9-14 s in every condition; it runs on the GPU.
- The thread count does not change the greedy decode result. The regression test still finds "fits snugly into memory".

## Consequences

- Revisit when transcribe-cpp decodes several utterances in parallel or stores the decoder weights in less than F32; both change the per-step cost.
- Two sessions on one model (decode one batch on the CPU while the next encodes on the GPU) could win back the idle cost. It is not implemented.
