-- Hybrid auth: local username/password + optional Google OAuth linkage.
--
-- Rebuilds the 0001 `users` table to:
--   * make password_hash nullable (OAuth-only accounts have no local password),
--   * add oauth_provider / oauth_subject / email / display_name / picture_url,
--   * enforce UNIQUE (oauth_provider, oauth_subject) for OAuth identity,
--   * keep username UNIQUE so existing local accounts log in unchanged.
--
-- Existing rows from 0001 keep their password_hash and get email := username
-- as a placeholder; operators backfill real emails afterwards. Refresh tokens
-- carry over via the FK rebind below.

PRAGMA foreign_keys=OFF;

CREATE TABLE IF NOT EXISTS users_new (
    id                    INTEGER PRIMARY KEY AUTOINCREMENT,
    username              TEXT NOT NULL UNIQUE,
    password_hash         TEXT,
    oauth_provider        TEXT,
    oauth_subject         TEXT,
    email                 TEXT NOT NULL,
    display_name          TEXT,
    picture_url           TEXT,
    role                  TEXT NOT NULL DEFAULT 'viewer'
                          CHECK (role IN ('admin','viewer')),
    password_changed_at   INTEGER,
    tokens_revoked_at     INTEGER,
    created_at            INTEGER NOT NULL DEFAULT (strftime('%s','now')),
    updated_at            INTEGER NOT NULL DEFAULT (strftime('%s','now')),
    UNIQUE (oauth_provider, oauth_subject)
) STRICT;

INSERT INTO users_new (
    id,
    username,
    password_hash,
    oauth_provider,
    oauth_subject,
    email,
    display_name,
    picture_url,
    role,
    password_changed_at,
    tokens_revoked_at,
    created_at,
    updated_at
)
SELECT
    id,
    username,
    password_hash,
    NULL,
    NULL,
    username,
    NULL,
    NULL,
    role,
    password_changed_at,
    tokens_revoked_at,
    created_at,
    updated_at
FROM users;

DROP TABLE users;
ALTER TABLE users_new RENAME TO users;

CREATE INDEX IF NOT EXISTS idx_users_email
    ON users(email);

PRAGMA foreign_keys=ON;
