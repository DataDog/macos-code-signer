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
- `scripts/install-rcodesign.sh`: installs a pinned, checksum-verified `rcodesign` build (macOS universal or Linux x86_64, picked from `uname -s`) into a given directory. Used by `scripts/sign-legacy.sh`, on both the macOS signing job and the Linux notarization job.
- `scripts/sign-legacy.sh`: signs release binaries in place or submits them for notarization, with the bootstrap Developer ID Application certificate (a `.p12` plus `rcodesign`). See "Releases" for where each mode runs and how its secrets reach it.
- `scripts/package.sh`: builds the release tarballs (binary + `README.md`) and `SHA256SUMS` for a given version (build mode), or copies already-built tarballs to fixed, unversioned names with a matching `SHA256SUMS` (`--latest` mode, see "Releases"). Used identically by `.github/workflows/release.yml`'s and `.gitlab-ci.yml`'s `package` jobs, so both releases are packaged the same way; `--latest` mode is also used by the GitLab `release-s3` job. Needs `tar` and a sha256 tool (`sha256sum` or `shasum`); neither mode needs macOS, even though the binaries it packages are macOS-only.
- `scripts/check-release-tag.sh`: fails unless a `vX.Y.Z` tag matches `Cargo.toml`'s version. Used by both CI systems before a release build runs.
- `scripts/cargo-version.sh`: prints `Cargo.toml`'s package version. Shared by `check-release-tag.sh` and both release workflows, so the version is only parsed in one place.
- `action.yml`: a composite GitHub Action that downloads the released binary for the runner's architecture, checks it against `SHA256SUMS`, verifies it like `verify-macos` does (signature, Datadog's Team ID `JKFCB4CN7C`, hardened runtime, online notarization check), and adds it to `PATH`.
- `tests/e2e.rs`: end-to-end tests that run the built binary against a mock service.
- `tests/common/`: E2E helpers. `mock_service.rs` is a `wiremock` mock of the service API that signs with the E2E fixture key. `artifacts.rs` builds inputs with `scripts/gen-test-artifacts/` (macOS only).
- `tests/fixtures/`: throwaway self-signed key pairs. `test-*.pem` for unit tests, `e2e-developer-id-*.pem` (Developer ID-style extensions) for E2E tests. See `tests/fixtures/README.md`.

## Dependencies

`apple-codesign` and `apple-xar` are patched in `Cargo.toml` to an unreleased upstream git rev, because the crates.io release of `apple-codesign` depends on an older `x509-certificate` than this crate uses. Remove the patch once upstream publishes a release on `x509-certificate` 0.25.

## Releases

Both pipelines package on every trusted ref (`main` and `v*` tags), but only publish on a `v*` tag that matches `Cargo.toml`'s version:

- The version string is the tag's version (without the leading `v`) on a tag, or `<Cargo.toml version>-<short commit SHA>` on `main`.
- GitHub Actions (`.github/workflows/release.yml`) triggers on push to `main` and on `v*` tags. `build` (macOS, `protected-main-env`) builds, signs and verifies the binaries, then uploads them as an artifact. `notarize` (Linux, `protected-main-env`) submits them to Apple and waits. `verify-macos` (macOS) checks the signature, Team ID, hardened runtime and notarization. `package` (Linux) downloads the verified binaries, packages with `scripts/package.sh` and uploads the workflow artifact. `check-tag` always runs, but its version check only runs on a tag; a failed check blocks `build` and `package`. `release` only runs on a tag, in the `protected-main-env` environment, so its ref restrictions and any configured approval rules apply.
- GitLab CI's `build-macos` builds the binaries, then signs and verifies them on a trusted ref (`main` or a `v*` tag; elsewhere there are no secrets to sign with, so it skips straight to uploading the unsigned binaries as an artifact). `notarize` (Linux, trusted refs only) submits them to Apple and waits. `verify-macos` (trusted refs only) checks the signature, Team ID, hardened runtime and notarization. `package` (Linux) packages the binaries with `scripts/package.sh` on `main` and on `v*` tags (`dist/` is a job artifact either way). `release-s3` only runs on `v*` tags.

**Signing and notarization** (both CI systems, bootstrap certificate, see `scripts/sign-legacy.sh`):

