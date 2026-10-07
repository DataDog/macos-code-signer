//! In-process mock of macos-signing-service's HTTP API.

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64;
use rsa::pkcs8::DecodePrivateKey as _;
use rsa::{Pkcs1v15Sign, RsaPrivateKey};
use serde_json::{Value, json};
use sha2::Sha256;
use wiremock::matchers::{method, path_regex};
use wiremock::{Mock, MockServer, Request, Respond, ResponseTemplate};
use x509_certificate::CapturedX509Certificate;

/// Bearer token the mock accepts.
pub const TOKEN: &str = "e2e-token";
/// Cert ID of the `application` certificate (Mach-O, bundles, `.dmg`).
pub const APPLICATION_CERT_ID: &str = "e2e-application";
/// Cert ID of the `installer` certificate (`.pkg`).
pub const INSTALLER_CERT_ID: &str = "e2e-installer";

/// Self-signed Developer ID-style certificate served for both cert IDs.
pub const CERT_PEM: &str = include_str!("../fixtures/e2e-developer-id-cert.pem");
/// Private key of `CERT_PEM`.
pub const KEY_PEM: &str = include_str!("../fixtures/e2e-developer-id-key.pem");

/// Key versions the mock advertises and accepts.
#[derive(Clone, Copy)]
struct KeyVersions {
    /// Returned by `GET .../certificates/{cert_id}`.
    advertised: i64,
    /// Sign requests pinned to an older version get `409`.
    min: i64,
}

pub struct MockService {
    server: MockServer,
}

impl MockService {
    /// Starts a mock whose certificates are current.
    pub async fn start() -> Self {
        Self::start_with(KeyVersions {
            advertised: 1,
            min: 1,
        })
        .await
    }

    /// Starts a mock that advertises key version 1 but only signs with 2 or
    /// later, as if the certificate was rotated between fetch and sign.
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    pub async fn start_rotated() -> Self {
        Self::start_with(KeyVersions {
            advertised: 1,
            min: 2,
        })
        .await
    }

    async fn start_with(versions: KeyVersions) -> Self {
        let server = MockServer::start().await;
        let key = RsaPrivateKey::from_pkcs8_pem(KEY_PEM).expect("fixture key should be PKCS#8");
        let not_after = CapturedX509Certificate::from_pem(CERT_PEM.as_bytes())
            .expect("fixture certificate should be valid PEM")
            .validity_not_after()
            .format("%Y-%m-%dT%H:%M:%SZ")
            .to_string();

        Mock::given(method("GET"))
            .and(path_regex(r"^/signer/macos/certificates/[^/]+$"))
            .respond_with(CertificateResponder {
                not_after,
                versions,
            })
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path_regex(r"^/signer/macos/certificates/[^/]+/sign$"))
            .respond_with(SignResponder { key, versions })
            .mount(&server)
            .await;

        Self { server }
    }

    pub fn url(&self) -> String {
        self.server.uri()
    }
}

/// Returns the cert type of `request`'s cert ID, or `None` if the request is
/// unauthenticated or the cert ID is unknown. Both cases get `401`, like the
/// real service, so callers can't tell them apart.
fn authorize(request: &Request) -> Option<(String, &'static str)> {
    let authorized = request
        .headers
        .get("authorization")
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value == format!("Bearer {TOKEN}"));
    if !authorized {
        return None;
    }

    let cert_id = request
        .url
        .path()
        .strip_prefix("/signer/macos/certificates/")?
        .split('/')
        .next()?
        .to_owned();
    let cert_type = match cert_id.as_str() {
        APPLICATION_CERT_ID => "application",
        INSTALLER_CERT_ID => "installer",
        _ => return None,
    };

    Some((cert_id, cert_type))
}

fn error(status: u16, detail: &str) -> ResponseTemplate {
    ResponseTemplate::new(status).set_body_json(json!({ "errors": [{ "detail": detail }] }))
}

struct CertificateResponder {
    /// Expiry of `CERT_PEM`, in RFC 3339.
    not_after: String,
    versions: KeyVersions,
}

impl Respond for CertificateResponder {
    fn respond(&self, request: &Request) -> ResponseTemplate {
        let Some((cert_id, cert_type)) = authorize(request) else {
            return error(401, "unauthorized");
        };

        ResponseTemplate::new(200).set_body_json(json!({
            "data": {
                "type": "certificate",
                "id": cert_id,
                "attributes": {
                    "certificate_pem": CERT_PEM,
                    "cert_type": cert_type,
                    "not_after": self.not_after,
                    "key_version": self.versions.advertised,
                },
            },
        }))
    }
}

struct SignResponder {
    key: RsaPrivateKey,
    versions: KeyVersions,
}

impl Respond for SignResponder {
    fn respond(&self, request: &Request) -> ResponseTemplate {
        let Some((cert_id, _)) = authorize(request) else {
            return error(401, "unauthorized");
        };

        let Ok(body) = serde_json::from_slice::<Value>(&request.body) else {
            return error(400, "malformed request body");
        };
        let Some(digest) = body["digest"]
            .as_str()
            .and_then(|digest| BASE64.decode(digest).ok())
            .filter(|digest| digest.len() == 32)
        else {
            return error(400, "digest must be a base64 SHA-256 digest");
        };

        // 0 or absent means "latest".
        let key_version = body["key_version"].as_i64().unwrap_or(0);
        if key_version != 0 && key_version < self.versions.min {
            return error(409, "key version was rotated");
        }

        // Prehashed RSASSA-PKCS1-v1_5: `digest` is already the SHA-256 of
        // the signed data, so it is wrapped in a DigestInfo and signed as is.
        let signature = self
            .key
            .sign(Pkcs1v15Sign::new::<Sha256>(), &digest)
            .expect("signing a 32-byte digest should not fail");

        ResponseTemplate::new(200).set_body_json(json!({
            "data": {
                "type": "signature",
                "id": cert_id,
                "attributes": {
                    "signature": BASE64.encode(signature),
                    "key_version": key_version.max(self.versions.min),
                },
            },
        }))
    }
}
