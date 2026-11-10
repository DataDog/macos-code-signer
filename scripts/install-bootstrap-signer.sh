#!/usr/bin/env bash
# Unless explicitly stated otherwise all files in this repository are licensed under the Apache-2.0 License.
# This product includes software developed at Datadog (https://www.datadoghq.com/).
# Copyright 2026-Present Datadog, Inc.

# Downloads a released universal macos-code-signer from binaries.ddbuild.io
# into <dest-dir>, checks it against a pinned SHA-256, then runs the same
# checks as action.yml: signature, Datadog's Team ID, hardened runtime and
# notarization. macOS only.
set -euo pipefail

if [ "$#" -ne 3 ]; then
  echo "usage: $0 <version> <sha256> <dest-dir>" >&2
  exit 2
fi
version="${1#v}"
sha256="$2"
dest_dir="$3"

if ! [[ "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
  echo "error: version must be X.Y.Z" >&2
  exit 1
fi

tmp_dir="$(mktemp -d)"
trap 'rm -rf "$tmp_dir"' EXIT
archive="$tmp_dir/macos-code-signer.tar.gz"
curl --proto '=https' --tlsv1.2 -sSfL -o "$archive" \
  "https://binaries.ddbuild.io/macos-code-signer/${version}/macos-code-signer_${version}_darwin_universal.tar.gz"
echo "${sha256}  ${archive}" | shasum -a 256 -c -

mkdir -p "$dest_dir"
tar -xzf "$archive" -C "$dest_dir" macos-code-signer
bin="$dest_dir/macos-code-signer"
chmod +x "$bin"

codesign --verify --strict --verbose=2 "$bin"
info="$(codesign -dvvv "$bin" 2>&1)"
if ! grep -qx "TeamIdentifier=JKFCB4CN7C" <<<"$info"; then
  echo "error: macos-code-signer is not signed by Datadog's Team ID" >&2
  exit 1
fi
if ! grep -q "flags=.*runtime" <<<"$info"; then
  echo "error: macos-code-signer is not signed with the hardened runtime" >&2
  exit 1
fi
# Online lookup: a bare binary can't be stapled.
codesign --verify --check-notarization --test-requirement="=notarized" -v "$bin"
