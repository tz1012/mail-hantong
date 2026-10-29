-- mail-hantong encrypted schema v6
-- This file is consumed only after SQLCipher keying and connection checks.
-- It never creates a plaintext fallback database.
-- Connection policy pragmas are applied by KeyedConnection before this DDL
-- runs; keeping them out of the transaction makes schema publication atomic.

CREATE TABLE app_metadata (
    singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
    database_format TEXT NOT NULL CHECK (database_format = 'mail-hantong'),
    schema_version INTEGER NOT NULL CHECK (schema_version > 0),
    created_at_utc_ms INTEGER NOT NULL CHECK (created_at_utc_ms > 0),
    updated_at_utc_ms INTEGER NOT NULL CHECK (updated_at_utc_ms >= created_at_utc_ms)
) STRICT;

-- The initializer inserts the singleton row with a bound UTC timestamp after
-- the encrypted connection has been keyed and verified. This file contains no
-- user or secret values and can therefore be parsed as a schema template.

CREATE TABLE settings (
    setting_key TEXT PRIMARY KEY CHECK (length(setting_key) BETWEEN 1 AND 128),
    value_json TEXT NOT NULL CHECK (json_valid(value_json)),
    version INTEGER NOT NULL CHECK (version >= 1),
    updated_at_utc_ms INTEGER NOT NULL CHECK (updated_at_utc_ms > 0)
) STRICT;

CREATE TABLE accounts (
    account_id TEXT PRIMARY KEY CHECK (length(account_id) BETWEEN 1 AND 128),
    provider TEXT NOT NULL CHECK (provider IN ('gmail', 'graph', 'imap', 'archive')),
    provider_subject TEXT,
    primary_address TEXT,
    display_name TEXT,
    color TEXT,
    account_number INTEGER NOT NULL CHECK (account_number >= 1),
    status TEXT NOT NULL,
    capabilities TEXT NOT NULL CHECK (json_valid(capabilities) AND json_type(capabilities) = 'array'),
    granted_scopes TEXT NOT NULL CHECK (json_valid(granted_scopes) AND json_type(granted_scopes) = 'array'),
    settings_version INTEGER NOT NULL CHECK (settings_version >= 1),
    CHECK (provider_subject IS NULL OR length(provider_subject) BETWEEN 1 AND 512)
) STRICT;

CREATE UNIQUE INDEX accounts_provider_subject_uq
    ON accounts(provider, provider_subject)
    WHERE provider_subject IS NOT NULL;

CREATE INDEX accounts_number_idx ON accounts(account_number);

-- Add registration infrastructure. No active identity or client is inserted.
CREATE TABLE server_identity (
    singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
    key_id BLOB NOT NULL UNIQUE CHECK (length(key_id) = 32),
    state TEXT NOT NULL CHECK (state IN ('creating', 'active')),
    public_key BLOB CHECK (public_key IS NULL OR length(public_key) = 72),
    key_epoch INTEGER NOT NULL CHECK (key_epoch >= 1),
    version INTEGER NOT NULL CHECK (version >= 1),
    created_at_utc_ms INTEGER NOT NULL CHECK (created_at_utc_ms > 0),
    updated_at_utc_ms INTEGER NOT NULL CHECK (updated_at_utc_ms >= created_at_utc_ms),
    CHECK ((state = 'creating' AND public_key IS NULL) OR (state = 'active' AND public_key IS NOT NULL))
) STRICT;

