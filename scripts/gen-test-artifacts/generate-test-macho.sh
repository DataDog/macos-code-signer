#!/usr/bin/env bash
# Unless explicitly stated otherwise all files in this repository are licensed under the Apache-2.0 License.
# This product includes software developed at Datadog (https://www.datadoghq.com/).
# Copyright 2026-Present Datadog, Inc.

# Compiles a trivial Mach-O binary for manually testing macos-code-signer
# against a deployed macos-signing-service. Same artifact as the E2E tests
# use. Like any freshly built binary, it is ad-hoc signed by the linker on
# arm64, unless --unsigned is passed. Requires the Xcode command line tools
# (`cc`).
set -euo pipefail

usage() {
  cat <<EOF
Usage: $0 --output <path> [--unsigned]

Options:
  --output <path>   Where to write the Mach-O binary (required).
  --unsigned        Skip the linker's ad-hoc signature.

Sign the result with:
  macos-code-signer --service-url <url> --cert-id <application-cert-id> \\
    sign <path> <path>-signed
EOF
}

output=""
unsigned=false
while [[ $# -gt 0 ]]; do
  case "$1" in
    --output)
      output="$2"
      shift 2
      ;;
    --unsigned)
      unsigned=true
      shift
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
printf 'int main(void) { return 0; }\n' > "$tmpdir/hello.c"
if [[ "$unsigned" == true ]]; then
  cc "$tmpdir/hello.c" -o "$output" -Wl,-no_adhoc_codesign
else
  cc "$tmpdir/hello.c" -o "$output"
fi

echo "wrote Mach-O binary to $output"
