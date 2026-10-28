#!/usr/bin/env bash
# Unless explicitly stated otherwise all files in this repository are licensed under the Apache-2.0 License.
# This product includes software developed at Datadog (https://www.datadoghq.com/).
# Copyright 2026-Present Datadog, Inc.

# Packages release archives and a checksum file. Used identically by GitHub
# Actions and GitLab CI, so both releases are packaged the same way.
#
# Build mode (used right after a release build, needs tar; packaging only
# copies and tars already-built binaries, so this runs on any OS, even
# though the binaries themselves are macOS-only):
#   package.sh <version> <output-dir>
# Builds macos-code-signer_<version>_darwin_{arm64,amd64,universal}.tar.gz
# (binary, README.md, LICENSE, NOTICE and COPYING, the third-party license
# notices) and a matching SHA256SUMS in <output-dir>. <version>
# has no leading "v" (e.g. "0.1.0" or "0.1.0-abc1234"). Expects
# target/<triple>/release/macos-code-signer to already exist for
# aarch64-apple-darwin, x86_64-apple-darwin and universal-apple-darwin.
#
# Latest mode (used only when publishing an actual release, any OS):
#   package.sh --latest <source-dir> <output-dir>
# Copies the already-built tarballs from <source-dir> (any version) into
# <output-dir> under fixed, unversioned names
# (macos-code-signer_darwin_{arm64,amd64,universal}.tar.gz), with a matching
# SHA256SUMS. The binaries are not rebuilt or re-tarred: this only copies
# and re-hashes, so the versioned archives stay exactly as packaged.
set -euo pipefail

ARCHES=(arm64 amd64 universal)
ARCHIVE_FILES=(macos-code-signer README.md LICENSE NOTICE COPYING)

usage() {
  echo "Usage: $0 <version> <output-dir>" >&2
  echo "       $0 --latest <source-dir> <output-dir>" >&2
  exit 1
}

# Prints "sha256sum" or "shasum -a 256", whichever is on PATH. macOS has
# shasum; the Linux aws-cli image used to publish latest/ has sha256sum.
sha256_tool() {
  if command -v sha256sum >/dev/null 2>&1; then
    echo "sha256sum"
  elif command -v shasum >/dev/null 2>&1; then
    echo "shasum -a 256"
  else
    echo "no sha256 tool (sha256sum or shasum) found" >&2
    exit 1
  fi
}

checksum_dir() {
  local dir="$1"
  shift
  local tool
  tool="$(sha256_tool)"
  (
    cd "$dir"
    # shellcheck disable=SC2086 # $tool is a deliberate two-word command
    $tool -- "$@" >SHA256SUMS
  )
}

# Fails unless <archive> holds exactly ARCHIVE_FILES. Build mode only: latest
# mode must not need tar (the release-s3 image has none).
check_archive() {
  local listing expected
  listing="$(tar -tzf "$1" | sort)"
  expected="$(printf '%s\n' "${ARCHIVE_FILES[@]}" | sort)"
  if [ "$listing" != "$expected" ]; then
    echo "unexpected contents in $1:" >&2
    echo "$listing" >&2
    exit 1
  fi
}

triple_for_arch() {
  case "$1" in
    arm64) echo "aarch64-apple-darwin" ;;
    amd64) echo "x86_64-apple-darwin" ;;
    universal) echo "universal-apple-darwin" ;;
    *)
      echo "unknown arch: $1" >&2
      exit 1
      ;;
  esac
}

build_mode() {
  local version="$1"
  local outdir="$2"
  local repo_root
  repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

  mkdir -p "$outdir"
  outdir="$(cd "$outdir" && pwd)"

  local arch triple bin stage_dir archive
  local archives=()
  for arch in "${ARCHES[@]}"; do
    triple="$(triple_for_arch "$arch")"
    bin="$repo_root/target/${triple}/release/macos-code-signer"
    if [ ! -f "$bin" ]; then
      echo "missing binary: $bin" >&2
      exit 1
    fi

    stage_dir="$(mktemp -d)"
    trap 'rm -rf "$stage_dir"' RETURN

    cp "$bin" "$stage_dir/macos-code-signer"
    chmod +x "$stage_dir/macos-code-signer"
    cp "$repo_root/README.md" "$stage_dir/README.md"
    cp "$repo_root/LICENSE" "$repo_root/NOTICE" "$repo_root/COPYING" "$stage_dir/"

    archive="macos-code-signer_${version}_darwin_${arch}.tar.gz"
    tar -czf "$outdir/$archive" -C "$stage_dir" "${ARCHIVE_FILES[@]}"
    check_archive "$outdir/$archive"
    echo "wrote $outdir/$archive"
    archives+=("$archive")
  done

  checksum_dir "$outdir" "${archives[@]}"
  echo "wrote $outdir/SHA256SUMS"
}

latest_mode() {
  local srcdir="$1"
  local outdir="$2"

  srcdir="$(cd "$srcdir" && pwd)"
  mkdir -p "$outdir"
  outdir="$(cd "$outdir" && pwd)"

  local arch src dest
  local dests=()
  for arch in "${ARCHES[@]}"; do
    local matches=()
    shopt -s nullglob
    matches=("$srcdir"/macos-code-signer_*_darwin_"${arch}".tar.gz)
    shopt -u nullglob
    if [ "${#matches[@]}" -eq 0 ]; then
      echo "missing archive for arch ${arch} in ${srcdir}" >&2
      exit 1
    fi
    src="${matches[0]}"

    dest="macos-code-signer_darwin_${arch}.tar.gz"
    cp "$src" "$outdir/$dest"
    echo "wrote $outdir/$dest"
    dests+=("$dest")
  done

  checksum_dir "$outdir" "${dests[@]}"
  echo "wrote $outdir/SHA256SUMS"
}

if [ "$#" -eq 3 ] && [ "$1" = "--latest" ]; then
  latest_mode "$2" "$3"
elif [ "$#" -eq 2 ]; then
  build_mode "$1" "$2"
else
  usage
fi
