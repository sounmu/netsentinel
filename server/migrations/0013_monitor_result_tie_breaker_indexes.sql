-- Per-monitor result history and summaries order by created_at plus id for a
-- deterministic same-second winner. Replace the older monitor/time indexes
-- with id-aware equivalents so SQLite does not need a temp b-tree for the
-- final ORDER BY term.
DROP INDEX IF EXISTS idx_http_results_monitor_time;
DROP INDEX IF EXISTS idx_ping_results_monitor_time;

CREATE INDEX IF NOT EXISTS idx_http_results_monitor_time
    ON http_monitor_results(monitor_id, created_at DESC, id DESC);

CREATE INDEX IF NOT EXISTS idx_ping_results_monitor_time
    ON ping_results(monitor_id, created_at DESC, id DESC);
