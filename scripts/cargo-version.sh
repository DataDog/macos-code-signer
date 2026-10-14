#!/usr/bin/env bash
# Prints Cargo.toml's package version. Shared by scripts/check-release-tag.sh
# and both release workflows, so the version is only parsed in one place.
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
version="$(awk -F'"' '/^version[[:space:]]*=/{print $2; exit}' "$repo_root/Cargo.toml")"

if [ -z "$version" ]; then
  echo "could not read version from Cargo.toml" >&2
  exit 1
fi

echo "$version"
