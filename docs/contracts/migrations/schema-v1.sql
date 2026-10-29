-- mail-hantong encrypted schema v1
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
