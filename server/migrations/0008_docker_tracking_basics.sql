-- Docker tracking baseline improvements.
--
-- 1) Add `docker` as an alert metric type. SQLite CHECK constraints are
--    table-level text, so widening the enum requires a table rebuild.
-- 2) Preserve docker_containers snapshots in the 5-minute rollup table.
--    This keeps lifecycle/Compose/health state available after raw rows age out.

CREATE TABLE IF NOT EXISTS alert_configs_v2 (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    host_key        TEXT REFERENCES hosts(host_key) ON DELETE CASCADE,
    metric_type     TEXT NOT NULL
                    CHECK (metric_type IN
                        ('cpu','memory','disk','load','network','temperature','gpu','docker')),
    sub_key         TEXT,
    enabled         INTEGER NOT NULL DEFAULT 1 CHECK (enabled IN (0,1)),
    threshold       REAL NOT NULL,
    sustained_secs  INTEGER NOT NULL DEFAULT 300
                    CHECK (sustained_secs BETWEEN 0 AND 3600),
    cooldown_secs   INTEGER NOT NULL DEFAULT 1800
                    CHECK (cooldown_secs BETWEEN 0 AND 86400),
    created_at      INTEGER NOT NULL DEFAULT (strftime('%s','now')),
    updated_at      INTEGER NOT NULL DEFAULT (strftime('%s','now'))
) STRICT;

INSERT INTO alert_configs_v2 (
    id, host_key, metric_type, sub_key, enabled, threshold,
    sustained_secs, cooldown_secs, created_at, updated_at
)
SELECT
    id, host_key, metric_type, sub_key, enabled, threshold,
    sustained_secs, cooldown_secs, created_at, updated_at
FROM alert_configs;

DROP TABLE alert_configs;
ALTER TABLE alert_configs_v2 RENAME TO alert_configs;

CREATE UNIQUE INDEX IF NOT EXISTS idx_alert_configs_scope_unique
    ON alert_configs (
        coalesce(host_key, ''),
        metric_type,
        coalesce(sub_key, '')
    );

CREATE INDEX IF NOT EXISTS idx_alert_configs_host
    ON alert_configs(host_key, metric_type);

CREATE TABLE IF NOT EXISTS metrics_5min_v2 (
    host_key              TEXT NOT NULL,
    bucket                INTEGER NOT NULL,
    cpu_usage_percent     REAL,
    memory_usage_percent  REAL,
    load_1min             REAL,
    load_5min             REAL,
    load_15min            REAL,
    is_online             INTEGER,
    sample_count          INTEGER NOT NULL DEFAULT 0,
    total_rx_bytes        INTEGER,
    total_tx_bytes        INTEGER,
    disks                 TEXT,
    temperatures          TEXT,
    gpus                  TEXT,
    docker_stats          TEXT,
    avg_rx_bytes_per_sec  REAL,
    avg_tx_bytes_per_sec  REAL,
    docker_containers     TEXT,
    PRIMARY KEY (host_key, bucket)
) STRICT, WITHOUT ROWID;

INSERT INTO metrics_5min_v2 (
    host_key, bucket,
    cpu_usage_percent, memory_usage_percent,
    load_1min, load_5min, load_15min,
    is_online, sample_count,
    total_rx_bytes, total_tx_bytes,
    disks, temperatures, gpus, docker_stats,
    avg_rx_bytes_per_sec, avg_tx_bytes_per_sec
)
SELECT
    host_key, bucket,
    cpu_usage_percent, memory_usage_percent,
    load_1min, load_5min, load_15min,
    is_online, sample_count,
    total_rx_bytes, total_tx_bytes,
    disks, temperatures, gpus, docker_stats,
    avg_rx_bytes_per_sec, avg_tx_bytes_per_sec
FROM metrics_5min;

DROP TABLE metrics_5min;
ALTER TABLE metrics_5min_v2 RENAME TO metrics_5min;

CREATE INDEX IF NOT EXISTS idx_metrics_5min_time
    ON metrics_5min(bucket DESC);

CREATE INDEX IF NOT EXISTS idx_metrics_5min_host_bucket_online
    ON metrics_5min(host_key, bucket DESC, is_online, sample_count);
