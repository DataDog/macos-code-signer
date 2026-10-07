//! Builds unsigned test artifacts with `scripts/gen-test-artifacts/`, so the
//! scripts used for manual testing are exercised too.

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

/// Unsigned Mach-O executable at `dir/hello`.
pub fn macho(dir: &Path) -> PathBuf {
    generate("macho", dir.join("hello"), &[])
}

/// Unsigned `.app` bundle at `dir/TestApp.app`.
pub fn app_bundle(dir: &Path) -> PathBuf {
    generate(
        "bundle",
        dir.join("TestApp.app"),
        &["--identifier", BUNDLE_IDENTIFIER],
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
