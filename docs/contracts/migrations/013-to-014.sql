-- v13 to v14: preserve all prior namespaces, jobs, and observation bytes.
PRAGMA defer_foreign_keys=ON;
ALTER TABLE imap_uid_reconcile_heads RENAME TO imap_uid_reconcile_heads_v13;
ALTER TABLE imap_uid_reconcile_requests RENAME TO imap_uid_reconcile_requests_v13;
ALTER TABLE imap_uid_reconcile_messages RENAME TO imap_uid_reconcile_messages_v13;
DROP INDEX imap_uid_reconcile_source_latest;
DROP INDEX imap_uid_reconcile_messages_execution;
CREATE TABLE imap_uid_epochs (
    account_id TEXT NOT NULL REFERENCES accounts(account_id) ON DELETE RESTRICT,
    mailbox_key TEXT NOT NULL CHECK(length(mailbox_key) BETWEEN 2 AND 2048 AND length(mailbox_key)%2=0 AND mailbox_key NOT GLOB '*[^0-9a-f]*'),
    mailbox_encoding TEXT NOT NULL CHECK(mailbox_encoding IN ('modified_utf7','utf8')),
    uid_validity INTEGER NOT NULL CHECK(uid_validity BETWEEN 1 AND 4294967295),
    epoch_generation INTEGER NOT NULL CHECK(epoch_generation>=1),
    first_cursor_version INTEGER NOT NULL CHECK(first_cursor_version>=1),
    first_operation_id TEXT NOT NULL UNIQUE REFERENCES imap_uid_page_operations(operation_id) ON DELETE RESTRICT,
    endpoint_version INTEGER NOT NULL CHECK(endpoint_version>=1),
    endpoint_hash BLOB NOT NULL CHECK(length(endpoint_hash)=32),
    created_at_utc_ms INTEGER NOT NULL CHECK(created_at_utc_ms>0),
    last_cursor_version INTEGER CHECK(last_cursor_version IS NULL OR last_cursor_version>=first_cursor_version),
    retired_last_operation_id TEXT REFERENCES imap_uid_page_operations(operation_id) ON DELETE RESTRICT,
    retired_at_utc_ms INTEGER CHECK(retired_at_utc_ms IS NULL OR retired_at_utc_ms>=created_at_utc_ms),
    recovery_execution_id BLOB REFERENCES imap_uid_recovery_requests(execution_id) ON DELETE RESTRICT CHECK(recovery_execution_id IS NULL OR (length(recovery_execution_id)=32 AND recovery_execution_id<>zeroblob(32))),
    PRIMARY KEY(account_id,mailbox_key,mailbox_encoding,uid_validity),
    UNIQUE(account_id,mailbox_key,mailbox_encoding,epoch_generation),
    UNIQUE(account_id,mailbox_key,mailbox_encoding,first_cursor_version),
    CHECK((epoch_generation=1 AND first_cursor_version=1 AND recovery_execution_id IS NULL) OR (epoch_generation>1 AND first_cursor_version>1 AND recovery_execution_id IS NOT NULL)),
    CHECK((last_cursor_version IS NULL AND retired_last_operation_id IS NULL AND retired_at_utc_ms IS NULL) OR (last_cursor_version IS NOT NULL AND retired_last_operation_id IS NOT NULL AND retired_at_utc_ms IS NOT NULL))
) STRICT;
CREATE UNIQUE INDEX imap_uid_epochs_current ON imap_uid_epochs(account_id,mailbox_key,mailbox_encoding) WHERE last_cursor_version IS NULL;

