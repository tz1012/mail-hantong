# Receive event contract

Availability: this contract is in the beta.23 release scope. Published beta.22 does not provide receive events, the MCP resource,
or the related v3 registration scope.

HanTong receive events are metadata-only hints for a v3 locally registered
external agent. They contain an event ID, account identifier, mailbox identity
and encoding, change kind, opaque cursor, and current registration epoch. They
never contain a message body, subject, sender, recipient, attachment, or
provider token.

Tracking is opt-in per active registration and its current profile epoch through
`mail.events.configure`. The first enable operation establishes a baseline and
emits no historical events. The receive worker checks provider changes about
every 60 seconds; this is a durable feed interval, not a guarantee of immediate
delivery.
New provider observations are committed with their event record in the same
database transaction. A registration may read pending events and acknowledge
an opaque cursor; revocation or epoch changes immediately invalidate both.

Events are retained for seven days and at most 10,000 rows per account;
expiry is enforced during delivery, observation, and read-time pruning.
Cursors are 32-byte random, persistently issued capabilities bound to the
registration epoch and exact enabled account/mailbox scope. They are never
derived from a sequence number and are rejected when unissued, expired, or
when the referenced event was pruned. At most 10,000 live cursor rows are
retained per registration epoch. Expired, pruned, or foreign opaque cursors return `CURSOR_EXPIRED` before
event data is returned. `SCOPE_DENIED` is reserved for registration scope or
authorization failure. Reads are
bounded to 100 events through `mail.events.read`; acknowledgements use
`mail.events.ack`. The MCP stdio client polls the durable feed about every five
seconds while subscribed and sends only a coalesced
`notifications/resources/updated` hint. The event feed does not wake an LLM or
authorize sending, deletion, or any other mutation.


## Authenticated application RPC

These methods use the ordinary schema-version-1 ToolRequest envelope and Core
scope, policy, rate-limit and audit gates. Every request includes a nonempty,
distinct `account_ids` array equal to the current registration's exact account
scope; empty never means all accounts. No owner ID, epoch, consent, or approval
is accepted from request data.

| Method | Additional params | Required scope |
| --- | --- | --- |
| `mail.events.configure` | `enabled` boolean | `mail.events.read` |
| `mail.events.read` | `cursor` string or null, `limit` integer 1..100, `pending_only` boolean (all required) | `mail.events.read` |
| `mail.events.ack` | nonempty issued `cursor` string | `mail.events.ack` |

MCP resource reads may omit cursor/limit/pending_only; their defaults are null/100/true before the client creates this complete RPC. Unknown fields and invalid types fail before provider work. Read responses are
`{schema_version:1, events:[...], next_cursor:string|null, has_more:boolean}`.
Each event has `event_id`, `account_id`, `mailbox_key`, `mailbox_encoding`,
`change_kind`, `cursor`, and `profile_epoch`. `change_kind` is currently exactly
`mailbox_changed`. A full page may conservatively set `has_more`; hosts continue
until an empty page. Configure returns `enabled` and `tracked_accounts`; ack
returns `acknowledged:true`, both with schema_version1.

## Observation and delivery limits

Only ready Gmail and IMAP INBOX targets are observed. Gmail fingerprints at
most200 INBOX message IDs; IMAP fingerprints EXAMINE's UIDVALIDITY/UIDNEXT.
No body or header fetch is part of monitoring. These are bounded change hints,
not an exact newly received-message count: a reset, removal, or intervening
large change can alter or evade a snapshot. The host uses ordinary authorized
mail tools to fetch the current inbox after a hint. Provider errors do not
advance the observation baseline; a later successful observation can deliver
the retained change. Offline providers and rate limits can delay delivery.

A repeat subscribe does not reset pending events. Disabling and re-enabling
starts a new baseline without replaying the opt-out interval. Subscription
state is local to each MCP process; unsubscribe does not disable another
host's tracking. Durable tracking/acknowledgement belong to the shared external
MCP identity, so an ack from one host clears that identity's pending events for
all hosts. Per-host identities are outside this version.

Every individual event carries a cursor, so a consumer acknowledges only its
last processed event. Reading is not acknowledgement. Cursors survive Core
restart until expiry/pruning, scope change, or registration replacement. On
CURSOR_EXPIRED the consumer restarts at its current pending snapshot; it must
not invent a cursor or treat a failure as acknowledgement. CLI `watch` and
`notify pending` expose the same metadata for a host-owned local scheduling
hook; HanTong itself executes no arbitrary command or callback URL.
