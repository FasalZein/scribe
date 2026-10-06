#!/bin/sh
# Install the scribe binary and check its runtime tools (macOS and Linux).
#
# - Installs scribe when it is missing or older than this skill's version:
#   downloads the prebuilt binary for this OS and CPU from the latest GitHub
#   Release, verifies SHA-256, --version and doctor before replacement in $SCRIBE_INSTALL_DIR
#   (default ~/.local/bin). On any release failure, builds from source
#   with cargo when cargo, cmake and a C++ compiler exist.
# - Checks ffmpeg, ffprobe, and uvx or yt-dlp. Prints the install command for
#   each missing tool. It never runs a package manager or sudo.
# Safe to run again. Exits 0 when everything is ready, 1 otherwise.
set -u

REPO=FasalZein/scribe
skill_dir=$(cd "$(dirname "$0")/.." && pwd)
install_dir=${SCRIBE_INSTALL_DIR:-$HOME/.local/bin}
missing=""   # one line per problem, printed in the summary

say() { printf '%s\n' "$*"; }
problem() { missing="$missing
- $*"; }
have() { command -v "$1" >/dev/null 2>&1; }

# Versions are dotted numbers. Prints 1 when $1 >= $2, 0 otherwise.
version_ge() {
  awk -v a="$1" -v b="$2" 'BEGIN {
    n = split(a, x, "."); m = split(b, y, "."); if (m > n) n = m
    for (i = 1; i <= n; i++) { if (x[i] + 0 > y[i] + 0) { print 1; exit } if (x[i] + 0 < y[i] + 0) { print 0; exit } }
    print 1 }'
}

# The skill folder has no Cargo.toml; SKILL.md metadata carries the version (CI checks it matches).
skill_version=$(sed -n 's/^  version: "\(.*\)"/\1/p' "$skill_dir/SKILL.md" | head -n 1)

# --- package manager, for the printed install commands ----------------------
pm=""
if have brew; then pm=brew
elif have apt-get; then pm=apt
elif have dnf; then pm=dnf
elif have pacman; then pm=pacman
fi
# Prints the install command for a tool: ffmpeg | uv | build
install_hint() {
  case "$pm:$1" in
    brew:ffmpeg) say "brew install ffmpeg" ;;
    apt:ffmpeg) say "sudo apt-get install -y ffmpeg" ;;
    dnf:ffmpeg) say "sudo dnf install -y ffmpeg" ;;
    pacman:ffmpeg) say "sudo pacman -S --needed ffmpeg" ;;
    apt:curl) say "sudo apt-get install -y curl ca-certificates" ;;
    dnf:curl) say "sudo dnf install -y curl" ;;
    pacman:curl) say "sudo pacman -S --needed curl" ;;
    *:curl) say "install curl" ;;
    brew:uv) say "brew install uv" ;;
    dnf:uv) say "sudo dnf install -y uv" ;;
    pacman:uv) say "sudo pacman -S --needed uv" ;;
    *:uv) say "curl -LsSf https://astral.sh/uv/install.sh | sh" ;;
    brew:build) say "xcode-select --install; brew install cmake rustup && rustup-init -y" ;;
    apt:build) say "sudo apt-get install -y build-essential cmake && curl https://sh.rustup.rs -sSf | sh -s -- -y" ;;
    dnf:build) say "sudo dnf install -y gcc-c++ make cmake && curl https://sh.rustup.rs -sSf | sh -s -- -y" ;;
    pacman:build) say "sudo pacman -S --needed base-devel cmake rustup && rustup default stable" ;;
    *:ffmpeg) say "install ffmpeg (with ffprobe) from https://ffmpeg.org/download.html" ;;
    *:build) say "install Rust (https://rustup.rs), CMake and a C++ compiler" ;;
  esac
}

