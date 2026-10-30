-- v15 to v16: retain exact consent, audit and UID histories.

PRAGMA defer_foreign_keys=ON;

CREATE TEMP TABLE audit_seq_highwater_v16(value INTEGER NOT NULL);

INSERT INTO audit_seq_highwater_v16 SELECT seq FROM sqlite_sequence WHERE name='audit_starts';

ALTER TABLE audit_starts RENAME TO audit_starts_v15;

ALTER TABLE audit_start_accounts RENAME TO audit_start_accounts_v15;

ALTER TABLE audit_terminals RENAME TO audit_terminals_v15;

ALTER TABLE folder_refresh_requests RENAME TO folder_refresh_requests_v15;

ALTER TABLE imap_uid_page_requests RENAME TO imap_uid_page_requests_v15;

ALTER TABLE imap_uid_recovery_requests RENAME TO imap_uid_recovery_requests_v15;

ALTER TABLE imap_uid_epochs RENAME TO imap_uid_epochs_v15;

ALTER TABLE imap_uid_reconcile_heads RENAME TO imap_uid_reconcile_heads_v15;

ALTER TABLE imap_uid_reconcile_requests RENAME TO imap_uid_reconcile_requests_v15;

ALTER TABLE imap_uid_reconcile_messages RENAME TO imap_uid_reconcile_messages_v15;

ALTER TABLE client_scope_refreshes RENAME TO client_scope_refreshes_v15;

DROP INDEX imap_uid_epochs_current;

DROP INDEX imap_uid_reconcile_source_latest;

DROP INDEX imap_uid_reconcile_messages_execution;

DROP INDEX client_scope_refresh_pending_uq;

DROP INDEX client_scope_refresh_epoch_uq;

