-- Explicit initial preview. Existing UID cursor, epochs, pages and identities are untouched.
CREATE TABLE imap_initial_preview_plans (
 plan_id BLOB PRIMARY KEY CHECK(length(plan_id)=32 AND plan_id<>zeroblob(32)),
 owner_registration_id BLOB NOT NULL REFERENCES client_registrations(registration_id) ON DELETE RESTRICT,
 account_id TEXT NOT NULL REFERENCES accounts(account_id) ON DELETE RESTRICT CHECK(length(account_id) BETWEEN 1 AND 128 AND account_id NOT GLOB '*[^A-Za-z0-9_-]*'),
 mailbox_key TEXT NOT NULL CHECK(length(mailbox_key) BETWEEN 2 AND 2048 AND length(mailbox_key)%2=0 AND mailbox_key NOT GLOB '*[^0-9a-f]*'),
 mailbox_encoding TEXT NOT NULL CHECK(mailbox_encoding IN ('modified_utf7','utf8')),
 generation INTEGER NOT NULL CHECK(generation>0),
 is_current INTEGER NOT NULL CHECK(is_current IN (0,1)),
 version INTEGER NOT NULL CHECK(version>0),
 uid_validity INTEGER NOT NULL CHECK(uid_validity BETWEEN 1 AND 4294967295),
 uid_next INTEGER NOT NULL CHECK(uid_next BETWEEN 1 AND 4294967295),
 recent_pages INTEGER NOT NULL CHECK(recent_pages BETWEEN 1 AND 8),
 completed_pages INTEGER NOT NULL CHECK(completed_pages BETWEEN 0 AND 8),
 state TEXT NOT NULL CHECK(state IN ('ready','running','pausing','paused_user','paused_budget','interrupted','complete','failed','stale_binding','needs_recovery')),
 model_version INTEGER NOT NULL CHECK(model_version>0),
 binding_json TEXT NOT NULL CHECK(length(CAST(binding_json AS BLOB)) BETWEEN 1 AND 4096),
 binding_hash BLOB NOT NULL CHECK(length(binding_hash)=32),
 active_execution_id BLOB REFERENCES imap_initial_preview_requests(execution_id) ON DELETE RESTRICT,
 latest_execution_id BLOB NOT NULL REFERENCES imap_initial_preview_requests(execution_id) ON DELETE RESTRICT,
 created_at_utc_ms INTEGER NOT NULL CHECK(created_at_utc_ms>0),
 updated_at_utc_ms INTEGER NOT NULL CHECK(updated_at_utc_ms>=created_at_utc_ms),
 UNIQUE(account_id,mailbox_key,mailbox_encoding,generation),
 CHECK((state IN ('running','pausing') AND active_execution_id IS NOT NULL) OR (state NOT IN ('running','pausing') AND active_execution_id IS NULL))
) STRICT;
CREATE UNIQUE INDEX imap_initial_preview_current ON imap_initial_preview_plans(account_id,mailbox_key,mailbox_encoding) WHERE is_current=1;
CREATE TABLE imap_initial_preview_requests (
 execution_id BLOB PRIMARY KEY REFERENCES audit_starts(execution_id) ON DELETE RESTRICT CHECK(length(execution_id)=32 AND execution_id<>zeroblob(32)),
 account_id TEXT NOT NULL REFERENCES accounts(account_id) ON DELETE RESTRICT CHECK(length(account_id) BETWEEN 1 AND 128 AND account_id NOT GLOB '*[^A-Za-z0-9_-]*'),
 mailbox_key TEXT NOT NULL CHECK(length(mailbox_key) BETWEEN 2 AND 2048 AND length(mailbox_key)%2=0 AND mailbox_key NOT GLOB '*[^0-9a-f]*'),
 mailbox_encoding TEXT NOT NULL CHECK(mailbox_encoding IN ('modified_utf7','utf8')),
 operation_id TEXT NOT NULL UNIQUE CHECK(length(operation_id) BETWEEN 1 AND 128 AND operation_id NOT GLOB '*[^A-Za-z0-9_-]*'),
 idempotency_key TEXT NOT NULL CHECK(length(idempotency_key) BETWEEN 1 AND 128 AND idempotency_key NOT GLOB '*[^A-Za-z0-9_-]*'),
 action TEXT NOT NULL CHECK(action IN ('initial-start','initial-resume','initial-pause')),
 request_json TEXT NOT NULL CHECK(length(CAST(request_json AS BLOB)) BETWEEN 1 AND 8192),
 request_hash BLOB NOT NULL CHECK(length(request_hash)=32),
 binding_json TEXT NOT NULL CHECK(length(CAST(binding_json AS BLOB)) BETWEEN 1 AND 4096),
 binding_hash BLOB NOT NULL CHECK(length(binding_hash)=32),
 instance_id BLOB NOT NULL CHECK(length(instance_id)=32 AND instance_id<>zeroblob(32)),
 plan_id BLOB REFERENCES imap_initial_preview_plans(plan_id) ON DELETE RESTRICT,
 step_version INTEGER CHECK(step_version>0),
 step_ordinal INTEGER CHECK(step_ordinal BETWEEN 0 AND 7),
 step_after_uid INTEGER CHECK(step_after_uid BETWEEN 0 AND 4294967294),
 step_through_uid INTEGER CHECK(step_through_uid BETWEEN 1 AND 4294967294),
 state TEXT NOT NULL CHECK(state IN ('admitted','running','succeeded','failed')),
 admitted_at_utc_ms INTEGER NOT NULL CHECK(admitted_at_utc_ms>0),
 started_at_utc_ms INTEGER CHECK(started_at_utc_ms>=admitted_at_utc_ms),
 completed_at_utc_ms INTEGER CHECK(completed_at_utc_ms>=coalesce(started_at_utc_ms,admitted_at_utc_ms)),
 result_json TEXT CHECK(length(CAST(result_json AS BLOB)) BETWEEN 1 AND 8192),
 result_hash BLOB CHECK(length(result_hash)=32),
 error_code TEXT CHECK(error_code IN ('INVALID_INPUT','AUTH_REQUIRED','SCOPE_DENIED','POLICY_BLOCKED','CONFLICT','RATE_LIMITED','OFFLINE','NOT_FOUND','SEND_UNKNOWN','APPROVAL_REQUIRED','APPROVAL_EXPIRED','APPROVAL_DENIED','APPROVAL_RATE_LIMITED','CLIENT_NOT_REGISTERED','CLIENT_SUSPENDED','HUMAN_STEP_REQUIRED','CORE_UNAVAILABLE','VERSION_MISMATCH','CURSOR_EXPIRED','CAPABILITY_UNSUPPORTED','QUOTA_EXCEEDED','INTERNAL_ERROR')),
 UNIQUE(account_id,idempotency_key),
 CHECK((action='initial-resume' AND plan_id IS NOT NULL AND step_version IS NOT NULL AND step_ordinal IS NOT NULL AND step_after_uid IS NOT NULL AND step_through_uid IS NOT NULL AND step_through_uid>step_after_uid AND step_through_uid-step_after_uid<=64) OR (action<>'initial-resume' AND step_version IS NULL AND step_ordinal IS NULL AND step_after_uid IS NULL AND step_through_uid IS NULL)),
 CHECK((state='admitted' AND started_at_utc_ms IS NULL AND completed_at_utc_ms IS NULL AND result_json IS NULL AND result_hash IS NULL AND error_code IS NULL) OR (state='running' AND started_at_utc_ms IS NOT NULL AND completed_at_utc_ms IS NULL AND result_json IS NULL AND result_hash IS NULL AND error_code IS NULL) OR (state='succeeded' AND completed_at_utc_ms IS NOT NULL AND result_json IS NOT NULL AND result_hash IS NOT NULL AND error_code IS NULL) OR (state='failed' AND completed_at_utc_ms IS NOT NULL AND error_code IS NOT NULL AND ((result_json IS NULL AND result_hash IS NULL) OR (result_json IS NOT NULL AND result_hash IS NOT NULL))))
) STRICT;
CREATE INDEX imap_initial_preview_pending ON imap_initial_preview_requests(account_id,mailbox_key,mailbox_encoding,state);
CREATE TABLE imap_initial_preview_pages (
 plan_id BLOB NOT NULL REFERENCES imap_initial_preview_plans(plan_id) ON DELETE RESTRICT,
 ordinal INTEGER NOT NULL CHECK(ordinal BETWEEN 0 AND 7),
 execution_id BLOB NOT NULL UNIQUE REFERENCES imap_initial_preview_requests(execution_id) ON DELETE RESTRICT,
 after_uid INTEGER NOT NULL CHECK(after_uid BETWEEN 0 AND 4294967294),
 covered_through_uid INTEGER NOT NULL CHECK(covered_through_uid BETWEEN 1 AND 4294967294 AND covered_through_uid>after_uid AND covered_through_uid-after_uid<=64),
 page_json TEXT NOT NULL CHECK(length(CAST(page_json AS BLOB)) BETWEEN 1 AND 8388608),
 page_hash BLOB NOT NULL CHECK(length(page_hash)=32),
 inserted_count INTEGER NOT NULL CHECK(inserted_count BETWEEN 0 AND 64),
 stored_at_utc_ms INTEGER NOT NULL CHECK(stored_at_utc_ms>0),
 PRIMARY KEY(plan_id,ordinal)
) STRICT;
CREATE TABLE imap_initial_preview_messages (
 plan_id BLOB NOT NULL,
 ordinal INTEGER NOT NULL,
 uid INTEGER NOT NULL CHECK(uid BETWEEN 1 AND 4294967294),
 internal_message_id TEXT NOT NULL CHECK(length(internal_message_id)=68 AND substr(internal_message_id,1,4)='msg_' AND substr(internal_message_id,5) NOT GLOB '*[^0-9a-f]*'),
 account_id TEXT NOT NULL CHECK(length(account_id) BETWEEN 1 AND 128 AND account_id NOT GLOB '*[^A-Za-z0-9_-]*'),
 mailbox_key TEXT NOT NULL CHECK(length(mailbox_key) BETWEEN 2 AND 2048 AND length(mailbox_key)%2=0 AND mailbox_key NOT GLOB '*[^0-9a-f]*'),
 mailbox_encoding TEXT NOT NULL CHECK(mailbox_encoding IN ('modified_utf7','utf8')),
 uid_validity INTEGER NOT NULL CHECK(uid_validity BETWEEN 1 AND 4294967295),
 metadata_json TEXT NOT NULL CHECK(length(CAST(metadata_json AS BLOB)) BETWEEN 1 AND 8388608),
 metadata_hash BLOB NOT NULL CHECK(length(metadata_hash)=32),
 PRIMARY KEY(plan_id,ordinal,uid), UNIQUE(plan_id,uid),
 FOREIGN KEY(plan_id,ordinal) REFERENCES imap_initial_preview_pages(plan_id,ordinal) ON DELETE RESTRICT
) STRICT;
CREATE INDEX imap_initial_preview_message_id ON imap_initial_preview_messages(internal_message_id);
CREATE INDEX imap_initial_preview_locator ON imap_initial_preview_messages(account_id,mailbox_key,mailbox_encoding,uid_validity,uid);