CREATE TABLE client_registration_requests (
    request_id BLOB PRIMARY KEY CHECK (length(request_id) = 32),
    client_kind TEXT NOT NULL CHECK (client_kind IN ('cli', 'mcp', 'gui')),
    display_label TEXT NOT NULL CHECK (length(display_label) BETWEEN 1 AND 128),
    requested_accounts_json TEXT NOT NULL CHECK (json_valid(requested_accounts_json) AND json_type(requested_accounts_json) = 'array' AND json_array_length(requested_accounts_json) <= 128),
    requested_tools_json TEXT NOT NULL CHECK (json_valid(requested_tools_json) AND json_type(requested_tools_json) = 'array' AND json_array_length(requested_tools_json) <= 128),
    state TEXT NOT NULL CHECK (state IN ('pending', 'approved', 'denied', 'expired')),
    decision_channel TEXT CHECK (decision_channel IS NULL OR decision_channel IN ('approval_helper', 'gui')),
    version INTEGER NOT NULL CHECK (version >= 1),
    created_at_utc_ms INTEGER NOT NULL CHECK (created_at_utc_ms > 0),
    expires_at_utc_ms INTEGER NOT NULL CHECK (expires_at_utc_ms > created_at_utc_ms AND expires_at_utc_ms - created_at_utc_ms <= 300000),
    decided_at_utc_ms INTEGER CHECK (decided_at_utc_ms IS NULL OR decided_at_utc_ms >= created_at_utc_ms),
    CHECK ((state = 'pending' AND decided_at_utc_ms IS NULL AND decision_channel IS NULL) OR (state = 'expired' AND decided_at_utc_ms IS NOT NULL AND decision_channel IS NULL) OR (state IN ('approved', 'denied') AND decided_at_utc_ms IS NOT NULL AND decision_channel IS NOT NULL))
) STRICT;

CREATE TABLE client_registrations (
    registration_id BLOB PRIMARY KEY CHECK (length(registration_id) = 16),
    request_id BLOB NOT NULL UNIQUE REFERENCES client_registration_requests(request_id) ON DELETE RESTRICT,
    client_kind TEXT NOT NULL CHECK (client_kind IN ('cli', 'mcp', 'gui')),
    display_label TEXT NOT NULL CHECK (length(display_label) BETWEEN 1 AND 128),
    public_key BLOB NOT NULL UNIQUE CHECK (length(public_key) = 72),
    profile_epoch INTEGER NOT NULL CHECK (profile_epoch >= 1),
    state TEXT NOT NULL CHECK (state IN ('provisioning', 'active', 'suspended', 'revoked')),
    read_only INTEGER NOT NULL CHECK (read_only IN (0, 1)),
    requests_per_minute INTEGER NOT NULL CHECK (requests_per_minute BETWEEN 1 AND 600),
    approved_channel TEXT NOT NULL CHECK (approved_channel IN ('approval_helper', 'gui')),
    approved_at_utc_ms INTEGER NOT NULL CHECK (approved_at_utc_ms > 0),
    updated_at_utc_ms INTEGER NOT NULL CHECK (updated_at_utc_ms >= approved_at_utc_ms),
    revoked_at_utc_ms INTEGER CHECK (revoked_at_utc_ms IS NULL OR revoked_at_utc_ms >= approved_at_utc_ms),
    version INTEGER NOT NULL CHECK (version >= 1),
    CHECK ((state = 'revoked' AND revoked_at_utc_ms IS NOT NULL) OR (state != 'revoked' AND revoked_at_utc_ms IS NULL))
) STRICT;

CREATE TABLE client_registration_accounts (
    registration_id BLOB NOT NULL REFERENCES client_registrations(registration_id) ON DELETE RESTRICT,
    account_id TEXT NOT NULL REFERENCES accounts(account_id) ON DELETE RESTRICT
        CHECK (length(account_id) BETWEEN 1 AND 128 AND account_id NOT GLOB '*[^A-Za-z0-9_-]*'),
    PRIMARY KEY (registration_id, account_id)
) STRICT;

CREATE TABLE client_registration_tools (
    registration_id BLOB NOT NULL REFERENCES client_registrations(registration_id) ON DELETE RESTRICT,
    tool_id TEXT NOT NULL CHECK (length(tool_id) BETWEEN 1 AND 96 AND tool_id GLOB '[a-z]*' AND tool_id NOT GLOB '*[^a-z0-9.]*'),
    PRIMARY KEY (registration_id, tool_id)
) STRICT;

-- Bounded append-only audit storage. No client or authorization is created.
CREATE TABLE audit_capacity (
    singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
    retained_executions INTEGER NOT NULL CHECK (retained_executions BETWEEN 0 AND 100000),
    retained_account_refs INTEGER NOT NULL CHECK (retained_account_refs BETWEEN 0 AND 1000000)
) STRICT;

INSERT INTO audit_capacity(singleton, retained_executions, retained_account_refs) VALUES(1, 0, 0);

