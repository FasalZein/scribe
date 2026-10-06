# No encode/decode overlap on one loaded model

ADR 0006 suggested two sessions on one model, decoding one batch on the CPU while the next encodes on the GPU. transcribe-cpp 0.3.1 forbids it: `run_batch` holds a model-wide lock for the whole native call, and the C header allows at most one computation per model across all sessions, with corrupt CPU decodes and Metal command-buffer failures if bypassed. Parallel work needs one model per worker.

Decision: no overlap work in scribe. A second loaded model would cost about 0.74 GB to save at most the decode time (8.7 s of 22.9 s engine time on the 38-minute field run). Faster decoding goes upstream instead, as requests for per-utterance parallel decode and F16 or quantized decoder weights. Measure end-to-end stage times first; the engine is only part of the wall time.
