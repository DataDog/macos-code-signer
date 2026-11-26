#!/usr/bin/env bash
# Unless explicitly stated otherwise all files in this repository are licensed under the Apache-2.0 License.
# This product includes software developed at Datadog (https://www.datadoghq.com/).
# Copyright 2026-Present Datadog, Inc.

# Signs release binaries in place with rcodesign and a Developer ID
# Application .p12 certificate. Only used by GitHub Actions, whose
# certificate is not in the signing service yet.
#
# Usage: sign-legacy.sh <binary>...
#
# Environment (required):
#   APPLE_DEVELOPER_ID_APPLICATION_P12           the .p12, base64
#   APPLE_DEVELOPER_ID_APPLICATION_P12_PASSWORD  its password
#
# Secret material is written under one temp directory, removed on exit.
set -euo pipefail

BINARY_IDENTIFIER="com.datadoghq.macos-code-signer"

if [ "$#" -lt 1 ]; then
  echo "Usage: $0 <binary>..." >&2
  exit 1
fi

: "${APPLE_DEVELOPER_ID_APPLICATION_P12:?APPLE_DEVELOPER_ID_APPLICATION_P12 is not set}"
: "${APPLE_DEVELOPER_ID_APPLICATION_P12_PASSWORD:?APPLE_DEVELOPER_ID_APPLICATION_P12_PASSWORD is not set}"

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

p12_file="$tmp_dir/cert.p12"
password_file="$tmp_dir/cert.p12.password"

printf '%s' "$APPLE_DEVELOPER_ID_APPLICATION_P12" | base64 -d >"$p12_file"
printf '%s' "$APPLE_DEVELOPER_ID_APPLICATION_P12_PASSWORD" >"$password_file"
chmod 600 "$p12_file" "$password_file"

for bin in "$@"; do
  echo "signing $bin"
  "$rcodesign" sign \
    --p12-file "$p12_file" \
    --p12-password-file "$password_file" \
    --code-signature-flags runtime \
    --binary-identifier "$BINARY_IDENTIFIER" \
    "$bin"
done
