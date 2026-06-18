-- Retention prunes monitor result tables by created_at only. The existing
-- (monitor_id, created_at) indexes serve per-monitor history lookups, but
-- cannot reliably provide a direct time-range search for retention.
CREATE INDEX IF NOT EXISTS idx_http_monitor_results_created_at
    ON http_monitor_results(created_at DESC);

CREATE INDEX IF NOT EXISTS idx_ping_results_created_at
    ON ping_results(created_at DESC);
