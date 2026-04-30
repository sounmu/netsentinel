-- Expand notification channel types.
--
-- SQLite stores CHECK constraints in the table definition, so widening the
-- accepted enum requires rebuilding the table. Existing rows are copied as-is.

CREATE TABLE IF NOT EXISTS notification_channels_v2 (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    name            TEXT NOT NULL,
    channel_type    TEXT NOT NULL
                    CHECK (channel_type IN ('discord','slack','email','teams','telegram','webhook')),
    enabled         INTEGER NOT NULL DEFAULT 1 CHECK (enabled IN (0,1)),
    config          TEXT NOT NULL DEFAULT '{}',
    created_at      INTEGER NOT NULL DEFAULT (strftime('%s','now')),
    updated_at      INTEGER NOT NULL DEFAULT (strftime('%s','now'))
) STRICT;

INSERT INTO notification_channels_v2 (id, name, channel_type, enabled, config, created_at, updated_at)
SELECT id, name, channel_type, enabled, config, created_at, updated_at
FROM notification_channels;

DROP TABLE notification_channels;
ALTER TABLE notification_channels_v2 RENAME TO notification_channels;
