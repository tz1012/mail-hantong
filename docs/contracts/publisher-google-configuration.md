# Publisher Google Desktop configuration provisioning

Local implementation extension for the 2026-11-02 install-to-use request.
**Human security review required.** The owner explicitly approved actual Desktop
publisher configuration distribution and existing-store recovery on 2026-11-02.
ADR-018 records that narrow authorization. It does not authorize publication of
user tokens, passwords, or web/server credentials, or substitute for human review.

The implementation may support an optional release-owned data asset at
`publisher/google-desktop.json`, using Google's downloaded `installed` OAuth
client JSON shape. Only Secrets may parse the credential and construct an
opaque GoogleClientSecret. Core may obtain the public client ID and pass the
opaque credential to the existing PKCE broker. No value is accepted on CLI,
reflected in errors, displayed, logged or committed. Tests use synthetic values.

The asset must reside under the current Core's installed release root, be a
bounded regular file without reparse ancestors, and match exactly one manifest
file entry with matching size and SHA-256. Missing asset yields no configuration;
an invalid present asset fails closed. No search of PATH, user Downloads, actual
account bundles outside Secrets, environment credential values or arbitrary
CLI paths is allowed. Google auth/token URLs must match the existing fixed
endpoints. A downloaded web/server client is not a Desktop client asset.

An explicit advanced client ID retains protected native capture. Existing
verified same-client account credentials retain priority. Implicit setup without
a matching reusable/publisher credential fails with HUMAN_STEP_REQUIRED and does
not open app-secret capture. Explicit advanced client-ID setup alone retains
that capture path. An implicit first
connection may use a matching installed publisher credential, without prompting
an end user for Google Cloud configuration. Account consent, PKCE/state,
loopback validation, native account verification, local client registration and
per-account grants are unchanged. Publisher config never grants mail authority.

Packaging support must remain opt-in, handle the source as an opaque data file,
never include source paths/values in metadata/errors, and reject input from the
source tree. No real publisher config is supplied or emitted by automated tests.
A package lacking this asset must not be advertised as clean-install ready.
The owner has resolved ADR-018's distribution prohibition for Desktop app data.
Before an actual asset is used or published, validate that it is the intended
Desktop client and excludes user credentials. Desktop client credentials cannot
be kept confidential in a distributed native application. Obfuscation/encryption
with a bundled key is not a confidentiality guarantee. Secret-free Google token
exchange must not be assumed: the recorded client required client_secret.
