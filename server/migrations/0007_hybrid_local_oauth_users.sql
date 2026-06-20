-- Hybrid auth: local username/password + optional Google OAuth linkage.
--
-- Adds Google OAuth profile/linkage columns while preserving the original
-- `users` table identity. Keeping the table in place is intentional:
-- `refresh_tokens.user_id` has `ON DELETE CASCADE`, and SQLx runs SQLite
-- migrations in a transaction. A DROP/RENAME table rebuild would therefore
-- delete existing refresh-token sessions before PRAGMA foreign_keys=OFF could
-- take effect.
--
-- `password_hash` remains NOT NULL for existing SQLite installs. OAuth-only
-- users store an internal sentinel hash value; repository SELECTs expose that
-- sentinel as NULL so application code still treats the account as having no
-- local password until one is set.

ALTER TABLE users
    ADD COLUMN oauth_provider TEXT;

ALTER TABLE users
    ADD COLUMN oauth_subject TEXT;

ALTER TABLE users
    ADD COLUMN email TEXT NOT NULL DEFAULT '';

ALTER TABLE users
    ADD COLUMN display_name TEXT;

ALTER TABLE users
    ADD COLUMN picture_url TEXT;

UPDATE users
SET email = username
WHERE email = '';

CREATE INDEX IF NOT EXISTS idx_users_email
    ON users(email);

CREATE UNIQUE INDEX IF NOT EXISTS idx_users_oauth_subject
    ON users(oauth_provider, oauth_subject);
