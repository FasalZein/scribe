# scribe

Rust CLI (`src/`) plus the agent skill in `skills/scribe/` (`SKILL.md`, `reference/`, `scripts/install.sh`), laid out like pc-style/x-md so `npx skills add FasalZein/scribe --skill scribe` copies only that folder. Domain terms live in `GLOSSARY.md`; decisions live in `docs/adr/`.

- Delegated work on this repository (workers, reviewers, any helper that edits or reviews the CLI or the skill) runs on Opus: `anthropic/claude-opus-5-5`. The user asked for this.

## Commands

| Command | What it does |
| --- | --- |
| `cargo build --release && cargo test && cargo clippy --all-targets -- -D warnings && cargo fmt --check` | Full check. Run before handoff. |
| `SCRIBE_REGRESSION_MEDIA=<Pi durable-sessions talk> cargo test --release -- --ignored` | Lost-speech regression test with the default model (cached or downloaded); `SCRIBE_REGRESSION_MODEL=<gguf>` overrides it. Plain `cargo test` skips it. Run after any change to chunking, the engine or the default model. |
| `scribe <source> --timings` | Per-stage wall times on stderr; see ADR 0005 for how to read them. |
| `SCRIBE_TEST_MODEL=<gguf> SCRIBE_TEST_MEDIA=<mp4> scripts/linux-test.sh [arm64\|amd64]` | Linux CPU build, tests and a 3-minute end-to-end run in Docker. |
| `sh scripts/tests/install-check.sh` | Offline installer checks: safe replacement, CPU tiers, portable fallback and override. `SCRIBE_CPUINFO` injects CPU data for tests only. |
| `scripts/portable-check.sh <binary> [arm64\|amd64]` | Check the Linux release ELF floor (glibc ≤ 2.28, no BLAS or dynamic C++ runtime) and CPU backend in clean Docker images; not an inference test. |
| `git tag vX.Y.Z && git push origin vX.Y.Z` | Release: `.github/workflows/release.yml` builds and attaches the binaries. The tag must equal `version` in `Cargo.toml` and `metadata.version` in `skills/scribe/SKILL.md`; CI fails when they differ, and the installer reads the SKILL.md value. |

## Traps

- The default chunk is 30 s on purpose: transcribe-cpp's Parakeet drops whole sentences from chunks near 60 s (ADR 0003).
- Word count alone does not prove accuracy. Compare against a reference transcript, as in ADR 0003.
- A cold Metal model load takes about 17 s instead of 0.2 s in the embedded-source compilation path. macOS caches the result, and a fresh `TMPDIR` does not reliably reset that cache. Record library compilation times and host load before treating a run as cold or warm. See `docs/metal-cold-start.md` for measurements and the precompiled-library blocker.
- YouTube returns HTTP 403 now and then. scribe retries a failed cached `uvx yt-dlp` call once with `uvx yt-dlp@latest`; if that also fails, run scribe again before you treat it as a bug. Exclude such runs from benchmarks.
- Benchmark decoder speed under a controlled load (busy loops), not on whatever the host runs: 8 decoder threads stall when the host is busy (ADR 0006).
- Builds for other machines need `TRANSCRIBE_CMAKE_ARGS=-DGGML_NATIVE=OFF` (CI and release set it). A native build fails with GCC 12 in an OrbStack arm64 VM.
