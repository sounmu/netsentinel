-- Restore local username/password auth while keeping Google OAuth linkage.
--
-- Existing OAuth-only rows are preserved with username=email. Local accounts
-- store password_hash and may later be linked to Google by verified email.

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
    email,
    NULL,
    oauth_provider,
    oauth_subject,
    email,
    display_name,
    picture_url,
    role,
    NULL,
    tokens_revoked_at,
    created_at,
    updated_at
FROM users;

DROP TABLE users;
ALTER TABLE users_new RENAME TO users;

CREATE INDEX IF NOT EXISTS idx_users_email
    ON users(email);

PRAGMA foreign_keys=ON;
