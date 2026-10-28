#!/usr/bin/env bash
# Unless explicitly stated otherwise all files in this repository are licensed under the Apache-2.0 License.
# This product includes software developed at Datadog (https://www.datadoghq.com/).
# Copyright 2026-Present Datadog, Inc.

# Generates COPYING, the third-party license notices shipped in the release
# tarballs: a pinned cargo-about renders the crate licenses from Cargo.lock
# with about.toml and about.hbs, then the Rust runtime notices, fetched from
# the pinned toolchain's tag and checksum-verified, are appended. Needs cargo
# and network access.
#
# Writes one file per release target and a union for the universal binary:
#   COPYING.aarch64-apple-darwin, COPYING.x86_64-apple-darwin, COPYING (both)
#
# Usage: gen-copying.sh          rewrite the COPYING files
#        gen-copying.sh --check  fail if any COPYING file is stale
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

check=false
case "$#:${1:-}" in
  0:) ;;
  1:--check) check=true ;;
  *)
    echo "Usage: $0 [--check]" >&2
    exit 1
    ;;
esac

# Rust runtime notices for the toolchain in rust-toolchain.toml. A toolchain
# bump fails here until the version and checksums are updated.
RUST_VERSION="1.95.0"
RUST_FILES=(
  "COPYRIGHT COPYRIGHT 172020dbfd5b53a226dfde77616190a48dcff519b0bc0e6deb91a8450782c4af"
  "LICENSE-MIT LICENSE-MIT b71bd43a069ca0641a9ecfe585ca7b3c53b5cc1608f8b68321168698e28b5ea1"
  "LICENSE-APACHE LICENSE-APACHE 62c7a1e35f56406896d7aa7ca52d0cc0d272ac022b5d2796e7d6905db8a3636a"
  "library/compiler-builtins/LICENSE.txt compiler-builtins-LICENSE.txt ab6eec6caf0fa5775e411c7a8bc6a45c4ef2956b0980b157ab74fc5cd62a928b"
)

if ! grep -qx "channel = \"$RUST_VERSION\"" "$repo_root/rust-toolchain.toml"; then
  echo "rust-toolchain.toml doesn't pin Rust $RUST_VERSION: update RUST_VERSION and RUST_FILES" >&2
  exit 1
fi

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

"$repo_root/scripts/install-cargo-about.sh" "$tmp_dir/bin" >&2

runtime_dir="$tmp_dir/rust"
mkdir -p "$runtime_dir"
tool="$(sha256_tool)"
for entry in "${RUST_FILES[@]}"; do
  read -r src dest expected <<<"$entry"
  curl --proto '=https' --tlsv1.2 -sSfL -o "$runtime_dir/$dest" \
    "https://raw.githubusercontent.com/rust-lang/rust/$RUST_VERSION/$src"
  # shellcheck disable=SC2086 # $tool is a deliberate two-word command
  actual="$($tool "$runtime_dir/$dest" | awk '{print $1}')"
  if [ "$actual" != "$expected" ]; then
    echo "checksum mismatch for rust $src: expected $expected, got $actual" >&2
    exit 1
  fi
done

# Renders COPYING for the given targets into <output>.
render() {
  local out="$1"
  shift
  local target_args=() t
  for t in "$@"; do
    target_args+=(--target "$t")
  done

  "$tmp_dir/bin/cargo-about" generate \
    --locked \
    --manifest-path "$repo_root/Cargo.toml" \
    --config "$repo_root/about.toml" \
    "${target_args[@]}" \
    --fail \
    -o "$out" \
    "$repo_root/about.hbs" 2>"$tmp_dir/log" || {
    cat "$tmp_dir/log" >&2
    exit 1
  }

  # --fail misses failed clarifications, which cargo-about only logs.
  if grep -E 'WARN|ERROR' "$tmp_dir/log" >&2; then
    echo "cargo-about reported problems, see above" >&2
    exit 1
  fi

  local targets="$*"
  sed "s/@TARGETS@/${targets// /, }/" "$out" >"$out.tmp"
  mv "$out.tmp" "$out"

  {
    echo
    echo "--------------------------------------------------------------------------------"
    echo "Rust $RUST_VERSION runtime"
    echo
    echo "The binaries statically link parts of the Rust $RUST_VERSION runtime: the"
    echo "standard library (std, core, alloc, panic_unwind, unwind), licensed"
    echo "MIT OR Apache-2.0, and compiler_builtins, licensed MIT AND Apache-2.0 WITH"
    echo "LLVM-exception. The notices below are the rust-lang/rust repository's"
    echo "COPYRIGHT and license files at that tag, which cover the standard library,"
    echo "and compiler-builtins' own LICENSE.txt with the LLVM exception text. The"
    echo "Rust release's COPYRIGHT-library.html has the per-file detail."
    echo "Source: https://github.com/rust-lang/rust/tree/$RUST_VERSION"
    for entry in "${RUST_FILES[@]}"; do
      read -r src dest _ <<<"$entry"
      echo
      echo "---- $src ----"
      echo
      cat "$runtime_dir/$dest"
    done
  } >>"$out"
}

mkdir -p "$tmp_dir/out"
render "$tmp_dir/out/COPYING.aarch64-apple-darwin" aarch64-apple-darwin
render "$tmp_dir/out/COPYING.x86_64-apple-darwin" x86_64-apple-darwin
render "$tmp_dir/out/COPYING" aarch64-apple-darwin x86_64-apple-darwin

stale=false
for f in COPYING COPYING.aarch64-apple-darwin COPYING.x86_64-apple-darwin; do
  if [ "$check" = true ]; then
    if ! diff -u "$repo_root/$f" "$tmp_dir/out/$f"; then
      echo "$f is stale: run scripts/gen-copying.sh and commit the result" >&2
      stale=true
    fi
  else
    cp "$tmp_dir/out/$f" "$repo_root/$f"
    echo "wrote $repo_root/$f"
  fi
done
if [ "$stale" = true ]; then
  exit 1
fi
if [ "$check" = true ]; then
  echo "COPYING files are up to date"
fi
