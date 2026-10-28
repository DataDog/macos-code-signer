#!/usr/bin/env bash
# Unless explicitly stated otherwise all files in this repository are licensed under the Apache-2.0 License.
# This product includes software developed at Datadog (https://www.datadoghq.com/).
# Copyright 2026-Present Datadog, Inc.

# Installs a pinned, checksum-verified cargo-about into
# <dest-dir>/cargo-about. Used by scripts/gen-copying.sh.
#
# Usage: install-cargo-about.sh <dest-dir>
set -euo pipefail

CARGO_ABOUT_VERSION="0.9.3"
CARGO_ABOUT_BASE_URL="https://github.com/EmbarkStudios/cargo-about/releases/download/${CARGO_ABOUT_VERSION}"

# From the release's published .sha256 files.
CARGO_ABOUT_SHA256_MACOS_ARM64="45520df8680978a6e7c4023ace0e3f311c73efc208cd68e5a7f4ed4822fa0349"
CARGO_ABOUT_SHA256_LINUX_X86_64="3ad1e94ea1e63d69d1961f4fb15ccbbe9575d906111f96d2cb4dce6df48f71b4"

if [ "$#" -ne 1 ]; then
  echo "Usage: $0 <dest-dir>" >&2
  exit 1
fi

dest_dir="$1"

case "$(uname -s)-$(uname -m)" in
  Darwin-arm64)
    triple="aarch64-apple-darwin"
    expected_sha256="$CARGO_ABOUT_SHA256_MACOS_ARM64"
    ;;
  Linux-x86_64)
    triple="x86_64-unknown-linux-musl"
    expected_sha256="$CARGO_ABOUT_SHA256_LINUX_X86_64"
    ;;
  *)
    echo "unsupported platform for cargo-about: $(uname -s)-$(uname -m)" >&2
    exit 1
    ;;
esac
asset="cargo-about-${CARGO_ABOUT_VERSION}-${triple}.tar.gz"

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

tmp_dir="$(mktemp -d)"
trap 'rm -rf "$tmp_dir"' EXIT

curl --proto '=https' --tlsv1.2 -sSfL -o "$tmp_dir/$asset" "$CARGO_ABOUT_BASE_URL/$asset"

tool="$(sha256_tool)"
# shellcheck disable=SC2086 # $tool is a deliberate two-word command
actual_sha256="$($tool "$tmp_dir/$asset" | awk '{print $1}')"
if [ "$actual_sha256" != "$expected_sha256" ]; then
  echo "checksum mismatch for $asset: expected $expected_sha256, got $actual_sha256" >&2
  exit 1
fi

tar -xzf "$tmp_dir/$asset" -C "$tmp_dir"

extracted="$(find "$tmp_dir" -type f -name cargo-about | head -1)"
if [ -z "$extracted" ]; then
  echo "cargo-about binary not found in $asset" >&2
  exit 1
fi

mkdir -p "$dest_dir"
cp "$extracted" "$dest_dir/cargo-about"
chmod +x "$dest_dir/cargo-about"
echo "installed cargo-about ${CARGO_ABOUT_VERSION} to $dest_dir/cargo-about"
