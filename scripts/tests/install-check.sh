#!/bin/sh
# Exercise the real installer offline, with release and cargo process boundaries replaced.
# Runs on the host or inside scripts/linux-test.sh. No network or model is needed.
# SCRIBE_INSTALL_TEST_BINARY optionally supplies the real working binary to preserve.
set -eu
repo=$(cd "$(dirname "$0")/../.." && pwd)
root=$(mktemp -d)
trap 'rm -rf "$root"' EXIT HUP INT TERM
mkdir -p "$root/tools" "$root/install" "$root/asset"
export TEST_ROOT="$root" SCRIBE_INSTALL_DIR="$root/install"
export SCRIBE_CPUINFO="$root/cpuinfo"
: >"$SCRIBE_CPUINFO"
export PATH="$root/install:$root/tools:$PATH"
installer="$repo/skills/scribe/scripts/install.sh"
mkdir -p "$root/skill/scripts"
cp "$installer" "$root/skill/scripts/install.sh"
printf '  version: "99.0.0"\n' >"$root/skill/SKILL.md"
installer="$root/skill/scripts/install.sh"
cat >"$root/tools/curl" <<'EOF'
#!/bin/sh
case "$*" in
  *releases/latest*) echo https://github.com/FasalZein/scribe/releases/tag/v99.0.0; exit 0 ;;
esac
while [ "$#" -gt 0 ]; do
  case "$1" in -o) dest=$2; shift 2 ;; *) url=$1; shift ;; esac
done
echo "$url" >>"$TEST_ROOT/downloads"
if [ "${TEST_FALLBACK:-}" = yes ]; then
  case "$url:${TEST_TUNED_FAILURE:-doctor}" in
    *-avx2.tar.gz:missing | *-i8mm.tar.gz:missing | *-dotprod.tar.gz:missing) exit 22 ;;
    *-avx2.tar.gz.sha256:checksum | *-i8mm.tar.gz.sha256:checksum | *-dotprod.tar.gz.sha256:checksum) echo wrong >"$dest"; exit ;;
  esac
  case "$url" in
    *-gnu.tar.gz) cp "$TEST_ROOT/portable.tar.gz" "$dest"; exit ;;
    *-gnu.tar.gz.sha256) cp "$TEST_ROOT/portable.sha256" "$dest"; exit ;;
  esac
fi
case "$url" in *.sha256) cp "$TEST_ROOT/release.sha256" "$dest" ;; *) cp "$TEST_ROOT/release.tar.gz" "$dest" ;; esac
EOF
cat >"$root/tools/cargo" <<'EOF'
#!/bin/sh
echo called >"$TEST_ROOT/cargo-called"
[ "$SOURCE_READY" != no ] || exit 1
while [ "$#" -gt 0 ]; do
  case "$1" in --root) build=$2; shift 2 ;; *) shift ;; esac
done
mkdir -p "$build/bin"
if [ "$SOURCE_READY" = yes ]; then cp "$TEST_ROOT/good" "$build/bin/scribe"
else cp "$TEST_ROOT/asset/scribe" "$build/bin/scribe"; fi
chmod +x "$build/bin/scribe"
EOF
for tool in ffmpeg ffprobe uvx cmake c++; do
  printf '#!/bin/sh\necho test-version\n' >"$root/tools/$tool"
done
chmod +x "$root/tools/"*
printf '#!/bin/sh\ncase "$1" in --version) echo "scribe 99.0.0" ;; doctor) exit 0 ;; *) exit 1 ;; esac\n' >"$root/good"
printf '#!/bin/sh\ncase "$1" in --version) echo "scribe 0.1.0" ;; doctor) exit 0 ;; *) exit 1 ;; esac\n' >"$root/old"
if [ -n "${SCRIBE_INSTALL_TEST_BINARY:-}" ]; then
  cp "$SCRIBE_INSTALL_TEST_BINARY" "$root/old"
  echo 'Testing preservation of the real built binary'
