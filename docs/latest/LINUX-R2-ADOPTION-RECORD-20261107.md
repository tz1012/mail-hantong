# Linux R2 contract adoption record

Received: 2026-11-07. This records the current user's report of the user's decision,
not an agent-observed review of future code or a fabricated reviewer conclusion.

## Exact authority and subject

Source commit: `572d3b93fc113f08ed1e672e23d40accc1b71cd2`.
Adopted subjects: ADR-033 R2, execution identity **E1–E9**, fixed delivery **D1–D6**.

The current user reports that the concrete design, one-time Python bootstrap,
verification/review-before-publication and three-account read goal were explained
and adoption was asked. The reported actual user's response is:

> 진행해

The current user explicitly instructs adoption of the full named design and
continued product integration. This includes existing OS Python3 for the first
beta29 offline transition only, OS curl prerequisites and the existing official
Pages version/full-commit prefix. It does not create or publish that prefix now.

## Effective contracts

- `docs/adr/ADR-033-linux-sealed-release-identity.md`
- `docs/contracts/linux-execution-identity-v2.md` (E1–E9 copied from fixed source)
- `docs/contracts/linux-static-pinned-delivery.md` (D1–D6 copied from fixed source)

The immutable original proposal at the source commit remains the adoption subject.
Later contract changes require their own applicable proposal/decision; this record
does not authorize an agent to silently widen guarantees or weaken checks.

## Boundaries retained

E1's initial trust and excluded guarantees remain explicit. The exact future
bootstrap payload/hash and unwritten implementation have not been reviewed.
The new full code diff requires actual human security review, independent findings
closure and mandatory same-final-commit native product tests before publication.
AI advice, this adoption and prior general publication authorization are separate
from those completion conditions. The prior 6322b9a user review confirmation is
preserved for its unchanged scope, not extended to new code.

After all gates pass, prior conditional Linux publication/Songpa-update authority
remains valid without asking the same general permission again. Execution-time
human steps and earlier denied permission changes must not be bypassed.

Final completion remains Songpa Gmail2+Daum1 each actually listing and reading mail.
Known beta29 refresh CONFLICT is not repeated. No account/vault/key/profile/mail
reset, automatic grant, send or Trash is authorized by this contract adoption.
