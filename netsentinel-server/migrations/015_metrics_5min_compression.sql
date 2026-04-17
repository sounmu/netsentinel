-- Compression policy for the metrics_5min continuous aggregate.
--
-- Context: the base `metrics` hypertable already has compression after 7 days
-- (migration 008), but the `metrics_5min` CA grows unbounded because its JSONB
-- snapshot columns (disks, temperatures, gpus, docker_stats) were added in
-- migration 014 and are the dominant storage cost for long-range queries.
-- Without compression the CA projects to ~5 GB over 90 days × 50 hosts.
--
-- Policy: compress chunks of the CA's underlying hypertable once they are
-- older than 14 days, segmented by host_key. 14 days is the boundary between
-- the "CA direct" and "CA re-aggregate" query paths in fetch_metrics_range,
-- so compression only hits the re-aggregate range where we scan whole chunks
-- anyway and compression decompression cost is amortized.
--
-- Idempotent: both the ALTER MATERIALIZED VIEW and add_compression_policy
-- calls tolerate re-runs (IF NOT EXISTS on the policy; ALTER is no-op when
-- already set to the same value).

ALTER MATERIALIZED VIEW metrics_5min
    SET (timescaledb.compress = true,
         timescaledb.compress_segmentby = 'host_key');

SELECT add_compression_policy(
    'metrics_5min',
    compress_after => INTERVAL '14 days',
    if_not_exists => TRUE
);
