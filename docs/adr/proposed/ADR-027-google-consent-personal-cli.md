# ADR-027: Google consent and personal CLI feature configuration

- Status: owner-directed local implementation; human security review required.
- Date: 2026-11-02
- Requirements: ACC-02/03, CLI-15, SEC-03, SETUP-01, APR-01.

The owner explicitly requested Google as the only account approval, followed by
feature use/configuration rather than another HanTong approval. For the guided
personal CLI, replace native account/registration/scope review with live verified
OAuth authority, finite read or read/write/send selection and honest google_oauth
provenance. This extends ADR-019's native-only setup source for this flow only.
It does not turn a CLI prompt, TTY, receipt or existing account into authority.

Choose the feature before Google so its OAuth scopes are least-privileged.
Google validates account identity/consent; Secrets validates PKCE/state/scopes;
Core binds a move-only authority to the observed local peer and exact account.
Existing CNG key, pin, profile epoch, CAS and common policy/audit remain mandatory.
DB v22 records distinct source and v8 scope history while preserving old hashes.

Rejected: renaming the old approval button; fake Windows verification results;
auto-grant from account existence; requesting all provider scopes upfront;
automatic MCP/external-AI access; bypassing per-message send/mutation review.

See ../../contracts/google-personal-cli.md and
../../latest/GOOGLE-FEATURE-SELECTION-PLAN.md for scope, interfaces and verification.
