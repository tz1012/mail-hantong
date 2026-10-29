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
