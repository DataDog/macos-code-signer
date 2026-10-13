# macos-code-signer

CLI that signs macOS artifacts with a Developer ID certificate whose private key stays in a remote signing service.

It signs Mach-O binaries, `.app` bundles, `.pkg` installers and `.dmg` disk images. The signature is computed locally with the [`apple-codesign`](https://github.com/indygreg/apple-platform-rs) crate. Only a 32-byte SHA-256 digest is sent to `macos-signing-service`, and only the signature comes back. The artifact and the private key never cross the network.

```
macos-code-signer sign <input>
  │ apple-codesign computes the CodeDirectory/CMS digest locally
  ▼
RemoteKeyInfoSigner
  │ SHA-256 digest --HTTPS + OIDC bearer token--> macos-signing-service
  ▼
signature comes back, apple-codesign embeds it in <input> or <output>
```

## Install

Download a prebuilt binary from the [releases page](https://github.com/DataDog/macos-code-signer/releases). Each release has `darwin_arm64`, `darwin_amd64` and `darwin_universal` tarballs, plus a `SHA256SUMS` file to check them against:

```
curl -LO https://github.com/DataDog/macos-code-signer/releases/download/vX.Y.Z/macos-code-signer_X.Y.Z_darwin_universal.tar.gz
curl -LO https://github.com/DataDog/macos-code-signer/releases/download/vX.Y.Z/SHA256SUMS
grep darwin_universal SHA256SUMS | shasum -a 256 -c -
tar xzf macos-code-signer_X.Y.Z_darwin_universal.tar.gz
```

In a GitHub Actions workflow, use the action instead. It downloads the right binary for the runner's architecture, checks it, and adds it to `PATH`:

```yaml
- uses: DataDog/macos-code-signer@v0.1.0
```

The action's `version` input defaults to its own ref, so pinning the action to a release tag is enough. Set `version` explicitly when it should differ from the ref, for example when the action is pinned by commit SHA:

```yaml
- uses: DataDog/macos-code-signer@a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2
  with:
    version: v0.1.0
```

Alternatively, build from source with a Rust toolchain (the version is pinned in `rust-toolchain.toml`):

```
cargo install --locked --git https://github.com/DataDog/macos-code-signer
```

## Usage

```
macos-code-signer --service-url <url> --cert-id <cert_id> sign [options] <input> [output]
```

Global flags, required on every invocation:
- `--service-url`: base URL of `macos-signing-service`. Can also be set with `MACOS_SIGNING_SERVICE_URL`. Must be HTTPS, except for loopback addresses (local testing).
- `--cert-id`: ID of the certificate to sign with, as registered in `macos-signing-service`.

The OIDC bearer token is read only from the `MACOS_SIGNING_SERVICE_TOKEN` environment variable. It is never a flag, so it can't leak through shell history or `ps`.

### `sign`

Signs `input` and writes the result to `output`, or signs `input` in place if `output` is omitted. The artifact type is inferred from `input`, the same way `rcodesign sign` does it.

Options:
- `--identifier`: CodeDirectory identifier for a standalone Mach-O or a `.dmg`. Bundles take their identifier from `CFBundleIdentifier` in `Info.plist`. `.pkg` files have no identifier.
- `--code-signature-flags <FLAG>`: code signature flag to set, e.g. `runtime` for the hardened runtime. Can be repeated. Valid values: `host`, `hard`, `kill`, `expires`, `library`, `runtime`, `linker-signed`.
- `--entitlements-xml-file <PATH>`: XML plist file with entitlements to embed in the signature.
- `--timestamp-url <URL>`: RFC 3161 time-stamp server, or `none` to sign without a secure timestamp. Defaults to `http://timestamp.apple.com/ts01`.

The certificate type must match the artifact. The CLI checks this before signing:
- `.pkg` installers need an `installer` certificate.
- Mach-O binaries, `.app` bundles and `.dmg` disk images need an `application` certificate.

Example:

```
export MACOS_SIGNING_SERVICE_TOKEN="$(<fetch an OIDC token for the macos-signing-service audience>)"
macos-code-signer \
  --service-url https://macos-signing-service.example \
  --cert-id my-application-cert \
  sign --code-signature-flags runtime --identifier com.example.tool ./tool
```

Signing does not notarize. Submit the signed artifact to Apple's notary service separately, e.g. with `rcodesign notary-submit` or `xcrun notarytool`.

## Signing service API

The CLI uses two endpoints. Both take an `Authorization: Bearer <token>` header and return JSON:API envelopes.

- `GET /signer/macos/certificates/{cert_id}` returns the leaf certificate (`certificate_pem`), its type (`cert_type`) and the key version it matches (`key_version`).
- `POST /signer/macos/certificates/{cert_id}/sign` takes `{"digest": "<base64 SHA-256>", "key_version": N}` and returns a base64 RSASSA-PKCS1-v1_5 signature. It returns `409 Conflict` if the certificate was rotated after it was fetched.

Only RSA certificates are supported.

## Development

```
cargo build
cargo test
cargo clippy --all-targets -- -Dwarnings
cargo fmt --check
```

Unit tests mock the signing service with [`wiremock`](https://crates.io/crates/wiremock).

End-to-end tests (`cargo test --test e2e`) run the built binary against a mock of the signing service that signs with a local test key. On macOS they build a Mach-O, an `.app`, a `.dmg` and a `.pkg` with the scripts in `scripts/gen-test-artifacts/`, sign them, then verify each signature with `codesign`, or with `apple-xar` for the `.pkg`. The Mach-O and the `.app` executable are ad-hoc signed by the linker on arm64, like any freshly built binary, so the tests also check that the CLI replaces an ad-hoc signature. They sign fully unsigned variants too. Elsewhere, only the error cases run.

To test against a deployed signing service, build test artifacts with the same scripts (macOS only), then sign them with the CLI:

```
scripts/gen-test-artifacts/generate-test-macho.sh --output hello
scripts/gen-test-artifacts/generate-test-bundle.sh --output TestApp.app
scripts/gen-test-artifacts/generate-test-dmg.sh --output test.dmg
scripts/gen-test-artifacts/generate-test-pkg.sh --output test.pkg
```

Sign the `.pkg` with an `installer` certificate and the others with an `application` certificate. Pass `--unsigned` to the Mach-O and `.app` scripts to skip the ad-hoc signature. Run a script with `--help` for its options.

`tests/fixtures/` holds the throwaway self-signed key pairs used by the tests. See [`tests/fixtures/README.md`](tests/fixtures/README.md).
