-- v12 to v13: immutable base rows remain unchanged.
CREATE TABLE imap_uid_reconcile_heads (
    account_id TEXT NOT NULL CHECK(length(account_id) BETWEEN 1 AND 128 AND length(CAST(account_id AS BLOB))=length(account_id) AND account_id NOT GLOB '*[^A-Za-z0-9_-]*'),
    mailbox_key TEXT NOT NULL CHECK(length(mailbox_key) BETWEEN 2 AND 2048 AND length(mailbox_key)%2=0 AND mailbox_key NOT GLOB '*[^0-9a-f]*'),
    mailbox_encoding TEXT NOT NULL CHECK(mailbox_encoding IN ('modified_utf7','utf8')),
    generation INTEGER NOT NULL CHECK(generation > 0),
    version INTEGER NOT NULL CHECK(version > 0),
    frozen_cursor_version INTEGER NOT NULL CHECK(frozen_cursor_version > 0),
    last_source_cursor_version INTEGER NOT NULL CHECK(last_source_cursor_version >= 0 AND last_source_cursor_version <= frozen_cursor_version),
    phase TEXT NOT NULL CHECK(phase IN ('open','complete')),
    uid_validity INTEGER NOT NULL CHECK(uid_validity BETWEEN 1 AND 4294967295),
    endpoint_version INTEGER NOT NULL CHECK(endpoint_version > 0),
    endpoint_hash BLOB NOT NULL CHECK(length(endpoint_hash)=32),
    updated_at_utc_ms INTEGER NOT NULL CHECK(updated_at_utc_ms > 0),
    last_execution_id BLOB REFERENCES imap_uid_reconcile_requests(execution_id) ON DELETE RESTRICT,
    PRIMARY KEY(account_id,mailbox_key,mailbox_encoding),
    FOREIGN KEY(account_id,mailbox_key,mailbox_encoding) REFERENCES imap_uid_cursors(account_id,mailbox_key,mailbox_encoding) ON DELETE RESTRICT,
    CHECK((last_source_cursor_version=0 AND last_execution_id IS NULL AND phase='open') OR (last_source_cursor_version>0 AND last_execution_id IS NOT NULL AND length(last_execution_id)=32 AND last_execution_id<>zeroblob(32)))
) STRICT;
CREATE TABLE imap_uid_reconcile_requests (
    execution_id BLOB PRIMARY KEY REFERENCES audit_starts(execution_id) ON DELETE RESTRICT CHECK(length(execution_id)=32 AND execution_id<>zeroblob(32)),
    operation_id TEXT NOT NULL UNIQUE CHECK(length(operation_id) BETWEEN 1 AND 128 AND length(CAST(operation_id AS BLOB))=length(operation_id) AND operation_id NOT GLOB '*[^A-Za-z0-9_-]*'),
    account_id TEXT NOT NULL REFERENCES accounts(account_id) ON DELETE RESTRICT CHECK(length(account_id) BETWEEN 1 AND 128 AND length(CAST(account_id AS BLOB))=length(account_id) AND account_id NOT GLOB '*[^A-Za-z0-9_-]*'),
    idempotency_key TEXT NOT NULL CHECK(length(idempotency_key) BETWEEN 1 AND 128 AND length(CAST(idempotency_key AS BLOB))=length(idempotency_key) AND idempotency_key NOT GLOB '*[^A-Za-z0-9_-]*'),
    mailbox_key TEXT NOT NULL CHECK(length(mailbox_key) BETWEEN 2 AND 2048 AND length(mailbox_key)%2=0 AND mailbox_key NOT GLOB '*[^0-9a-f]*'),
    mailbox_encoding TEXT NOT NULL CHECK(mailbox_encoding IN ('modified_utf7','utf8')),
    intent_hash BLOB NOT NULL CHECK(length(intent_hash)=32),
    expected_catalog_generation INTEGER NOT NULL CHECK(expected_catalog_generation>0),
    expected_cursor_version INTEGER NOT NULL CHECK(expected_cursor_version BETWEEN 1 AND 9223372036854775806),
    bound_account_version INTEGER NOT NULL CHECK(bound_account_version>0),
    bound_endpoint_version INTEGER NOT NULL CHECK(bound_endpoint_version>0),
    bound_endpoint_hash BLOB NOT NULL CHECK(length(bound_endpoint_hash)=32),
    uid_validity INTEGER NOT NULL CHECK(uid_validity BETWEEN 1 AND 4294967295),
    minimum_uid_next INTEGER NOT NULL CHECK(minimum_uid_next BETWEEN 1 AND 4294967295),
    generation INTEGER NOT NULL CHECK(generation>0),
    head_version INTEGER NOT NULL CHECK(head_version>0),
    frozen_cursor_version INTEGER NOT NULL CHECK(frozen_cursor_version>0 AND frozen_cursor_version<=expected_cursor_version),
    source_operation_id TEXT NOT NULL REFERENCES imap_uid_page_operations(operation_id) ON DELETE RESTRICT CHECK(length(source_operation_id) BETWEEN 1 AND 128 AND length(CAST(source_operation_id AS BLOB))=length(source_operation_id) AND source_operation_id NOT GLOB '*[^A-Za-z0-9_-]*'),
    source_cursor_version INTEGER NOT NULL CHECK(source_cursor_version>0 AND source_cursor_version<=frozen_cursor_version),
    source_page_hash BLOB NOT NULL CHECK(length(source_page_hash)=32),
    after_uid INTEGER NOT NULL CHECK(after_uid BETWEEN 0 AND 4294967293),
    through_uid INTEGER NOT NULL CHECK(through_uid>after_uid AND through_uid<4294967295 AND through_uid-after_uid<=64 AND through_uid<minimum_uid_next),
    state TEXT NOT NULL CHECK(state IN ('admitted','running','succeeded','failed')),
    admitted_at_utc_ms INTEGER NOT NULL CHECK(admitted_at_utc_ms>0),
    started_at_utc_ms INTEGER CHECK(started_at_utc_ms>=admitted_at_utc_ms),
    completed_at_utc_ms INTEGER CHECK(completed_at_utc_ms>=coalesce(started_at_utc_ms,admitted_at_utc_ms)),
    result_json TEXT CHECK(length(CAST(result_json AS BLOB))<=1048576),
    result_hash BLOB CHECK(length(result_hash)=32),
    error_code TEXT CHECK (error_code IS NULL OR error_code IN ('INVALID_INPUT','AUTH_REQUIRED','SCOPE_DENIED','POLICY_BLOCKED','CONFLICT','RATE_LIMITED','OFFLINE','NOT_FOUND','SEND_UNKNOWN','APPROVAL_REQUIRED','APPROVAL_EXPIRED','APPROVAL_DENIED','APPROVAL_RATE_LIMITED','CLIENT_NOT_REGISTERED','CLIENT_SUSPENDED','HUMAN_STEP_REQUIRED','CORE_UNAVAILABLE','VERSION_MISMATCH','CURSOR_EXPIRED','CAPABILITY_UNSUPPORTED','QUOTA_EXCEEDED','INTERNAL_ERROR')),
    UNIQUE(account_id,idempotency_key),
    FOREIGN KEY(account_id,mailbox_key,mailbox_encoding) REFERENCES imap_uid_reconcile_heads(account_id,mailbox_key,mailbox_encoding) ON DELETE RESTRICT,
    CHECK((state='admitted' AND started_at_utc_ms IS NULL AND completed_at_utc_ms IS NULL AND result_json IS NULL AND result_hash IS NULL AND error_code IS NULL)
       OR (state='running' AND started_at_utc_ms IS NOT NULL AND completed_at_utc_ms IS NULL AND result_json IS NULL AND result_hash IS NULL AND error_code IS NULL)
       OR (state='succeeded' AND started_at_utc_ms IS NOT NULL AND completed_at_utc_ms IS NOT NULL AND result_json IS NOT NULL AND result_hash IS NOT NULL AND error_code IS NULL)
       OR (state='failed' AND completed_at_utc_ms IS NOT NULL AND result_json IS NULL AND result_hash IS NULL AND error_code IS NOT NULL))
) STRICT;
CREATE INDEX imap_uid_reconcile_source_latest ON imap_uid_reconcile_requests(source_operation_id,state,generation DESC);
CREATE TABLE imap_uid_reconcile_messages (
    account_id TEXT NOT NULL CHECK(length(account_id) BETWEEN 1 AND 128 AND length(CAST(account_id AS BLOB))=length(account_id) AND account_id NOT GLOB '*[^A-Za-z0-9_-]*'),
    mailbox_key TEXT NOT NULL CHECK(length(mailbox_key) BETWEEN 2 AND 2048 AND length(mailbox_key)%2=0 AND mailbox_key NOT GLOB '*[^0-9a-f]*'),
    mailbox_encoding TEXT NOT NULL CHECK(mailbox_encoding IN ('modified_utf7','utf8')),
    uid_validity INTEGER NOT NULL CHECK(uid_validity BETWEEN 1 AND 4294967295),
    uid INTEGER NOT NULL CHECK(uid BETWEEN 1 AND 4294967295),
    presence TEXT NOT NULL CHECK(presence IN ('present','remote_absent')),
    flags_json TEXT CHECK(length(CAST(flags_json AS BLOB))<=16384),
    generation INTEGER NOT NULL CHECK(generation>0),
    observed_at_utc_ms INTEGER NOT NULL CHECK(observed_at_utc_ms>0),
    last_execution_id BLOB NOT NULL REFERENCES imap_uid_reconcile_requests(execution_id) ON DELETE RESTRICT CHECK(length(last_execution_id)=32 AND last_execution_id<>zeroblob(32)),
    PRIMARY KEY(account_id,mailbox_key,mailbox_encoding,uid_validity,uid),
    FOREIGN KEY(account_id,mailbox_key,mailbox_encoding,uid_validity,uid) REFERENCES imap_uid_messages(account_id,mailbox_key,mailbox_encoding,uid_validity,uid) ON DELETE RESTRICT,
    CHECK((presence='present' AND flags_json IS NOT NULL) OR (presence='remote_absent' AND flags_json IS NULL))
) STRICT;
CREATE INDEX imap_uid_reconcile_messages_execution ON imap_uid_reconcile_messages(last_execution_id,uid);
