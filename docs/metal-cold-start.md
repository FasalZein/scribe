# Metal cold start

Finding for [issue #13](https://github.com/FasalZein/scribe/issues/13), measured on 2026-10-06 UTC.
A precompiled Metal library contains shaders compiled before scribe starts.
The pinned engine supports this path, but this host cannot build the library.
No release or Cargo configuration changes enable it yet.

## Measurements

The host runs macOS 27.0 (26A428) on an Apple M4 Pro with 14 CPU cores and 48 GB RAM.
The test uses the first 30 seconds of the Pi durable-sessions source and the cached `parakeet-ultra-Q8_0.gguf` model.
Each run uses the same release binary, explicit `--backend metal`, one decoder thread, and `--timings`.
Runs execute one at a time and write separate transcripts.

The first run uses a fresh `TMPDIR` directory.
The second run reuses that directory.
The third run uses another fresh directory to test whether `TMPDIR` reliably isolates the shader cache.
No shared cache files are deleted.

| Run | Library source compile | Model load | Process wall | Load averages before (1/5/15 min) | Load averages after (1/5/15 min) |
| --- | ---: | ---: | ---: | --- | --- |
| Fresh TMPDIR A | 17.689 s | 17.92 s | 18.3829 s | 8.18 / 8.21 / 12.16 | 7.96 / 8.17 / 12.07 |
| Reuse TMPDIR A | 0.012 s | 0.23 s | 0.6942 s | 7.96 / 8.17 / 12.07 | 7.56 / 8.08 / 12.02 |
| Fresh TMPDIR B | 0.012 s | 0.24 s | 0.6940 s | 7.56 / 8.08 / 12.02 | 7.56 / 8.08 / 12.02 |

All three runs exit with code 0.
`uptime` reports 2 days and 6:37 of host uptime for all measurements.
The first run reproduces the cold delay, but the third run remains warm despite a fresh `TMPDIR`.
A fresh `TMPDIR` is therefore not a reliable cold-cache reset on this host.

The cold log reports `using embedded metal library` and `loaded 20 libraries from embedded data in 17.689 sec`.
Both `fa` and `binbcast` report 17.689 seconds within that parallel compilation interval.
The engine takes 0.36 seconds to transcribe after model load.
The delay occurs inside the library source-compilation path, before transcription.

## Engine path and blocker

`transcribe-cpp-sys` 0.3.1 sets `GGML_METAL_EMBED_LIBRARY=ON` in `bindings/rust/sys/build.rs`.
Despite the name, this configuration embeds UTF-8 Metal source, not a compiled `.metallib`.
`ggml/src/ggml-metal/ggml-metal-device.m` passes that source to `newLibraryWithSource`.
macOS can cache compilation results from this call.
These timings do not separate source translation from internal work in the macOS compiler and cache.

The same build script forwards `TRANSCRIBE_CMAKE_ARGS` after its feature configuration.
`-DGGML_METAL_EMBED_LIBRARY=OFF` selects the existing precompiled path without a crate fork or registry edits.
The native CMake build compiles the kernels with `xcrun metal` and links `default.metallib` with `xcrun metallib`.
SDK 26 or newer also builds `ggml-tensor.metallib` for devices that support the tensor API.
The engine loads these files from a bundle or beside the running executable through `newLibraryWithURL`.

This host selects `/Library/Developer/CommandLineTools` and has SDK 27.0.
Both `xcrun --find metal` and `xcrun --find metallib` fail with code 72.
An out-of-tree CMake build with embedding disabled configures successfully, then fails on its first shader:

```text
[  8%] Compiling kernels/argsort.metal
xcrun: error: unable to find utility "metal", not a developer tool or in PATH
make[3]: *** [bin/argsort.air] Error 72
```

The CMake build exits with code 2.
No precompiled library exists from this probe, so no before/after comparison is available.
This is a local toolchain blocker, not proof that the engine cannot support precompiled libraries.

## Conditions for enabling it

A later implementation needs a host with both Metal compiler tools and a measured cold run of the precompiled variant.
It must also account for these existing contracts:

- Ordinary `cargo build --release` must place the library beside the executable or embed compiled bytes through an upstream-supported path.
- Release archives must contain the required libraries, and the installer must install them beside scribe.
- The current installer extracts only `scribe`, so adding a library to the archive alone does not install it.
- Supported OS and GPU combinations need tests for shader language, numeric configuration, and device feature differences.
- A precompiled library avoids `newLibraryWithSource`, but GPU pipeline specialization and model I/O still occur.

Keep the existing self-contained binary until those conditions have evidence.
Do not promise removal of the entire cold-start delay from this measurement alone.
The finding satisfies the ticket's documented-blocker alternative.
