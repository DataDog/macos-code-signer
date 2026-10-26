// Unless explicitly stated otherwise all files in this repository are licensed under the Apache-2.0 License.
// This product includes software developed at Datadog (https://www.datadoghq.com/).
// Copyright 2026-Present Datadog, Inc.

//! End-to-end tests: run the `macos-code-signer` binary against an
//! in-process mock of macos-signing-service.
//!
//! Signing and verification need the macOS developer tools, so those tests
//! only build on macOS. Error cases run everywhere.

mod common;

use common::mock_service::{APPLICATION_CERT_ID, INSTALLER_CERT_ID, MockService};
use common::{run_err, sign_command};
use tempfile::TempDir;

#[test]
fn fails_without_a_token() {
    let mut command = sign_command("http://127.0.0.1:1", APPLICATION_CERT_ID);
    command
        .env_remove("MACOS_SIGNING_SERVICE_TOKEN")
        .arg("hello");

    let stderr = run_err(&mut command);

    assert!(
        stderr.contains("MACOS_SIGNING_SERVICE_TOKEN must be set"),
        "{stderr}"
    );
}

#[test]
fn rejects_a_non_https_service_url() {
    let mut command = sign_command("http://signing.example", APPLICATION_CERT_ID);
    command.arg("hello");

    let stderr = run_err(&mut command);

    assert!(stderr.contains("must use HTTPS"), "{stderr}");
}

