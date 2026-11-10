#!/usr/bin/env bash
# Unless explicitly stated otherwise all files in this repository are licensed under the Apache-2.0 License.
# This product includes software developed at Datadog (https://www.datadoghq.com/).
# Copyright 2026-Present Datadog, Inc.

# Signs the three release binaries in <in-dir> with a staging test
# certificate held by macos-signing-service, writing them under <out-dir>
# with the same layout. Uses the CLI's default RFC 3161 timestamp server.
#
# Environment (all required):
#   MACOS_CODE_SIGNER            path to the bootstrap macos-code-signer binary
#   MACOS_SIGNING_SERVICE_URL    HTTPS base URL of the service
#   STAGING_CERT_ID              certificate ID to sign with
#   MACOS_SIGNING_SERVICE_TOKEN  OIDC token for the service
#
# Before signing, a preflight GET of the certificate checks the URL, the
# certificate ID and the token. It sends the token in a header read from
# stdin, never as an argument, follows no redirects and only reports the
# HTTP status.
set +x
set -euo pipefail

if [ "$#" -ne 2 ]; then
  echo "usage: $0 <in-dir> <out-dir>" >&2
  exit 2
fi
in_dir="${1%/}"
out_dir="${2%/}"

for var in MACOS_CODE_SIGNER MACOS_SIGNING_SERVICE_URL STAGING_CERT_ID MACOS_SIGNING_SERVICE_TOKEN; do
  if [ -z "${!var:-}" ]; then
    echo "error: $var must be set" >&2
    exit 1
  fi
done

# HTTPS only, no credentials, query or fragment, optional plain path prefix.
url_re='^https://[A-Za-z0-9.-]+(:[0-9]+)?(/[A-Za-z0-9._~-]+)*/*$'
if ! [[ "$MACOS_SIGNING_SERVICE_URL" =~ $url_re ]]; then
  echo "error: MACOS_SIGNING_SERVICE_URL must be a plain https:// URL without credentials, query or fragment" >&2
  exit 1
fi
if [[ "/$MACOS_SIGNING_SERVICE_URL/" =~ /\.\.?/ ]]; then
  echo "error: MACOS_SIGNING_SERVICE_URL must not contain . or .. path segments" >&2
  exit 1
fi
base_url="$MACOS_SIGNING_SERVICE_URL"
while [ "${base_url%/}" != "$base_url" ]; do base_url="${base_url%/}"; done

# Unreserved characters only, so the ID is the same single path segment
# here and in the CLI. A leading alphanumeric rules out . and ..
if ! [[ "$STAGING_CERT_ID" =~ ^[A-Za-z0-9][A-Za-z0-9._-]*$ ]]; then
  echo "error: STAGING_CERT_ID must only contain letters, digits, '.', '_' and '-', starting with a letter or digit" >&2
  exit 1
fi

"$(dirname "$0")/check-release-binaries.sh" "$in_dir"

curl_status=0
http_code="$(printf 'Authorization: Bearer %s\n' "$MACOS_SIGNING_SERVICE_TOKEN" |
  curl --proto '=https' --tlsv1.2 --max-redirs 0 --connect-timeout 10 --max-time 30 \
    -s -o /dev/null -w '%{http_code}' -H @- \
    "${base_url}/signer/macos/certificates/${STAGING_CERT_ID}" 2>/dev/null)" || curl_status=$?
if [ "$curl_status" -ne 0 ]; then
  echo "error: preflight request to the signing service failed (curl exit $curl_status)" >&2
  exit 1
fi
if [ "$http_code" != "200" ]; then
  echo "error: preflight request to the signing service returned HTTP $http_code" >&2
  exit 1
fi
echo "preflight: certificate is reachable with this token"

export MACOS_SIGNING_SERVICE_URL="$base_url"
for triple in aarch64 x86_64 universal; do
  rel="${triple}-apple-darwin/release/macos-code-signer"
  mkdir -p "$(dirname "$out_dir/$rel")"
  echo "signing $rel"
  "$MACOS_CODE_SIGNER" --cert-id "$STAGING_CERT_ID" sign \
    --identifier com.datadoghq.macos-code-signer \
    --code-signature-flags runtime \
    "$in_dir/$rel" "$out_dir/$rel"
done
