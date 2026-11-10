# Unless explicitly stated otherwise all files in this repository are licensed under the Apache-2.0 License.
# This product includes software developed at Datadog (https://www.datadoghq.com/).
# Copyright 2026-Present Datadog, Inc.

"""Tests for scripts/sign-staging.sh, against a local HTTPS mock of the
signing service and a stub signer. No real token or service is used."""

import shutil
import socket
import tempfile
import unittest
from pathlib import Path

from fixtures import (BINARY, SCRIPTS, STUB_SIGNER, MockSigningService, Toolchain, read_stub_calls,
                      run)

SIGN = SCRIPTS / "sign-staging.sh"
TOKEN = "synthetic-token-7f3c"
TARGETS = ["aarch64-apple-darwin", "x86_64-apple-darwin", "universal-apple-darwin"]


def setUpModule():
    global work, toolchain, service, unsigned
    work = Path(tempfile.mkdtemp())
    toolchain = Toolchain(work)
    service = MockSigningService(work)
    unsigned = toolchain.unsigned_tree(work / "unsigned")


def tearDownModule():
    service.stop()
    shutil.rmtree(work)


def closed_port_url():
    with socket.socket() as sock:
        sock.bind(("127.0.0.1", 0))
        port = sock.getsockname()[1]
    return f"https://127.0.0.1:{port}"


class SignStagingTest(unittest.TestCase):
    def setUp(self):
        self.dir = Path(tempfile.mkdtemp(dir=work))
        self.out = self.dir / "signed"
        self.stub_log = self.dir / "stub.log"
        service.requests.clear()

    def env(self, **overrides):
        """A valid configuration pointing at the mock service; overrides replace or, if None, remove values."""
        env = {
            "MACOS_CODE_SIGNER": str(STUB_SIGNER),
            "MACOS_SIGNING_SERVICE_URL": service.url,
            "STAGING_CERT_ID": "ok",
            "MACOS_SIGNING_SERVICE_TOKEN": TOKEN,
            "CURL_CA_BUNDLE": str(service.ca_bundle),
            "STUB_LOG": str(self.stub_log),
            "EXPECTED_TOKEN": TOKEN,
        }
        env.update(overrides)
        return {key: value for key, value in env.items() if value is not None}

    def sign(self, input_dir=None, **overrides):
        return run([SIGN, input_dir or unsigned, self.out], self.env(**overrides))

    def output(self, result):
        """Script output with the token redacted, for failure messages."""
        return (result.stdout + result.stderr).replace(TOKEN, "<token>")

    def assert_fails_with(self, result, message):
        self.assertNotEqual(result.returncode, 0, self.output(result))
        self.assertIn(message, self.output(result))
        self.assert_token_not_printed(result)
        self.assertEqual(read_stub_calls(self.stub_log), [], "signer must not run after a failure")

    def assert_token_not_printed(self, result):
        self.assertFalse(TOKEN in result.stdout + result.stderr, "token printed")

    def test_each_variable_is_required(self):
        for name in ["MACOS_CODE_SIGNER", "MACOS_SIGNING_SERVICE_URL", "STAGING_CERT_ID",
                     "MACOS_SIGNING_SERVICE_TOKEN"]:
            with self.subTest(name):
                self.assert_fails_with(self.sign(**{name: ""}), f"{name} must be set")

    def test_unsafe_urls_are_rejected_without_a_request(self):
        for url in [
            service.url.replace("https:", "http:"),
            service.url.replace("://", "://user:pw@"),
            f"{service.url}/?q=1",
            f"{service.url}/#f",
            f"{service.url}/a/../b",
            f"{service.url}/./b",
            "ftp://h",
            "https://h/%2e",
        ]:
            with self.subTest(url):
                result = self.sign(MACOS_SIGNING_SERVICE_URL=url)
                self.assert_fails_with(result, "MACOS_SIGNING_SERVICE_URL must")
                self.assertEqual(service.requests, [])

    def test_unsafe_cert_ids_are_rejected_without_a_request(self):
        for cert_id in [".", "..", "../ok", "a/b", "a%2fb", "a?b", "a#b", "-x", "a b"]:
            with self.subTest(cert_id):
                result = self.sign(STAGING_CERT_ID=cert_id)
                self.assert_fails_with(result, "STAGING_CERT_ID must only contain")
                self.assertEqual(service.requests, [])

    def test_unexpected_input_layout_is_rejected_without_a_request(self):
        bad_input = self.dir / "bad-input"
        (bad_input / "x").mkdir(parents=True)
        self.assert_fails_with(self.sign(input_dir=bad_input), "unexpected files")
        self.assertEqual(service.requests, [])

    def test_denied_token_reports_only_the_status(self):
        # The mock echoes the Authorization header in the 401 body.
        self.assert_fails_with(self.sign(STAGING_CERT_ID="denied"), "returned HTTP 401")

    def test_redirect_is_not_followed(self):
        self.assert_fails_with(self.sign(STAGING_CERT_ID="redirect"), "returned HTTP 302")
        self.assertEqual([request.path for request in service.requests],
                         ["/signer/macos/certificates/redirect"])

    def test_unreachable_service_fails(self):
        result = self.sign(MACOS_SIGNING_SERVICE_URL=closed_port_url())
        self.assert_fails_with(result, "failed (curl exit 7)")

    def test_untrusted_tls_certificate_fails(self):
        result = self.sign(CURL_CA_BUNDLE=str(Path(__file__).parents[1] / "fixtures" / "test-cert.pem"))
        self.assert_fails_with(result, "failed (curl exit 60)")

    def test_signs_every_binary_after_a_successful_preflight(self):
        result = self.sign(MACOS_SIGNING_SERVICE_URL=f"{service.url}//")

        self.assertEqual(result.returncode, 0, self.output(result))
        self.assert_token_not_printed(result)
        self.assertEqual(
            [(request.path, request.authorization) for request in service.requests],
            [("/signer/macos/certificates/ok", f"Bearer {TOKEN}")],
        )
        calls = read_stub_calls(self.stub_log)
        self.assertEqual([call["token_in_env"] for call in calls], [True, True, True])
        self.assertEqual([call["args"] for call in calls], [
            ["--cert-id", "ok", "sign", "--identifier", "com.datadoghq.macos-code-signer",
             "--code-signature-flags", "runtime", str(unsigned / t / BINARY), str(self.out / t / BINARY)]
            for t in TARGETS
        ])
        layout = run([SCRIPTS / "check-release-binaries.sh", self.out])
        self.assertEqual(layout.returncode, 0, layout.stdout + layout.stderr)


if __name__ == "__main__":
    unittest.main()
