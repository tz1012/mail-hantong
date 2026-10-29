-- Rebuild both setup tables inside the caller's migration transaction.
-- TEMP copies retain every v5 column without changing values or persisting a second history.
CREATE TEMP TABLE imap_setup_requests_v5_copy AS SELECT * FROM imap_setup_requests;
CREATE TEMP TABLE imap_setup_events_v5_copy AS SELECT * FROM imap_setup_events;
DROP TABLE imap_setup_events;
DROP TABLE imap_setup_requests;
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

INSERT INTO imap_setup_requests(request_id,account_id,operation_id,idempotency_key,display_name,color,endpoint_version,host,port,tls,login_name,authentication,payload_hash,origin,phase,version,created_at_utc_ms,expires_at_utc_ms,updated_at_utc_ms) SELECT request_id,account_id,operation_id,idempotency_key,display_name,color,endpoint_version,host,port,tls,login_name,authentication,payload_hash,origin,phase,version,created_at_utc_ms,expires_at_utc_ms,updated_at_utc_ms FROM temp.imap_setup_requests_v5_copy;
INSERT INTO imap_setup_events(request_id,seq,kind,occurred_at_utc_ms,result_kind,error_code,probe_outcome,capabilities,account_version,endpoint_version,observation_version) SELECT request_id,seq,kind,occurred_at_utc_ms,result_kind,error_code,probe_outcome,capabilities,account_version,endpoint_version,observation_version FROM temp.imap_setup_events_v5_copy;
DROP TABLE temp.imap_setup_events_v5_copy;
DROP TABLE temp.imap_setup_requests_v5_copy;
