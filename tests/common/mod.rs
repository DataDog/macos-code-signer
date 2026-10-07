//! Shared helpers for the end-to-end tests.

#[cfg(target_os = "macos")]
pub mod artifacts;
pub mod mock_service;

use std::process::{Command, Output};

/// `macos-code-signer --service-url <service_url> --cert-id <cert_id> sign
/// --timestamp-url none`, authenticated with `mock_service::TOKEN`. Callers
/// append the input, output and any extra options.
///
/// Loopback is excluded from any proxy configured in the environment, so the
/// CLI always reaches the mock directly.
pub fn sign_command(service_url: &str, cert_id: &str) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_macos-code-signer"));
    command
        .env_remove("MACOS_SIGNING_SERVICE_URL")
        .env("NO_PROXY", "127.0.0.1,localhost")
        .env("no_proxy", "127.0.0.1,localhost")
        .env("MACOS_SIGNING_SERVICE_TOKEN", mock_service::TOKEN)
        .args(["--service-url", service_url, "--cert-id", cert_id])
        .args(["sign", "--timestamp-url", "none"]);
    command
}

/// Runs `command` and returns its output, with stderr decoded.
pub fn run(command: &mut Command) -> (Output, String) {
    let output = command
        .output()
        .unwrap_or_else(|err| panic!("failed to run {command:?}: {err}"));
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    (output, stderr)
}

/// Runs `command` and panics with its stderr unless it succeeds.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub fn run_ok(command: &mut Command) {
    let (output, stderr) = run(command);
    assert!(output.status.success(), "{command:?} failed: {stderr}");
}

/// Runs `command` and returns its stderr, panicking unless it fails.
pub fn run_err(command: &mut Command) -> String {
    let (output, stderr) = run(command);
    assert!(
        !output.status.success(),
        "{command:?} unexpectedly succeeded"
    );
    stderr
}
