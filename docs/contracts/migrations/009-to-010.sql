-- Rebuild the audit parent and children so the tool allow-list is published
-- atomically. Existing rows, child rows and AUTOINCREMENT high-water are kept.
CREATE TEMP TABLE audit_seq_highwater(value INTEGER NOT NULL);
INSERT INTO audit_seq_highwater(value)
SELECT COALESCE(seq, 0) FROM sqlite_sequence WHERE name = 'audit_starts';

ALTER TABLE audit_start_accounts RENAME TO audit_start_accounts_v9;
ALTER TABLE audit_terminals RENAME TO audit_terminals_v9;
ALTER TABLE audit_starts RENAME TO audit_starts_v9;

CREATE TABLE audit_starts (
    audit_seq INTEGER PRIMARY KEY AUTOINCREMENT,
    execution_id BLOB NOT NULL UNIQUE CHECK (length(execution_id) = 32),
    registration_id BLOB NOT NULL REFERENCES client_registrations(registration_id) ON DELETE RESTRICT
        CHECK (length(registration_id) = 16),
    profile_epoch INTEGER NOT NULL CHECK (profile_epoch >= 1),
    client_kind TEXT NOT NULL CHECK (client_kind IN ('cli', 'mcp', 'gui')),
    display_label TEXT NOT NULL CHECK (length(display_label) BETWEEN 1 AND 128 AND length(CAST(display_label AS BLOB)) <= 512),
    tool_code TEXT NOT NULL CHECK (tool_code IN ('mail.accounts.list', 'mail.send.execute', 'mail.folders.list', 'mail.folders.refresh')),
    started_at_utc_ms INTEGER NOT NULL CHECK (started_at_utc_ms > 0)
) STRICT;

CREATE TABLE audit_start_accounts (
    execution_id BLOB NOT NULL REFERENCES audit_starts(execution_id) ON DELETE RESTRICT,
    account_id TEXT NOT NULL CHECK (length(account_id) BETWEEN 1 AND 128 AND account_id NOT GLOB '*[^A-Za-z0-9_-]*'),
    PRIMARY KEY(execution_id, account_id)
) STRICT;

CREATE TABLE audit_terminals (
    execution_id BLOB PRIMARY KEY REFERENCES audit_starts(execution_id) ON DELETE RESTRICT
        CHECK (length(execution_id) = 32),
    completed_at_utc_ms INTEGER NOT NULL CHECK (completed_at_utc_ms > 0),
    result_kind TEXT NOT NULL CHECK (result_kind IN ('returned_ok', 'returned_error')),
    error_code TEXT CHECK (error_code IS NULL OR error_code IN (
        'INVALID_INPUT', 'AUTH_REQUIRED', 'SCOPE_DENIED', 'POLICY_BLOCKED', 'CONFLICT',
        'RATE_LIMITED', 'OFFLINE', 'NOT_FOUND', 'SEND_UNKNOWN', 'APPROVAL_REQUIRED',
        'APPROVAL_EXPIRED', 'APPROVAL_DENIED', 'APPROVAL_RATE_LIMITED', 'CLIENT_NOT_REGISTERED',
        'CLIENT_SUSPENDED', 'HUMAN_STEP_REQUIRED', 'CORE_UNAVAILABLE', 'VERSION_MISMATCH',
        'CURSOR_EXPIRED', 'CAPABILITY_UNSUPPORTED', 'QUOTA_EXCEEDED', 'INTERNAL_ERROR'
    )),
    result_count INTEGER CHECK (result_count IS NULL OR result_count >= 0),
    CHECK ((result_kind = 'returned_ok' AND error_code IS NULL)
        OR (result_kind = 'returned_error' AND error_code IS NOT NULL AND result_count IS NULL))
) STRICT;

INSERT INTO audit_starts(audit_seq,execution_id,registration_id,profile_epoch,client_kind,display_label,tool_code,started_at_utc_ms)
SELECT audit_seq,execution_id,registration_id,profile_epoch,client_kind,display_label,tool_code,started_at_utc_ms
FROM audit_starts_v9;
INSERT INTO audit_start_accounts(execution_id,account_id)
SELECT execution_id,account_id FROM audit_start_accounts_v9;
INSERT INTO audit_terminals(execution_id,completed_at_utc_ms,result_kind,error_code,result_count)
SELECT execution_id,completed_at_utc_ms,result_kind,error_code,result_count FROM audit_terminals_v9;

DROP TABLE audit_start_accounts_v9;
DROP TABLE audit_terminals_v9;
DROP TABLE audit_starts_v9;

-- INSERT ... SELECT into an empty AUTOINCREMENT table still creates seq=0.
-- Preserve absence when the original audit log has never allocated a sequence.
DELETE FROM sqlite_sequence WHERE name='audit_starts'
    AND NOT EXISTS (SELECT 1 FROM audit_seq_highwater)
    AND NOT EXISTS (SELECT 1 FROM audit_starts);
