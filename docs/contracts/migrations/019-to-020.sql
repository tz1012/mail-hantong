
-- Immutable bytes inside SQLCipher; no filesystem paths or credential material.
CREATE TABLE mail_draft_attachments (
 draft_id BLOB NOT NULL REFERENCES mail_drafts(draft_id) ON DELETE RESTRICT,
 ordinal INTEGER NOT NULL CHECK(ordinal BETWEEN 0 AND 7),
 filename TEXT NOT NULL CHECK(length(filename) BETWEEN 1 AND 120 AND filename NOT GLOB '*[^A-Za-z0-9._-]*'),
 media_type TEXT NOT NULL CHECK(media_type IN ('text/plain','application/octet-stream','application/pdf','image/png','image/jpeg')),
 content BLOB NOT NULL CHECK(length(content)<=1048576),
 PRIMARY KEY(draft_id,ordinal)
) STRICT;
