> Latest user correction (2026-11-02): [all-terminal workflows](terminal-workflows.md) supersedes native-window and Windows Hello requirements for every CLI/TUI path. Confirmation happens in the same console; provider OAuth retains its browser. Legacy descriptions below remain historical context. Security code requires human review.

# Google multi-account onboarding — 2026-11-01 implementation draft


> 2026-11-02: [Google personal CLI extension](google-personal-cli.md) supersedes native-only account/CLI setup clauses only for the bounded guided personal flow. Live verified OAuth authorizes its fixed feature configuration; feature selection alone does not. Per-send/mutation approval and advanced native setup remain. Human security review required.

ACC-01/02/03, CLI-15, SETUP-01/02, SEC-01/03/08. **Human security review required.** This work authorizes source and synthetic tests only, not real new accounts, grants, credential publication or provider calls.

## App configuration and account isolation

Reuse the opaque client secret from an already verified format-4 account bundle for the exact same Desktop client ID. Core selects only a matching stored binding; Secrets validates account/subject/client/generation and protected-file checks and returns only an opaque GoogleClientSecret. No token is exported, borrowed, refreshed or changed by reuse. New OAuth always uses fresh PKCE/state and new subject-specific tokens. Reauthorization binds to its own account; another subject is rejected. First successful connection stores the app credential as before; subsequent additions need no repeated app-secret input. Legacy bundles without an app credential still require protected capture. Corrupt/missing/mismatched protected data fails closed; no cross-client fallback. No new secret format or DB migration. Changing a Desktop client is a deliberate first-time setup again. A first connection cancelled before verified publication still discards its unsaved credential.

## Human flow

Interactive login google guides app credential (only if needed), Google consent, account verification, local CLI registration if absent, and exact readonly account access. Each human/browser stage has its own bounded 300-second deadline started only when that stage is ready; polling never renews deadlines. A live loopback listener exists before browser launch. Callback response is static bilingual HTML with no reflected input and explicit 'return to HanTong; connection is not yet complete' guidance. Token exchange still precedes native account verification; browser page is not proof of successful connection.

Resume an existing public request with login google --resume-request ID. After account publication, login google --continue-account ACCOUNT resumes local access only, without another OAuth flow or sync. Both are interactive-only and retain native approval. Errors display a fixed action appropriate to the stage; no OAuth URL/code/state/token is printed by the interactive flow. Native windows may request normal OS foreground activation, never attach input threads or bypass OS focus/Hello. No automatic sync/mail-body read is part of onboarding: report connected/access-ready and the explicit next read commands.

## Adding one readonly account

A new scope command registration.scope.google_readonly.add, CLI clients add-readonly ACCOUNT, preserves existing B scope accounts and adds exactly the selected registrable Gmail account. Initial L is accepted only when it already contains exactly that account, producing B for that account. Never enumerate all accounts into a grant; never alter existing writable scopes or downgrade them. Reject conflicting pending/staged/running targets. Existing B already including the account is a no-op conflict at Core; the interactive flow first probes that account's readonly access and skips unnecessary review. Existing CLI registration is never replaced. Native immutable snapshot review, 300-second proof, key/pin binding, grants, policy and audit remain unchanged.

## Verification

Synthetic protected-bundle reuse across two accounts/client mismatch/corruption, fake OAuth subject isolation, scoped-add pending/staged/running conflicts, interactive orchestration with injected boundaries and no writes/sync, terminal resume, deadlines and redacted error output. All existing regression checks remain required. Preserve the earlier real readonly-success result separately; it is not validation of this new UX build.

Authenticated read-access probing returns missing-access separately from authentication errors: protected-path SCOPE_DENIED must never open a grant request. No actual new-account or grant changes are authorized by this draft.
