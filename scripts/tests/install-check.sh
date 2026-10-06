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
export PATH="$root/install:$root/tools:$PATH"
installer="$repo/skills/scribe/scripts/install.sh"
if [ -n "${SCRIBE_INSTALL_TEST_BINARY:-}" ]; then
  mkdir -p "$root/skill/scripts"
  cp "$installer" "$root/skill/scripts/install.sh"
  printf '  version: "99.0.0"\n' >"$root/skill/SKILL.md"
  installer="$root/skill/scripts/install.sh"
fi
cat >"$root/tools/curl" <<'EOF'
#!/bin/sh
case "$*" in
  *releases/latest*) echo https://github.com/FasalZein/scribe/releases/tag/v99.0.0; exit 0 ;;
esac
while [ "$#" -gt 0 ]; do
  case "$1" in -o) dest=$2; shift 2 ;; *) url=$1; shift ;; esac
done
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