- Signing uses a pinned, checksum-verified `rcodesign` and a Developer ID Application certificate stored as a legacy `.p12`: `[GH]` (GitHub secrets, `protected-main-env`) on GitHub Actions, `[I]` (Vault, `trusted-jobs` CI Identity) on GitLab. Both sign with `--code-signature-flags runtime --binary-identifier com.datadoghq.macos-code-signer`.
- Notarization submits a zip of the signed binaries to Apple's notary service with `rcodesign notary-submit --api-key-file ... --wait`, using an App Store Connect API key from the same secret stores. Bare Mach-O binaries can't be stapled, so the notarization ticket is only ever checked online, never attached to the binary.
- `verify-macos` runs `codesign --verify --strict --verbose=2`, checks `TeamIdentifier` and the `runtime` flag in `codesign -dvvv`, and checks the notarization ticket with `codesign --verify --check-notarization --test-requirement="=notarized"`. `--check-notarization` forces an online lookup: a bare binary can't be stapled, and a fresh runner has no cached ticket.
- On GitLab, `build-macos` reads the certificate from `kv/ci/datadog/macos-code-signer/trusted-jobs/apple-application-signing` (fields `certificate`, `passphrase`) and `notarize` reads the App Store Connect key from `.../apple-connect-api-key` (field `json`), both via a pinned `ci-identities-ci-job-client` and an `id_tokens` block requesting a `ci-identities` audience token, the same mechanism `release-s3` uses for AWS.

On GitLab, `build-macos` and `package` both declare `needs: check-release-tag` (`optional: true`, since that job only exists on a `v*` tag). If the tag doesn't match `Cargo.toml`'s version, `check-release-tag` fails and blocks both: a mismatched tag never even starts the macOS build, and packaging never runs. `release-s3` uses `dependencies: [package]` instead of `needs`, so it's gated by stage order rather than its own need: it waits for every earlier stage (lint, including `check-release-tag`, then test, build, notarize, verify and package) to finish before it can start. The `optional: true` need is a no-op on branch and MR pipelines, where `check-release-tag` doesn't exist.

**Publishing:**

- GitHub Releases are immutable once published, so `release` creates a draft with `gh release create --draft --verify-tag --generate-notes` and all the release files, then publishes it with `gh release edit --draft=false`. Nothing is ever uploaded to an already-published release.
- `release-s3` uploads to `s3://${RELEASE_S3_BUCKET}/macos-code-signer/<ver>/` (the versioned files, as packaged) and to `.../latest/` (the same files, copied and re-hashed under fixed, unversioned names with `scripts/package.sh --latest`, so `latest/` only ever holds the current release). It authenticates as the `trusted-jobs` CI Identity: an `id_tokens` block requests a `ci-identities` audience token, a pinned `ci-identities-ci-job-client` build exchanges it for AWS credentials (`assume-role --account-id 486234852809`) written to `AWS_SHARED_CREDENTIALS_FILE`, and that identity's IAM policy only allows `s3:PutObject`/`s3:GetObject` under `macos-code-signer/*` and `s3:ListBucket` with that prefix. There is no delete or ACL call anywhere in the job.

## Testing

`cargo test` runs the unit tests for all four modules and the E2E tests. `client.rs` and `signer.rs` tests mock the service with `wiremock`. `signer.rs` also asserts byte-identical output between remote signing and local signing with the same key, using `tests/fixtures/`.

`cargo test --test e2e` runs only the E2E tests. Error cases (missing token, non-HTTPS URL, cert type mismatch, `401`, unknown flag) run on any OS. Signing tests need macOS with the Xcode command line tools (`cc`, `codesign`, `hdiutil`, `pkgbuild`, `pkgutil`) and are compiled out elsewhere. They check the signature state of the generated inputs, sign each artifact type (Mach-O and `.app` both ad-hoc signed and unsigned), verify the signature and its certificate (`codesign`, or `apple-xar` for `.pkg`, since `pkgutil` doesn't tell an invalid signature from an untrusted certificate), and compare the CodeDirectory against local signing with the same key.

Before committing: `cargo fmt --check` and `cargo clippy --all-targets -- -Dwarnings`.

## CI

`.gitlab-ci.yml` runs `fmt`, `shellcheck` (on every `.sh` file under `scripts/`), `clippy` and `test-linux` on Linux. `test-macos` (full E2E) and `build-macos` (release binaries for both targets, plus a universal one made with `lipo`, signed on trusted refs) run on a macOS runner. macOS runners are scarcer than Linux ones, so these jobs run less often. They always run on the default branch and on tags. On other branches, they run only when the code, tests, scripts, toolchain or CI config differ from `main`. All jobs install the toolchain pinned in `rust-toolchain.toml`, whatever the runner image ships. `notarize`, `verify-macos`, `package` and `release-s3` run after `build-macos`, in that order; see "Releases" above for what each does and which refs they run on.

`check-release-tag` (GitLab) only exists on a `v*` tag; the `check-tag` job (GitHub, `.github/workflows/release.yml`) always runs, but its `scripts/check-release-tag.sh` step only runs on a tag. Either way, a mismatched tag fails the check, which blocks building, packaging and publishing. See "Releases" above for exactly how each CI system gates its build, packaging and publishing on that check, and for how `.github/workflows/release.yml` also runs on `main`.
