#!/usr/bin/env bash
# Signs or notarizes release binaries with a legacy Developer ID
# Application .p12 certificate, used for the first releases.
#
# Usage:
#   sign-legacy.sh sign <binary>...
#     Signs each binary in place. Reads the certificate from
#     APPLE_DEVELOPER_ID_APPLICATION_P12 (base64) and
#     APPLE_DEVELOPER_ID_APPLICATION_P12_PASSWORD.
#
#   sign-legacy.sh notarize <binary>...
#     Zips the binaries and submits them to Apple's notary service,
#     waiting for the result. Bare Mach-O binaries can't be stapled, so
#     the ticket is only checked online. Reads the App Store Connect API
#     key from APPLE_APP_STORE_CONNECT_API_DATA (JSON).
#
# Secret material is written under one temp directory, removed on exit.
set -euo pipefail

BINARY_IDENTIFIER="com.datadoghq.macos-code-signer"

usage() {
  echo "Usage: $0 sign <binary>..." >&2
  echo "       $0 notarize <binary>..." >&2
  exit 1
}

if [ "$#" -lt 2 ]; then
  usage
fi

mode="$1"
shift

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

sign() {
  : "${APPLE_DEVELOPER_ID_APPLICATION_P12:?APPLE_DEVELOPER_ID_APPLICATION_P12 is not set}"
  : "${APPLE_DEVELOPER_ID_APPLICATION_P12_PASSWORD:?APPLE_DEVELOPER_ID_APPLICATION_P12_PASSWORD is not set}"

  local p12_file="$tmp_dir/cert.p12"
  local password_file="$tmp_dir/cert.p12.password"

  printf '%s' "$APPLE_DEVELOPER_ID_APPLICATION_P12" | base64 -d >"$p12_file"
  printf '%s' "$APPLE_DEVELOPER_ID_APPLICATION_P12_PASSWORD" >"$password_file"
  chmod 600 "$p12_file" "$password_file"

  local bin
  for bin in "$@"; do
    echo "signing $bin"
    "$rcodesign" sign \
      --p12-file "$p12_file" \
      --p12-password-file "$password_file" \
      --code-signature-flags runtime \
      --binary-identifier "$BINARY_IDENTIFIER" \
      "$bin"
  done
}

notarize() {
  : "${APPLE_APP_STORE_CONNECT_API_DATA:?APPLE_APP_STORE_CONNECT_API_DATA is not set}"

  local api_key_file="$tmp_dir/connect-api-key.json"
  printf '%s' "$APPLE_APP_STORE_CONNECT_API_DATA" >"$api_key_file"
  chmod 600 "$api_key_file"

  local stage_dir="$tmp_dir/notarize"
  mkdir -p "$stage_dir"

  local bin triple
  for bin in "$@"; do
    triple="$(basename "$(dirname "$(dirname "$bin")")")"
    cp "$bin" "$stage_dir/macos-code-signer-$triple"
  done

  local zip_file="$tmp_dir/notarize.zip"
  (cd "$stage_dir" && zip -q "$zip_file" ./*)

  echo "submitting $zip_file for notarization"
  "$rcodesign" notary-submit --api-key-file "$api_key_file" --wait "$zip_file"
}

case "$mode" in
  sign) sign "$@" ;;
  notarize) notarize "$@" ;;
  *) usage ;;
esac