#[tokio::test]
async fn rejects_an_application_cert_for_a_pkg() {
    let service = MockService::start().await;
    let dir = TempDir::new().unwrap();
    let mut command = sign_command(&service.url(), APPLICATION_CERT_ID);
    command.arg(dir.path().join("test.pkg"));

    let stderr = run_err(&mut command);

    assert!(
        stderr.contains(r#"requires an "installer" certificate"#),
        "{stderr}"
    );
}

#[tokio::test]
async fn rejects_an_installer_cert_for_a_macho() {
    let service = MockService::start().await;
    let dir = TempDir::new().unwrap();
    let mut command = sign_command(&service.url(), INSTALLER_CERT_ID);
    command.arg(dir.path().join("hello"));

    let stderr = run_err(&mut command);

    assert!(
        stderr.contains(r#"requires an "application" certificate"#),
        "{stderr}"
    );
}

#[tokio::test]
async fn reports_401_for_a_wrong_token() {
    let service = MockService::start().await;
    let mut command = sign_command(&service.url(), APPLICATION_CERT_ID);
    command
        .env("MACOS_SIGNING_SERVICE_TOKEN", "wrong-token")
        .arg("hello");

    let stderr = run_err(&mut command);

    assert!(stderr.contains("401"), "{stderr}");
}

#[tokio::test]
async fn reports_401_for_an_unknown_cert_id() {
    let service = MockService::start().await;
    let mut command = sign_command(&service.url(), "unknown-cert");
    command.arg("hello");

    let stderr = run_err(&mut command);

    assert!(stderr.contains("401"), "{stderr}");
}

#[tokio::test]
async fn rejects_an_unknown_code_signature_flag() {
    let service = MockService::start().await;
    let mut command = sign_command(&service.url(), APPLICATION_CERT_ID);
    command
        .args(["--code-signature-flags", "not-a-flag"])
        .arg("hello");

    let stderr = run_err(&mut command);

    assert!(
        stderr.contains("unknown code signature flag: not-a-flag"),
        "{stderr}"
    );
}

#[cfg(target_os = "macos")]
mod macos {
    use super::common::artifacts::{self, BUNDLE_IDENTIFIER};
    use super::common::mock_service::{CERT_PEM, KEY_PEM};
    use super::common::{run, run_ok};
    use super::*;
    use apple_codesign::{SettingsScope, SigningSettings, UnifiedSigner};
    use apple_xar::reader::XarReader;
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::process::Command;
    use x509_certificate::{CapturedX509Certificate, InMemorySigningKeyPair};

    /// Signing authority `codesign` reports for the E2E fixture certificate.
    const AUTHORITY: &str = "Authority=Developer ID Application: Example (E2ETEST123)\n";

    /// `codesign --verify --deep --strict`, panicking on failure. Also checks
    /// that the signature is made with the fixture certificate: `--verify`
    /// alone accepts an ad-hoc signature.
    fn codesign_verify(path: &Path) {
        run_ok(
            Command::new("codesign")
                .args(["--verify", "--deep", "--strict", "--verbose=2"])
                .arg(path),
        );
        let details = codesign_details(path);
        assert!(details.contains(AUTHORITY), "{details}");
    }

    /// `codesign -dvvv` output (it prints to stderr).
    fn codesign_details(path: &Path) -> String {
        let (output, stderr) = run(Command::new("codesign").arg("-dvvv").arg(path));
        assert!(output.status.success(), "codesign -dvvv failed: {stderr}");
        stderr
    }

    /// Checks a `.pkg` signature cryptographically. `pkgutil --check-signature`
    /// reports a self-signed certificate as untrusted whether or not the
    /// signature is valid, so it is only used to check that macOS sees one.
    fn pkg_verify(path: &Path) {
        let output = Command::new("pkgutil")
            .arg("--check-signature")
            .arg(path)
            .output()
            .unwrap();
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(
            stdout.contains("Status: signed by untrusted certificate"),
            "{stdout}"
        );

        let mut xar = XarReader::new(fs::File::open(path).unwrap()).unwrap();
        assert!(xar.verify_table_of_contents_checksum().unwrap());
        assert!(xar.verify_rsa_checksum_signature().unwrap());
        assert!(xar.verify_cms_signature().unwrap());

        let (_, certificates) = xar.rsa_signature().unwrap().unwrap();
        let expected = CapturedX509Certificate::from_pem(CERT_PEM.as_bytes()).unwrap();
        assert_eq!(
            certificates[0].constructed_data(),
            expected.constructed_data()
        );
    }

    /// Path for the signed copy of `input`, with the same file name.
    fn signed_path(dir: &Path, input: &Path) -> PathBuf {
        let signed_dir = dir.join("signed");
        fs::create_dir_all(&signed_dir).unwrap();
        signed_dir.join(input.file_name().unwrap())
    }

    /// Signs a fresh artifact to an output path and another one in place,
    /// then checks both with `verify`.
    async fn assert_signs(build: fn(&Path) -> PathBuf, cert_id: &str, verify: fn(&Path)) {
        let service = MockService::start().await;

        let dir = TempDir::new().unwrap();
        let input = build(dir.path());
        let output = signed_path(dir.path(), &input);
        run_ok(
            sign_command(&service.url(), cert_id)
                .arg(&input)
                .arg(&output),
        );
        verify(&output);

        let dir = TempDir::new().unwrap();
        let input = build(dir.path());
        run_ok(sign_command(&service.url(), cert_id).arg(&input));
        verify(&input);
    }

    // The default Mach-O and bundle inputs are ad-hoc signed on arm64, like
    // any freshly built binary, so these also check that the ad-hoc
    // signature is replaced.
    #[tokio::test]
    async fn signs_a_macho() {
        assert_signs(artifacts::macho, APPLICATION_CERT_ID, codesign_verify).await;
    }

    #[tokio::test]
    async fn signs_an_unsigned_macho() {
        assert_signs(
            artifacts::unsigned_macho,
            APPLICATION_CERT_ID,
            codesign_verify,
        )
        .await;
    }

    #[tokio::test]
    async fn signs_an_app_bundle() {
        assert_signs(artifacts::app_bundle, APPLICATION_CERT_ID, codesign_verify).await;
    }

    #[tokio::test]
    async fn signs_an_unsigned_app_bundle() {
        assert_signs(
            artifacts::unsigned_app_bundle,
            APPLICATION_CERT_ID,
            codesign_verify,
        )
        .await;
    }

    #[tokio::test]
    async fn signs_a_dmg() {
        assert_signs(artifacts::dmg, APPLICATION_CERT_ID, codesign_verify).await;
    }

    #[tokio::test]
    async fn signs_a_pkg() {
        assert_signs(artifacts::pkg, INSTALLER_CERT_ID, pkg_verify).await;
    }

    /// `codesign -dv` output for `path`, signed or not.
    fn codesign_describe(path: &Path) -> String {
        run(Command::new("codesign").arg("-dv").arg(path)).1
    }

    // The linker only ad-hoc signs on arm64.
    #[cfg(target_arch = "aarch64")]
    #[test]
    fn generates_ad_hoc_signed_inputs() {
        let dir = TempDir::new().unwrap();

        for input in [
            artifacts::macho(dir.path()),
            artifacts::app_bundle(dir.path()),
        ] {
            let stderr = codesign_describe(&input);
            assert!(stderr.contains("Signature=adhoc"), "{stderr}");
        }
    }

    #[test]
    fn generates_unsigned_inputs() {
        for build in [
            artifacts::unsigned_macho,
            artifacts::unsigned_app_bundle,
            artifacts::dmg,
        ] {
            let dir = TempDir::new().unwrap();
            let stderr = codesign_describe(&build(dir.path()));
            assert!(
                stderr.contains("code object is not signed at all"),
                "{stderr}"
            );
        }

        let dir = TempDir::new().unwrap();
        let pkg = artifacts::pkg(dir.path());
        let mut xar = XarReader::new(fs::File::open(pkg).unwrap()).unwrap();
        assert!(xar.rsa_signature().unwrap().is_none());
        assert!(xar.cms_signature().unwrap().is_none());
    }

    /// The CodeDirectory lines of `codesign -dvvv`, which identify what was
    /// signed and how, but not when.
    fn code_directory_lines(details: &str) -> Vec<&str> {
        details
            .lines()
            .filter(|line| {
                [
                    "Identifier=",
                    "CodeDirectory ",
                    "Hash type=",
                    "Hash choices=",
                    "CDHash=",
                ]
                .iter()
                .any(|prefix| line.starts_with(prefix))
            })
            .collect()
    }

    #[tokio::test]
    async fn matches_local_signing_with_the_same_key() {
        let service = MockService::start().await;
        let dir = TempDir::new().unwrap();
        let input = artifacts::macho(dir.path());
        let identifier = "com.example.parity";

        let remote = dir.path().join("hello-remote");
        run_ok(
            sign_command(&service.url(), APPLICATION_CERT_ID)
                .args(["--identifier", identifier])
                .arg(&input)
                .arg(&remote),
        );

        let local = dir.path().join("hello-local");
        let key = InMemorySigningKeyPair::from_pkcs8_pem(KEY_PEM).unwrap();
        let certificate = CapturedX509Certificate::from_pem(CERT_PEM.as_bytes()).unwrap();
        let mut settings = SigningSettings::default();
        settings.set_signing_key(&key, certificate);
        settings.set_binary_identifier(SettingsScope::Main, identifier);
        UnifiedSigner::new(settings)
            .sign_path(&input, &local)
            .unwrap();

        let remote_details = codesign_details(&remote);
        let local_details = codesign_details(&local);
        let remote_lines = code_directory_lines(&remote_details);
        assert_eq!(remote_lines.len(), 5, "{remote_details}");
        assert_eq!(remote_lines, code_directory_lines(&local_details));
    }

    #[tokio::test]
    async fn sets_the_identifier_of_a_macho() {
        let service = MockService::start().await;
        let dir = TempDir::new().unwrap();
        let input = artifacts::macho(dir.path());

        run_ok(
            sign_command(&service.url(), APPLICATION_CERT_ID)
                .args(["--identifier", "com.example.custom"])
                .arg(&input),
        );

        let details = codesign_details(&input);
        assert!(
            details.contains("Identifier=com.example.custom\n"),
            "{details}"
        );
    }

    #[tokio::test]
    async fn sets_the_identifier_of_a_dmg() {
        let service = MockService::start().await;
        let dir = TempDir::new().unwrap();
        let input = artifacts::dmg(dir.path());

        run_ok(
            sign_command(&service.url(), APPLICATION_CERT_ID)
                .args(["--identifier", "com.example.custom"])
                .arg(&input),
        );

        let details = codesign_details(&input);
        assert!(
            details.contains("Identifier=com.example.custom\n"),
            "{details}"
        );
    }

    #[tokio::test]
    async fn takes_a_bundle_identifier_from_info_plist() {
        let service = MockService::start().await;
        let dir = TempDir::new().unwrap();
        let input = artifacts::app_bundle(dir.path());

        run_ok(
            sign_command(&service.url(), APPLICATION_CERT_ID)
                .args(["--identifier", "com.example.ignored"])
                .arg(&input),
        );

        let details = codesign_details(&input);
        assert!(
            details.contains(&format!("Identifier={BUNDLE_IDENTIFIER}\n")),
            "{details}"
        );
    }

    #[tokio::test]
    async fn sets_the_hardened_runtime_flag() {
        let service = MockService::start().await;
        let dir = TempDir::new().unwrap();
        let input = artifacts::macho(dir.path());

        run_ok(
            sign_command(&service.url(), APPLICATION_CERT_ID)
                .args(["--code-signature-flags", "runtime"])
                .arg(&input),
        );

        let details = codesign_details(&input);
        assert!(details.contains("(runtime)"), "{details}");
        codesign_verify(&input);
    }

    #[tokio::test]
    async fn embeds_entitlements() {
        let service = MockService::start().await;
        let dir = TempDir::new().unwrap();
        let input = artifacts::macho(dir.path());
        let entitlements = dir.path().join("entitlements.plist");
        fs::write(
            &entitlements,
            r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
	<key>com.apple.security.cs.allow-jit</key>
	<true/>
</dict>
</plist>
"#,
        )
        .unwrap();

        run_ok(
            sign_command(&service.url(), APPLICATION_CERT_ID)
                .arg("--entitlements-xml-file")
                .arg(&entitlements)
                .arg(&input),
        );

        let (output, stderr) = run(Command::new("codesign")
            .args(["-d", "--entitlements", "-"])
            .arg(&input));
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(output.status.success(), "{stderr}");
        assert!(
            stdout.contains("com.apple.security.cs.allow-jit"),
            "{stdout}"
        );
    }

    #[tokio::test]
    async fn reports_a_certificate_rotated_since_it_was_fetched() {
        let service = MockService::start_rotated().await;
        let dir = TempDir::new().unwrap();
        let input = artifacts::macho(dir.path());

        let stderr = run_err(sign_command(&service.url(), APPLICATION_CERT_ID).arg(&input));

        assert!(stderr.contains("rotated"), "{stderr}");
    }
}
