# Rustls RSA-2048 test TLS fixture provenance

## Origin

These exact files are copied without modification from the Rustls source tree
at the immutable upstream tag `v/0.23.46`:

- <https://raw.githubusercontent.com/rustls/rustls/v/0.23.46/test-ca/rsa-2048/ca.cert>
- <https://raw.githubusercontent.com/rustls/rustls/v/0.23.46/test-ca/rsa-2048/end.fullchain>
- <https://raw.githubusercontent.com/rustls/rustls/v/0.23.46/test-ca/rsa-2048/end.key>
- <https://raw.githubusercontent.com/rustls/rustls/v/0.23.46/test-ca/README.md>

Rustls describes this directory as integration-test certificate authority,
intermediate, end-entity, and client-certificate data. This fixture was added
for mail-hantong's in-memory TLS tests on 2026-10-30.

## Integrity manifest

SHA-256 digests are over the checked-in bytes:

| File | SHA-256 |
| --- | --- |
| `ca.cert` | `ff007992ecb5f95990f9587b4ddd7902b90b32727c439b1268327a18dc02c009` |
| `end.fullchain` | `4531321b952f325f6505a77f2296f4a499ca8b233e2286870bcc60e0bee3169d` |
| `end.key` | `b8bc05c6b34043701ee34a21c92eb3299408499edaffafb6fcd60ccc8a7dc81b` |

## Safety boundary

`end.key` is a **publicly distributed upstream test private key**, not an
application credential. It is permitted only in offline/in-memory test code.
It MUST NOT be used for production TLS, shipped as a production trust anchor,
placed in a secrets store, or accepted as a real account credential.

## Certificate identity

Native Windows `certutil -dump` inspection of the checked-in PEM files found:

- Test CA subject/issuer: `CN=ponytown RSA 2048 CA`.
- Leaf subject: `CN=testserver.com`; issuer:
  `CN=ponytown RSA 2048 level 2 intermediate`.
- Leaf SANs: `testserver.com`, `second.testserver.com`, `localhost`,
  `198.51.100.1`, and `2001:db8::1`.
- Leaf and CA validity: 1975-01-01 through 4096-01-01.

Use `localhost` in in-memory client tests so hostname verification remains
enabled; do not disable hostname or certificate validation for this fixture.

## License notice

The upstream Rustls repository at this pinned tag includes Apache-2.0, MIT,
and ISC license texts. Unmodified copies are retained beside this fixture:

- `LICENSE-APACHE` — <https://raw.githubusercontent.com/rustls/rustls/v/0.23.46/LICENSE-APACHE>
- `LICENSE-MIT` — <https://raw.githubusercontent.com/rustls/rustls/v/0.23.46/LICENSE-MIT>
- `LICENSE-ISC` — <https://raw.githubusercontent.com/rustls/rustls/v/0.23.46/LICENSE-ISC>
