-- Host summary lookups need the latest raw metric per host with a stable
-- same-second tie-breaker. Replace the older host/time index with a strict
-- superset that also covers the summary's online flag.
DROP INDEX IF EXISTS idx_metrics_host_time;

CREATE INDEX IF NOT EXISTS idx_metrics_host_time_id_online
    ON metrics(host_key, timestamp DESC, id DESC, is_online);
