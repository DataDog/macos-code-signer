// Unless explicitly stated otherwise all files in this repository are licensed under the Apache-2.0 License.
// This product includes software developed at Datadog (https://www.datadoghq.com/).
// Copyright 2026-Present Datadog, Inc.

use crate::signer::RemoteKeyInfoSigner;
use apple_codesign::{CodeSignatureFlags, SettingsScope, SigningSettings};
use std::str::FromStr;
use x509_certificate::CapturedX509Certificate;

/// Default RFC 3161 time-stamp server URL, matching `rcodesign sign`'s own
/// `--timestamp-url` default.
pub const DEFAULT_TIMESTAMP_URL: &str = "http://timestamp.apple.com/ts01";

/// Builds [`SigningSettings`] that sign with `signer`, using `leaf_certificate`
/// as the signing identity. The issuing chain is resolved from the leaf via
/// `chain_apple_certificates()`; a self-signed leaf (used in local/CI tests)
/// is signed with no chain, since it can never resolve one — any other
/// unresolvable leaf is a provisioning error and fails here.
///
/// `binary_identifier` sets the CodeDirectory identifier for a standalone
/// Mach-O or `.dmg`; a bundle always takes its identifier from `Info.plist`'s
/// `CFBundleIdentifier` instead, and `.pkg` has no identifier concept.
/// `code_signature_flags` and `entitlements_xml` mirror `rcodesign`'s
/// `--code-signature-flags` / `--entitlements-xml-file`, both applied to
/// `SettingsScope::Main`.
///
/// `timestamp_url` mirrors `rcodesign sign`'s `--timestamp-url`: an RFC 3161
/// secure timestamp is requested from that server during signing so the
/// signature stays verifiable after the signing certificate expires, unless
/// `timestamp_url` is the special value `"none"`, which disables it (used by
/// local/CI tests signing with a throwaway certificate, to stay hermetic).
pub fn build<'key>(
    signer: &'key RemoteKeyInfoSigner,
    leaf_certificate: CapturedX509Certificate,
    binary_identifier: Option<&str>,
    code_signature_flags: &[String],
    entitlements_xml: Option<&str>,
    timestamp_url: &str,
) -> anyhow::Result<SigningSettings<'key>> {
    // A same-name-as-issuer certificate is only actually self-signed if its
    // signature verifies against its own public key; otherwise it's just a
    // certificate whose issuer's real certificate wasn't provided.
    let is_self_signed = leaf_certificate.subject_is_issuer()
        && leaf_certificate
            .verify_signed_by_certificate(&leaf_certificate)
            .is_ok();

    let mut settings = SigningSettings::default();
    settings.set_signing_key(signer, leaf_certificate);

    if settings.chain_apple_certificates().is_none() && !is_self_signed {
        anyhow::bail!(
            "leaf certificate does not chain to a known Apple root CA; \
             re-provision it in Vault with the correct Developer ID certificate"
        );
    }

    // No-op for the self-signed certificates used in local/CI tests, since
    // Apple only embeds a team ID in certificates it has itself signed.
    settings.set_team_id_from_signing_certificate();

    if let Some(identifier) = binary_identifier {
        settings.set_binary_identifier(SettingsScope::Main, identifier);
    }

    if !code_signature_flags.is_empty() {
        let mut flags = CodeSignatureFlags::empty();
        for flag in code_signature_flags {
            flags |= CodeSignatureFlags::from_str(flag)
                .map_err(|_| anyhow::anyhow!("unknown code signature flag: {flag}"))?;
        }
        settings.set_code_signature_flags(SettingsScope::Main, flags);
    }

    if let Some(xml) = entitlements_xml {
        settings.set_entitlements_xml(SettingsScope::Main, xml)?;
    }

    if timestamp_url != "none" {
        settings.set_time_stamp_url(timestamp_url)?;
    }

    Ok(settings)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client::SigningServiceClient;
    use x509_certificate::{KeyAlgorithm, X509CertificateBuilder};

    const TEST_CERT_PEM: &str = include_str!("../tests/fixtures/test-cert.pem");

    fn test_signer(certificate: CapturedX509Certificate) -> RemoteKeyInfoSigner {
        let client = SigningServiceClient::new("http://127.0.0.1:0", "token").unwrap();
        RemoteKeyInfoSigner::new("cert1", certificate, 1, client)
    }

    #[test]
    fn build_signs_a_self_signed_leaf_with_no_chain() {
        let leaf = CapturedX509Certificate::from_pem(TEST_CERT_PEM.as_bytes()).unwrap();
        let signer = test_signer(leaf.clone());

        let settings = build(&signer, leaf, None, &[], None, "none")
            .expect("a self-signed leaf should not error");

        assert!(settings.certificate_chain().is_empty());
    }

    #[test]
    fn build_sets_the_time_stamp_url_by_default() {
        let leaf = CapturedX509Certificate::from_pem(TEST_CERT_PEM.as_bytes()).unwrap();
        let signer = test_signer(leaf.clone());

        let settings = build(&signer, leaf, None, &[], None, DEFAULT_TIMESTAMP_URL)
            .expect("a self-signed leaf should not error");

        assert_eq!(
            settings.time_stamp_url().map(|url| url.as_str()),
            Some(DEFAULT_TIMESTAMP_URL)
        );
    }

    #[test]
    fn build_skips_the_time_stamp_url_when_none() {
        let leaf = CapturedX509Certificate::from_pem(TEST_CERT_PEM.as_bytes()).unwrap();
        let signer = test_signer(leaf.clone());

        let settings = build(&signer, leaf, None, &[], None, "none")
            .expect("a self-signed leaf should not error");

        assert!(settings.time_stamp_url().is_none());
    }

    #[test]
    fn build_does_not_set_a_team_id_for_a_self_signed_leaf() {
        // `set_team_id_from_signing_certificate` only derives a team ID from
        // certificates that chain to an Apple root CA, which the throwaway
        // self-signed certs used by local/CI tests never do; this documents
        // that `build` calling it unconditionally stays a no-op for them.
        let leaf = CapturedX509Certificate::from_pem(TEST_CERT_PEM.as_bytes()).unwrap();
        let signer = test_signer(leaf.clone());

        let settings = build(&signer, leaf, None, &[], None, "none")
            .expect("a self-signed leaf should not error");

        assert!(settings.team_id().is_none());
    }

    #[test]
    fn build_errors_for_a_same_name_leaf_with_an_invalid_self_signature() {
        let leaf = CapturedX509Certificate::from_pem(TEST_CERT_PEM.as_bytes()).unwrap();

        // Flip the DER's last byte, part of the trailing signature BIT
        // STRING: subject and issuer stay equal, but the embedded signature
        // no longer verifies against the certificate's own key.
        let mut tampered_der = leaf.constructed_data().to_vec();
        let last = tampered_der.len() - 1;
        tampered_der[last] ^= 0xff;
        let tampered_leaf = CapturedX509Certificate::from_der(tampered_der)
            .expect("flipping a signature byte should not break DER parsing");
        assert!(tampered_leaf.subject_is_issuer());
        assert!(
            tampered_leaf
                .verify_signed_by_certificate(&tampered_leaf)
                .is_err(),
            "the tampered signature should no longer verify"
        );

        let signer = test_signer(tampered_leaf.clone());

        let err = build(&signer, tampered_leaf, None, &[], None, "none")
            .err()
            .expect("a same-name leaf with an invalid self-signature should error");

        assert!(
            err.to_string()
                .contains("does not chain to a known Apple root CA")
        );
    }

    #[test]
    fn build_errors_when_the_leaf_does_not_chain_to_an_apple_root() {
        let mut builder = X509CertificateBuilder::default();
        builder
            .subject()
            .append_common_name_utf8_string("not-self-signed-leaf")
            .unwrap();
        builder
            .issuer()
            .append_common_name_utf8_string("unknown-issuer")
            .unwrap();
        let (leaf, _key_pair) = builder
            .create_with_random_keypair(KeyAlgorithm::Ed25519)
            .expect("building a throwaway certificate should not fail");
        let signer = test_signer(leaf.clone());

        let err = build(&signer, leaf, None, &[], None, "none")
            .err()
            .expect("a leaf with an unresolvable issuer should error");

        assert!(
            err.to_string()
                .contains("does not chain to a known Apple root CA")
        );
    }

    #[test]
    fn build_applies_code_signature_flags() {
        let leaf = CapturedX509Certificate::from_pem(TEST_CERT_PEM.as_bytes()).unwrap();
        let signer = test_signer(leaf.clone());

        let settings = build(
            &signer,
            leaf,
            None,
            &["runtime".to_owned(), "hard".to_owned()],
            None,
            "none",
        )
        .expect("valid code signature flags should not error");

        assert_eq!(
            settings.code_signature_flags(SettingsScope::Main),
            Some(CodeSignatureFlags::RUNTIME | CodeSignatureFlags::FORCE_HARD)
        );
    }

    #[test]
    fn build_rejects_an_unknown_code_signature_flag() {
        let leaf = CapturedX509Certificate::from_pem(TEST_CERT_PEM.as_bytes()).unwrap();
        let signer = test_signer(leaf.clone());

        let err = build(
            &signer,
            leaf,
            None,
            &["not-a-real-flag".to_owned()],
            None,
            "none",
        )
        .err()
        .expect("an unknown flag should error");

        assert!(err.to_string().contains("not-a-real-flag"));
    }

    #[test]
    fn build_applies_entitlements_xml() {
        let leaf = CapturedX509Certificate::from_pem(TEST_CERT_PEM.as_bytes()).unwrap();
        let signer = test_signer(leaf.clone());
        let entitlements = r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>com.apple.security.app-sandbox</key>
    <true/>
</dict>
</plist>
"#;

        let settings = build(&signer, leaf, None, &[], Some(entitlements), "none")
            .expect("valid entitlements XML should not error");

        assert!(
            settings
                .entitlements_xml(SettingsScope::Main)
                .unwrap()
                .is_some()
        );
    }
}
