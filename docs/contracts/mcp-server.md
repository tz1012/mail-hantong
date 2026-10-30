# MCP stdio server

Status: contract draft; human security review required. Requirements MCP-03, CLI-15, SEC-03, AI-09.

`hantong-mcp` is a newline-delimited UTF-8 JSON-RPC stdio server, using MCP 2025-11-25 initialization, tools/list, tools/call and ping. It writes only protocol messages to stdout. Frames are bounded to 1 MiB and nested duplicate JSON keys are rejected. Calls before initialization, unknown fields/tools and malformed protocol versions fail closed; a well-formed client version negotiates the server-supported 2025-11-25 version. No remote HTTP listener is opened.

Registration is explicitly requested by a person with `hantong-mcp register`, then queried with `hantong-mcp status REQUEST_ID`. These commands request the existing native Windows human verification; they never issue approval. The fixed client kind is `mcp`, label `mail-hantong MCP`, profile `mcp.json`, and independent registration-derived CNG key. CLI/default and GUI profiles are never used as fallbacks. Native review uses an MCP-specific proof domain and names the AI agent, exact accounts and tools in English/Korean. Pending/provisioning recovery follows the existing native registration transaction and protected profile publication.

The frozen legacy read-only MCP scope is mail.accounts.list, mail.folders.list, mail.messages.search. New registrations use the separately reviewed full-mail v2 flow below. tools/list authenticates the MCP profile and returns only currently granted implemented tools, with self-contained input schemas. It does not return account IDs or grant authority. Tools/call creates one mutually authenticated Core connection, retains protected profile handles, verifies requested scope and routes the ordinary tool envelope through Core policy, rate limiting and audit. Unknown and ungranted tools are refused. Mail results remain untrusted data. No tool grants approval or changes policy. Body/send request tools are advertised only for explicitly reviewed full-mail grants; direct send execution is never advertised.

No actual mail accounts, Core user database, Hello or provider networks are used in automatic tests. This contract is not a human security approval.

Sources: https://modelcontextprotocol.io/specification/2025-11-25/basic/transports and https://modelcontextprotocol.io/specification/2025-11-25/server/tools

Version negotiation: https://modelcontextprotocol.io/specification/2025-11-25/basic/lifecycle

MCP mail.accounts.list accepts only optional include_capabilities. The backend injects the authenticated MCP profile account scope; empty scope is SCOPE_DENIED and never means all accounts. This permits authorized account discovery without requiring an agent to guess account IDs. Other tools require explicit account IDs from these results.

## Explicit external-AI account consent (AI-07)

Provider login is independent of mail disclosure. Native MCP registration review explicitly discloses that the listed account data (legacy metadata, or full-mail v2 bodies and mail operations as displayed) may reach the connected external AI model. On native-confirmed MCP activation, Store inserts exact registration/account/epoch/request/approved-time consent rows in the SAME transaction as activation. No migration, startup, profile parse or pending/staged request creates consent. No global policy flag is enabled.

Store `check_mcp_ai_consent(registration_id, expected_epoch, account_ids)` reads strict current registration and consent in one read snapshot. It requires active MCP identity, current epoch and explicit nonempty permitted accounts. Missing consent is POLICY_BLOCKED; empty scope is SCOPE_DENIED; stale epoch is AUTH_REQUIRED; suspended/revoked registrations are CLIENT_SUSPENDED. Corrupt request/time/account provenance is INTERNAL_ERROR. Core checks this policy before MCP account/header/body output, in addition to ordinary authentication/scope/rate/audit policy. Deleting one account consent denies only that account. Existing profiles without consent remain blocked until a new explicit authorized consent flow; no auto-grant.
# Explicit mail capability registration v2 (implementation draft)

New requests use a distinct v2 native-review domain and the exact full mail set:
accounts.list, folders.list, folders.refresh, sync.page, messages.search,
messages.get, drafts.create, send.request, send.execute. Tool IDs have their
`mail.` prefix. The registration is writable, but direct send.execute RPC stays
blocked and is not advertised: only the native coordinator may execute a send
after consuming protected human verification. Tools/list advertises the eight
actual request/read operations intersected with current authenticated grants.

Legacy metadata read3 constructors, profile bytes, native proof domain, and
grants retain their meaning. An update or profile parse grants nothing. Explicit
register on an active legacy MCP identity asks for a new native review with full
account disclosure, body access, local drafts, refresh/sync, and outgoing review
requests. Successful staging atomically revokes the exact old read3 identity
and provisions a new request-derived registration and CNG key. Historical
records remain for audit and consent provenance. The fixed mcp.json replacement
compares exact old/new bytes through the existing protected ACL and backup
publication protocol. Activation creates only the new identity's account
consents. A publication failure leaves the new identity provisioning and both
identities unable to authenticate; startup may finish publication/activation
from the already verified staged record without prompting or widening grants.
No CLI profile, key, process, or approval issuance is reused by MCP.


The MCP draft input omits payload_hash. The adapter computes the canonical compose SHA256 locally, forwards the ordinary complete Core request, and Core validates it again. This calculation creates no approval.
