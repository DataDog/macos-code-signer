#!/usr/bin/env bash
# Builds a minimal `.app` bundle for manually testing
# macos-code-signer against a deployed macos-signing-service. Same artifact
# as the E2E tests use. Like any freshly built binary, its executable is
# ad-hoc signed by the linker on arm64, unless --unsigned is passed. A
# bundle's identifier always comes from Info.plist's CFBundleIdentifier:
# `sign --identifier` is ignored. Requires the Xcode command line tools
# (`cc`).
set -euo pipefail

usage() {
  cat <<EOF
Usage: $0 --output <path.app> [--identifier <id>] [--unsigned]

Options:
  --output <path>     Where to write the .app bundle (required).
  --identifier <id>   Info.plist CFBundleIdentifier (default: com.example.test).
  --unsigned          Skip the linker's ad-hoc signature on the executable.

Sign the result with:
  macos-code-signer --service-url <url> --cert-id <application-cert-id> \\
    sign <path.app> <path>-signed.app
EOF
}

output=""
identifier="com.example.test"
unsigned=false
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

macos_dir="$output/Contents/MacOS"
mkdir -p "$macos_dir"

tmpdir="$(mktemp -d)"
trap 'rm -rf "$tmpdir"' EXIT
printf 'int main(void) { return 0; }\n' > "$tmpdir/hello.c"
if [[ "$unsigned" == true ]]; then
  cc "$tmpdir/hello.c" -o "$macos_dir/TestApp" -Wl,-no_adhoc_codesign
else
  cc "$tmpdir/hello.c" -o "$macos_dir/TestApp"
fi

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

echo "wrote .app bundle to $output"
