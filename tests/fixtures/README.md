# Test fixtures

Throwaway self-signed RSA key pairs. They are not trusted by anything and only exist for tests.

- `test-cert.pem`/`test-key.pem`: plain self-signed certificate with no extensions. Used by unit tests.
- `e2e-developer-id-cert.pem`/`e2e-developer-id-key.pem`: self-signed certificate shaped like a Developer ID Application certificate (code signing extended key usage). Served by the mock service in E2E tests. Valid until October 2036.

To regenerate the E2E pair, from this directory:

```
openssl req -x509 -newkey rsa:2048 -nodes -sha256 -days 3650 \
  -keyout e2e-developer-id-key.pem -out e2e-developer-id-cert.pem \
  -subj "/CN=Developer ID Application: Example (E2ETEST123)/OU=E2ETEST123/O=Example/C=US" \
  -addext "basicConstraints=critical,CA:FALSE" \
  -addext "keyUsage=critical,digitalSignature" \
  -addext "extendedKeyUsage=critical,codeSigning"
```
