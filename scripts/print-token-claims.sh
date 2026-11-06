#!/usr/bin/env bash
# Reads one JWT on stdin and prints its decoded payload (claims) only.
# Fails unless the payload is a JSON object whose top-level aud is, or
# contains, the expected audience and whose top-level sub is a non-empty
# string. Parse-only: the signature is not verified. Requires jq.
# Usage: get-token ... | print-token-claims.sh <expected-audience>
set -euo pipefail

expected_aud="${1:?usage: $0 <expected-audience>}"

input="$(cat)"
if ! [[ "$input" =~ ^[A-Za-z0-9_-]+\.([A-Za-z0-9_-]+)\.[A-Za-z0-9_-]+$ ]]; then
  echo "error: input is not a single JWT" >&2
  exit 1
fi
payload="${BASH_REMATCH[1]}"
unset input

# base64url to base64, with padding.
payload="$(printf '%s' "$payload" | tr '_-' '/+')"
while [ $(( ${#payload} % 4 )) -ne 0 ]; do payload="${payload}="; done

if ! claims="$(printf '%s' "$payload" | base64 -d 2>/dev/null | jq -es 'if length == 1 and (.[0] | type) == "object" then .[0] else error end' 2>/dev/null)"; then
  echo "error: JWT payload is not a base64-encoded JSON object" >&2
  exit 1
fi

echo "CI Identity token claims (decoded, signature not verified):"
printf '%s\n' "$claims"

if ! jq -e --arg aud "$expected_aud" \
  '.aud | if type == "string" then . == $aud elif type == "array" then any(.[]; . == $aud) else false end' \
  <<<"$claims" >/dev/null; then
  echo "error: top-level aud is not ${expected_aud} and does not contain it" >&2
  exit 1
fi
if ! jq -e '.sub | type == "string" and length > 0' <<<"$claims" >/dev/null; then
  echo "error: top-level sub is missing, empty or not a string" >&2
  exit 1
fi
