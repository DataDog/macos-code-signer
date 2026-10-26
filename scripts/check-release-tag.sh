#!/usr/bin/env bash
# Unless explicitly stated otherwise all files in this repository are licensed under the Apache-2.0 License.
# This product includes software developed at Datadog (https://www.datadoghq.com/).
# Copyright 2026-Present Datadog, Inc.

# Fails unless a release tag matches Cargo.toml's version. Used by both
# GitHub Actions and GitLab CI before a release build runs.
#
# Usage: check-release-tag.sh <tag>
#
# <tag> is a full tag name, e.g. "v0.1.0".
set -euo pipefail

if [ "$#" -ne 1 ]; then
  echo "Usage: $0 <tag>" >&2
  exit 1
fi

tag="$1"
script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

cargo_version="$("$script_dir/cargo-version.sh")"
expected_tag="v${cargo_version}"
if [ "$tag" != "$expected_tag" ]; then
  echo "tag ${tag} does not match Cargo.toml version: expected ${expected_tag}" >&2
  exit 1
fi

echo "tag ${tag} matches Cargo.toml version ${cargo_version}"
