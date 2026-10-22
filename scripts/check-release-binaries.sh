#!/usr/bin/env bash
# Checks that <target-dir> holds exactly the three expected release
# binaries, as regular files with the expected architectures. Run by the
# signing jobs before signing: rejects unexpected files, non-regular files
# or unexpected architectures. It does not prove provenance. macOS only
# (uses lipo).
set -euo pipefail

if [ "$#" -ne 1 ]; then
  echo "usage: $0 <target-dir>" >&2
  exit 2
fi
dir="${1%/}"

expected="aarch64-apple-darwin/release/macos-code-signer
universal-apple-darwin/release/macos-code-signer
x86_64-apple-darwin/release/macos-code-signer"

actual="$(cd "$dir" && find . ! -type d | sed 's|^\./||' | LC_ALL=C sort)"
if [ "$actual" != "$expected" ]; then
  echo "error: unexpected files in $dir:" >&2
  echo "$actual" >&2
  echo "expected:" >&2
  echo "$expected" >&2
  exit 1
fi

check_archs() {
  local path="$dir/$1" want="$2" got
  if [ -L "$path" ] || [ ! -f "$path" ]; then
    echo "error: $path is not a regular file" >&2
    exit 1
  fi
  got="$(lipo -archs "$path" | tr ' ' '\n' | LC_ALL=C sort | xargs)"
  if [ "$got" != "$want" ]; then
    echo "error: $path has architectures '$got', expected '$want'" >&2
    exit 1
  fi
  echo "ok: $path ($got)"
}

check_archs aarch64-apple-darwin/release/macos-code-signer "arm64"
check_archs x86_64-apple-darwin/release/macos-code-signer "x86_64"
check_archs universal-apple-darwin/release/macos-code-signer "arm64 x86_64"