INSERT INTO sqlite_sequence(name,seq)
SELECT 'audit_starts', MAX(value) FROM audit_seq_highwater
HAVING COUNT(*) > 0 AND NOT EXISTS (SELECT 1 FROM sqlite_sequence WHERE name='audit_starts');
UPDATE sqlite_sequence SET seq = MAX(seq, COALESCE((SELECT MAX(value) FROM audit_seq_highwater), seq))
WHERE name='audit_starts';
DROP TABLE audit_seq_highwater;

CREATE TABLE folder_refresh_requests (
    execution_id BLOB PRIMARY KEY REFERENCES audit_starts(execution_id) ON DELETE RESTRICT
        CHECK (length(execution_id) = 32 AND execution_id <> zeroblob(32)),
    operation_id TEXT NOT NULL UNIQUE CHECK (length(operation_id) BETWEEN 1 AND 128 AND length(CAST(operation_id AS BLOB)) = length(operation_id) AND operation_id NOT GLOB '*[^A-Za-z0-9_-]*'),
    account_id TEXT NOT NULL REFERENCES accounts(account_id) ON DELETE RESTRICT
        CHECK (length(account_id) BETWEEN 1 AND 128 AND length(CAST(account_id AS BLOB)) = length(account_id) AND account_id NOT GLOB '*[^A-Za-z0-9_-]*'),
    idempotency_key TEXT NOT NULL CHECK (length(idempotency_key) BETWEEN 1 AND 128 AND length(CAST(idempotency_key AS BLOB)) = length(idempotency_key) AND idempotency_key NOT GLOB '*[^A-Za-z0-9_-]*'),
    intent_hash BLOB NOT NULL CHECK (length(intent_hash) = 32),
    expected_generation INTEGER NOT NULL CHECK (expected_generation BETWEEN 0 AND 9223372036854775806),
    bound_account_version INTEGER NOT NULL CHECK (bound_account_version >= 1),
    bound_endpoint_version INTEGER NOT NULL CHECK (bound_endpoint_version >= 1),
    bound_endpoint_hash BLOB NOT NULL CHECK (length(bound_endpoint_hash) = 32),
    state TEXT NOT NULL CHECK (state IN ('admitted','running','succeeded','failed')),
    admitted_at_utc_ms INTEGER NOT NULL CHECK (admitted_at_utc_ms > 0),
    started_at_utc_ms INTEGER CHECK (started_at_utc_ms IS NULL OR started_at_utc_ms >= admitted_at_utc_ms),
    completed_at_utc_ms INTEGER CHECK (completed_at_utc_ms IS NULL OR (completed_at_utc_ms >= admitted_at_utc_ms AND (started_at_utc_ms IS NULL OR completed_at_utc_ms >= started_at_utc_ms))),
    result_generation INTEGER CHECK (result_generation IS NULL OR result_generation = expected_generation + 1),
    error_code TEXT CHECK (error_code IS NULL OR error_code IN ('INVALID_INPUT','AUTH_REQUIRED','SCOPE_DENIED','POLICY_BLOCKED','CONFLICT','RATE_LIMITED','OFFLINE','NOT_FOUND','SEND_UNKNOWN','APPROVAL_REQUIRED','APPROVAL_EXPIRED','APPROVAL_DENIED','APPROVAL_RATE_LIMITED','CLIENT_NOT_REGISTERED','CLIENT_SUSPENDED','HUMAN_STEP_REQUIRED','CORE_UNAVAILABLE','VERSION_MISMATCH','CURSOR_EXPIRED','CAPABILITY_UNSUPPORTED','QUOTA_EXCEEDED','INTERNAL_ERROR')),
    catalog_operation_id TEXT REFERENCES imap_mailbox_catalog_operations(operation_id) ON DELETE RESTRICT,
    result_snapshot_hash BLOB CHECK (result_snapshot_hash IS NULL OR length(result_snapshot_hash) = 32),
    result_payload_hash BLOB CHECK (result_payload_hash IS NULL OR length(result_payload_hash) = 32),
    UNIQUE (account_id, idempotency_key),
    CHECK (
        (state = 'admitted' AND started_at_utc_ms IS NULL AND completed_at_utc_ms IS NULL AND result_generation IS NULL AND error_code IS NULL AND catalog_operation_id IS NULL AND result_snapshot_hash IS NULL AND result_payload_hash IS NULL)
        OR (state = 'running' AND started_at_utc_ms IS NOT NULL AND completed_at_utc_ms IS NULL AND result_generation IS NULL AND error_code IS NULL AND catalog_operation_id IS NULL AND result_snapshot_hash IS NULL AND result_payload_hash IS NULL)
        OR (state = 'succeeded' AND started_at_utc_ms IS NOT NULL AND completed_at_utc_ms IS NOT NULL AND result_generation IS NOT NULL AND error_code IS NULL AND catalog_operation_id IS NOT NULL AND catalog_operation_id = operation_id AND result_snapshot_hash IS NOT NULL AND result_payload_hash IS NOT NULL)
        OR (state = 'failed' AND completed_at_utc_ms IS NOT NULL AND result_generation IS NULL AND error_code IS NOT NULL AND catalog_operation_id IS NULL AND result_snapshot_hash IS NULL AND result_payload_hash IS NULL)
    )
) STRICT;