fi
chmod +x "$root/good" "$root/old"

run_case() {
  mode=$1; export SOURCE_READY=$2
  cp "$root/old" "$root/install/scribe"
  rm -f "$root/cargo-called"
  case "$mode" in
    version) printf '#!/bin/sh\nexit 126\n' >"$root/asset/scribe" ;;
    doctor) printf '#!/bin/sh\ncase "$1" in --version) echo "scribe 99.0.0" ;; *) exit 1 ;; esac\n' >"$root/asset/scribe" ;;
    older) cp "$root/old" "$root/asset/scribe" ;;
    good) cp "$root/good" "$root/asset/scribe" ;;
  esac
  chmod +x "$root/asset/scribe"
  tar -czf "$root/release.tar.gz" -C "$root/asset" scribe
  if command -v sha256sum >/dev/null; then sha256sum "$root/release.tar.gz" >"$root/release.sha256"
  else shasum -a 256 "$root/release.tar.gz" >"$root/release.sha256"; fi
  status=0
  sh "$installer" >"$root/output" 2>&1 || status=$?
  if [ "$mode" != good ]; then
    if [ ! -f "$root/cargo-called" ]; then cat "$root/output"; echo 'FAIL: source fallback was not attempted'; exit 1; fi
  fi
  if [ "$SOURCE_READY" = yes ] || [ "$mode" = good ]; then
    [ "$status" -eq 0 ] && cmp "$root/good" "$root/install/scribe" || { cat "$root/output"; echo 'FAIL: checked candidate did not install'; exit 1; }
  else
    [ "$status" -ne 0 ] && cmp "$root/old" "$root/install/scribe" || { cat "$root/output"; echo 'FAIL: broken candidate replaced working binary'; exit 1; }
  fi
  echo "PASS: download=$mode source=$SOURCE_READY exit=$status"
}
run_case version no
run_case doctor no
run_case older no
run_case doctor broken
run_case doctor yes
run_case good no

# A current binary still needs a passing doctor. Version alone is not readiness.
cp "$root/asset/scribe" "$root/install/scribe"
printf '#!/bin/sh\ncase "$1" in --version) echo "scribe 99.0.0" ;; *) exit 1 ;; esac\n' >"$root/install/scribe"
status=0
sh "$installer" >"$root/output" 2>&1 || status=$?
[ "$status" -ne 0 ] || { cat "$root/output"; echo 'FAIL: broken installed backend reported ready'; exit 1; }
echo "PASS: current binary doctor fails exit=$status"

