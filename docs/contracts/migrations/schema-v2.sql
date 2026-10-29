-- mail-hantong encrypted schema v2
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
