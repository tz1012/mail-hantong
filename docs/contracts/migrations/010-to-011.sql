-- Bounded read-only new UID observations. No existing rows are changed.
CREATE TABLE imap_uid_page_operations (
    operation_id TEXT PRIMARY KEY CHECK (length(operation_id) BETWEEN 1 AND 128 AND length(CAST(operation_id AS BLOB)) = length(operation_id) AND operation_id NOT GLOB '*[^A-Za-z0-9_-]*'),
    account_id TEXT NOT NULL REFERENCES accounts(account_id) ON DELETE RESTRICT,
    idempotency_key TEXT NOT NULL CHECK (length(idempotency_key) BETWEEN 1 AND 128 AND length(CAST(idempotency_key AS BLOB)) = length(idempotency_key) AND idempotency_key NOT GLOB '*[^A-Za-z0-9_-]*'),
    mailbox_key TEXT NOT NULL CHECK (length(mailbox_key) BETWEEN 2 AND 2048 AND length(mailbox_key) % 2 = 0 AND mailbox_key NOT GLOB '*[^0-9a-f]*'),
    mailbox_encoding TEXT NOT NULL CHECK (mailbox_encoding IN ('modified_utf7','utf8')),
    expected_cursor_version INTEGER NOT NULL CHECK (expected_cursor_version BETWEEN 0 AND 9223372036854775806),
    result_cursor_version INTEGER NOT NULL CHECK (result_cursor_version = expected_cursor_version + 1),
    account_version INTEGER NOT NULL CHECK (account_version >= 1),
    catalog_generation INTEGER NOT NULL CHECK (catalog_generation >= 1),
    endpoint_version INTEGER NOT NULL CHECK (endpoint_version >= 1),
    endpoint_hash BLOB NOT NULL CHECK (length(endpoint_hash) = 32),
    page_json TEXT NOT NULL CHECK (length(CAST(page_json AS BLOB)) BETWEEN 1 AND 8388608),
    page_hash BLOB NOT NULL CHECK (length(page_hash) = 32),
    payload_hash BLOB NOT NULL CHECK (length(payload_hash) = 32),
    uid_validity INTEGER NOT NULL CHECK (uid_validity BETWEEN 1 AND 4294967295),
    uid_next_at_start INTEGER NOT NULL CHECK (uid_next_at_start BETWEEN 1 AND 4294967295),
    after_uid INTEGER NOT NULL CHECK (after_uid BETWEEN 0 AND 4294967294),
    covered_through_uid INTEGER NOT NULL CHECK (covered_through_uid = min(after_uid + 64, uid_next_at_start - 1) AND covered_through_uid >= after_uid),
    inserted_count INTEGER NOT NULL CHECK (inserted_count BETWEEN 0 AND 64 AND inserted_count <= covered_through_uid - after_uid),
    applied_at_utc_ms INTEGER NOT NULL CHECK (applied_at_utc_ms > 0),
    UNIQUE(account_id, idempotency_key),
    UNIQUE(account_id, mailbox_key, mailbox_encoding, result_cursor_version)
) STRICT;

CREATE TABLE imap_uid_cursors (
    account_id TEXT NOT NULL REFERENCES accounts(account_id) ON DELETE RESTRICT,
    mailbox_key TEXT NOT NULL CHECK (length(mailbox_key) BETWEEN 2 AND 2048 AND length(mailbox_key) % 2 = 0 AND mailbox_key NOT GLOB '*[^0-9a-f]*'),
    mailbox_encoding TEXT NOT NULL CHECK (mailbox_encoding IN ('modified_utf7','utf8')),
    version INTEGER NOT NULL CHECK (version >= 1),
    account_version INTEGER NOT NULL CHECK (account_version >= 1),
    catalog_generation INTEGER NOT NULL CHECK (catalog_generation >= 1),
    endpoint_version INTEGER NOT NULL CHECK (endpoint_version >= 1),
    endpoint_hash BLOB NOT NULL CHECK (length(endpoint_hash) = 32),
    uid_validity INTEGER NOT NULL CHECK (uid_validity BETWEEN 1 AND 4294967295),
    uid_next_at_start INTEGER NOT NULL CHECK (uid_next_at_start BETWEEN 1 AND 4294967295),
    covered_through_uid INTEGER NOT NULL CHECK (covered_through_uid BETWEEN 0 AND 4294967294 AND covered_through_uid < uid_next_at_start),
    stored_at_utc_ms INTEGER NOT NULL CHECK (stored_at_utc_ms > 0),
    last_operation_id TEXT NOT NULL REFERENCES imap_uid_page_operations(operation_id) ON DELETE RESTRICT,
    PRIMARY KEY(account_id, mailbox_key, mailbox_encoding)
) STRICT;

CREATE TABLE imap_uid_messages (
    account_id TEXT NOT NULL,
    mailbox_key TEXT NOT NULL,
    mailbox_encoding TEXT NOT NULL,
    uid_validity INTEGER NOT NULL CHECK (uid_validity BETWEEN 1 AND 4294967295),
    uid INTEGER NOT NULL CHECK (uid BETWEEN 1 AND 4294967295),
    metadata_json TEXT NOT NULL CHECK (length(CAST(metadata_json AS BLOB)) BETWEEN 1 AND 262144),
    metadata_hash BLOB NOT NULL CHECK (length(metadata_hash) = 32),
    stored_at_utc_ms INTEGER NOT NULL CHECK (stored_at_utc_ms > 0),
    operation_id TEXT NOT NULL REFERENCES imap_uid_page_operations(operation_id) ON DELETE RESTRICT,
    PRIMARY KEY(account_id, mailbox_key, mailbox_encoding, uid_validity, uid),
    FOREIGN KEY(account_id, mailbox_key, mailbox_encoding) REFERENCES imap_uid_cursors(account_id, mailbox_key, mailbox_encoding) ON DELETE RESTRICT
) STRICT;

CREATE INDEX imap_uid_messages_operation_idx ON imap_uid_messages(operation_id, uid);

CREATE INDEX imap_uid_page_coverage_idx ON imap_uid_page_operations(account_id, mailbox_key, mailbox_encoding, uid_validity, covered_through_uid, after_uid);
