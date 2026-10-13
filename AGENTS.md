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
- `scripts/gen-test-artifacts/`: shell scripts that build Mach-O, `.app`, `.dmg` and `.pkg` artifacts. The Mach-O and `.app` executable are ad-hoc signed by the linker on arm64 unless `--unsigned` is passed. The others are unsigned. The E2E tests build their inputs with them. Also used for manual testing against a deployed service.
- `scripts/package.sh`: builds the release tarballs (binary + `README.md`) and `SHA256SUMS` for a given version (build mode), or copies already-built tarballs to fixed, unversioned names with a matching `SHA256SUMS` (`--latest` mode). Needs `tar` and a sha256 tool (`sha256sum` or `shasum`); neither mode needs macOS, even though the binaries it packages are macOS-only. Used by `.github/workflows/release.yml`'s `package` job.
- `scripts/check-release-tag.sh`: fails unless a `vX.Y.Z` tag matches `Cargo.toml`'s version. Used by `.github/workflows/release.yml`'s `check-tag` job before a release build runs.
- `scripts/cargo-version.sh`: prints `Cargo.toml`'s package version. Shared by `check-release-tag.sh` and `.github/workflows/release.yml`'s version resolution, so the version is only parsed in one place.
- `tests/e2e.rs`: end-to-end tests that run the built binary against a mock service.
- `tests/common/`: E2E helpers. `mock_service.rs` is a `wiremock` mock of the service API that signs with the E2E fixture key. `artifacts.rs` builds inputs with `scripts/gen-test-artifacts/` (macOS only).
- `tests/fixtures/`: throwaway self-signed key pairs. `test-*.pem` for unit tests, `e2e-developer-id-*.pem` (Developer ID-style extensions) for E2E tests. See `tests/fixtures/README.md`.

## Dependencies

`apple-codesign` and `apple-xar` are patched in `Cargo.toml` to an unreleased upstream git rev, because the crates.io release of `apple-codesign` depends on an older `x509-certificate` than this crate uses. Remove the patch once upstream publishes a release on `x509-certificate` 0.25.

## Releases

Binaries are not signed yet. Signing will be added in the build job (see the placeholder comment in `.github/workflows/release.yml`). GitHub Actions triggers on push to `main` and on `v*` tags, but only publishes on a `v*` tag that matches `Cargo.toml`'s version:

- The version string is the tag's version (without the leading `v`) on a tag, or `<Cargo.toml version>-<short commit SHA>` on `main`.
- `build` (macOS) always builds and uploads the binaries as an artifact; `package` (Linux) downloads them, packages with `scripts/package.sh` and uploads the workflow artifact. `check-tag` always runs, but its version check only runs on a tag; a failed check blocks `build` and `package`. `release` only runs on a tag, in the `protected-main-env` environment, so its ref restrictions and any configured approval rules apply.

**Publishing:**

- GitHub Releases are immutable once published, so `release` creates a draft with `gh release create --draft --verify-tag --generate-notes` and all the release files, then publishes it with `gh release edit --draft=false`. Nothing is ever uploaded to an already-published release.

## Testing

`cargo test` runs the unit tests for all four modules and the E2E tests. `client.rs` and `signer.rs` tests mock the service with `wiremock`. `signer.rs` also asserts byte-identical output between remote signing and local signing with the same key, using `tests/fixtures/`.

`cargo test --test e2e` runs only the E2E tests. Error cases (missing token, non-HTTPS URL, cert type mismatch, `401`, unknown flag) run on any OS. Signing tests need macOS with the Xcode command line tools (`cc`, `codesign`, `hdiutil`, `pkgbuild`, `pkgutil`) and are compiled out elsewhere. They check the signature state of the generated inputs, sign each artifact type (Mach-O and `.app` both ad-hoc signed and unsigned), verify the signature and its certificate (`codesign`, or `apple-xar` for `.pkg`, since `pkgutil` doesn't tell an invalid signature from an untrusted certificate), and compare the CodeDirectory against local signing with the same key.

Before committing: `cargo fmt --check` and `cargo clippy --all-targets -- -Dwarnings`.

## CI

`.gitlab-ci.yml` runs `fmt`, `shellcheck` (on every `.sh` file under `scripts/`), `clippy` and `test-linux` on Linux. `test-macos` (full E2E) and `build-macos` (release binaries for both targets, plus a universal one made with `lipo`) run on a macOS runner. macOS runners are scarcer than Linux ones, so these jobs run less often. They always run on the default branch and on tags. On other branches, they run only when the code, tests, scripts, toolchain or CI config differ from `main`. All jobs install the toolchain pinned in `rust-toolchain.toml`, whatever the runner image ships.

The `check-tag` job in `.github/workflows/release.yml` always runs, but its `scripts/check-release-tag.sh` step only runs on a `v*` tag, so a mismatched tag fails the job and blocks `build` and `package`. See "Releases" above for exactly how it gates them, and for how `release` only runs on a tag.
