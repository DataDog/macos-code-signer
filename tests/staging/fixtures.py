# Unless explicitly stated otherwise all files in this repository are licensed under the Apache-2.0 License.
# This product includes software developed at Datadog (https://www.datadoghq.com/).
# Copyright 2026-Present Datadog, Inc.

"""Throwaway fixtures for the staging signing script tests.

Everything is created under a temporary directory: self-signed
certificates, small Mach-O binaries signed with rcodesign, a local HTTPS
mock of the signing service and a stub signer.
"""

import http.server
import json
import os
import shutil
import ssl
import subprocess
import threading
from dataclasses import dataclass
from pathlib import Path

REPO = Path(__file__).resolve().parents[2]
SCRIPTS = REPO / "scripts"
STUB_SIGNER = Path(__file__).resolve().parent / "stub_signer.py"

IDENTIFIER = "com.datadoghq.macos-code-signer"
STAGING_CN = "Staging Test Signer"
BINARY = "release/macos-code-signer"
# (target directory, architecture) for every slice the verifier checks.
SLICES = [
    ("aarch64-apple-darwin", "arm64"),
    ("x86_64-apple-darwin", "x86_64"),
    ("universal-apple-darwin", "arm64"),
    ("universal-apple-darwin", "x86_64"),
]


def run(command, env=None):
    """Runs command with only PATH, HOME and TMPDIR from this process, plus env.

    CI variables of the same names as the scripts' inputs are never inherited.
    """
    base = {key: os.environ[key] for key in ("PATH", "HOME", "TMPDIR") if key in os.environ}
    return subprocess.run(
        [str(part) for part in command],
        env={**base, **(env or {})},
        capture_output=True,
        text=True,
        check=False,
    )


def run_checked(command, env=None):
    result = run(command, env)
    if result.returncode != 0:
        raise RuntimeError(f"{command[0]} failed:\n{result.stdout}{result.stderr}")
    return result


@dataclass
class Certificate:
    key: Path
    cert: Path


def make_certificate(directory, name, common_name):
    """Creates a self-signed code signing certificate and its key."""
    key, cert = directory / f"{name}-key.pem", directory / f"{name}-cert.pem"
    run_checked([
        "openssl", "req", "-x509", "-newkey", "rsa:2048", "-nodes", "-sha256", "-days", "2",
        "-keyout", key, "-out", cert,
        "-subj", f"/CN={common_name}/OU=STAGINGTEST/O=Example/C=US",
        "-addext", "basicConstraints=critical,CA:FALSE",
        "-addext", "keyUsage=critical,digitalSignature",
        "-addext", "extendedKeyUsage=critical,codeSigning",
    ])
    return Certificate(key, cert)


class Toolchain:
    """rcodesign and one unsigned Mach-O per architecture, built once."""

    def __init__(self, directory):
        self.directory = directory
        run_checked([SCRIPTS / "install-rcodesign.sh", directory / "bin"])
        self.rcodesign = directory / "bin" / "rcodesign"
        source = directory / "hello.c"
        source.write_text("int main(void) { return 0; }\n")
        self.unsigned = {}
        for arch in ("arm64", "x86_64"):
            self.unsigned[arch] = directory / f"hello-{arch}"
            run_checked(["cc", "-arch", arch, source, "-o", self.unsigned[arch], "-Wl,-no_adhoc_codesign"])

    def sign(self, certificate, arch, output, identifier=IDENTIFIER, runtime=True, team_id=None):
        """Writes a copy of the arch binary signed with certificate, without a timestamp."""
        shutil.copy(self.unsigned[arch], output)
        command = [
            self.rcodesign, "sign", "--pem-file", certificate.key, "--pem-file", certificate.cert,
            "--timestamp-url", "none", "--binary-identifier", identifier,
        ]
        if runtime:
            command += ["--code-signature-flags", "runtime"]
        if team_id:
            command += ["--team-name", team_id]
        run_checked(command + [output])

    def release_tree(self, directory, aarch64, x86_64, universal_arm64, universal_x86_64):
        """Creates the three-binary release layout, each slice signed with the given certificate."""
        self._place(directory, "aarch64-apple-darwin", lambda out: self.sign(aarch64, "arm64", out))
        self._place(directory, "x86_64-apple-darwin", lambda out: self.sign(x86_64, "x86_64", out))
        arm64_slice, x86_64_slice = directory / "slice-arm64", directory / "slice-x86_64"
        self.sign(universal_arm64, "arm64", arm64_slice)
        self.sign(universal_x86_64, "x86_64", x86_64_slice)
        self._place(directory, "universal-apple-darwin",
                    lambda out: run_checked(["lipo", "-create", "-output", out, arm64_slice, x86_64_slice]))
        arm64_slice.unlink()
        x86_64_slice.unlink()
        return directory

    def unsigned_tree(self, directory):
        """Creates the three-binary release layout, unsigned."""
        self._place(directory, "aarch64-apple-darwin", lambda out: shutil.copy(self.unsigned["arm64"], out))
        self._place(directory, "x86_64-apple-darwin", lambda out: shutil.copy(self.unsigned["x86_64"], out))
        self._place(directory, "universal-apple-darwin", lambda out: run_checked(
            ["lipo", "-create", "-output", out, self.unsigned["arm64"], self.unsigned["x86_64"]]))
        return directory

    @staticmethod
    def _place(directory, target, write):
        output = directory / target / BINARY
        output.parent.mkdir(parents=True, exist_ok=True)
        write(output)


@dataclass
class Request:
    path: str
    authorization: str


class MockSigningService:
    """Local HTTPS server answering GET /signer/macos/certificates/<id>.

    The response depends on the certificate ID: "denied" gets a 401 whose
    body echoes the Authorization header, "redirect" gets a 302 to "ok",
    anything else gets a 200.
    """

    def __init__(self, directory):
        self.ca_bundle = directory / "tls-cert.pem"
        key = directory / "tls-key.pem"
        run_checked([
            "openssl", "req", "-x509", "-newkey", "rsa:2048", "-nodes", "-sha256", "-days", "2",
            "-keyout", key, "-out", self.ca_bundle, "-subj", "/CN=127.0.0.1",
            "-addext", "subjectAltName=IP:127.0.0.1",
        ])
        self.requests = []
        service = self

        class Handler(http.server.BaseHTTPRequestHandler):
            def do_GET(self):
                service.requests.append(Request(self.path, self.headers.get("Authorization", "")))
                cert_id = self.path.rsplit("/", 1)[-1]
                if cert_id == "denied":
                    self.send_response(401)
                    self.end_headers()
                    self.wfile.write(f"denied: {self.headers.get('Authorization')}".encode())
                elif cert_id == "redirect":
                    self.send_response(302)
                    self.send_header("Location", f"{service.url}/signer/macos/certificates/ok")
                    self.end_headers()
                else:
                    self.send_response(200)
                    self.end_headers()
                    self.wfile.write(b"{}")

            def log_message(self, *args):
                pass

        self._server = http.server.HTTPServer(("127.0.0.1", 0), Handler)
        context = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
        context.load_cert_chain(self.ca_bundle, key)
        self._server.socket = context.wrap_socket(self._server.socket, server_side=True)
        self.url = f"https://127.0.0.1:{self._server.server_address[1]}"
        threading.Thread(target=self._server.serve_forever, daemon=True).start()

    def stop(self):
        self._server.shutdown()
        self._server.server_close()


def read_stub_calls(log):
    """Returns the stub signer's recorded calls: [{"args": [...], "token_in_env": bool}]."""
    if not log.exists():
        return []
    return [json.loads(line) for line in log.read_text().splitlines()]
