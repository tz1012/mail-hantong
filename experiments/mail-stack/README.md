# mail-hantong mail-stack experiment

Standalone Rust 1.95 compatibility and protocol tests, outside the product workspace.
No real network or accounts are used. TLS uses in-memory Tokio duplex streams and
public upstream Rustls test certificates; AUTH uses fixed synthetic strings.

Pinned versions: io-imap 0.7.1 (client/scram), lettre 0.11.23 (Tokio/Rustls/platform
verifier/ring), mail-parser 0.11.9 (full_encoding), mail-builder 1.0.0, Tokio 1.54.1,
rustls 0.23.46, tokio-rustls 0.26.6. The direct nom 7.1.3 alloc feature corrects
imap-codec 2.0.0-alpha.9's published feature omission without a source patch.

The initial MIME-only smoke passed one test. The expanded experiment passes 40:
MIME roundtrip, bounded reads, STARTTLS and custom implicit TLS, certificate and
hostname rejection, unsupported authentication, stalls, oversized responses and
no retry after authentication rejection. The implicit path verifies TLS itself;
it does not exercise Lettre's built-in TCP wrapper or its is_encrypted state.
The later codec-only IMAP tests cover strict tags, frame/literal/capability bounds,
STARTTLS reply injection, SASL exchanges and logout ordering. Product integration
adds actual bound DPAPI credentials and stricter challenge/budget handling.

This experiment is not the sealed product broker. Later product review reproduced
pre-TLS reply injection through Lettre's retained BufReader. The product additionally
uses a STARTTLS read gate, actual DPAPI-bound credentials, exact AUTH/QUIT success
codes and a fixed whole-operation deadline. See
[the product contract](../../docs/contracts/smtp-probe.md) and product tests.
Do not copy the experiment's direct STARTTLS call as the complete product boundary.

[Fixture provenance and licenses](tests/fixtures/rustls-rsa-2048/PROVENANCE.md)
identify the pinned upstream sources and non-production test keys.

From the repository root:

```powershell
python -X utf8 scripts/cargo_native.py test --manifest-path experiments/mail-stack/Cargo.toml --target-dir D:/BuildCache/hantong-mail-stack-experiment --locked --offline
python -X utf8 scripts/cargo_native.py clippy --manifest-path experiments/mail-stack/Cargo.toml --target-dir D:/BuildCache/hantong-mail-stack-experiment --all-targets --locked --offline -- -D warnings
```

Logs: `target/imap-probe-experiment-all.log`, `target/imap-probe-experiment-clippy.log`.
These checks do not prove real provider behavior, OS trust on a live connection,
macOS execution, throughput, or end-to-end product completion.
