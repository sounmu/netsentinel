-- Agent enrollment and per-agent scrape secrets.
--
-- New installs should not require copying the server-wide JWT_SECRET onto
-- every monitored host. Enrollment tokens are short-lived, one-time bootstrap
-- credentials. A successful claim writes an agent-specific scrape secret to
-- hosts.agent_auth_secret; legacy rows with NULL continue to use JWT_SECRET.

ALTER TABLE hosts ADD COLUMN agent_auth_secret TEXT;

CREATE TABLE IF NOT EXISTS agent_enrollment_tokens (
    id                  INTEGER PRIMARY KEY AUTOINCREMENT,
    label               TEXT,
    token_hash          BLOB NOT NULL UNIQUE,
    expires_at          INTEGER NOT NULL,
    used_at             INTEGER,
    used_by_host_key    TEXT,
    created_by_user_id  INTEGER REFERENCES users(id) ON DELETE SET NULL,
    created_at          INTEGER NOT NULL DEFAULT (strftime('%s','now'))
) STRICT;

CREATE INDEX IF NOT EXISTS idx_agent_enrollment_tokens_expires
    ON agent_enrollment_tokens(expires_at);
