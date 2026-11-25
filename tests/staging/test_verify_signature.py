# Unless explicitly stated otherwise all files in this repository are licensed under the Apache-2.0 License.
# This product includes software developed at Datadog (https://www.datadoghq.com/).
# Copyright 2026-Present Datadog, Inc.

"""Tests for scripts/verify-signature.sh.

Fixtures are self-signed and signed without a secure timestamp (no
time-stamp server is available locally), so even a correctly signed
staging tree fails the final timestamp check. Tests assert that every
slice passed the identity checks first. A fully passing run, and the
production checks passing on an Apple-issued certificate, are not covered.
"""

import shutil
import tempfile
import unittest
from pathlib import Path

from fixtures import BINARY, SCRIPTS, SLICES, STAGING_CN, Toolchain, make_certificate, run

VERIFY = SCRIPTS / "verify-signature.sh"
DATADOG_TEAM_ID = "JKFCB4CN7C"


def setUpModule():
    global work, toolchain, staging, rotated, other_cn, spoofed
    work = Path(tempfile.mkdtemp())
    toolchain = Toolchain(work)
    staging = make_certificate(work, "staging", STAGING_CN)
    rotated = make_certificate(work, "rotated", STAGING_CN)
    other_cn = make_certificate(work, "other-cn", "Other Test Signer")
    spoofed = make_certificate(work, "spoofed", STAGING_CN, organizational_unit=DATADOG_TEAM_ID)


def tearDownModule():
    shutil.rmtree(work)


class VerifySignatureTest(unittest.TestCase):
    def setUp(self):
        self.dir = Path(tempfile.mkdtemp(dir=work))

    def tree(self, aarch64=None, x86_64=None, universal_arm64=None, universal_x86_64=None):
        """Release tree signed with the staging certificate, unless a slice's certificate is given."""
        return toolchain.release_tree(
            self.dir,
            aarch64=aarch64 or staging,
            x86_64=x86_64 or staging,
            universal_arm64=universal_arm64 or staging,
            universal_x86_64=universal_x86_64 or staging,
        )

    def verify(self, tree, environment="staging", authority=STAGING_CN):
        env = {"MACOS_SIGNING_ENVIRONMENT": environment, "MACOS_SIGNING_CERT_AUTHORITY": authority}
        return run([VERIFY, tree], {key: value for key, value in env.items() if value is not None})

    def assert_fails_with(self, result, message):
        output = result.stdout + result.stderr
        self.assertNotEqual(result.returncode, 0, output)
        self.assertIn(message, output)

    def assert_identity_checks_passed_on_every_slice(self, result, tree):
        for target, arch in SLICES:
            self.assertIn(f"ok: {tree}/{target}/{BINARY} ({arch})", result.stdout)

    def test_staging_signed_tree_passes_identity_checks_and_stops_at_timestamp(self):
        tree = self.tree()
        result = self.verify(tree)
        self.assert_identity_checks_passed_on_every_slice(result, tree)
        self.assert_fails_with(result, f"{tree}/aarch64-apple-darwin/{BINARY} (arm64): no secure timestamp")

    def test_rotated_key_with_same_cn_passes_identity_checks(self):
        tree = self.tree(aarch64=rotated, universal_arm64=rotated, universal_x86_64=rotated)
        result = self.verify(tree)
        self.assert_identity_checks_passed_on_every_slice(result, tree)
        self.assert_fails_with(result, "no secure timestamp")

    def test_wrong_cn_is_rejected(self):
        result = self.verify(self.tree(aarch64=other_cn))
        self.assert_fails_with(result, f"aarch64-apple-darwin/{BINARY} (arm64): leaf authority is not {STAGING_CN}")

    def test_wrong_cn_on_universal_x86_64_slice_is_rejected(self):
        result = self.verify(self.tree(universal_x86_64=other_cn))
        self.assert_fails_with(result, f"universal-apple-darwin/{BINARY} (x86_64): leaf authority is not {STAGING_CN}")

    def test_datadog_team_id_is_rejected(self):
        tree = self.tree()
        toolchain.sign(staging, "arm64", tree / "aarch64-apple-darwin" / BINARY, team_id=DATADOG_TEAM_ID)
        result = self.verify(tree)
        self.assert_fails_with(result, "(arm64): signed with Datadog's Team ID")

    def test_missing_hardened_runtime_is_rejected(self):
        tree = self.tree()
        toolchain.sign(staging, "arm64", tree / "aarch64-apple-darwin" / BINARY, runtime=False)
        result = self.verify(tree)
        self.assert_fails_with(result, "(arm64): hardened runtime flag missing")

    def test_wrong_identifier_is_rejected(self):
        tree = self.tree()
        toolchain.sign(staging, "arm64", tree / "aarch64-apple-darwin" / BINARY, identifier="com.example.other")
        result = self.verify(tree)
        self.assert_fails_with(result, "(arm64): unexpected identifier")

    def test_modified_binary_is_rejected(self):
        tree = self.tree()
        with open(tree / "x86_64-apple-darwin" / BINARY, "r+b") as binary:
            binary.seek(4100)
            binary.write(b"x")
        result = self.verify(tree)
        self.assert_fails_with(result, f"x86_64-apple-darwin/{BINARY} (x86_64): invalid signature")

    def test_missing_binary_is_rejected(self):
        tree = self.tree()
        (tree / "x86_64-apple-darwin" / BINARY).unlink()
        result = self.verify(tree)
        self.assert_fails_with(result, "unexpected files")

    def test_unset_authority_is_rejected_for_staging(self):
        result = self.verify(self.tree(), authority=None)
        self.assert_fails_with(result, "MACOS_SIGNING_CERT_AUTHORITY must be set for staging")

    def test_missing_or_unknown_environment_is_rejected(self):
        for environment in [None, "", "prod", "Production"]:
            with self.subTest(environment):
                result = self.verify(self.tree(), environment=environment)
                self.assert_fails_with(result, "MACOS_SIGNING_ENVIRONMENT must be staging or production")

    def test_production_rejects_another_team_id(self):
        result = self.verify(self.tree(), environment="production", authority=None)
        self.assert_fails_with(result, f"aarch64-apple-darwin/{BINARY} (arm64): not signed with Datadog's Team ID")

    def test_production_rejects_a_self_signed_certificate_with_datadog_team_id(self):
        tree = self.tree(aarch64=spoofed)
        toolchain.sign(spoofed, "arm64", tree / "aarch64-apple-darwin" / BINARY, team_id=DATADOG_TEAM_ID)
        result = self.verify(tree, environment="production", authority=None)
        self.assert_fails_with(
            result,
            f"aarch64-apple-darwin/{BINARY} (arm64): not signed with an Apple-issued Developer ID certificate",
        )

    def test_production_checks_the_authority_when_set(self):
        result = self.verify(self.tree(), environment="production", authority="Other Test Signer")
        self.assert_fails_with(result, f"aarch64-apple-darwin/{BINARY} (arm64): leaf authority is not Other Test Signer")


if __name__ == "__main__":
    unittest.main()
