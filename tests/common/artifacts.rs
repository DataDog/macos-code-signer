// Unless explicitly stated otherwise all files in this repository are licensed under the Apache-2.0 License.
// This product includes software developed at Datadog (https://www.datadoghq.com/).
// Copyright 2026-Present Datadog, Inc.

//! Builds test artifacts with `scripts/gen-test-artifacts/`, so the scripts
//! used for manual testing are exercised too.

use std::path::{Path, PathBuf};
use std::process::Command;

/// `CFBundleIdentifier` of the test bundle.
pub const BUNDLE_IDENTIFIER: &str = "com.example.e2etest";

const SCRIPTS_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/scripts/gen-test-artifacts");

/// Runs `generate-test-<kind>.sh --output <output> <args>` and returns
/// `output`, panicking with the script's output on failure.
fn generate(kind: &str, output: PathBuf, args: &[&str]) -> PathBuf {
    let script = Path::new(SCRIPTS_DIR).join(format!("generate-test-{kind}.sh"));
    let result = Command::new(&script)
        .arg("--output")
        .arg(&output)
        .args(args)
        .output()
        .unwrap_or_else(|err| panic!("failed to run {}: {err}", script.display()));
    assert!(
        result.status.success(),
        "{} failed:\n{}{}",
        script.display(),
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    output
}

/// Mach-O executable at `dir/hello`, as the toolchain builds it: ad-hoc
/// signed by the linker on arm64.
pub fn macho(dir: &Path) -> PathBuf {
    generate("macho", dir.join("hello"), &[])
}

/// Mach-O executable at `dir/hello`, with no signature at all.
pub fn unsigned_macho(dir: &Path) -> PathBuf {
    generate("macho", dir.join("hello"), &["--unsigned"])
}

/// `.app` bundle at `dir/TestApp.app`, whose executable is ad-hoc signed by
/// the linker on arm64.
pub fn app_bundle(dir: &Path) -> PathBuf {
    generate(
        "bundle",
        dir.join("TestApp.app"),
        &["--identifier", BUNDLE_IDENTIFIER],
    )
}

/// `.app` bundle at `dir/TestApp.app`, with no signature at all.
pub fn unsigned_app_bundle(dir: &Path) -> PathBuf {
    generate(
        "bundle",
        dir.join("TestApp.app"),
        &["--identifier", BUNDLE_IDENTIFIER, "--unsigned"],
    )
}

/// Unsigned compressed disk image at `dir/test.dmg`.
pub fn dmg(dir: &Path) -> PathBuf {
    generate("dmg", dir.join("test.dmg"), &[])
}

/// Unsigned flat installer package at `dir/test.pkg`.
pub fn pkg(dir: &Path) -> PathBuf {
    generate(
        "pkg",
        dir.join("test.pkg"),
        &["--identifier", BUNDLE_IDENTIFIER],
    )
}