CREATE TABLE imap_uid_recovery_requests (
    execution_id BLOB PRIMARY KEY REFERENCES audit_starts(execution_id) ON DELETE RESTRICT
        CHECK (length(execution_id) = 32 AND execution_id <> zeroblob(32)),
    operation_id TEXT NOT NULL UNIQUE CHECK (length(operation_id) BETWEEN 1 AND 128 AND length(CAST(operation_id AS BLOB)) = length(operation_id) AND operation_id NOT GLOB '*[^A-Za-z0-9_-]*'),
    account_id TEXT NOT NULL REFERENCES accounts(account_id) ON DELETE RESTRICT
        CHECK (length(account_id) BETWEEN 1 AND 128 AND length(CAST(account_id AS BLOB)) = length(account_id) AND account_id NOT GLOB '*[^A-Za-z0-9_-]*'),
    idempotency_key TEXT NOT NULL CHECK (length(idempotency_key) BETWEEN 1 AND 128 AND length(CAST(idempotency_key AS BLOB)) = length(idempotency_key) AND idempotency_key NOT GLOB '*[^A-Za-z0-9_-]*'),
    mailbox_key TEXT NOT NULL CHECK (length(mailbox_key) BETWEEN 2 AND 2048 AND length(mailbox_key) % 2 = 0 AND mailbox_key NOT GLOB '*[^0-9a-f]*'),
    mailbox_encoding TEXT NOT NULL CHECK (mailbox_encoding IN ('modified_utf7','utf8')),
    intent_hash BLOB NOT NULL CHECK (length(intent_hash) = 32),
    expected_catalog_generation INTEGER NOT NULL CHECK (expected_catalog_generation >= 1),
    expected_cursor_version INTEGER NOT NULL CHECK (expected_cursor_version BETWEEN 1 AND 9223372036854775806),
    bound_account_version INTEGER NOT NULL CHECK (bound_account_version >= 1),
    bound_endpoint_version INTEGER NOT NULL CHECK (bound_endpoint_version >= 1),
    bound_endpoint_hash BLOB NOT NULL CHECK (length(bound_endpoint_hash) = 32),
    previous_uid_validity INTEGER NOT NULL CHECK (previous_uid_validity BETWEEN 1 AND 4294967295),
    bound_epoch_generation INTEGER NOT NULL CHECK(bound_epoch_generation >= 1),
    state TEXT NOT NULL CHECK (state IN ('admitted','running','succeeded','failed')),
    admitted_at_utc_ms INTEGER NOT NULL CHECK (admitted_at_utc_ms > 0),
    started_at_utc_ms INTEGER CHECK (started_at_utc_ms IS NULL OR started_at_utc_ms >= admitted_at_utc_ms),
    completed_at_utc_ms INTEGER CHECK (completed_at_utc_ms IS NULL OR (completed_at_utc_ms >= admitted_at_utc_ms AND (started_at_utc_ms IS NULL OR completed_at_utc_ms >= started_at_utc_ms))),
    result_epoch_generation INTEGER CHECK(result_epoch_generation IS NULL OR result_epoch_generation=bound_epoch_generation+1),
    result_uid_validity INTEGER CHECK(result_uid_validity IS NULL OR (result_uid_validity BETWEEN 1 AND 4294967295 AND result_uid_validity<>previous_uid_validity)),
    result_cursor_version INTEGER CHECK (result_cursor_version IS NULL OR result_cursor_version = expected_cursor_version + 1),
    result_covered_through_uid INTEGER CHECK (result_covered_through_uid IS NULL OR (result_covered_through_uid BETWEEN 0 AND 64)),
    result_inserted_count INTEGER CHECK (result_inserted_count IS NULL OR result_inserted_count BETWEEN 0 AND 64),
    error_code TEXT CHECK (error_code IS NULL OR error_code IN ('INVALID_INPUT','AUTH_REQUIRED','SCOPE_DENIED','POLICY_BLOCKED','CONFLICT','RATE_LIMITED','OFFLINE','NOT_FOUND','SEND_UNKNOWN','APPROVAL_REQUIRED','APPROVAL_EXPIRED','APPROVAL_DENIED','APPROVAL_RATE_LIMITED','CLIENT_NOT_REGISTERED','CLIENT_SUSPENDED','HUMAN_STEP_REQUIRED','CORE_UNAVAILABLE','VERSION_MISMATCH','CURSOR_EXPIRED','CAPABILITY_UNSUPPORTED','QUOTA_EXCEEDED','INTERNAL_ERROR')),
    page_operation_id TEXT REFERENCES imap_uid_page_operations(operation_id) ON DELETE RESTRICT,
    result_page_hash BLOB CHECK (result_page_hash IS NULL OR length(result_page_hash) = 32),
    result_payload_hash BLOB CHECK (result_payload_hash IS NULL OR length(result_payload_hash) = 32),
    UNIQUE (account_id, idempotency_key),
    CHECK (
        (state = 'admitted' AND started_at_utc_ms IS NULL AND completed_at_utc_ms IS NULL AND result_epoch_generation IS NULL AND result_uid_validity IS NULL AND result_cursor_version IS NULL AND result_covered_through_uid IS NULL AND result_inserted_count IS NULL AND error_code IS NULL AND page_operation_id IS NULL AND result_page_hash IS NULL AND result_payload_hash IS NULL)
        OR (state = 'running' AND started_at_utc_ms IS NOT NULL AND completed_at_utc_ms IS NULL AND result_epoch_generation IS NULL AND result_uid_validity IS NULL AND result_cursor_version IS NULL AND result_covered_through_uid IS NULL AND result_inserted_count IS NULL AND error_code IS NULL AND page_operation_id IS NULL AND result_page_hash IS NULL AND result_payload_hash IS NULL)
        OR (state = 'succeeded' AND started_at_utc_ms IS NOT NULL AND completed_at_utc_ms IS NOT NULL AND result_epoch_generation IS NOT NULL AND result_uid_validity IS NOT NULL AND result_cursor_version IS NOT NULL AND result_covered_through_uid IS NOT NULL AND result_inserted_count IS NOT NULL AND error_code IS NULL AND page_operation_id IS NOT NULL AND page_operation_id = operation_id AND result_page_hash IS NOT NULL AND result_payload_hash IS NOT NULL)
        OR (state = 'failed' AND completed_at_utc_ms IS NOT NULL AND result_epoch_generation IS NULL AND result_uid_validity IS NULL AND result_cursor_version IS NULL AND result_covered_through_uid IS NULL AND result_inserted_count IS NULL AND error_code IS NOT NULL AND page_operation_id IS NULL AND result_page_hash IS NULL AND result_payload_hash IS NULL)
    )
) STRICT;


