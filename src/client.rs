use base64::Engine as _;
use reqwest::StatusCode;
use reqwest::blocking::{Client, Response};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::time::Duration;
use url::Host;

/// How long to wait for a single request to macos-signing-service before
/// giving up, so a hung connection can't block the CLI forever.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

/// The public leaf certificate for a signing certificate, as returned by
/// `GET /signer/macos/certificates/{cert_id}`. The issuing chain is not part of this
/// response — `settings::build` resolves it itself from the leaf, via
/// `apple_codesign`'s `chain_apple_certificates()`.
#[derive(Debug, Clone, Deserialize)]
pub struct Certificate {
    /// PEM-encoded leaf certificate.
    pub certificate_pem: String,
    /// Which kind of Developer ID certificate this is: `"application"` (Mach-O
    /// binaries, bundles, `.dmg`) or `"installer"` (`.pkg`). Callers must
    /// check this against the artifact they're signing before proceeding —
    /// macOS rejects a signature made with the wrong certificate type.
    pub cert_type: String,
    /// The Vault Transit key version whose public key corresponds to
    /// `certificate_pem`'s public key. Pass this back as `sign_digest`'s
    /// `key_version` argument as a freshness precondition: macos-signing-service
    /// rejects the sign request with 409 Conflict if the certificate has
    /// since been rotated to a different key version, rather than silently
    /// signing with a key that no longer matches it.
    pub key_version: i64,
}

/// A signature produced over a caller-supplied digest, as returned by
/// `POST /signer/macos/certificates/{cert_id}/sign`.
#[derive(Debug, Clone, Deserialize)]
pub struct DigestSignature {
    /// Base64-encoded raw PKCS#1v1.5 signature bytes.
    pub signature: String,
}

#[derive(Debug, Serialize)]
struct SignDigestRequest<'a> {
    digest: &'a str,
    /// Freshness precondition, not a key selector; see `Certificate::key_version`.
    #[serde(skip_serializing_if = "Option::is_none")]
    key_version: Option<i64>,
}

/// Speaks to macos-signing-service's JSON:API-shaped HTTP surface on behalf
/// of a caller holding a pre-fetched OIDC token from an issuer
/// macos-signing-service trusts.
pub struct SigningServiceClient {
    http: Client,
    base_url: String,
    oidc_token: String,
}

impl SigningServiceClient {
    /// Builds a client against `base_url`, authenticated with `oidc_token`.
    ///
    /// `base_url` must be HTTPS, unless it points at a local loopback
    /// address (to allow testing against a local mock server): plaintext
    /// HTTP would otherwise leak `oidc_token` to anyone on the network path.
    pub fn new(base_url: &str, oidc_token: &str) -> anyhow::Result<Self> {
        Self::require_https_or_loopback(base_url)?;

        // Disable redirects: reqwest keeps the Authorization header on a same-host
        // redirect, which would leak `oidc_token` if an HTTPS endpoint ever
        // redirected to plaintext HTTP.
        let http = Client::builder()
            .timeout(REQUEST_TIMEOUT)
            .redirect(reqwest::redirect::Policy::none())
            .build()?;

        Ok(Self {
            http,
            base_url: base_url.trim_end_matches('/').to_owned(),
            oidc_token: oidc_token.to_owned(),
        })
    }

    /// Fetches the public leaf certificate for `cert_id`.
    pub fn get_certificate(&self, cert_id: &str) -> anyhow::Result<Certificate> {
        let response = self
            .http
            .get(format!(
                "{}/signer/macos/certificates/{cert_id}",
                self.base_url
            ))
            .bearer_auth(&self.oidc_token)
            .send()?;

        Self::attributes(Self::body(response)?)
    }

    /// Signs `digest` (raw bytes, not base64) with `cert_id`'s private key,
    /// returning the raw signature bytes. `key_version`, when supplied, pins
    /// the request to the key version the caller last observed from
    /// `get_certificate`; see `Certificate::key_version`.
    pub fn sign_digest(
        &self,
        cert_id: &str,
        digest: &[u8],
        key_version: Option<i64>,
    ) -> anyhow::Result<Vec<u8>> {
        let request = SignDigestRequest {
            digest: &base64::engine::general_purpose::STANDARD.encode(digest),
            key_version,
        };

        let response = self
            .http
            .post(format!(
                "{}/signer/macos/certificates/{cert_id}/sign",
                self.base_url
            ))
            .bearer_auth(&self.oidc_token)
            .json(&request)
            .send()?;

        if response.status() == StatusCode::CONFLICT {
            anyhow::bail!(
                "macos-signing-service rejected signing for \"{cert_id}\": the certificate was \
                 rotated since it was fetched; re-fetch the certificate and retry"
            );
        }

        let signature: DigestSignature = Self::attributes(Self::body(response)?)?;

        Ok(base64::engine::general_purpose::STANDARD.decode(signature.signature)?)
    }

    /// Extracts and deserializes the `data.attributes` object of a JSON:API
    /// envelope into `T`.
    fn attributes<T: DeserializeOwned>(body: Value) -> anyhow::Result<T> {
        let attributes = body
            .get("data")
            .and_then(|data| data.get("attributes"))
            .ok_or_else(|| anyhow::anyhow!("response is missing data.attributes"))?;

        Ok(serde_json::from_value(attributes.clone())?)
    }

    /// Returns `response`'s parsed JSON body, or an error including the
    /// response text (unlike `error_for_status`, which discards it).
    fn body(response: Response) -> anyhow::Result<Value> {
        let status = response.status();
        let text = response.text()?;

        if !status.is_success() {
            anyhow::bail!("macos-signing-service returned {status}: {text}");
        }

        Ok(serde_json::from_str(&text)?)
    }

