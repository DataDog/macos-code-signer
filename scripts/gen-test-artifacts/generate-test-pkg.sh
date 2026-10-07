#!/usr/bin/env bash
# Builds a trivial, unsigned `.pkg` installer for manually testing
# macos-code-signer against a deployed macos-signing-service. Same artifact
# as the E2E tests use. A `.pkg` needs an "installer" certificate.
# Requires `pkgbuild`.
set -euo pipefail

usage() {
  cat <<EOF
Usage: $0 --output <path.pkg> [--identifier <id>] [--version <version>]

Options:
  --output <path>        Where to write the unsigned .pkg (required).
  --identifier <id>      Package identifier (default: com.example.test).
  --version <version>    Package version (default: 1.0).

Sign the result with:
  macos-code-signer --service-url <url> --cert-id <installer-cert-id> \\
    sign <path.pkg> <path>-signed.pkg
EOF
}

output=""
identifier="com.example.test"
version="1.0"
while [[ $# -gt 0 ]]; do
  case "$1" in
    --output)
      output="$2"
      shift 2
      ;;
    --identifier)
      identifier="$2"
      shift 2
      ;;
    --version)
      version="$2"
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
root="$tmpdir/root"
mkdir -p "$root/usr/local/bin"
printf '#!/bin/sh\necho hi\n' > "$root/usr/local/bin/hello"
chmod 755 "$root/usr/local/bin/hello"

pkgbuild --root "$root" --identifier "$identifier" --version "$version" "$output"

echo "wrote unsigned .pkg to $output"
