# macos-code-signer: macOS code-signing CLI

Rust CLI that signs Mach-O binaries, `.app` bundles, `.pkg` installers and `.dmg` disk images with a Developer ID certificate whose private key never leaves the signing service. It computes the CodeDirectory/CMS digest locally with the `apple-codesign` crate, sends only that digest to `macos-signing-service` over HTTPS with an OIDC bearer token, and embeds the returned signature locally.

See `README.md` for usage and the service API.

## Design notes

- **CLI parsing** (`src/main.rs`): `clap`-based. Fetches the certificate for `--cert-id` up front (`SigningServiceClient::get_certificate`), builds a `RemoteKeyInfoSigner`, checks the cert type against the input, then delegates to `apple_codesign::UnifiedSigner`.
- **HTTP client** (`src/client.rs`): `SigningServiceClient` speaks the service's JSON:API-shaped HTTP surface. Requires HTTPS unless the target is loopback (for local testing). Disables redirects: reqwest keeps the `Authorization` header on a same-host redirect, which could leak the OIDC token to plaintext HTTP. Times out after 30 seconds. `sign_digest` reports a `409` as a clear "certificate was rotated since fetched, re-fetch and retry" error.
- **Remote signer** (`src/signer.rs`): `RemoteKeyInfoSigner` implements `x509_certificate`'s `KeyInfoSigner`/`Sign`/`Signer` traits so `apple_codesign::SigningSettings` can use it as a drop-in signing key. Hashes with SHA-256 and signs PKCS#1 v1.5, so only RSA certificates are supported. No private key material is held locally: `private_key_data` always returns `None`.
- **Signing settings** (`src/settings.rs`): `build()` constructs `apple_codesign::SigningSettings` and resolves the issuing chain from the leaf with `chain_apple_certificates()`. A self-signed leaf (throwaway test certificates) is signed with no chain. Any other unresolvable leaf is a provisioning error and fails.

## Layout

- `src/main.rs`: CLI entrypoint, argument parsing, the `sign` subcommand.
- `src/client.rs`: `SigningServiceClient`, the HTTP client for the signing service.
- `src/signer.rs`: `RemoteKeyInfoSigner`, the remote-backed `KeyInfoSigner`.
- `src/settings.rs`: `SigningSettings` construction (chain resolution, flags, entitlements, timestamp).
- `tests/fixtures/`: `test-cert.pem`/`test-key.pem`, a throwaway self-signed key pair used by unit tests.

## Dependencies

`apple-codesign` and `apple-xar` are patched in `Cargo.toml` to an unreleased upstream git rev, because the crates.io release of `apple-codesign` depends on an older `x509-certificate` than this crate uses. Remove the patch once upstream publishes a release on `x509-certificate` 0.25.

## Testing

`cargo test` runs the unit tests for all four modules. `client.rs` and `signer.rs` tests mock the service with `wiremock`. `signer.rs` also asserts byte-identical output between remote signing and local signing with the same key, using `tests/fixtures/`.

Before committing: `cargo fmt --check` and `cargo clippy --all-targets -- -Dwarnings`.
