#!/usr/bin/env bash
# Unless explicitly stated otherwise all files in this repository are licensed under the Apache-2.0 License.
# This product includes software developed at Datadog (https://www.datadoghq.com/).
# Copyright 2026-Present Datadog, Inc.

# Builds an unsigned `.dmg` disk image for manually testing
# macos-code-signer against a deployed macos-signing-service. Same artifact
# as the E2E tests use. Creates a blank read-write image on disk (no
# mount needed) and converts it to UDZO: apple-codesign only recognizes a
# `.dmg` with the UDIF "koly" trailer, which only `hdiutil convert` writes.
# Requires `hdiutil`.
set -euo pipefail

usage() {
  cat <<EOF
Usage: $0 --output <path.dmg>

Options:
  --output <path>   Where to write the unsigned .dmg (required).

Sign the result with:
  macos-code-signer --service-url <url> --cert-id <application-cert-id> \\
    sign <path.dmg> <path>-signed.dmg
EOF
}

output=""
while [[ $# -gt 0 ]]; do
  case "$1" in
    --output)
      output="$2"
      shift 2
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      echo "unknown argument: $1" >&2
      usage >&2
      exit 1
      ;;
  esac
done

if [[ -z "$output" ]]; then
  usage >&2
  exit 1
fi

tmpdir="$(mktemp -d)"
trap 'rm -rf "$tmpdir"' EXIT
raw="$tmpdir/unsigned-raw.dmg"
hdiutil create -size 1m -fs HFS+ -volname TestDmg -ov "$raw"
hdiutil convert "$raw" -format UDZO -o "$output"

echo "wrote unsigned .dmg to $output"
