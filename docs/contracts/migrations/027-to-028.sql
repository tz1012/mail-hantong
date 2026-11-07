-- v27 to v28: add isolated account-qualified model-2 scope and decision ledger.
-- Legacy registration scope/history and all existing rights remain untouched.

CREATE TABLE account_scope_v2 (
    registration_id BLOB PRIMARY KEY REFERENCES client_registrations(registration_id) ON DELETE RESTRICT CHECK(length(registration_id)=16),
    profile_epoch INTEGER NOT NULL CHECK(profile_epoch>=1),
    baseline_registration_version INTEGER NOT NULL CHECK(baseline_registration_version>=1),
    baseline_profile_epoch INTEGER NOT NULL CHECK(baseline_profile_epoch>=1),
    effective_registration_version INTEGER NOT NULL CHECK(effective_registration_version>=1),
    effective_profile_epoch INTEGER NOT NULL CHECK(effective_profile_epoch>=1),
    state TEXT NOT NULL CHECK(state IN ('active','suspended','revoked')),
    baseline_binding_hash BLOB NOT NULL CHECK(length(baseline_binding_hash)=32),
    scope_json TEXT NOT NULL CHECK(json_valid(scope_json) AND json_type(scope_json)='object' AND length(CAST(scope_json AS BLOB))<=24576),
    scope_hash BLOB NOT NULL CHECK(length(scope_hash)=32),
    updated_at_utc_ms INTEGER NOT NULL CHECK(updated_at_utc_ms>0)
) STRICT;
CREATE TABLE account_scope_decisions_v2 (
    decision_id BLOB PRIMARY KEY CHECK(length(decision_id)=32),
    registration_id BLOB NOT NULL REFERENCES client_registrations(registration_id) ON DELETE RESTRICT CHECK(length(registration_id)=16),
    expected_registration_version INTEGER NOT NULL CHECK(expected_registration_version>=1),
    expected_profile_epoch INTEGER NOT NULL CHECK(expected_profile_epoch>=1),
    baseline_scope_hash BLOB NOT NULL CHECK(length(baseline_scope_hash)=32),
    baseline_binding_hash BLOB NOT NULL CHECK(length(baseline_binding_hash)=32 AND baseline_binding_hash<>zeroblob(32)),
    authority_binding_hash BLOB CHECK(authority_binding_hash IS NULL OR (length(authority_binding_hash)=32 AND authority_binding_hash<>zeroblob(32))),
    published_profile_hash BLOB CHECK(published_profile_hash IS NULL OR (length(published_profile_hash)=32 AND published_profile_hash<>zeroblob(32))),
    target_account_id TEXT NOT NULL REFERENCES accounts(account_id) ON DELETE RESTRICT CHECK(length(target_account_id) BETWEEN 1 AND 128 AND target_account_id NOT GLOB '*[^A-Za-z0-9_-]*'),
    target_scope_json TEXT NOT NULL CHECK(json_valid(target_scope_json) AND json_type(target_scope_json)='object' AND length(CAST(target_scope_json AS BLOB))<=24576),
    target_scope_hash BLOB NOT NULL CHECK(length(target_scope_hash)=32),
    purpose TEXT NOT NULL CHECK(purpose IN ('add','upgrade')),
    operation_id TEXT NOT NULL CHECK(length(operation_id) BETWEEN 1 AND 128 AND operation_id NOT GLOB '*[^A-Za-z0-9_-]*'),
    idempotency_key TEXT NOT NULL CHECK(length(idempotency_key) BETWEEN 1 AND 128 AND idempotency_key NOT GLOB '*[^A-Za-z0-9_-]*'),
    peer_digest BLOB NOT NULL CHECK(length(peer_digest)=32),
    server_public_key_hash BLOB NOT NULL CHECK(length(server_public_key_hash)=32 AND server_public_key_hash<>zeroblob(32)),
    state TEXT NOT NULL CHECK(state IN ('pending','cancelled','expired','staged','active')),
    created_at_utc_ms INTEGER NOT NULL CHECK(created_at_utc_ms>0),
    expires_at_utc_ms INTEGER NOT NULL CHECK(expires_at_utc_ms>created_at_utc_ms AND expires_at_utc_ms-created_at_utc_ms<=300000),
    consumed_at_utc_ms INTEGER,
    updated_at_utc_ms INTEGER NOT NULL CHECK(updated_at_utc_ms>=created_at_utc_ms),
    UNIQUE(registration_id,operation_id), UNIQUE(registration_id,idempotency_key),
    CHECK((state='pending' AND consumed_at_utc_ms IS NULL) OR (state<>'pending' AND consumed_at_utc_ms IS NOT NULL)),
    CHECK((state IN ('pending','cancelled','expired') AND authority_binding_hash IS NULL AND published_profile_hash IS NULL)
       OR (state='staged' AND authority_binding_hash IS NOT NULL AND published_profile_hash IS NULL)
       OR (state='active' AND authority_binding_hash IS NOT NULL AND published_profile_hash IS NOT NULL))
) STRICT;
CREATE INDEX account_scope_decisions_v2_state_idx ON account_scope_decisions_v2(registration_id,state);
CREATE UNIQUE INDEX account_scope_decisions_v2_open_uq ON account_scope_decisions_v2(registration_id) WHERE state IN ('pending','staged');
CREATE TABLE account_full_mail_readiness (
    account_id TEXT PRIMARY KEY REFERENCES accounts(account_id) ON DELETE RESTRICT CHECK(length(account_id) BETWEEN 1 AND 128 AND account_id NOT GLOB '*[^A-Za-z0-9_-]*'),
    account_version INTEGER NOT NULL CHECK(account_version>=1),
    imap_settings_version INTEGER NOT NULL CHECK(imap_settings_version>=1),
    smtp_settings_version INTEGER NOT NULL CHECK(smtp_settings_version>=1),
    catalog_generation INTEGER NOT NULL CHECK(catalog_generation>=1),
    trash_binding_hash BLOB NOT NULL CHECK(length(trash_binding_hash)=32),
    state TEXT NOT NULL CHECK(state IN ('full','incomplete')),
    reason TEXT CHECK(reason IS NULL OR reason IN ('imap','smtp','trash','move','catalog')),
    observed_at_utc_ms INTEGER NOT NULL CHECK(observed_at_utc_ms>0),
    CHECK((state='full' AND reason IS NULL) OR (state='incomplete' AND reason IS NOT NULL))
) STRICT;
