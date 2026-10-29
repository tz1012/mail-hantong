-- Add bounded provider-neutral connection settings, observations, and operations.
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
