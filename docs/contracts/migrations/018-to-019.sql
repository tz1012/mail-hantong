-- Additive Gmail read cache and durable, audited sync ledger. No identity,
-- credential, client grant, AI consent, or pending network work is created.
CREATE TABLE google_connections (
    account_id TEXT PRIMARY KEY REFERENCES accounts(account_id) ON DELETE RESTRICT,
    subject TEXT NOT NULL UNIQUE CHECK(length(subject) BETWEEN 1 AND 255),
    client_id TEXT NOT NULL CHECK(length(client_id) BETWEEN 1 AND 256),
    credential_version INTEGER NOT NULL CHECK(credential_version >= 1),
    updated_at_utc_ms INTEGER NOT NULL CHECK(updated_at_utc_ms > 0)
) STRICT;
CREATE TABLE google_labels (
    account_id TEXT NOT NULL REFERENCES google_connections(account_id) ON DELETE RESTRICT,
    label_id TEXT NOT NULL CHECK(length(label_id) BETWEEN 1 AND 256),
    name TEXT NOT NULL CHECK(length(CAST(name AS BLOB)) <= 1024),
    kind TEXT NOT NULL CHECK(kind IN ('system','user')),
    PRIMARY KEY(account_id,label_id)
) STRICT;
CREATE TABLE google_messages (
    account_id TEXT NOT NULL REFERENCES google_connections(account_id) ON DELETE RESTRICT,
    message_id TEXT NOT NULL CHECK(length(message_id) BETWEEN 1 AND 256),
    internal_date_ms INTEGER NOT NULL CHECK(internal_date_ms >= 0),
    message_json TEXT NOT NULL CHECK(json_valid(message_json) AND json_type(message_json)='object'),
    PRIMARY KEY(account_id,message_id)
) STRICT;
CREATE INDEX google_messages_newest_idx ON google_messages(account_id,internal_date_ms DESC,message_id DESC);
CREATE TABLE google_sync_state (
    account_id TEXT PRIMARY KEY REFERENCES google_connections(account_id) ON DELETE RESTRICT,
    version INTEGER NOT NULL CHECK(version >= 0),
    next_page_token TEXT CHECK(next_page_token IS NULL OR length(CAST(next_page_token AS BLOB)) BETWEEN 1 AND 2048),
    label_id TEXT CHECK(label_id IS NULL OR length(label_id) BETWEEN 1 AND 256)
) STRICT;
CREATE TABLE google_sync_requests (
    execution_id BLOB PRIMARY KEY REFERENCES audit_starts(execution_id) ON DELETE RESTRICT CHECK(length(execution_id)=32),
    owner BLOB NOT NULL REFERENCES client_registrations(registration_id) ON DELETE RESTRICT CHECK(length(owner)=16),
    profile_epoch INTEGER NOT NULL CHECK(profile_epoch >= 1),
    account_id TEXT NOT NULL REFERENCES google_connections(account_id) ON DELETE RESTRICT,
    operation_id TEXT NOT NULL CHECK(length(operation_id) BETWEEN 1 AND 256),
    idempotency_key TEXT NOT NULL CHECK(length(idempotency_key) BETWEEN 1 AND 256),
    request_hash BLOB NOT NULL CHECK(length(request_hash)=32),
    request_json TEXT NOT NULL CHECK(json_valid(request_json) AND json_type(request_json)='object'),
    binding_version INTEGER NOT NULL CHECK(binding_version >= 1),
    account_version INTEGER NOT NULL CHECK(account_version >= 1),
    state TEXT NOT NULL CHECK(state IN ('pending','running','completed','failed','interrupted')),
    result_version INTEGER NOT NULL CHECK(result_version >= 0),
    next_page_token TEXT CHECK(next_page_token IS NULL OR length(CAST(next_page_token AS BLOB)) BETWEEN 1 AND 2048),
    downloaded_count INTEGER NOT NULL CHECK(downloaded_count BETWEEN 0 AND 32),
    error_code TEXT CHECK(error_code IS NULL OR error_code IN (
        'INVALID_INPUT','AUTH_REQUIRED','SCOPE_DENIED','POLICY_BLOCKED','CONFLICT',
        'RATE_LIMITED','OFFLINE','NOT_FOUND','SEND_UNKNOWN','APPROVAL_REQUIRED',
        'APPROVAL_EXPIRED','APPROVAL_DENIED','APPROVAL_RATE_LIMITED','CLIENT_NOT_REGISTERED',
        'CLIENT_SUSPENDED','HUMAN_STEP_REQUIRED','CORE_UNAVAILABLE','VERSION_MISMATCH',
        'CURSOR_EXPIRED','CAPABILITY_UNSUPPORTED','QUOTA_EXCEEDED','INTERNAL_ERROR'
    )),
    admitted_at_utc_ms INTEGER NOT NULL CHECK(admitted_at_utc_ms > 0),
    completed_at_utc_ms INTEGER CHECK(completed_at_utc_ms IS NULL OR completed_at_utc_ms >= admitted_at_utc_ms),
    UNIQUE(account_id,operation_id),
    UNIQUE(account_id,idempotency_key),
    CHECK((state IN ('pending','running') AND completed_at_utc_ms IS NULL AND error_code IS NULL AND downloaded_count=0)
       OR (state='completed' AND completed_at_utc_ms IS NOT NULL AND error_code IS NULL)
       OR (state IN ('failed','interrupted') AND completed_at_utc_ms IS NOT NULL AND error_code IS NOT NULL AND downloaded_count=0))
) STRICT;
CREATE UNIQUE INDEX google_sync_one_pending_idx ON google_sync_requests(account_id) WHERE state IN ('pending','running');
