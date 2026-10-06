# transcribe-cpp (ggml) over sherpa-onnx

We use the `transcribe-cpp` crate with Parakeet TDT 0.6B v3 Q8_0, not the `sherpa-onnx` crate, even though sherpa-onnx builds without cmake and ships VAD. On an M4 Pro, 27 min of audio took 19.4 s with transcribe-cpp on Metal in 60 s chunks, against 134 s with sherpa-onnx on the CPU. transcribe-cpp also gives Vulkan/CUDA on Linux and Windows and runs other model families (Whisper, Qwen3-ASR) from the same engine.

## Consequences

- Building needs cmake and a C++ toolchain.
- CPU-only speed is about 1.4x slower than sherpa-onnx (185 s against 134 s on the same Mac).
- Long audio must be cut into chunks; one pass over a whole file is about 6x slower.
