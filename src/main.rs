// Unless explicitly stated otherwise all files in this repository are licensed under the Apache-2.0 License.
// This product includes software developed at Datadog (https://www.datadoghq.com/).
// Copyright 2026-Present Datadog, Inc.

mod client;
mod settings;
mod signer;

use crate::client::SigningServiceClient;
use crate::signer::RemoteKeyInfoSigner;
use apple_codesign::UnifiedSigner;
use clap::{Parser, Subcommand};
use std::env;
use std::path::{Path, PathBuf};
use x509_certificate::CapturedX509Certificate;

/// Token is env-var only (never a flag) so it can't leak via shell history or `ps`.
const SIGNING_SERVICE_TOKEN_ENV_VAR: &str = "MACOS_SIGNING_SERVICE_TOKEN";

/// Signs macOS artifacts using a Developer ID certificate whose private key
/// is held by macos-signing-service; only digests and signatures cross the
/// network.
#[derive(Parser)]
struct Cli {
    /// Base URL of macos-signing-service, e.g. `https://macos-signing-service.example`.
    // clap forbids combining `global` with `required`, so this can't be a
    // plain `String` despite always being needed — presence is checked by
    // hand in `main` instead, once the subcommand has also been parsed.
    #[arg(long, env = "MACOS_SIGNING_SERVICE_URL", global = true)]
    service_url: Option<String>,

    /// ID of the certificate to sign with, as registered with macos-signing-service.
    #[arg(long, global = true)]
    cert_id: Option<String>,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Sign a Mach-O binary, `.app` bundle, `.pkg` installer, or `.dmg` disk
    /// image. The entity type is inferred from `input`, the same way
    /// `rcodesign sign` does.
    Sign {
        /// Path to the unsigned Mach-O binary, bundle directory, `.pkg`, or `.dmg`.
        input: PathBuf,
        /// Path to write the signed output to. Defaults to signing `input` in place.
        output: Option<PathBuf>,
        /// CodeDirectory identifier for a standalone Mach-O or `.dmg`; no
        /// effect on bundles or `.pkg`. See `settings::build`.
        #[arg(long)]
        identifier: Option<String>,

        /// Code signature flag to set, e.g. `runtime` for the hardened
        /// runtime. May be repeated. Valid values: host, hard, kill,
        /// expires, library, runtime, linker-signed.
        #[arg(long = "code-signature-flags", value_name = "FLAG")]
        code_signature_flags: Vec<String>,

        /// Path to an XML plist file containing entitlements to embed in
        /// the signature.
        #[arg(long = "entitlements-xml-file", value_name = "PATH")]
        entitlements_xml_file: Option<PathBuf>,

        /// URL of the RFC 3161 time-stamp server to use, or `none` to sign
        /// without a secure timestamp. Mirrors `rcodesign sign`'s
        /// `--timestamp-url`.
        #[arg(long = "timestamp-url", default_value = settings::DEFAULT_TIMESTAMP_URL)]
        timestamp_url: String,
    },
}

/// Requires that `value` was supplied, since `global = true` prevents clap
/// from enforcing that itself (see `service_url`/`cert_id`'s doc comments on
/// `Cli`).
fn require<T>(value: Option<T>, message: &'static str) -> anyhow::Result<T> {
    value.ok_or_else(|| anyhow::anyhow!(message))
}

/// Requires that `cert_type` (from `Certificate::cert_type`) matches the kind
/// of Developer ID certificate `input` needs: `.pkg` installers need an
/// "installer" certificate, everything else (Mach-O binaries, bundles,
/// `.dmg`) needs an "application" certificate. macOS rejects a signature
/// made with the wrong certificate type, so this is checked up front rather
/// than surfacing as a confusing failure later.
fn require_matching_cert_type(cert_type: &str, input: &Path) -> anyhow::Result<()> {
    let is_pkg = input
        .extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("pkg"));
    let expected = if is_pkg { "installer" } else { "application" };

    if cert_type != expected {
        anyhow::bail!(
            "cert-id's certificate type is \"{cert_type}\", but signing \"{}\" requires an \
             \"{expected}\" certificate",
            input.display()
        );
    }

    Ok(())
}

/// Requires that the signing-service token env var was set, surfacing
/// `token_lookup`'s absence as an actionable error instead of clap's default
/// "environment variable not found" message.
fn require_token(token_lookup: Result<String, env::VarError>) -> anyhow::Result<String> {
    token_lookup.map_err(|_| anyhow::anyhow!("{SIGNING_SERVICE_TOKEN_ENV_VAR} must be set"))
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    let service_url = require(
        cli.service_url,
        "--service-url (or MACOS_SIGNING_SERVICE_URL) is required",
    )?;
    let cert_id = require(cli.cert_id, "--cert-id is required")?;
    let token = require_token(env::var(SIGNING_SERVICE_TOKEN_ENV_VAR))?;

    let client = SigningServiceClient::new(&service_url, &token)?;
    let certificate = client.get_certificate(&cert_id)?;
    let leaf = CapturedX509Certificate::from_pem(certificate.certificate_pem.as_bytes())?;
    let signer = RemoteKeyInfoSigner::new(&cert_id, leaf.clone(), certificate.key_version, client);

    match cli.command {
        Command::Sign {
            input,
            output,
            identifier,
            code_signature_flags,
            entitlements_xml_file,
            timestamp_url,
        } => {
            require_matching_cert_type(&certificate.cert_type, &input)?;

            let entitlements_xml = entitlements_xml_file
                .map(std::fs::read_to_string)
                .transpose()?;
            let settings = settings::build(
                &signer,
                leaf,
                identifier.as_deref(),
                &code_signature_flags,
                entitlements_xml.as_deref(),
                &timestamp_url,
            )?;
            let unified_signer = UnifiedSigner::new(settings);
            match &output {
                Some(output) => unified_signer.sign_path(&input, output)?,
                None => unified_signer.sign_path_in_place(&input)?,
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn require_rejects_missing() {
        let err = require::<String>(None, "value is required").unwrap_err();
        assert_eq!(err.to_string(), "value is required");
    }

    #[test]
    fn require_accepts_present() {
        let value = require(Some("value"), "unused").unwrap();
        assert_eq!(value, "value");
    }

    #[test]
    fn require_token_rejects_missing() {
        let err = require_token(Err(env::VarError::NotPresent)).unwrap_err();
        assert!(err.to_string().contains(SIGNING_SERVICE_TOKEN_ENV_VAR));
    }

    #[test]
    fn require_token_accepts_present() {
        let token = require_token(Ok("secret-token".to_string())).unwrap();
        assert_eq!(token, "secret-token");
    }

    #[test]
    fn require_matching_cert_type_accepts_application_cert_for_non_pkg() {
        assert!(require_matching_cert_type("application", &PathBuf::from("hello.app")).is_ok());
    }

    #[test]
    fn require_matching_cert_type_accepts_installer_cert_for_pkg() {
        assert!(require_matching_cert_type("installer", &PathBuf::from("hello.pkg")).is_ok());
    }

    #[test]
    fn require_matching_cert_type_rejects_application_cert_for_pkg() {
        let err =
            require_matching_cert_type("application", &PathBuf::from("hello.pkg")).unwrap_err();
        assert!(err.to_string().contains("installer"));
    }

    #[test]
    fn require_matching_cert_type_rejects_installer_cert_for_non_pkg() {
        let err = require_matching_cert_type("installer", &PathBuf::from("hello.app")).unwrap_err();
        assert!(err.to_string().contains("application"));
    }
}
