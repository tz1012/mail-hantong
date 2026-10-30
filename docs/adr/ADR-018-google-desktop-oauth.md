# ADR-018 Google Desktop OAuth and Gmail read transport

Implementation decision under the user's continuing mandate; **human security review required**.

Use Desktop OAuth authorization code + PKCE S256 + loopback system-browser login. Use Gmail REST with gmail.readonly rather than IMAP's full-mail scope. Reuse current Windows DPAPI and SQLCipher boundaries and existing common tool/policy/audit coordinator. Gmail's immutable IDs remain distinct from IMAP identities. Use the already selected windows-sys WinHTTP APIs inside Secrets; add no HTTP/OAuth package dependency. Production PKCE/account SHA-256 uses Windows CNG; the portable hash is test-only for non-Windows fake-provider fixtures. A bounded native HTTPS broker keeps access/refresh tokens out of Core, clients, tool results and logs. Non-Windows product support stays unavailable.

There is no registered publisher Desktop client ID in this repository. The first-run setup accepts public client configuration and explains Google Cloud/API/consent prerequisites; do not invent a working default or run actual provider registration during development. Desktop client_secret is optional per Google's current native-app tables and is not used.

Alternatives: IMAP OAuth would reuse existing mailbox projection but requires broader https://mail.google.com/ authorization. An embedded webview or subscription-token borrowing violates the intended browser/credential boundary. A hosted OAuth relay would introduce a server, data-handling obligations and deployment that the current local product does not require.

Details and primary sources: [Google mail contract](../contracts/google-mail.md).