# --- scribe -----------------------------------------------------------------
# Prints the version of the scribe binary at $1, or nothing.
scribe_version() {
  version_output=$("$1" --version 2>/dev/null) || return 1
  printf '%s\n' "$version_output" | awk '$1 == "scribe" && $2 ~ /^[0-9]+[.][0-9]+[.][0-9]+$/ { print $2; exit }'
}

check_candidate() {
  candidate_version=$(scribe_version "$1") || { say "candidate --version failed: $1"; return 1; }
  if [ -z "$candidate_version" ] || [ "$(version_ge "$candidate_version" "$skill_version")" != 1 ]; then
    say "candidate is older than this skill ($skill_version) or has no version: $1"
    return 1
  fi
  "$1" doctor || { say "candidate doctor failed: $1"; return 1; }
}

# Stage beside the target for an atomic rename. Never replace an unchecked binary.
publish_candidate() {
  check_candidate "$1" || return 1
  mkdir -p "$install_dir" || return 1
  staged=$(mktemp "$install_dir/.scribe.new.XXXXXX") || return 1
  if cp "$1" "$staged" && chmod 755 "$staged" && mv -f "$staged" "$install_dir/scribe"; then
    return 0
  fi
  rm -f "$staged"
  return 1
}

find_scribe() {
  if [ -x "$install_dir/scribe" ]; then say "$install_dir/scribe"
  elif have scribe; then command -v scribe
  fi
}

fetch() { # url dest
  if have curl; then curl -fsSL --retry 2 -o "$2" "$1"
  elif have wget; then wget -q -O "$2" "$1"
  else return 1
  fi
}

latest_tag() {
  if have curl; then
    url=$(curl -fsLI -o /dev/null -w '%{url_effective}' "https://github.com/$REPO/releases/latest") || return 1
  elif have wget; then
    url=$(wget -q -S --spider "https://github.com/$REPO/releases/latest" 2>&1 | sed -n 's/^ *[Ll]ocation: //p' | tail -n 1)
  else
    return 1
  fi
  case $url in */releases/tag/v*) say "${url##*/tag/}" | tr -d '\r' ;; *) return 1 ;; esac
}

sha256_of() {
  if have sha256sum; then sha256sum "$1" | awk '{ print $1 }'
  elif have shasum; then shasum -a 256 "$1" | awk '{ print $1 }'
  else return 1
  fi
}

release_target() {
  case "$(uname -s):$(uname -m)" in
    Darwin:arm64) say aarch64-apple-darwin ;;
    Linux:x86_64) say x86_64-unknown-linux-gnu ;;
    Linux:aarch64 | Linux:arm64) say aarch64-unknown-linux-gnu ;;
    *) return 1 ;;
  esac
}

# Downloads, verifies and installs the release binary. Returns 2 when no
# release asset matches this machine, 1 on any other failure.
install_release() {
  target=$(release_target) || { say "no prebuilt scribe for $(uname -s) $(uname -m)"; return 2; }
  tag=$(latest_tag) || { say "cannot find the latest release of $REPO (curl or wget and network needed)"; return 2; }
  asset="scribe-${tag#v}-$target.tar.gz"
  base="https://github.com/$REPO/releases/download/$tag"
  tmp=$(mktemp -d) || return 1
  say "downloading $base/$asset"
  if ! fetch "$base/$asset" "$tmp/$asset" || ! fetch "$base/$asset.sha256" "$tmp/$asset.sha256"; then
    say "release $tag has no asset $asset"
    rm -rf "$tmp"; return 2
  fi
  expected=$(awk '{ print $1; exit }' "$tmp/$asset.sha256")
  actual=$(sha256_of "$tmp/$asset") || { say "need sha256sum or shasum to verify the download"; rm -rf "$tmp"; return 1; }
  if [ "$expected" != "$actual" ]; then
    say "SHA-256 mismatch for $asset: expected $expected, got $actual"
    rm -rf "$tmp"; return 1
  fi
  say "SHA-256 verified: $actual"
  tar -xzf "$tmp/$asset" -C "$tmp" scribe || { rm -rf "$tmp"; return 1; }
  chmod 755 "$tmp/scribe" && publish_candidate "$tmp/scribe"
  status=$?
  rm -rf "$tmp"
  [ $status -eq 0 ] && say "installed $install_dir/scribe ($tag)"
  return $status
}

