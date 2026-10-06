#!/bin/sh
# Build and test scribe on Linux in Docker (for example OrbStack), on the CPU backend.
#
# Usage: scripts/linux-test.sh [arm64|amd64]   (default: arm64)
#
# Steps: build the image, cargo build --release, cargo test, the ignored
# regression test, installer preservation checks, and a 3-minute transcription.
# Environment:
#   SCRIBE_TEST_MODEL  Parakeet GGUF on the host (mounted read-only). Required.
#   SCRIBE_TEST_MEDIA  media file on the host (mounted read-only). Required.
#                      The regression test needs the Pi durable-sessions talk.
#   SCRIBE_CLIP_SECS   clip length for the end-to-end run (default 180).
#   TRANSCRIBE_CMAKE_ARGS  extra CMake arguments (default -DGGML_NATIVE=OFF
#                      -DTRANSCRIBE_USE_SYSTEM_BLAS=OFF, as in the Linux release;
#                      a native build fails with GCC 12 in an OrbStack arm64 VM).
set -eu

arch=${1:-arm64}
case $arch in arm64 | amd64) ;; *) echo "usage: $0 [arm64|amd64]" >&2; exit 2 ;; esac
: "${SCRIBE_TEST_MODEL:?set SCRIBE_TEST_MODEL to a Parakeet GGUF path}"
: "${SCRIBE_TEST_MEDIA:?set SCRIBE_TEST_MEDIA to a media file path}"
clip_secs=${SCRIBE_CLIP_SECS:-180}
cmake_args=${TRANSCRIBE_CMAKE_ARGS:--DGGML_NATIVE=OFF -DTRANSCRIBE_USE_SYSTEM_BLAS=OFF}
repo=$(cd "$(dirname "$0")/.." && pwd)
image=scribe-linux-test:$arch

docker build --platform "linux/$arch" -t "$image" - <<'EOF'
FROM rust:1-bookworm
RUN apt-get update \
 && apt-get install -y --no-install-recommends cmake g++ ffmpeg time \
 && rm -rf /var/lib/apt/lists/*
COPY --from=ghcr.io/astral-sh/uv:latest /uv /uvx /usr/local/bin/
EOF

# The source mounts read-only; build output and the cargo registry live in
# per-architecture volumes, so the host target/ stays untouched.
docker run --rm --platform "linux/$arch" \
  -v "$repo:/src:ro" \
  -v "scribe-linux-target-$arch:/target" \
  -v "scribe-linux-cargo-$arch:/usr/local/cargo/registry" \
  -v "$SCRIBE_TEST_MODEL:/model.gguf:ro" \
  -v "$SCRIBE_TEST_MEDIA:/media/input:ro" \
  -e CARGO_TARGET_DIR=/target -e CLIP_SECS="$clip_secs" \
  -e TRANSCRIBE_CMAKE_ARGS="$cmake_args" \
  -w /src "$image" sh -euc '
    now() { date +%s; }
    echo "== $(uname -m), $(nproc) CPUs"
    t=$(now); cargo build --release --locked; echo "== build: $(( $(now) - t )) s"
    SCRIBE_INSTALL_TEST_BINARY=/target/release/scribe sh /src/scripts/tests/install-check.sh
    /target/release/scribe doctor --backend cpu --model /model.gguf
    t=$(now); cargo test --release --locked; echo "== cargo test: $(( $(now) - t )) s"
    t=$(now)
    SCRIBE_REGRESSION_MODEL=/model.gguf SCRIBE_REGRESSION_MEDIA=/media/input \
      cargo test --release --locked -- --ignored
    echo "== regression test: $(( $(now) - t )) s"
    ffmpeg -loglevel error -y -t "$CLIP_SECS" -i /media/input -c copy /tmp/clip.mp4
    /usr/bin/time -v /target/release/scribe /tmp/clip.mp4 -m /model.gguf \
      --backend cpu --timings -o /tmp/out 2>/tmp/stderr.log | tee /tmp/index.path
    cat /tmp/stderr.log
    index=$(cat /tmp/index.path)
    words=$(sed -n "s/^words: //p" "$index" | head -n 1)
    echo "== end-to-end: clip ${CLIP_SECS} s, words ${words}"
    grep -E "Elapsed \(wall clock\)|Maximum resident set size" /tmp/stderr.log
  '
