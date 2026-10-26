// Unless explicitly stated otherwise all files in this repository are licensed under the Apache-2.0 License.
// This product includes software developed at Datadog (https://www.datadoghq.com/).
// Copyright 2026-Present Datadog, Inc.

use crate::client::SigningServiceClient;
use bytes::Bytes;
use sha2::{Digest as _, Sha256};
use signature::{Error as SignatureError, Signer};
use x509_certificate::{
    CapturedX509Certificate, KeyAlgorithm, KeyInfoSigner, Sign, Signature, SignatureAlgorithm,
    X509CertificateError,
};
use zeroize::Zeroizing;

/// A `KeyInfoSigner` that delegates digest-signing to macos-signing-service
/// over HTTP instead of holding key material locally; always hashes with
/// SHA-256 and signs PKCS#1v1.5, so only RSA certificates are supported
/// (see [`Sign::signature_algorithm`]).
pub struct RemoteKeyInfoSigner {
    cert_id: String,
    certificate: CapturedX509Certificate,
    /// Freshness precondition on every sign request; see `Certificate::key_version`.
    key_version: i64,
    client: SigningServiceClient,
}

impl RemoteKeyInfoSigner {
    /// Builds a signer that signs on behalf of `cert_id` via `client`, using
    /// `certificate` and `key_version` (both previously fetched for
    /// `cert_id`) for public-key metadata and staleness detection.
    pub fn new(
        cert_id: &str,
        certificate: CapturedX509Certificate,
        key_version: i64,
        client: SigningServiceClient,
    ) -> Self {
        Self {
            cert_id: cert_id.to_owned(),
            certificate,
            key_version,
            client,
        }
    }
}

impl Signer<Signature> for RemoteKeyInfoSigner {
    fn try_sign(&self, msg: &[u8]) -> Result<Signature, SignatureError> {
        let digest = Sha256::digest(msg);

        let raw_signature = self
            .client
            .sign_digest(&self.cert_id, &digest, Some(self.key_version))
            .map_err(SignatureError::from_source)?;

        Ok(Signature::from(raw_signature))
    }
}

impl Sign for RemoteKeyInfoSigner {
    fn key_algorithm(&self) -> Option<KeyAlgorithm> {
        self.certificate.key_algorithm()
    }

    fn private_key_data(&self) -> Option<Zeroizing<Vec<u8>>> {
        None
    }

    fn public_key_data(&self) -> Bytes {
        self.certificate.public_key_data()
    }

    fn rsa_primes(
        &self,
    ) -> Result<Option<(Zeroizing<Vec<u8>>, Zeroizing<Vec<u8>>)>, X509CertificateError> {
        Ok(None)
    }

    #[allow(deprecated)]
    fn sign(&self, message: &[u8]) -> Result<(Vec<u8>, SignatureAlgorithm), X509CertificateError> {
        Ok((self.try_sign(message)?.into(), self.signature_algorithm()?))
    }

    fn signature_algorithm(&self) -> Result<SignatureAlgorithm, X509CertificateError> {
        match self.certificate.key_algorithm() {
            Some(KeyAlgorithm::Rsa) => Ok(SignatureAlgorithm::RsaSha256),
            other => Err(X509CertificateError::Other(format!(
                "remote signing only supports RSA certificates, but \"{}\" has key algorithm {other:?}",
                self.cert_id
            ))),
        }
    }
}

impl KeyInfoSigner for RemoteKeyInfoSigner {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client::SigningServiceClient;
    use base64::Engine as _;
    use serde_json::json;
    use wiremock::matchers::{body_json, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    const TEST_KEY_PEM: &str = include_str!("../tests/fixtures/test-key.pem");
    const TEST_CERT_PEM: &str = include_str!("../tests/fixtures/test-cert.pem");

    // Byte-identical to local signing with the same key.
    #[tokio::test]
    async fn try_sign_matches_local_signing_with_the_same_key() {
        let local_signer = x509_certificate::InMemorySigningKeyPair::from_pkcs8_pem(TEST_KEY_PEM)
            .expect("test fixture key should be valid PKCS#8 PEM");
        let message = b"macos-code-signer parity test message";
        let expected_signature: Vec<u8> = local_signer.try_sign(message).unwrap().into();

        let server = MockServer::start().await;
        let expected_digest = Sha256::digest(message);
        Mock::given(method("POST"))
            .and(path("/signer/macos/certificates/cert1/sign"))
            .and(body_json(json!({
                "digest": base64::engine::general_purpose::STANDARD.encode(expected_digest),
                "key_version": 1,
            })))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "data": {
                    "type": "signature",
                    "id": "cert1",
                    "attributes": {
                        "signature": base64::engine::general_purpose::STANDARD.encode(&expected_signature),
                        "key_version": 1,
                    },
                },
            })))
            .mount(&server)
            .await;

        let base_url = server.uri();
        let signature = tokio::task::spawn_blocking(move || {
            let client = SigningServiceClient::new(&base_url, "token").unwrap();
            let certificate = CapturedX509Certificate::from_pem(TEST_CERT_PEM.as_bytes()).unwrap();
            let remote_signer = RemoteKeyInfoSigner::new("cert1", certificate, 1, client);

            remote_signer.try_sign(message)
        })
        .await
        .unwrap()
        .unwrap();

        assert_eq!(Vec::<u8>::from(signature), expected_signature);
    }
}
