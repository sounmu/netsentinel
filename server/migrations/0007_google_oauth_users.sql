-- Google OAuth replaces local username/password accounts.
--
-- This migration is intentionally destructive for auth tables: existing
-- local-password users and refresh sessions cannot be safely mapped to a
-- verified Google subject. Operators should plan this as an auth cutover.

DROP TABLE IF EXISTS refresh_tokens;
DROP TABLE IF EXISTS users;

CREATE TABLE IF NOT EXISTS users (
    id                    INTEGER PRIMARY KEY AUTOINCREMENT,
    oauth_provider        TEXT NOT NULL DEFAULT 'google',
    oauth_subject         TEXT NOT NULL,
    email                 TEXT NOT NULL,
    display_name          TEXT,
    picture_url           TEXT,
    role                  TEXT NOT NULL DEFAULT 'viewer'
                          CHECK (role IN ('admin','viewer')),
    tokens_revoked_at     INTEGER,
    created_at            INTEGER NOT NULL DEFAULT (strftime('%s','now')),
    updated_at            INTEGER NOT NULL DEFAULT (strftime('%s','now')),
    UNIQUE (oauth_provider, oauth_subject)
) STRICT;

CREATE INDEX IF NOT EXISTS idx_users_email
    ON users(email);

CREATE TABLE IF NOT EXISTS refresh_tokens (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    user_id         INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    token_hash      BLOB NOT NULL UNIQUE,
    family_id       BLOB NOT NULL,
    parent_id       INTEGER REFERENCES refresh_tokens(id) ON DELETE SET NULL,
    issued_at       INTEGER NOT NULL DEFAULT (strftime('%s','now')),
    expires_at      INTEGER NOT NULL,
    revoked_at      INTEGER,
    user_agent      TEXT,
    ip              TEXT
) STRICT;

CREATE INDEX IF NOT EXISTS idx_refresh_tokens_user
    ON refresh_tokens(user_id, expires_at DESC);

CREATE INDEX IF NOT EXISTS idx_refresh_tokens_family
    ON refresh_tokens(family_id);