CREATE TABLE audit_starts (
    audit_seq INTEGER PRIMARY KEY AUTOINCREMENT,
    execution_id BLOB NOT NULL UNIQUE CHECK (length(execution_id) = 32),
    registration_id BLOB NOT NULL REFERENCES client_registrations(registration_id) ON DELETE RESTRICT
        CHECK (length(registration_id) = 16),
    profile_epoch INTEGER NOT NULL CHECK (profile_epoch >= 1),
    client_kind TEXT NOT NULL CHECK (client_kind IN ('cli', 'mcp', 'gui')),
    display_label TEXT NOT NULL CHECK (length(display_label) BETWEEN 1 AND 128 AND length(CAST(display_label AS BLOB)) <= 512),
    tool_code TEXT NOT NULL CHECK (tool_code IN ('mail.accounts.list', 'mail.send.execute', 'mail.folders.list', 'mail.folders.refresh', 'mail.sync.page', 'mail.messages.search', 'app.settings.display.get', 'app.diagnostics.snapshot')),
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

CREATE TABLE imap_uid_page_requests (
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
    expected_cursor_version INTEGER NOT NULL CHECK (expected_cursor_version BETWEEN 0 AND 9223372036854775806),
    bound_account_version INTEGER NOT NULL CHECK (bound_account_version >= 1),
    bound_endpoint_version INTEGER NOT NULL CHECK (bound_endpoint_version >= 1),
    bound_endpoint_hash BLOB NOT NULL CHECK (length(bound_endpoint_hash) = 32),
    expected_uid_validity INTEGER CHECK (expected_uid_validity IS NULL OR expected_uid_validity BETWEEN 1 AND 4294967295),
    after_uid INTEGER NOT NULL CHECK (after_uid BETWEEN 0 AND 4294967294),
    state TEXT NOT NULL CHECK (state IN ('admitted','running','succeeded','failed')),
    admitted_at_utc_ms INTEGER NOT NULL CHECK (admitted_at_utc_ms > 0),
    started_at_utc_ms INTEGER CHECK (started_at_utc_ms IS NULL OR started_at_utc_ms >= admitted_at_utc_ms),
    completed_at_utc_ms INTEGER CHECK (completed_at_utc_ms IS NULL OR (completed_at_utc_ms >= admitted_at_utc_ms AND (started_at_utc_ms IS NULL OR completed_at_utc_ms >= started_at_utc_ms))),
    result_cursor_version INTEGER CHECK (result_cursor_version IS NULL OR result_cursor_version = expected_cursor_version + 1),
    result_covered_through_uid INTEGER CHECK (result_covered_through_uid IS NULL OR (result_covered_through_uid BETWEEN after_uid AND 4294967294 AND result_covered_through_uid <= after_uid + 64)),
    result_inserted_count INTEGER CHECK (result_inserted_count IS NULL OR result_inserted_count BETWEEN 0 AND 64),
    error_code TEXT CHECK (error_code IS NULL OR error_code IN ('INVALID_INPUT','AUTH_REQUIRED','SCOPE_DENIED','POLICY_BLOCKED','CONFLICT','RATE_LIMITED','OFFLINE','NOT_FOUND','SEND_UNKNOWN','APPROVAL_REQUIRED','APPROVAL_EXPIRED','APPROVAL_DENIED','APPROVAL_RATE_LIMITED','CLIENT_NOT_REGISTERED','CLIENT_SUSPENDED','HUMAN_STEP_REQUIRED','CORE_UNAVAILABLE','VERSION_MISMATCH','CURSOR_EXPIRED','CAPABILITY_UNSUPPORTED','QUOTA_EXCEEDED','INTERNAL_ERROR')),
    page_operation_id TEXT REFERENCES imap_uid_page_operations(operation_id) ON DELETE RESTRICT,
    result_page_hash BLOB CHECK (result_page_hash IS NULL OR length(result_page_hash) = 32),
    result_payload_hash BLOB CHECK (result_payload_hash IS NULL OR length(result_payload_hash) = 32),
    UNIQUE (account_id, idempotency_key),
    CHECK ((expected_cursor_version = 0 AND expected_uid_validity IS NULL AND after_uid = 0)
        OR (expected_cursor_version > 0 AND expected_uid_validity IS NOT NULL)),
    CHECK (
        (state = 'admitted' AND started_at_utc_ms IS NULL AND completed_at_utc_ms IS NULL AND result_cursor_version IS NULL AND result_covered_through_uid IS NULL AND result_inserted_count IS NULL AND error_code IS NULL AND page_operation_id IS NULL AND result_page_hash IS NULL AND result_payload_hash IS NULL)
        OR (state = 'running' AND started_at_utc_ms IS NOT NULL AND completed_at_utc_ms IS NULL AND result_cursor_version IS NULL AND result_covered_through_uid IS NULL AND result_inserted_count IS NULL AND error_code IS NULL AND page_operation_id IS NULL AND result_page_hash IS NULL AND result_payload_hash IS NULL)
        OR (state = 'succeeded' AND started_at_utc_ms IS NOT NULL AND completed_at_utc_ms IS NOT NULL AND result_cursor_version IS NOT NULL AND result_covered_through_uid IS NOT NULL AND result_inserted_count IS NOT NULL AND error_code IS NULL AND page_operation_id IS NOT NULL AND page_operation_id = operation_id AND result_page_hash IS NOT NULL AND result_payload_hash IS NOT NULL)
        OR (state = 'failed' AND completed_at_utc_ms IS NOT NULL AND result_cursor_version IS NULL AND result_covered_through_uid IS NULL AND result_inserted_count IS NULL AND error_code IS NOT NULL AND page_operation_id IS NULL AND result_page_hash IS NULL AND result_payload_hash IS NULL)
    )
) STRICT;

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

CREATE TABLE client_scope_refreshes (
    refresh_id BLOB PRIMARY KEY CHECK (length(refresh_id)=32 AND refresh_id<>zeroblob(32)),
    registration_id BLOB NOT NULL REFERENCES client_registrations(registration_id) ON DELETE RESTRICT CHECK (length(registration_id)=16),
    base_registration_version INTEGER NOT NULL CHECK (base_registration_version BETWEEN 1 AND 9223372036854775805),
    base_profile_epoch INTEGER NOT NULL CHECK (base_profile_epoch BETWEEN 1 AND 9223372036854775806),
    target_profile_epoch INTEGER NOT NULL CHECK (target_profile_epoch=base_profile_epoch+1),
    base_accounts_json TEXT NOT NULL CHECK (json_valid(base_accounts_json) AND json_type(base_accounts_json)='array' AND json_array_length(base_accounts_json)<=128),
    target_accounts_json TEXT NOT NULL CHECK (json_valid(target_accounts_json) AND json_type(target_accounts_json)='array' AND json_array_length(target_accounts_json)<=128),
    client_public_key_hash BLOB NOT NULL CHECK (length(client_public_key_hash)=32 AND client_public_key_hash<>zeroblob(32)),
    server_public_key_hash BLOB NOT NULL CHECK (length(server_public_key_hash)=32 AND server_public_key_hash<>zeroblob(32)),
    payload_hash BLOB NOT NULL CHECK (length(payload_hash)=32 AND payload_hash<>zeroblob(32)),
    phase TEXT NOT NULL CHECK (phase IN ('pending','staged','active','denied','expired')),
    version INTEGER NOT NULL CHECK (version BETWEEN 1 AND 3),
    created_at_utc_ms INTEGER NOT NULL CHECK (created_at_utc_ms>0),
    expires_at_utc_ms INTEGER NOT NULL CHECK (expires_at_utc_ms>created_at_utc_ms AND expires_at_utc_ms-created_at_utc_ms=300000),
    updated_at_utc_ms INTEGER NOT NULL CHECK (updated_at_utc_ms>=created_at_utc_ms),
    decided_at_utc_ms INTEGER,
    activated_at_utc_ms INTEGER,
    hash_version INTEGER NOT NULL DEFAULT 1 CHECK (hash_version IN (1,2,3,4,5)),
    base_tools_json TEXT NOT NULL DEFAULT '["mail.accounts.list"]' CHECK (base_tools_json IN ('["mail.accounts.list"]','["mail.accounts.list","mail.folders.list","mail.folders.refresh"]','["mail.accounts.list","mail.folders.list","mail.folders.refresh","mail.sync.page"]','["mail.accounts.list","mail.folders.list","mail.folders.refresh","mail.messages.search","mail.sync.page"]','["app.diagnostics.snapshot","app.settings.display.get","mail.accounts.list","mail.folders.list","mail.folders.refresh","mail.messages.search","mail.sync.page"]')),
    target_tools_json TEXT NOT NULL DEFAULT '["mail.accounts.list"]' CHECK (target_tools_json IN ('["mail.accounts.list"]','["mail.accounts.list","mail.folders.list","mail.folders.refresh"]','["mail.accounts.list","mail.folders.list","mail.folders.refresh","mail.sync.page"]','["mail.accounts.list","mail.folders.list","mail.folders.refresh","mail.messages.search","mail.sync.page"]','["app.diagnostics.snapshot","app.settings.display.get","mail.accounts.list","mail.folders.list","mail.folders.refresh","mail.messages.search","mail.sync.page"]')),
    CHECK (base_accounts_json<>target_accounts_json OR base_tools_json<>target_tools_json),
    CHECK ((hash_version=1 AND base_tools_json IN ('["mail.accounts.list"]') AND target_tools_json IN ('["mail.accounts.list"]')) OR (hash_version=2 AND base_tools_json IN ('["mail.accounts.list"]','["mail.accounts.list","mail.folders.list","mail.folders.refresh"]') AND target_tools_json IN ('["mail.accounts.list"]','["mail.accounts.list","mail.folders.list","mail.folders.refresh"]')) OR (hash_version=3 AND base_tools_json IN ('["mail.accounts.list"]','["mail.accounts.list","mail.folders.list","mail.folders.refresh"]','["mail.accounts.list","mail.folders.list","mail.folders.refresh","mail.sync.page"]') AND target_tools_json IN ('["mail.accounts.list"]','["mail.accounts.list","mail.folders.list","mail.folders.refresh"]','["mail.accounts.list","mail.folders.list","mail.folders.refresh","mail.sync.page"]')) OR (hash_version=4 AND base_tools_json IN ('["mail.accounts.list"]','["mail.accounts.list","mail.folders.list","mail.folders.refresh"]','["mail.accounts.list","mail.folders.list","mail.folders.refresh","mail.sync.page"]','["mail.accounts.list","mail.folders.list","mail.folders.refresh","mail.messages.search","mail.sync.page"]') AND target_tools_json IN ('["mail.accounts.list"]','["mail.accounts.list","mail.folders.list","mail.folders.refresh"]','["mail.accounts.list","mail.folders.list","mail.folders.refresh","mail.sync.page"]','["mail.accounts.list","mail.folders.list","mail.folders.refresh","mail.messages.search","mail.sync.page"]')) OR (hash_version=5 AND base_tools_json IN ('["mail.accounts.list"]','["mail.accounts.list","mail.folders.list","mail.folders.refresh"]','["mail.accounts.list","mail.folders.list","mail.folders.refresh","mail.sync.page"]','["mail.accounts.list","mail.folders.list","mail.folders.refresh","mail.messages.search","mail.sync.page"]','["app.diagnostics.snapshot","app.settings.display.get","mail.accounts.list","mail.folders.list","mail.folders.refresh","mail.messages.search","mail.sync.page"]') AND target_tools_json IN ('["mail.accounts.list"]','["mail.accounts.list","mail.folders.list","mail.folders.refresh"]','["mail.accounts.list","mail.folders.list","mail.folders.refresh","mail.sync.page"]','["mail.accounts.list","mail.folders.list","mail.folders.refresh","mail.messages.search","mail.sync.page"]','["app.diagnostics.snapshot","app.settings.display.get","mail.accounts.list","mail.folders.list","mail.folders.refresh","mail.messages.search","mail.sync.page"]'))),
    CHECK ((base_tools_json='["mail.accounts.list"]' AND target_tools_json IN ('["mail.accounts.list"]','["mail.accounts.list","mail.folders.list","mail.folders.refresh"]','["mail.accounts.list","mail.folders.list","mail.folders.refresh","mail.sync.page"]','["mail.accounts.list","mail.folders.list","mail.folders.refresh","mail.messages.search","mail.sync.page"]','["app.diagnostics.snapshot","app.settings.display.get","mail.accounts.list","mail.folders.list","mail.folders.refresh","mail.messages.search","mail.sync.page"]')) OR (base_tools_json='["mail.accounts.list","mail.folders.list","mail.folders.refresh"]' AND target_tools_json IN ('["mail.accounts.list","mail.folders.list","mail.folders.refresh"]','["mail.accounts.list","mail.folders.list","mail.folders.refresh","mail.sync.page"]','["mail.accounts.list","mail.folders.list","mail.folders.refresh","mail.messages.search","mail.sync.page"]','["app.diagnostics.snapshot","app.settings.display.get","mail.accounts.list","mail.folders.list","mail.folders.refresh","mail.messages.search","mail.sync.page"]')) OR (base_tools_json='["mail.accounts.list","mail.folders.list","mail.folders.refresh","mail.sync.page"]' AND target_tools_json IN ('["mail.accounts.list","mail.folders.list","mail.folders.refresh","mail.sync.page"]','["mail.accounts.list","mail.folders.list","mail.folders.refresh","mail.messages.search","mail.sync.page"]','["app.diagnostics.snapshot","app.settings.display.get","mail.accounts.list","mail.folders.list","mail.folders.refresh","mail.messages.search","mail.sync.page"]')) OR (base_tools_json='["mail.accounts.list","mail.folders.list","mail.folders.refresh","mail.messages.search","mail.sync.page"]' AND target_tools_json IN ('["mail.accounts.list","mail.folders.list","mail.folders.refresh","mail.messages.search","mail.sync.page"]','["app.diagnostics.snapshot","app.settings.display.get","mail.accounts.list","mail.folders.list","mail.folders.refresh","mail.messages.search","mail.sync.page"]')) OR (base_tools_json='["app.diagnostics.snapshot","app.settings.display.get","mail.accounts.list","mail.folders.list","mail.folders.refresh","mail.messages.search","mail.sync.page"]' AND target_tools_json IN ('["app.diagnostics.snapshot","app.settings.display.get","mail.accounts.list","mail.folders.list","mail.folders.refresh","mail.messages.search","mail.sync.page"]'))),
    CHECK (
        (phase='pending' AND version=1 AND updated_at_utc_ms=created_at_utc_ms AND decided_at_utc_ms IS NULL AND activated_at_utc_ms IS NULL)
        OR (phase IN ('staged','denied') AND version=2 AND decided_at_utc_ms IS NOT NULL AND decided_at_utc_ms>=created_at_utc_ms AND decided_at_utc_ms<expires_at_utc_ms AND updated_at_utc_ms=decided_at_utc_ms AND activated_at_utc_ms IS NULL)
        OR (phase='expired' AND version=2 AND decided_at_utc_ms IS NOT NULL AND decided_at_utc_ms>=expires_at_utc_ms AND updated_at_utc_ms=decided_at_utc_ms AND activated_at_utc_ms IS NULL)
        OR (phase='active' AND version=3 AND decided_at_utc_ms IS NOT NULL AND decided_at_utc_ms>=created_at_utc_ms AND decided_at_utc_ms<expires_at_utc_ms AND activated_at_utc_ms IS NOT NULL AND activated_at_utc_ms>=decided_at_utc_ms AND updated_at_utc_ms=activated_at_utc_ms)
    )
) STRICT;

CREATE UNIQUE INDEX imap_uid_epochs_current ON imap_uid_epochs(account_id,mailbox_key,mailbox_encoding) WHERE last_cursor_version IS NULL;

CREATE INDEX imap_uid_reconcile_source_latest ON imap_uid_reconcile_requests(source_operation_id,state,generation DESC);

CREATE INDEX imap_uid_reconcile_messages_execution ON imap_uid_reconcile_messages(last_execution_id,uid);

CREATE UNIQUE INDEX client_scope_refresh_pending_uq ON client_scope_refreshes(registration_id)
    WHERE phase IN ('pending','staged');

CREATE UNIQUE INDEX client_scope_refresh_epoch_uq ON client_scope_refreshes(registration_id,target_profile_epoch)
    WHERE phase IN ('staged','active');

INSERT INTO audit_starts SELECT * FROM audit_starts_v15 ORDER BY rowid;

INSERT INTO audit_start_accounts SELECT * FROM audit_start_accounts_v15 ORDER BY rowid;

INSERT INTO audit_terminals SELECT * FROM audit_terminals_v15 ORDER BY rowid;

INSERT INTO folder_refresh_requests SELECT * FROM folder_refresh_requests_v15 ORDER BY rowid;

INSERT INTO imap_uid_page_requests SELECT * FROM imap_uid_page_requests_v15 ORDER BY rowid;

INSERT INTO imap_uid_recovery_requests SELECT * FROM imap_uid_recovery_requests_v15 ORDER BY rowid;

INSERT INTO imap_uid_epochs SELECT * FROM imap_uid_epochs_v15 ORDER BY rowid;

INSERT INTO imap_uid_reconcile_heads SELECT * FROM imap_uid_reconcile_heads_v15 ORDER BY rowid;

INSERT INTO imap_uid_reconcile_requests SELECT * FROM imap_uid_reconcile_requests_v15 ORDER BY rowid;

INSERT INTO imap_uid_reconcile_messages SELECT * FROM imap_uid_reconcile_messages_v15 ORDER BY rowid;

INSERT INTO client_scope_refreshes SELECT * FROM client_scope_refreshes_v15 ORDER BY rowid;

DROP TABLE client_scope_refreshes_v15;

DROP TABLE imap_uid_reconcile_messages_v15;

DROP TABLE imap_uid_reconcile_requests_v15;

DROP TABLE imap_uid_reconcile_heads_v15;

DROP TABLE imap_uid_epochs_v15;

DROP TABLE imap_uid_recovery_requests_v15;

DROP TABLE imap_uid_page_requests_v15;

DROP TABLE folder_refresh_requests_v15;

DROP TABLE audit_terminals_v15;

DROP TABLE audit_start_accounts_v15;

DROP TABLE audit_starts_v15;

DELETE FROM sqlite_sequence WHERE name='audit_starts';

INSERT INTO sqlite_sequence(name,seq) SELECT 'audit_starts',value FROM audit_seq_highwater_v16;

DROP TABLE audit_seq_highwater_v16;
