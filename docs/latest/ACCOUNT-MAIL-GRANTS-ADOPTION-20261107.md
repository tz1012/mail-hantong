# Full-mail account-grant implementation authorization

The user required every supported account to offer reading, composing/sending and
Trash, including generic IMAP/SMTP, and rejected a same-toolset-only final UX.
After receiving the per-account grant / app+fresh-OAuth proposal, the user said:

> 변경 진행해

Adopt the bounded implementation scope in `../contracts/account-mail-grants-v2.md`
and proceed locally. Baseline is c22e1bf1507430fd88e3fa1625d96ed09b2e3c40 in a
separate worktree. No new dependency, real-account operation, credential creation,
automatic grant, send/Trash operation or publication is authorized by this record.
This is not human security code-review completion. Preserve original c22 review
assets and its separate release gates. Unknown provider capabilities cannot be
reported as full support. New code needs exact-target review and native verification.