install_from_source() {
  cxx=""; for c in c++ g++ clang++; do have "$c" && { cxx=$c; break; }; done
  if have cargo && have cmake && [ -n "$cxx" ]; then
    say "building scribe from source with cargo (a few minutes)"
    # An isolated cargo root keeps a failed build from replacing a binary on PATH.
    build_root=$(mktemp -d) || return 1
    if cargo install --locked --root "$build_root" --git "https://github.com/$REPO" --tag "v$skill_version" scribe; then
      if publish_candidate "$build_root/bin/scribe"; then
        rm -rf "$build_root"
        return 0
      fi
      rm -rf "$build_root"
      problem "scribe: source build failed its readiness check or cannot be installed"
      return 1
    fi
    rm -rf "$build_root"
    problem "cargo install --git https://github.com/$REPO --tag v$skill_version failed; see the output above"
    return 1
  fi
  need=""
  have cargo || need="$need cargo"
  have cmake || need="$need cmake"
  [ -n "$cxx" ] || need="$need C++-compiler"
  problem "scribe: no prebuilt binary and cannot build from source (missing:$need). Install them: $(install_hint build); then run this script again."
  return 1
}

current=$(find_scribe)
current_version=""
[ -n "$current" ] && current_version=$(scribe_version "$current")
if [ -n "$current_version" ] && [ "$(version_ge "$current_version" "$skill_version")" = 1 ]; then
  say "ok: scribe $current_version ($current)"
else
  if [ -n "$current_version" ]; then
    say "scribe $current_version is older than $skill_version; updating"
  else
    say "scribe is not installed; installing"
  fi
  if have curl || have wget; then
    install_release
  else
    false
  fi
  case $? in
    0) ;;
    *) say "release install failed; keeping the old binary and trying a source build"
       install_from_source ;;
  esac
  current=$(find_scribe)
  [ -n "$current" ] && current_version=$(scribe_version "$current")
  if [ -z "$current" ]; then
    [ -n "$missing" ] || problem "scribe: not found after the install"
  elif [ -z "$current_version" ]; then problem "scribe: $current --version fails on this machine"
  elif [ "$(version_ge "$current_version" "$skill_version")" = 1 ]; then say "ok: scribe $current_version ($current)"
  else
    # The latest release is older than this skill: the skill came from a newer commit.
    problem "scribe $current_version is older than this skill ($skill_version); its release is not published yet. Build it: cargo install --locked --git https://github.com/$REPO --tag v$skill_version scribe"
  fi
fi

if [ -x "$install_dir/scribe" ]; then
  case ":$PATH:" in
    *":$install_dir:"*) ;;
    *) problem "PATH: $install_dir is not on PATH. Run: export PATH=\"$install_dir:\$PATH\" (add it to your shell profile), or call $install_dir/scribe directly." ;;
  esac
fi

# --- runtime tools ----------------------------------------------------------
for tool in ffmpeg ffprobe; do
  if have $tool; then say "ok: $tool"
  else problem "$tool: not found. Install: $(install_hint ffmpeg)"
  fi
done
if have uvx; then say "ok: uvx (runs yt-dlp@latest)"
elif have yt-dlp; then say "ok: yt-dlp (keep it current; old versions get HTTP 403 from YouTube)"
else problem "uvx or yt-dlp: not found. Install uv: $(install_hint uv)"
fi

if [ -n "$current" ] && ! "$current" doctor; then
  problem "scribe: doctor failed for $current; see the output above"
fi

if [ -n "$missing" ]; then
  say ""
  say "NOT READY:$missing"
  exit 1
fi
say "ready: scribe $current_version"
