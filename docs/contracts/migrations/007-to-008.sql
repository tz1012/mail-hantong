-- Folder observations only; no message, UID or user mapping rows are changed.
CREATE TABLE imap_mailbox_catalog_operations (
    operation_id TEXT PRIMARY KEY CHECK (length(operation_id) BETWEEN 1 AND 128 AND length(CAST(operation_id AS BLOB)) = length(operation_id) AND operation_id NOT GLOB '*[^A-Za-z0-9_-]*'),
    account_id TEXT NOT NULL REFERENCES accounts(account_id) ON DELETE RESTRICT CHECK (length(account_id) BETWEEN 1 AND 128 AND length(CAST(account_id AS BLOB)) = length(account_id) AND account_id NOT GLOB '*[^A-Za-z0-9_-]*'),
    idempotency_key TEXT NOT NULL CHECK (length(idempotency_key) BETWEEN 1 AND 128 AND length(CAST(idempotency_key AS BLOB)) = length(idempotency_key) AND idempotency_key NOT GLOB '*[^A-Za-z0-9_-]*'),
    account_version INTEGER NOT NULL CHECK (account_version >= 1),
    endpoint_version INTEGER NOT NULL CHECK (endpoint_version >= 1),
    endpoint_hash BLOB NOT NULL CHECK (length(endpoint_hash) = 32),
    snapshot_hash BLOB NOT NULL CHECK (length(snapshot_hash) = 32),
    expected_version INTEGER NOT NULL CHECK (expected_version BETWEEN 0 AND 9223372036854775806),
    result_version INTEGER NOT NULL CHECK (result_version = expected_version + 1),
    payload_hash BLOB NOT NULL CHECK (length(payload_hash) = 32),
    completed_at_utc_ms INTEGER NOT NULL CHECK (completed_at_utc_ms > 0),
    UNIQUE (account_id, idempotency_key)
) STRICT;

CREATE TABLE imap_mailbox_catalogs (
    account_id TEXT PRIMARY KEY REFERENCES accounts(account_id) ON DELETE RESTRICT CHECK (length(account_id) BETWEEN 1 AND 128 AND length(CAST(account_id AS BLOB)) = length(account_id) AND account_id NOT GLOB '*[^A-Za-z0-9_-]*'),
    account_version INTEGER NOT NULL CHECK (account_version >= 1),
    endpoint_version INTEGER NOT NULL CHECK (endpoint_version >= 1),
    endpoint_hash BLOB NOT NULL CHECK (length(endpoint_hash) = 32),
    generation INTEGER NOT NULL CHECK (generation >= 1),
    snapshot_json TEXT NOT NULL CHECK (
        length(CAST(snapshot_json AS BLOB)) BETWEEN 2 AND 8388608
        AND json_valid(snapshot_json) AND json_type(snapshot_json) = 'object'
    ),
    snapshot_hash BLOB NOT NULL CHECK (length(snapshot_hash) = 32),
    last_operation_id TEXT NOT NULL REFERENCES imap_mailbox_catalog_operations(operation_id) ON DELETE RESTRICT CHECK (length(last_operation_id) BETWEEN 1 AND 128 AND length(CAST(last_operation_id AS BLOB)) = length(last_operation_id) AND last_operation_id NOT GLOB '*[^A-Za-z0-9_-]*'),
    stored_at_utc_ms INTEGER NOT NULL CHECK (stored_at_utc_ms > 0)
) STRICT;
