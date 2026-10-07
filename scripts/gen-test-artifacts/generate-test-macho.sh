#!/usr/bin/env bash
# Compiles a trivial, unsigned Mach-O binary for manually testing
# macos-code-signer against a deployed macos-signing-service. Same artifact
# as the E2E tests use. Requires the Xcode command line tools (`cc`).
set -euo pipefail

usage() {
  cat <<EOF
Usage: $0 --output <path>

Options:
  --output <path>   Where to write the unsigned Mach-O binary (required).

Sign the result with:
  macos-code-signer --service-url <url> --cert-id <application-cert-id> \\
    sign <path> <path>-signed
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
printf 'int main(void) { return 0; }\n' > "$tmpdir/hello.c"
cc "$tmpdir/hello.c" -o "$output"

echo "wrote unsigned Mach-O binary to $output"
