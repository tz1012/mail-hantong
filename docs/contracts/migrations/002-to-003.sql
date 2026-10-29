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
