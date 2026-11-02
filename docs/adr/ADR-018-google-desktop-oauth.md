# ADR-018 Google Desktop OAuth and Gmail read transport

Implementation decision under the user's continuing mandate; **human security review required**.

2026-11-02 explicit owner authorization: the user approved including publisher
Desktop client configuration in the installation data asset and recovering the
existing account store to the stable user location. This supersedes the earlier
"No publisher secret is shipped" prohibition for that Desktop asset only.
Actual account tokens, user passwords and web/server credentials remain excluded
from source, logs, tests, commits and release assets. Desktop client credentials
are extractable public-client configuration, never an authentication boundary;
PKCE/state, fixed Google endpoints, browser consent and protected per-user tokens
remain mandatory. Approval is not evidence of completed human security review.

Use Desktop OAuth authorization code + PKCE S256 + loopback system-browser login. Use Gmail REST with gmail.readonly rather than IMAP's full-mail scope. Reuse current Windows DPAPI and SQLCipher boundaries and existing common tool/policy/audit coordinator. Gmail's immutable IDs remain distinct from IMAP identities. Use the already selected windows-sys WinHTTP APIs inside Secrets; add no HTTP/OAuth package dependency. Production PKCE/account SHA-256 uses Windows CNG; the portable hash is test-only for non-Windows fake-provider fixtures. A bounded native HTTPS broker keeps access/refresh tokens out of Core, clients, tool results and logs. Non-Windows product support stays unavailable.

There is no registered publisher Desktop client ID in this repository. The first-run setup accepts public client configuration and explains Google Cloud/API/consent prerequisites; do not invent a working default or run actual provider registration during development. The original no-client-secret assumption was contradicted by an authorized native interoperability test: callback validation succeeded, then the token endpoint returned HTTP 400 invalid_request/client_secret_missing. On 2026-10-31 the authorized correction adds opaque client-secret capture through the existing protected native input/Windows verification pipeline. Secrets alone sends it to the fixed Google token endpoint and stores it with the final verified account credential. PKCE, state, scopes and final account verification remain unchanged. Legacy bundles remain readable; new format-4 bundles require this version, so an older binary cannot refresh newly connected format-4 accounts. No publisher secret is shipped. [Human review required].

Alternatives: IMAP OAuth would reuse existing mailbox projection but requires broader https://mail.google.com/ authorization. An embedded webview or subscription-token borrowing violates the intended browser/credential boundary. A hosted OAuth relay would introduce a server, data-handling obligations and deployment that the current local product does not require.

Details and primary sources: [Google mail contract](../contracts/google-mail.md).


2026-11-01 multi-account UX extension (source/synthetic verification only, human security review required): reuse an opaque same-client app credential from an existing verified protected account bundle, without borrowing its account tokens. Preserve fresh PKCE and per-account approval. Interactive onboarding requests only the selected readonly account, retains existing B accounts, and can resume local approval after Google publication. Authenticated missing-access is distinct from authentication/protected-path failure; the latter never triggers registration or scope expansion. Details: [Google onboarding contract](../contracts/google-onboarding.md). No migration, hosted relay, global secret export, actual new-account authorization, or production rollout is included.
