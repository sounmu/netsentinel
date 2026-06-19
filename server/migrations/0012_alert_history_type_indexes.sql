-- Alert-history pagination can filter by alert_type alone or together with
-- host_key while ordering by newest first. These indexes avoid scanning the
-- time index and discarding unrelated alert kinds.
CREATE INDEX IF NOT EXISTS idx_alert_history_type_time
    ON alert_history(alert_type, created_at DESC);

CREATE INDEX IF NOT EXISTS idx_alert_history_host_type_time
    ON alert_history(host_key, alert_type, created_at DESC);