CREATE TABLE imap_uid_reconcile_heads (
    account_id TEXT NOT NULL CHECK(length(account_id) BETWEEN 1 AND 128 AND length(CAST(account_id AS BLOB))=length(account_id) AND account_id NOT GLOB '*[^A-Za-z0-9_-]*'),
    mailbox_key TEXT NOT NULL CHECK(length(mailbox_key) BETWEEN 2 AND 2048 AND length(mailbox_key)%2=0 AND mailbox_key NOT GLOB '*[^0-9a-f]*'),
    mailbox_encoding TEXT NOT NULL CHECK(mailbox_encoding IN ('modified_utf7','utf8')),
    generation INTEGER NOT NULL CHECK(generation > 0),
    epoch_first_cursor_version INTEGER NOT NULL CHECK(epoch_first_cursor_version>0),
    version INTEGER NOT NULL CHECK(version > 0),
    frozen_cursor_version INTEGER NOT NULL CHECK(frozen_cursor_version >= epoch_first_cursor_version),
    last_source_cursor_version INTEGER NOT NULL CHECK(last_source_cursor_version >= epoch_first_cursor_version-1 AND last_source_cursor_version <= frozen_cursor_version),
    phase TEXT NOT NULL CHECK(phase IN ('open','complete')),
    uid_validity INTEGER NOT NULL CHECK(uid_validity BETWEEN 1 AND 4294967295),
    endpoint_version INTEGER NOT NULL CHECK(endpoint_version > 0),
    endpoint_hash BLOB NOT NULL CHECK(length(endpoint_hash)=32),
    updated_at_utc_ms INTEGER NOT NULL CHECK(updated_at_utc_ms > 0),
    last_execution_id BLOB REFERENCES imap_uid_reconcile_requests(execution_id) ON DELETE RESTRICT,
    PRIMARY KEY(account_id,mailbox_key,mailbox_encoding,uid_validity),
    FOREIGN KEY(account_id,mailbox_key,mailbox_encoding,uid_validity) REFERENCES imap_uid_epochs(account_id,mailbox_key,mailbox_encoding,uid_validity) ON DELETE RESTRICT,
    CHECK((last_source_cursor_version=epoch_first_cursor_version-1 AND last_execution_id IS NULL AND phase='open') OR (last_source_cursor_version>=epoch_first_cursor_version AND last_execution_id IS NOT NULL AND length(last_execution_id)=32 AND last_execution_id<>zeroblob(32)))
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
    FOREIGN KEY(account_id,mailbox_key,mailbox_encoding,uid_validity) REFERENCES imap_uid_reconcile_heads(account_id,mailbox_key,mailbox_encoding,uid_validity) ON DELETE RESTRICT,
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

-- BACKFILL_EPOCHS --
-- Store validates frozen v13 scalar history and inserts initial epochs here.
INSERT INTO imap_uid_reconcile_heads(account_id,mailbox_key,mailbox_encoding,generation,epoch_first_cursor_version,version,frozen_cursor_version,last_source_cursor_version,phase,uid_validity,endpoint_version,endpoint_hash,updated_at_utc_ms,last_execution_id)
SELECT account_id,mailbox_key,mailbox_encoding,generation,1,version,frozen_cursor_version,last_source_cursor_version,phase,uid_validity,endpoint_version,endpoint_hash,updated_at_utc_ms,last_execution_id FROM imap_uid_reconcile_heads_v13 ORDER BY rowid;
INSERT INTO imap_uid_reconcile_requests SELECT * FROM imap_uid_reconcile_requests_v13 ORDER BY rowid;
INSERT INTO imap_uid_reconcile_messages SELECT * FROM imap_uid_reconcile_messages_v13 ORDER BY rowid;
DROP TABLE imap_uid_reconcile_messages_v13;
DROP TABLE imap_uid_reconcile_requests_v13;
DROP TABLE imap_uid_reconcile_heads_v13;
