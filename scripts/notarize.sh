#!/usr/bin/env bash
# Unless explicitly stated otherwise all files in this repository are licensed under the Apache-2.0 License.
# This product includes software developed at Datadog (https://www.datadoghq.com/).
# Copyright 2026-Present Datadog, Inc.

# Zips signed release binaries and submits them to Apple's notary
# service with rcodesign, waiting for the result. Bare Mach-O binaries
# can't be stapled, so the ticket is only checked online.
#
# Usage: notarize.sh <binary>...
#
# Environment (required):
#   APPLE_APP_STORE_CONNECT_API_DATA  App Store Connect API key (JSON)
#
# The key is written under a temp directory, removed on exit.
set -euo pipefail

if [ "$#" -lt 1 ]; then
  echo "Usage: $0 <binary>..." >&2
  exit 1
fi

: "${APPLE_APP_STORE_CONNECT_API_DATA:?APPLE_APP_STORE_CONNECT_API_DATA is not set}"

# Fail early on a joined or unexpanded path list.
for bin in "$@"; do
  if [ ! -f "$bin" ]; then
    echo "not a file: '$bin' (pass one binary per argument)" >&2
    exit 1
  fi
done

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

tmp_dir="$(mktemp -d)"
trap 'rm -rf "$tmp_dir"' EXIT

"$script_dir/install-rcodesign.sh" "$tmp_dir/bin"
rcodesign="$tmp_dir/bin/rcodesign"

api_key_file="$tmp_dir/connect-api-key.json"
printf '%s' "$APPLE_APP_STORE_CONNECT_API_DATA" >"$api_key_file"
chmod 600 "$api_key_file"

stage_dir="$tmp_dir/notarize"
mkdir -p "$stage_dir"

for bin in "$@"; do
  triple="$(basename "$(dirname "$(dirname "$bin")")")"
  cp "$bin" "$stage_dir/macos-code-signer-$triple"
done

zip_file="$tmp_dir/notarize.zip"
(cd "$stage_dir" && zip -q "$zip_file" ./*)

echo "submitting $zip_file for notarization"
"$rcodesign" notary-submit --api-key-file "$api_key_file" --wait "$zip_file"