# The installer reads injected CPU data but still runs its real download,
# checksum and pre-replace checks. uname is the platform boundary fake.
cat >"$root/tools/uname" <<'EOF'
#!/bin/sh
case "$1" in -s) echo "${TEST_OS:-Linux}" ;; -m) echo "$TEST_ARCH" ;; esac
EOF
chmod +x "$root/tools/uname"
export TEST_ARCH=aarch64
run_tier_case() {
  flags=$1; tier=$2
  if [ "$flags" = unreadable ]; then rm -f "$SCRIBE_CPUINFO"
  else printf '%s\n' "$flags" >"$SCRIBE_CPUINFO"; fi
  cp "$root/old" "$root/install/scribe"
  cp "$root/good" "$root/asset/scribe"
  tar -czf "$root/release.tar.gz" -C "$root/asset" scribe
  if command -v sha256sum >/dev/null; then sha256sum "$root/release.tar.gz" >"$root/release.sha256"
  else shasum -a 256 "$root/release.tar.gz" >"$root/release.sha256"; fi
  : >"$root/downloads"
  rm -f "$root/cargo-called"
  sh "$installer" >"$root/output" 2>&1 || { cat "$root/output"; exit 1; }
  case "${TEST_OS:-Linux}:$TEST_ARCH" in Darwin:arm64) target=aarch64-apple-darwin ;; Linux:aarch64) target=aarch64-unknown-linux-gnu ;; Linux:x86_64) target=x86_64-unknown-linux-gnu ;; esac
  suffix="-$tier"; [ "$tier" != portable ] || suffix=""
  grep -Fx "https://github.com/FasalZein/scribe/releases/download/v99.0.0/scribe-99.0.0-$target$suffix.tar.gz" "$root/downloads" >/dev/null || {
    cat "$root/downloads" "$root/output"; echo "FAIL: expected $tier tier"; exit 1;
  }
  grep -F "CPU tier: $tier" "$root/output" >/dev/null || { cat "$root/output"; exit 1; }
  [ ! -f "$root/cargo-called" ]
  cmp "$root/good" "$root/install/scribe"
  [ "$(grep -c '[.]tar[.]gz$' "$root/downloads")" -eq 1 ]
  echo "PASS: ${TEST_OS:-Linux}/$TEST_ARCH tier=$tier exit=0 flags=$flags override=${SCRIBE_CPU_TIER:-auto}"
}
# Raspberry Pi 5, Graviton 2 and Ampere Altra support dotprod/FP16, not i8mm.
run_tier_case 'Features : fp asimd asimddp fphp asimdhp' dotprod
# Graviton 3 also supports i8mm.
run_tier_case 'Features : fp asimd asimddp fphp asimdhp i8mm' i8mm
run_tier_case 'Features : fp asimd' portable
run_tier_case 'Features : fp asimd asimddp fphp' portable
run_tier_case 'Features : fp asimd asimddp asimdhp' portable
run_tier_case 'Features : fp asimd fphp asimdhp i8mm' portable
run_tier_case 'Features : fp asimd asimddp fphp asimdhp i8mm
Features : fp asimd asimddp fphp asimdhp' dotprod
run_tier_case 'Features : fp asimd asimddpx fphp asimdhp' portable
run_tier_case '' portable
run_tier_case unreadable portable
export SCRIBE_CPU_TIER=portable
run_tier_case 'Features : fp asimd asimddp fphp asimdhp i8mm' portable
unset SCRIBE_CPU_TIER
export TEST_ARCH=x86_64
run_tier_case 'flags : sse4_2 avx avx2 fma f16c bmi2' avx2
run_tier_case unreadable portable
# AVX and SSE4.2 are explicitly enabled in the tuned build too.
run_tier_case 'flags : sse4_2 avx2 fma f16c bmi2' portable
run_tier_case 'flags : avx avx2 fma f16c bmi2' portable
run_tier_case 'flags : sse4_2 avx fma f16c bmi2' portable
run_tier_case 'flags : sse4_2 avx avx2 f16c bmi2' portable
run_tier_case 'flags : sse4_2 avx avx2 fma bmi2' portable
run_tier_case 'flags : sse4_2 avx avx2 fma f16c' portable
run_tier_case 'flags : sse4_2 avx avx2 fma f16c bmi2
flags : sse2' portable
export SCRIBE_CPU_TIER=portable
run_tier_case 'flags : sse4_2 avx avx2 fma f16c bmi2' portable
unset SCRIBE_CPU_TIER

# Linux-looking CPU data must not change the macOS asset.
export TEST_OS=Darwin TEST_ARCH=arm64
run_tier_case 'Features : asimddp fphp asimdhp i8mm' portable
unset TEST_OS
export TEST_ARCH=x86_64

