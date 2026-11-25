#!/usr/bin/env bash
# Unless explicitly stated otherwise all files in this repository are licensed under the Apache-2.0 License.
# This product includes software developed at Datadog (https://www.datadoghq.com/).
# Copyright 2026-Present Datadog, Inc.

# Checks that the three release binaries in <dir> are signed for the
# given signing service environment. Every architecture of every binary
# is checked: signature, identifier, hardened runtime, leaf common name
# (when set), then the environment's identity checks, then, for all of
# them, a secure timestamp. The leaf key is not pinned, so key rotations
# don't break it. Does not check notarization. macOS only.
#
# Identity checks per environment:
#   staging     a Team ID other than Datadog's. No Gatekeeper or chain
#               trust check: the staging certificate is self-signed.
#   production  Datadog's Team ID, and a Developer ID certificate issued
#               by Apple for that Team ID.
#
# Environment:
#   MACOS_SIGNING_ENVIRONMENT     staging or production (required)
#   MACOS_SIGNING_CERT_AUTHORITY  expected leaf common name (required for
#                                 staging, checked when set for production)
set -euo pipefail

DATADOG_TEAM_ID="JKFCB4CN7C"
# Apple's Developer ID requirement: Apple anchor, Developer ID
# intermediate and leaf, and the leaf's Team ID.
DEVELOPER_ID_REQUIREMENT="anchor apple generic and certificate 1[field.1.2.840.113635.100.6.2.6] exists and certificate leaf[field.1.2.840.113635.100.6.1.13] exists and certificate leaf[subject.OU] = \"${DATADOG_TEAM_ID}\""

if [ "$#" -ne 1 ]; then
  echo "usage: $0 <dir>" >&2
  exit 2
fi
dir="${1%/}"

authority="${MACOS_SIGNING_CERT_AUTHORITY:-}"
case "${MACOS_SIGNING_ENVIRONMENT:-}" in
  staging)
    if [ -z "$authority" ]; then
      echo "error: MACOS_SIGNING_CERT_AUTHORITY must be set for staging" >&2
      exit 1
    fi
    ;;
  production) ;;
  *)
    echo "error: MACOS_SIGNING_ENVIRONMENT must be staging or production" >&2
    exit 1
    ;;
esac

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
  if [ -n "$authority" ]; then
    [ "$(grep -m1 '^Authority=' <<<"$info")" = "Authority=${authority}" ] ||
      fail "$what: leaf authority is not ${authority}"
  fi

  if [ "$MACOS_SIGNING_ENVIRONMENT" = staging ]; then
    if grep -qx "TeamIdentifier=${DATADOG_TEAM_ID}" <<<"$info"; then
      fail "$what: signed with Datadog's Team ID, not a staging certificate"
    fi
  else
    grep -qx "TeamIdentifier=${DATADOG_TEAM_ID}" <<<"$info" ||
      fail "$what: not signed with Datadog's Team ID"
    codesign --verify --arch "$arch" --test-requirement="=${DEVELOPER_ID_REQUIREMENT}" "$bin" ||
      fail "$what: not signed with an Apple-issued Developer ID certificate for Datadog"
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
