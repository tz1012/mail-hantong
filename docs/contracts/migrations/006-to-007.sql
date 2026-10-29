-- Dedicated default CLI scope history. Refresh IDs are never key selectors.
CREATE TABLE client_scope_refreshes (
    refresh_id BLOB PRIMARY KEY CHECK (length(refresh_id)=32 AND refresh_id<>zeroblob(32)),
    registration_id BLOB NOT NULL REFERENCES client_registrations(registration_id) ON DELETE RESTRICT CHECK (length(registration_id)=16),
    base_registration_version INTEGER NOT NULL CHECK (base_registration_version BETWEEN 1 AND 9223372036854775805),
    base_profile_epoch INTEGER NOT NULL CHECK (base_profile_epoch BETWEEN 1 AND 9223372036854775806),
    target_profile_epoch INTEGER NOT NULL CHECK (target_profile_epoch=base_profile_epoch+1),
    base_accounts_json TEXT NOT NULL CHECK (json_valid(base_accounts_json) AND json_type(base_accounts_json)='array' AND json_array_length(base_accounts_json)<=128),
    target_accounts_json TEXT NOT NULL CHECK (json_valid(target_accounts_json) AND json_type(target_accounts_json)='array' AND json_array_length(target_accounts_json)<=128 AND target_accounts_json<>base_accounts_json),
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
    CHECK (
        (phase='pending' AND version=1 AND updated_at_utc_ms=created_at_utc_ms AND decided_at_utc_ms IS NULL AND activated_at_utc_ms IS NULL)
        OR (phase IN ('staged','denied') AND version=2 AND decided_at_utc_ms IS NOT NULL AND decided_at_utc_ms>=created_at_utc_ms AND decided_at_utc_ms<expires_at_utc_ms AND updated_at_utc_ms=decided_at_utc_ms AND activated_at_utc_ms IS NULL)
        OR (phase='expired' AND version=2 AND decided_at_utc_ms IS NOT NULL AND decided_at_utc_ms>=expires_at_utc_ms AND updated_at_utc_ms=decided_at_utc_ms AND activated_at_utc_ms IS NULL)
        OR (phase='active' AND version=3 AND decided_at_utc_ms IS NOT NULL AND decided_at_utc_ms>=created_at_utc_ms AND decided_at_utc_ms<expires_at_utc_ms AND activated_at_utc_ms IS NOT NULL AND activated_at_utc_ms>=decided_at_utc_ms AND updated_at_utc_ms=activated_at_utc_ms)
    )
) STRICT;

CREATE UNIQUE INDEX client_scope_refresh_pending_uq ON client_scope_refreshes(registration_id)
    WHERE phase IN ('pending','staged');
CREATE UNIQUE INDEX client_scope_refresh_epoch_uq ON client_scope_refreshes(registration_id,target_profile_epoch)
    WHERE phase IN ('staged','active');
