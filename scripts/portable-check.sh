#!/bin/sh
# Check a Linux release binary, not a source build. No packages are installed
# in the clean images. Usage: scripts/portable-check.sh <binary> [arm64|amd64]
# Also accepts linux/arm64 and linux/amd64; defaults to the Docker host's arch.
#
# "Works" means --version succeeds, ldd resolves every library, and doctor
# initializes the CPU backend and reports its device. Doctor must exit 1 only
# for missing runtime tools. A clean image has neither tools nor a model, so
# this is a loader/backend check, not an inference or install-readiness test.
set -eu

if [ "$#" -lt 1 ] || [ "$#" -gt 2 ] || [ ! -f "$1" ]; then
  echo "usage: $0 <binary> [arm64|amd64]" >&2
  exit 2
fi
binary=$(cd "$(dirname "$1")" && pwd)/$(basename "$1")
arch=${2:-$(docker info --format '{{.Architecture}}')}
case $arch in
  arm64 | aarch64 | linux/arm64) arch=arm64; builder=quay.io/pypa/manylinux_2_28_aarch64 ;;
  amd64 | x86_64 | linux/amd64) arch=amd64; builder=quay.io/pypa/manylinux_2_28_x86_64 ;;
  *) echo "unsupported platform: $arch" >&2; exit 2 ;;
esac

# Inspect with Linux binutils even when the host is macOS. The build image
# supplies these tools; the clean runtime images remain unmodified.
echo "== ELF requirements (linux/$arch)"
docker run --rm --platform "linux/$arch" -v "$binary:/scribe:ro" "$builder" sh -euc '
  objdump -T /scribe > /tmp/symbols
  objdump -p /scribe > /tmp/headers
  cat /tmp/headers
  if grep -q GLIBCXX_ /tmp/symbols; then
    echo "FAIL: dynamic GLIBCXX symbols remain" >&2; exit 1
  fi
  if grep -Ei "NEEDED.*(blas|stdc[+][+]|gcc_s)" /tmp/headers; then
    echo "FAIL: BLAS or compiler runtime dependency remains" >&2; exit 1
  fi
  # Nothing except glibc components may be required by the portable binary.
  awk '\''/NEEDED/ && $2 !~ /^(lib(c|m|pthread|dl|rt|resolv|util)\.so\.[0-9]+|ld-linux[^ ]*\.so\.[0-9]+)$/ { bad=1; print "FAIL: unexpected dependency " $2 } END { exit bad }'\'' /tmp/headers
  grep -oE "GLIBC_[0-9]+(\.[0-9]+)*" /tmp/symbols | sort -Vu > /tmp/glibc
  test -s /tmp/glibc
  highest=$(tail -n 1 /tmp/glibc)
  echo "highest required symbol: $highest"
  echo "$highest" | awk -F "[_.]" '\''{ if ($2 > 2 || ($2 == 2 && ($3 > 28 || ($3 == 28 && $4 > 0)))) exit 1 }'\'' || {
    echo "FAIL: glibc floor exceeds 2.28" >&2; exit 1
  }
'

images='ubuntu:24.04 debian:12 almalinux:8'
if [ "$arch" = amd64 ]; then images="$images archlinux:latest"; fi
for image in $images; do
  echo "== clean $image (linux/$arch)"
  docker run --rm --platform "linux/$arch" --network none \
    -v "$binary:/scribe:ro" "$image" sh -euc '
      /scribe --version
      ldd /scribe > /tmp/ldd 2>&1
      cat /tmp/ldd
      if grep -q "not found" /tmp/ldd; then exit 1; fi
      status=0
      /scribe doctor --backend cpu > /tmp/doctor 2>&1 || status=$?
      cat /tmp/doctor
      test "$status" -eq 1
      grep -Fx "backend: cpu (requested)" /tmp/doctor
      grep -E "^device: .* \(cpu\)$" /tmp/doctor
      grep -Fx "scribe: required runtime tools are missing or broken" /tmp/doctor
      echo "PASS: binary loads and CPU backend initializes (doctor exit $status: missing tools)"
    '
done
if [ "$arch" = arm64 ]; then
  echo 'SKIP: archlinux:latest has no official arm64 image'
fi