    /// Rejects `base_url` unless it is HTTPS or targets a local loopback
    /// address; see [`Self::new`].
    fn require_https_or_loopback(base_url: &str) -> anyhow::Result<()> {
        let parsed = url::Url::parse(base_url)
            .map_err(|err| anyhow::anyhow!("\"{base_url}\" is not a valid URL: {err}"))?;

        if parsed.scheme() == "https" {
            return Ok(());
        }

        let is_loopback = match parsed.host() {
            Some(Host::Ipv4(ip)) => ip.is_loopback(),
            Some(Host::Ipv6(ip)) => ip.is_loopback(),
            Some(Host::Domain(domain)) => domain == "localhost",
            None => false,
        };

        if is_loopback {
            return Ok(());
        }

        anyhow::bail!(
            "\"{base_url}\" must use HTTPS: macos-signing-service requires an encrypted \
             connection so the OIDC bearer token isn't sent in plaintext"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use wiremock::matchers::{body_json, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[tokio::test]
    async fn get_certificate_parses_jsonapi_envelope() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/signer/macos/certificates/cert1"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "data": {
                    "type": "certificate",
                    "id": "cert1",
                    "attributes": {
                        "certificate_pem": "leaf-pem",
                        "cert_type": "application",
                        "not_after": "2030-01-01T00:00:00Z",
                        "key_version": 2,
                    },
                },
            })))
            .mount(&server)
            .await;

        let base_url = server.uri();
        let certificate = tokio::task::spawn_blocking(move || {
            SigningServiceClient::new(&base_url, "token")
                .unwrap()
                .get_certificate("cert1")
        })
        .await
        .unwrap()
        .unwrap();

        assert_eq!(certificate.certificate_pem, "leaf-pem");
        assert_eq!(certificate.cert_type, "application");
        assert_eq!(certificate.key_version, 2);
    }

    #[tokio::test]
    async fn sign_digest_sends_base64_digest_and_key_version_and_decodes_response() {
        let server = MockServer::start().await;
        let digest = b"fixed-size-sha256-digest-------";
        let signature = b"raw-signature-bytes";

        Mock::given(method("POST"))
            .and(path("/signer/macos/certificates/cert1/sign"))
            .and(body_json(json!({
                "digest": base64::engine::general_purpose::STANDARD.encode(digest),
                "key_version": 2,
            })))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "data": {
                    "type": "signature",
                    "id": "cert1",
                    "attributes": {
                        "signature": base64::engine::general_purpose::STANDARD.encode(signature),
                        "key_version": 2,
                    },
                },
            })))
            .mount(&server)
            .await;

        let base_url = server.uri();
        let digest = digest.to_vec();
        let result = tokio::task::spawn_blocking(move || {
            SigningServiceClient::new(&base_url, "token")
                .unwrap()
                .sign_digest("cert1", &digest, Some(2))
        })
        .await
        .unwrap()
        .unwrap();

        assert_eq!(result, signature);
    }

    #[tokio::test]
    async fn sign_digest_omits_key_version_when_not_pinned() {
        let server = MockServer::start().await;
        let digest = b"fixed-size-sha256-digest-------";
        let signature = b"raw-signature-bytes";

        Mock::given(method("POST"))
            .and(path("/signer/macos/certificates/cert1/sign"))
            .and(body_json(json!({
                "digest": base64::engine::general_purpose::STANDARD.encode(digest),
            })))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "data": {
                    "type": "signature",
                    "id": "cert1",
                    "attributes": {
                        "signature": base64::engine::general_purpose::STANDARD.encode(signature),
                        "key_version": 1,
                    },
                },
            })))
            .mount(&server)
            .await;

        let base_url = server.uri();
        let digest = digest.to_vec();
        let result = tokio::task::spawn_blocking(move || {
            SigningServiceClient::new(&base_url, "token")
                .unwrap()
                .sign_digest("cert1", &digest, None)
        })
        .await
        .unwrap()
        .unwrap();

        assert_eq!(result, signature);
    }

    #[test]
    fn new_rejects_non_https_urls_other_than_loopback() {
        let err = match SigningServiceClient::new("http://signing.example.com", "token") {
            Ok(_) => panic!("expected a non-HTTPS, non-loopback URL to be rejected"),
            Err(err) => err,
        };

        assert!(err.to_string().contains("HTTPS"));
    }

    #[test]
    fn new_accepts_http_to_loopback_for_local_testing() {
        assert!(SigningServiceClient::new("http://127.0.0.1:8080", "token").is_ok());
        assert!(SigningServiceClient::new("http://localhost:8080", "token").is_ok());
    }

    #[tokio::test]
    async fn sign_digest_reports_a_stale_key_version_clearly() {
        let server = MockServer::start().await;
        let digest = b"fixed-size-sha256-digest-------";

        Mock::given(method("POST"))
            .and(path("/signer/macos/certificates/cert1/sign"))
            .respond_with(ResponseTemplate::new(409))
            .mount(&server)
            .await;

        let base_url = server.uri();
        let digest = digest.to_vec();
        let err = tokio::task::spawn_blocking(move || {
            SigningServiceClient::new(&base_url, "token")
                .unwrap()
                .sign_digest("cert1", &digest, Some(1))
        })
        .await
        .unwrap()
        .unwrap_err();

        assert!(err.to_string().contains("rotated"));
    }
}
