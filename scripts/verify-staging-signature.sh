#!/usr/bin/env bash
# Unless explicitly stated otherwise all files in this repository are licensed under the Apache-2.0 License.
# This product includes software developed at Datadog (https://www.datadoghq.com/).
# Copyright 2026-Present Datadog, Inc.

# Checks that the three release binaries in <dir> are signed with the
# staging test certificate with the expected common name. Every
# architecture of every binary is checked: signature, identifier,
# hardened runtime, leaf common name, Team ID other than Datadog's, then,
# for all of them, a secure timestamp. The leaf key is not pinned, so key
# rotations don't break it. Does not check notarization, Gatekeeper or
# chain trust: a self-signed staging certificate passes none of them.
# macOS only.
#
# Environment (required):
#   STAGING_CERT_AUTHORITY  expected leaf common name
set -euo pipefail

if [ "$#" -ne 1 ]; then
  echo "usage: $0 <dir>" >&2
  exit 2
fi
dir="${1%/}"

if [ -z "${STAGING_CERT_AUTHORITY:-}" ]; then
  echo "error: STAGING_CERT_AUTHORITY must be set" >&2
  exit 1
fi

"$(dirname "$0")/check-release-binaries.sh" "$dir"

fail() {
  echo "error: $1" >&2
  exit 1
}

check_slice() {
  local bin="$1" arch="$2" what="$1 ($2)" info
  codesign --verify --strict --verbose=2 --arch "$arch" "$bin" ||
    fail "$what: invalid signature"
  info="$(codesign -dvvv --arch "$arch" "$bin" 2>&1)"

  grep -qx 'Identifier=com.datadoghq.macos-code-signer' <<<"$info" ||
    fail "$what: unexpected identifier"
  grep -Eq '^CodeDirectory .*flags=0x[0-9a-f]+\(([a-z-]+,)*runtime[,)]' <<<"$info" ||
    fail "$what: hardened runtime flag missing"
  [ "$(grep -m1 '^Authority=' <<<"$info")" = "Authority=${STAGING_CERT_AUTHORITY}" ] ||
    fail "$what: leaf authority is not ${STAGING_CERT_AUTHORITY}"

  if grep -qx 'TeamIdentifier=JKFCB4CN7C' <<<"$info"; then
    fail "$what: signed with Datadog's Team ID, not a staging certificate"
  fi
  echo "ok: $what"
}

# Last, so the checks above are reached even when signed without a timestamp.
check_timestamp() {
  codesign -dvvv --arch "$2" "$1" 2>&1 | grep -q '^Timestamp=' ||
    fail "$1 ($2): no secure timestamp"
}

check_slice "$dir/aarch64-apple-darwin/release/macos-code-signer" arm64
check_slice "$dir/x86_64-apple-darwin/release/macos-code-signer" x86_64
check_slice "$dir/universal-apple-darwin/release/macos-code-signer" arm64
check_slice "$dir/universal-apple-darwin/release/macos-code-signer" x86_64

check_timestamp "$dir/aarch64-apple-darwin/release/macos-code-signer" arm64
check_timestamp "$dir/x86_64-apple-darwin/release/macos-code-signer" x86_64
check_timestamp "$dir/universal-apple-darwin/release/macos-code-signer" arm64
check_timestamp "$dir/universal-apple-darwin/release/macos-code-signer" x86_64
echo "ok: all slices have a secure timestamp"
