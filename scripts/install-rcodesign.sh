#!/usr/bin/env bash
# Installs a pinned, checksum-verified rcodesign (from the apple-codesign
# project) into <dest-dir>/rcodesign. Used by scripts/sign-legacy.sh on both
# macOS (signing) and Linux (notarization) CI jobs.
#
# Usage: install-rcodesign.sh <dest-dir>
set -euo pipefail

RCODESIGN_VERSION="0.29.0"
RCODESIGN_BASE_URL="https://github.com/indygreg/apple-platform-rs/releases/download/apple-codesign/${RCODESIGN_VERSION}"

# From the release's published SHA256SUMS.
RCODESIGN_SHA256_MACOS_UNIVERSAL="d98372d5524226ccf9dc0eda03d4e4f5826182dabb2fc3f2bd303ed9113a748d"
RCODESIGN_SHA256_LINUX_X86_64="dbe85cedd8ee4217b64e9a0e4c2aef92ab8bcaaa41f20bde99781ff02e600002"

if [ "$#" -ne 1 ]; then
  echo "Usage: $0 <dest-dir>" >&2
  exit 1
fi

dest_dir="$1"

case "$(uname -s)" in
  Darwin)
    asset="apple-codesign-${RCODESIGN_VERSION}-macos-universal.tar.gz"
    expected_sha256="$RCODESIGN_SHA256_MACOS_UNIVERSAL"
    ;;
  Linux)
    asset="apple-codesign-${RCODESIGN_VERSION}-x86_64-unknown-linux-musl.tar.gz"
    expected_sha256="$RCODESIGN_SHA256_LINUX_X86_64"
    ;;
  *)
    echo "unsupported OS for rcodesign: $(uname -s)" >&2
    exit 1
    ;;
esac

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

curl --proto '=https' --tlsv1.2 -sSfL -o "$tmp_dir/$asset" "$RCODESIGN_BASE_URL/$asset"

tool="$(sha256_tool)"
# shellcheck disable=SC2086 # $tool is a deliberate two-word command
actual_sha256="$($tool "$tmp_dir/$asset" | awk '{print $1}')"
if [ "$actual_sha256" != "$expected_sha256" ]; then
  echo "checksum mismatch for $asset: expected $expected_sha256, got $actual_sha256" >&2
  exit 1
fi

tar -xzf "$tmp_dir/$asset" -C "$tmp_dir"

extracted="$(find "$tmp_dir" -type f -name rcodesign | head -1)"
if [ -z "$extracted" ]; then
  echo "rcodesign binary not found in $asset" >&2
  exit 1
fi

mkdir -p "$dest_dir"
cp "$extracted" "$dest_dir/rcodesign"
chmod +x "$dest_dir/rcodesign"
echo "installed rcodesign ${RCODESIGN_VERSION} to $dest_dir/rcodesign"
