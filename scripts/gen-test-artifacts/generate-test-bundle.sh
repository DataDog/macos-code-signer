#!/usr/bin/env bash
# Builds a minimal, unsigned `.app` bundle for manually testing
# macos-code-signer against a deployed macos-signing-service. Same artifact
# as the E2E tests use. A bundle's identifier always comes from
# Info.plist's CFBundleIdentifier: `sign --identifier` is ignored. Requires
# the Xcode command line tools (`cc`).
set -euo pipefail

usage() {
  cat <<EOF
Usage: $0 --output <path.app> [--identifier <id>]

Options:
  --output <path>     Where to write the unsigned .app bundle (required).
  --identifier <id>   Info.plist CFBundleIdentifier (default: com.example.test).

Sign the result with:
  macos-code-signer --service-url <url> --cert-id <application-cert-id> \\
    sign <path.app> <path>-signed.app
EOF
}

output=""
identifier="com.example.test"
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

macos_dir="$output/Contents/MacOS"
mkdir -p "$macos_dir"

tmpdir="$(mktemp -d)"
trap 'rm -rf "$tmpdir"' EXIT
printf 'int main(void) { return 0; }\n' > "$tmpdir/hello.c"
cc "$tmpdir/hello.c" -o "$macos_dir/TestApp"

cat > "$output/Contents/Info.plist" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
	<key>CFBundleExecutable</key>
	<string>TestApp</string>
	<key>CFBundleIdentifier</key>
	<string>$identifier</string>
	<key>CFBundlePackageType</key>
	<string>APPL</string>
</dict>
</plist>
EOF

echo "wrote unsigned .app bundle to $output"