# Separate portable and tuned archives prove retry order and publication.
cp "$root/release.tar.gz" "$root/portable.tar.gz"
cp "$root/release.sha256" "$root/portable.sha256"
export TEST_FALLBACK=yes SOURCE_READY=no
for tuned in avx2 dotprod i8mm; do
  case "$tuned" in
    avx2) TEST_ARCH=x86_64; printf 'flags : sse4_2 avx avx2 fma f16c bmi2\n' >"$SCRIBE_CPUINFO"; target=x86_64-unknown-linux-gnu ;;
    dotprod) TEST_ARCH=aarch64; printf 'Features : asimddp fphp asimdhp\n' >"$SCRIBE_CPUINFO"; target=aarch64-unknown-linux-gnu ;;
    i8mm) TEST_ARCH=aarch64; printf 'Features : asimddp fphp asimdhp i8mm\n' >"$SCRIBE_CPUINFO"; target=aarch64-unknown-linux-gnu ;;
  esac
  export TEST_ARCH
  for TEST_TUNED_FAILURE in doctor version missing checksum; do
    export TEST_TUNED_FAILURE
    case "$TEST_TUNED_FAILURE" in
      version) printf '#!/bin/sh\nexit 132\n' >"$root/asset/scribe" ;;
      *) printf '#!/bin/sh\ncase "$1" in --version) echo "scribe 99.0.0" ;; *) exit 132 ;; esac\n' >"$root/asset/scribe" ;;
    esac
    chmod +x "$root/asset/scribe"
    tar -czf "$root/release.tar.gz" -C "$root/asset" scribe
    if command -v sha256sum >/dev/null; then sha256sum "$root/release.tar.gz" >"$root/release.sha256"
    else shasum -a 256 "$root/release.tar.gz" >"$root/release.sha256"; fi
    cp "$root/old" "$root/install/scribe"
    : >"$root/downloads"
    status=0
    sh "$installer" >"$root/output" 2>&1 || status=$?
    [ "$status" -eq 0 ] || { cat "$root/output"; exit 1; }
    cmp "$root/good" "$root/install/scribe"
    [ ! -f "$root/cargo-called" ]
    grep -F 'CPU tier: portable' "$root/output" >/dev/null
    grep -F "/scribe-99.0.0-$target-$tuned.tar.gz" "$root/downloads" >/dev/null
    grep -F "/scribe-99.0.0-$target.tar.gz" "$root/downloads" >/dev/null
    [ "$TEST_TUNED_FAILURE" != doctor ] || grep -F 'candidate doctor failed' "$root/output" >/dev/null
    # The archive request must precede the portable archive request.
    archives=$(grep '[.]tar[.]gz$' "$root/downloads")
    expected=$(printf '%s\n%s' "https://github.com/FasalZein/scribe/releases/download/v99.0.0/scribe-99.0.0-$target-$tuned.tar.gz" "https://github.com/FasalZein/scribe/releases/download/v99.0.0/scribe-99.0.0-$target.tar.gz")
    [ "$archives" = "$expected" ] || { cat "$root/downloads"; exit 1; }
    echo "PASS: $TEST_ARCH $tuned $TEST_TUNED_FAILURE failure installs portable without a source build"
  done
done

# Exhausted tuned and portable candidates cannot report ready or replace the old binary.
unset TEST_FALLBACK TEST_TUNED_FAILURE
: >"$root/downloads"
run_case doctor no
grep -F '/scribe-99.0.0-aarch64-unknown-linux-gnu-i8mm.tar.gz' "$root/downloads" >/dev/null
grep -F '/scribe-99.0.0-aarch64-unknown-linux-gnu.tar.gz' "$root/downloads" >/dev/null
echo 'PASS: tuned and portable doctor failures preserve old binary and exit 1'

# The override does not replace an already current, ready installation.
cp "$root/good" "$root/install/scribe"
export SCRIBE_CPU_TIER=portable
: >"$root/downloads"
rm -f "$root/cargo-called"
status=0
sh "$installer" >"$root/output" 2>&1 || status=$?
[ "$status" -eq 0 ]
[ ! -s "$root/downloads" ]
[ ! -f "$root/cargo-called" ]
cmp "$root/good" "$root/install/scribe"
echo 'PASS: portable override leaves a current ready installation unchanged, exit 0'
