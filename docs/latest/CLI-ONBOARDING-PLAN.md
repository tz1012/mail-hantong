# CLI first-run implementation — 2026-11-02

Scope: implement the user-approved install → launch → connect → approve → use
flow, keeping existing uncommitted work and real-account boundaries.

1. Extend the invocation contract with `hantong init` and interactive bare launch.
   Preserve headless/help/version behavior; gate before all effects.
2. Reuse or start the verified installed sibling Core once, with a readiness
   deadline and no PATH executable selection or automatic permission changes.
3. Add a localized numbered menu, allowed account selection and existing Google
   login/access-review reuse. Keep advanced identifiers out of the normal flow.
4. Add explicit inbox selection through bounded common sync/status/list calls.
   Connection does not implicitly fetch mail. Failed/cancelled stages do not
   repeat OAuth, change authority or write mail.
5. Update installation/help/start guides; run synthetic orchestration and package
   tests, then integrated workspace checks and fix failures. Preserve/restore
   the operating Core for singleton pipe tests, using its existing hash.

Evidence and remaining constraints accumulate in PROGRESS.md. Google project
registration already has recorded real success. Production publishing is being
rechecked separately; no Console setting will be changed. A clean installation
still needs a real publisher/client configuration supplied through the existing
protected setup boundary, which must not be fabricated or extracted as a secret.