CREATE TABLE audit_starts (
    audit_seq INTEGER PRIMARY KEY AUTOINCREMENT,
    execution_id BLOB NOT NULL UNIQUE CHECK (length(execution_id) = 32),
    registration_id BLOB NOT NULL REFERENCES client_registrations(registration_id) ON DELETE RESTRICT
        CHECK (length(registration_id) = 16),
    profile_epoch INTEGER NOT NULL CHECK (profile_epoch >= 1),
    client_kind TEXT NOT NULL CHECK (client_kind IN ('cli', 'mcp', 'gui')),
    display_label TEXT NOT NULL CHECK (length(display_label) BETWEEN 1 AND 128 AND length(CAST(display_label AS BLOB)) <= 512),
    tool_code TEXT NOT NULL CHECK (tool_code IN ('mail.accounts.list', 'mail.send.execute')),
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

-- Provider-neutral IMAP/SMTP endpoint settings. Secrets are never stored here.
CREATE TABLE mail_connection_settings (
    account_id TEXT NOT NULL REFERENCES accounts(account_id) ON DELETE RESTRICT
        CHECK (length(account_id) BETWEEN 1 AND 128 AND account_id NOT GLOB '*[^A-Za-z0-9_-]*'),
    endpoint_kind TEXT NOT NULL CHECK (endpoint_kind IN ('imap', 'smtp')),
    settings_version INTEGER NOT NULL CHECK (settings_version BETWEEN 1 AND 9223372036854775807),
    host TEXT NOT NULL CHECK (length(host) BETWEEN 1 AND 253),
    port INTEGER NOT NULL CHECK (port BETWEEN 1 AND 65535),
    tls TEXT NOT NULL CHECK (tls IN ('implicit_tls', 'starttls_required')),
    login_name TEXT NOT NULL CHECK (length(login_name) BETWEEN 1 AND 320 AND length(CAST(login_name AS BLOB)) <= 1280),
    authentication TEXT NOT NULL CHECK (authentication IN ('app_password', 'oauth2')),
    updated_at_utc_ms INTEGER NOT NULL CHECK (updated_at_utc_ms > 0),
    PRIMARY KEY (account_id, endpoint_kind)
) STRICT;

CREATE TABLE mail_connection_observations (
    account_id TEXT NOT NULL,
    endpoint_kind TEXT NOT NULL CHECK (endpoint_kind IN ('imap', 'smtp')),
    observed_settings_version INTEGER NOT NULL CHECK (observed_settings_version BETWEEN 1 AND 9223372036854775807),
    observation_version INTEGER NOT NULL CHECK (observation_version BETWEEN 1 AND 9223372036854775807),
    state TEXT NOT NULL CHECK (state IN ('ready', 'auth_required', 'tls_failed', 'offline', 'error')),
    observed_at_utc_ms INTEGER NOT NULL CHECK (observed_at_utc_ms > 0),
    PRIMARY KEY (account_id, endpoint_kind),
    FOREIGN KEY (account_id, endpoint_kind)
        REFERENCES mail_connection_settings(account_id, endpoint_kind) ON DELETE CASCADE
) STRICT;

CREATE TABLE mail_connection_operations (
    operation_id TEXT NOT NULL PRIMARY KEY CHECK (length(operation_id) BETWEEN 1 AND 128 AND operation_id NOT GLOB '*[^A-Za-z0-9_-]*'),
    account_id TEXT NOT NULL REFERENCES accounts(account_id) ON DELETE RESTRICT
        CHECK (length(account_id) BETWEEN 1 AND 128 AND account_id NOT GLOB '*[^A-Za-z0-9_-]*'),
    idempotency_key TEXT NOT NULL CHECK (length(idempotency_key) BETWEEN 1 AND 128 AND idempotency_key NOT GLOB '*[^A-Za-z0-9_-]*'),
    endpoint_kind TEXT NOT NULL CHECK (endpoint_kind IN ('imap', 'smtp')),
    payload_hash BLOB NOT NULL CHECK (length(payload_hash) = 32),
    expected_version INTEGER NOT NULL CHECK (expected_version BETWEEN 0 AND 9223372036854775806),
    result_version INTEGER NOT NULL CHECK (result_version = expected_version + 1),
    completed_at_utc_ms INTEGER NOT NULL CHECK (completed_at_utc_ms > 0),
    UNIQUE (account_id, idempotency_key)
) STRICT;

-- Bounded explicit setup attempts. No credential values or approval assertions.
CREATE TABLE imap_setup_requests (
    request_id BLOB PRIMARY KEY CHECK (length(request_id)=32 AND request_id<>zeroblob(32)),
    account_id TEXT NOT NULL CHECK (length(account_id) BETWEEN 1 AND 128 AND account_id NOT GLOB '*[^A-Za-z0-9_-]*'),
    operation_id TEXT NOT NULL UNIQUE CHECK (length(operation_id) BETWEEN 1 AND 128 AND operation_id NOT GLOB '*[^A-Za-z0-9_-]*'),
    idempotency_key TEXT NOT NULL CHECK (length(idempotency_key) BETWEEN 1 AND 128 AND idempotency_key NOT GLOB '*[^A-Za-z0-9_-]*'),
    display_name TEXT CHECK (display_name IS NULL OR (length(display_name) BETWEEN 1 AND 128 AND length(CAST(display_name AS BLOB))<=512)),
    color TEXT CHECK (color IS NULL OR length(color)=7),
    endpoint_version INTEGER NOT NULL CHECK (endpoint_version=1),
    host TEXT NOT NULL CHECK (length(host) BETWEEN 1 AND 253),
    port INTEGER NOT NULL CHECK (port BETWEEN 1 AND 65535),
    tls TEXT NOT NULL CHECK (tls IN ('implicit_tls','starttls_required')),
    login_name TEXT NOT NULL CHECK (length(login_name) BETWEEN 1 AND 320 AND length(CAST(login_name AS BLOB))<=1280),
    authentication TEXT NOT NULL CHECK (authentication='app_password'),
    payload_hash BLOB NOT NULL CHECK (length(payload_hash)=32 AND payload_hash<>zeroblob(32)),
    origin TEXT NOT NULL CHECK (origin='local_same_logon_bootstrap'),
    phase TEXT NOT NULL CHECK (phase IN ('review_pending','account_staged','review_closed','credential_store_started','credential_store_returned','probe_started','probe_finished','superseded','resume_staged')),
    version INTEGER NOT NULL CHECK (version BETWEEN 1 AND 8),
    created_at_utc_ms INTEGER NOT NULL CHECK (created_at_utc_ms>0),
    expires_at_utc_ms INTEGER NOT NULL CHECK (expires_at_utc_ms>created_at_utc_ms AND expires_at_utc_ms-created_at_utc_ms=300000),
    updated_at_utc_ms INTEGER NOT NULL CHECK (updated_at_utc_ms>=created_at_utc_ms),
    resume_of_request_id BLOB UNIQUE REFERENCES imap_setup_requests(request_id) ON DELETE RESTRICT
        CHECK (resume_of_request_id IS NULL OR (length(resume_of_request_id)=32 AND resume_of_request_id<>zeroblob(32) AND resume_of_request_id<>request_id)),
    resume_parent_version INTEGER CHECK (resume_parent_version IS NULL OR resume_parent_version BETWEEN 1 AND 7),
    resume_account_version INTEGER CHECK (resume_account_version IS NULL OR resume_account_version BETWEEN 1 AND 9223372036854775805),
    resume_observation_version INTEGER CHECK (resume_observation_version IS NULL OR resume_observation_version BETWEEN 0 AND 9223372036854775806),
    resume_payload_hash BLOB CHECK (resume_payload_hash IS NULL OR (length(resume_payload_hash)=32 AND resume_payload_hash<>zeroblob(32))),
    UNIQUE(account_id,idempotency_key),
    CHECK (
        (resume_of_request_id IS NULL AND resume_parent_version IS NULL AND resume_account_version IS NULL AND resume_observation_version IS NULL AND resume_payload_hash IS NULL)
        OR (resume_of_request_id IS NOT NULL AND resume_parent_version IS NOT NULL AND resume_payload_hash IS NOT NULL
            AND ((resume_account_version IS NULL AND resume_observation_version IS NULL)
                OR (resume_account_version IS NOT NULL AND resume_observation_version IS NOT NULL)))
    )
) STRICT;

CREATE UNIQUE INDEX imap_setup_active_account_uq ON imap_setup_requests(account_id)
    WHERE phase IN ('review_pending','account_staged','credential_store_started','credential_store_returned','probe_started','resume_staged');

CREATE TABLE imap_setup_events (
    request_id BLOB NOT NULL REFERENCES imap_setup_requests(request_id) ON DELETE RESTRICT CHECK (length(request_id)=32 AND request_id<>zeroblob(32)),
    seq INTEGER NOT NULL CHECK (seq BETWEEN 1 AND 8),
    kind TEXT NOT NULL CHECK (kind IN ('request_created','account_staged','review_closed','credential_store_started','credential_store_returned','probe_started','probe_finished','superseded','resume_staged')),
    occurred_at_utc_ms INTEGER NOT NULL CHECK (occurred_at_utc_ms>0),
    result_kind TEXT CHECK (result_kind IS NULL OR result_kind IN ('returned_ok','returned_error')),
    error_code TEXT CHECK (error_code IS NULL OR error_code IN ('INVALID_INPUT','SCOPE_DENIED','APPROVAL_REQUIRED','APPROVAL_DENIED','APPROVAL_EXPIRED','APPROVAL_RATE_LIMITED','HUMAN_STEP_REQUIRED','CAPABILITY_UNSUPPORTED','INTERNAL_ERROR')),
    probe_outcome TEXT CHECK (probe_outcome IS NULL OR probe_outcome IN ('ready','auth_required','tls_failed','offline','error')),
    capabilities TEXT CHECK (capabilities IS NULL OR (json_valid(capabilities) AND json_type(capabilities)='array')),
    account_version INTEGER,
    endpoint_version INTEGER,
    observation_version INTEGER,
    child_request_id BLOB REFERENCES imap_setup_requests(request_id) ON DELETE RESTRICT
        CHECK (child_request_id IS NULL OR (length(child_request_id)=32 AND child_request_id<>zeroblob(32) AND child_request_id<>request_id)),
    PRIMARY KEY(request_id,seq),
    UNIQUE(request_id,kind),
    CHECK (kind='superseded' OR child_request_id IS NULL),
    CHECK (
        (kind IN ('request_created','credential_store_started','probe_started') AND result_kind IS NULL AND error_code IS NULL AND probe_outcome IS NULL AND capabilities IS NULL AND account_version IS NULL AND endpoint_version IS NULL AND observation_version IS NULL)
        OR (kind='account_staged' AND result_kind IS NULL AND error_code IS NULL AND probe_outcome IS NULL AND capabilities IS NULL AND account_version IS NOT NULL AND account_version=1 AND endpoint_version IS NOT NULL AND endpoint_version=1 AND observation_version IS NOT NULL AND observation_version=0)
        OR (kind='review_closed' AND result_kind IS NOT NULL AND result_kind='returned_error' AND error_code IS NOT NULL AND probe_outcome IS NULL AND capabilities IS NULL AND account_version IS NULL AND endpoint_version IS NULL AND observation_version IS NULL)
        OR (kind='credential_store_returned' AND result_kind IS NOT NULL AND ((result_kind='returned_ok' AND error_code IS NULL) OR (result_kind='returned_error' AND error_code IS NOT NULL AND error_code IN ('INVALID_INPUT','SCOPE_DENIED','APPROVAL_EXPIRED','CAPABILITY_UNSUPPORTED','INTERNAL_ERROR'))) AND probe_outcome IS NULL AND capabilities IS NULL AND account_version IS NULL AND endpoint_version IS NULL AND observation_version IS NULL)
        OR (kind='resume_staged' AND result_kind IS NULL AND error_code IS NULL AND probe_outcome IS NULL AND capabilities IS NULL AND account_version IS NOT NULL AND account_version BETWEEN 2 AND 9223372036854775806 AND endpoint_version IS NOT NULL AND endpoint_version=1 AND observation_version IS NOT NULL AND observation_version BETWEEN 0 AND 9223372036854775806)
        OR (kind='superseded' AND result_kind IS NULL AND error_code IS NULL AND probe_outcome IS NULL AND capabilities IS NULL AND account_version IS NULL AND endpoint_version IS NULL AND observation_version IS NULL AND child_request_id IS NOT NULL)
        OR (kind='probe_finished' AND result_kind IS NULL AND error_code IS NULL AND probe_outcome IS NOT NULL AND capabilities IS NOT NULL AND (probe_outcome='ready' OR json_array_length(capabilities)=0) AND account_version IS NOT NULL AND account_version BETWEEN 2 AND 9223372036854775807 AND endpoint_version IS NOT NULL AND endpoint_version=1 AND observation_version IS NOT NULL AND observation_version BETWEEN 1 AND 9223372036854775807)
    )
) STRICT;
