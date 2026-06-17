use chrono::{DateTime, Utc};

use crate::db::DbPool;

use super::rows::{MetricsRow, MetricsRowRaw};
use super::tiers::{RANGE_RAW_BOUNDARY_HOURS, ROLLUP_BOUNDARY_HOURS};

/// Fetch the most recent 50 metrics for a host, ordered newest first.
///
/// Trimmed projection — `processes` / `cpu_cores` / `network_interfaces` /
/// `ports` / `docker_containers` are returned as NULL because:
///   1. Only the latest row drives the dashboard's headline summary cards
///      (cpu / memory / disk / network rate). Older points are catch-up
///      data, not history — for history the UI calls the chart endpoint.
///   2. Live updates flow through SSE, so any heavy-JSON snapshot is
///      already streamed in within seconds of the page mounting.
///   3. Each row's full snapshot can be tens of KB on hosts with many
///      processes / containers; 50 rows × that × N hosts cold-loads was a
///      noticeable dashboard p95 hit before this trim.
///
/// Mirrors the ≤6h branch of `fetch_metrics_range` to keep the row shape
/// consistent across both code paths that feed the same `MetricsRow` type.
pub async fn fetch_recent_metrics(
    pool: &DbPool,
    host_key: &str,
) -> Result<Vec<MetricsRow>, sqlx::Error> {
    // `total_rx_bytes` / `total_tx_bytes` are now read directly (migration
    // 0005). The TryFrom path uses these to synthesize `networks` for
    // headline-card display when the heavy `networks` JSON has been read.
    // Keep `networks` itself in the projection because it carries
    // per-interface detail the host card may render (top device, interface
    // counters); only the totals are needed for the chart-style summary.
    let raws = sqlx::query_as::<_, MetricsRowRaw>(
        r#"
        SELECT id, host_key, display_name, is_online,
               cpu_usage_percent, memory_usage_percent,
               load_1min, load_5min, load_15min,
               networks,
               NULL AS docker_containers,
               NULL AS ports,
               disks,
               NULL AS processes,
               temperatures,
               gpus,
               NULL AS cpu_cores,
               NULL AS network_interfaces,
               docker_stats,
               rx_bytes_per_sec, tx_bytes_per_sec,
               total_rx_bytes,
               total_tx_bytes,
               timestamp
        FROM metrics
        WHERE host_key = ?1
        ORDER BY timestamp DESC, id DESC
        LIMIT 50
        "#,
    )
    .bind(host_key)
    .fetch_all(pool)
    .await?;
    raws.into_iter().map(MetricsRow::try_from).collect()
}

/// Fetch metrics for a host within a given time range, ordered oldest first.
///
/// Automatically downsamples long ranges to keep response size manageable:
/// - ≤6h: raw rows (every 10s) from `metrics` table
/// - 6h–14d: 5-minute pre-aggregated rows from `metrics_5min` rollup table
/// - >14d: 15-minute re-aggregated rows from `metrics_5min`
///
/// JSON columns (processes, temperatures, gpus, disks, docker_containers, ports) are
/// trimmed from time-range queries when the chart layer doesn't need them.
pub async fn fetch_metrics_range(
    pool: &DbPool,
    host_key: &str,
    start: DateTime<Utc>,
    end: DateTime<Utc>,
) -> Result<Vec<MetricsRow>, sqlx::Error> {
    let duration = end - start;
    let hours = duration.num_hours();

    if hours <= RANGE_RAW_BOUNDARY_HOURS {
        // Short range: raw rows. Trim JSON columns the chart layer never
        // reads (ports, docker_containers, cpu_cores, processes).
        // `total_rx_bytes` / `total_tx_bytes` come from migration 0005's
        // scalar columns instead of `json_extract(networks, …)` per row.
        let raws = sqlx::query_as::<_, MetricsRowRaw>(
            r#"
            SELECT id, host_key, display_name, is_online,
                   cpu_usage_percent, memory_usage_percent,
                   load_1min, load_5min, load_15min,
                   networks,
                   NULL AS docker_containers,
                   NULL AS ports,
                   disks,
                   NULL AS processes,
                   temperatures,
                   gpus,
                   NULL AS cpu_cores,
                   NULL AS network_interfaces,
                   docker_stats,
                   rx_bytes_per_sec,
                   tx_bytes_per_sec,
                   total_rx_bytes,
                   total_tx_bytes,
                   timestamp
            FROM metrics
            WHERE host_key = ?1
              AND timestamp >= ?2
              AND timestamp <= ?3
            ORDER BY timestamp ASC, id ASC
            "#,
        )
        .bind(host_key)
        .bind(start.timestamp())
        .bind(end.timestamp())
        .fetch_all(pool)
        .await?;
        // `id ASC` is the tie-breaker for rows inserted within the same
        // second — without it, `ORDER BY timestamp ASC` alone leaves the
        // relative ordering of same-second rows unspecified, which showed
        // up as line-chart jitter when a flapping host emitted two scrapes
        // in the same wall-clock second.
        return raws.into_iter().map(MetricsRow::try_from).collect();
    }

    if hours <= ROLLUP_BOUNDARY_HOURS {
        // 6h–14d: direct read from metrics_5min rollup (populated by the
        // rollup worker). Return the scalar bandwidth totals directly; the
        // `networks` JSON object is synthesized Rust-side in
        // `TryFrom<MetricsRowRaw>`. Skipping SQLite's per-row
        // `json_object(...)` measurably lowers query CPU over long ranges.
        let raws = sqlx::query_as::<_, MetricsRowRaw>(
            r#"
            SELECT
                0 AS id,
                host_key,
                '' AS display_name,
                CAST(is_online AS INTEGER) AS is_online,
                cpu_usage_percent,
                memory_usage_percent,
                load_1min, load_5min, load_15min,
                NULL AS networks,
                docker_containers,
                NULL AS ports,
                disks,
                NULL AS processes,
                temperatures,
                gpus,
                NULL AS cpu_cores,
                NULL AS network_interfaces,
                docker_stats,
                avg_rx_bytes_per_sec AS rx_bytes_per_sec,
                avg_tx_bytes_per_sec AS tx_bytes_per_sec,
                total_rx_bytes,
                total_tx_bytes,
                bucket AS timestamp
            FROM metrics_5min
            WHERE host_key = ?1
              AND bucket >= ?2
              AND bucket <= ?3
            ORDER BY bucket ASC
            "#,
        )
        .bind(host_key)
        .bind(start.timestamp())
        .bind(end.timestamp())
        .fetch_all(pool)
        .await?;
        return raws.into_iter().map(MetricsRow::try_from).collect();
    }

    // >14d: re-aggregate the 5-min rollup into 15-min buckets.
    //
    // The previous shape issued **four correlated subqueries per output row**
    // (disks / temperatures / gpus / docker_stats), each a separate index
    // lookup on `metrics_5min`. At 30 days × 96 buckets/day that is 11 520
    // subquery probes per host per request — the biggest single contributor
    // to dashboard p95 latency for long ranges.
    //
    // Rewritten shape: a single CTE tags every 5-min row with its 15-min
    // bucket and a `ROW_NUMBER() OVER (PARTITION BY host_key, bucket_15m
    // ORDER BY bucket DESC)` window so the "last row in bucket" can be
    // picked in one pass with `MAX(CASE WHEN rn = 1 THEN col END)`. Scalar
    // averages stay as plain aggregates. Net effect: 1 table scan + 1
    // window + 1 GROUP BY instead of the N×4 correlated subqueries.
    let raws = sqlx::query_as::<_, MetricsRowRaw>(
        r#"
        WITH tagged AS (
            SELECT
                host_key,
                (bucket / 900) * 900 AS bucket_15m,
                bucket,
                is_online,
                cpu_usage_percent,
                memory_usage_percent,
                load_1min, load_5min, load_15min,
                total_rx_bytes, total_tx_bytes,
                avg_rx_bytes_per_sec, avg_tx_bytes_per_sec,
                disks, temperatures, gpus, docker_stats, docker_containers,
                ROW_NUMBER() OVER (
                    PARTITION BY host_key, (bucket / 900) * 900
                    ORDER BY bucket DESC
                ) AS rn
            FROM metrics_5min
            WHERE host_key = ?1
              AND bucket >= ?2
              AND bucket <= ?3
        )
        SELECT
            0 AS id,
            host_key,
            '' AS display_name,
            CAST(MIN(is_online) AS INTEGER) AS is_online,
            CAST(AVG(cpu_usage_percent) AS REAL) AS cpu_usage_percent,
            CAST(AVG(memory_usage_percent) AS REAL) AS memory_usage_percent,
            CAST(AVG(load_1min) AS REAL) AS load_1min,
            CAST(AVG(load_5min) AS REAL) AS load_5min,
            CAST(AVG(load_15min) AS REAL) AS load_15min,
            NULL AS networks,
            MAX(CASE WHEN rn = 1 THEN docker_containers END) AS docker_containers,
            NULL AS ports,
            MAX(CASE WHEN rn = 1 THEN disks END) AS disks,
            NULL AS processes,
            MAX(CASE WHEN rn = 1 THEN temperatures END) AS temperatures,
            MAX(CASE WHEN rn = 1 THEN gpus END) AS gpus,
            NULL AS cpu_cores,
            NULL AS network_interfaces,
            MAX(CASE WHEN rn = 1 THEN docker_stats END) AS docker_stats,
            CAST(AVG(avg_rx_bytes_per_sec) AS REAL) AS rx_bytes_per_sec,
            CAST(AVG(avg_tx_bytes_per_sec) AS REAL) AS tx_bytes_per_sec,
            MAX(total_rx_bytes) AS total_rx_bytes,
            MAX(total_tx_bytes) AS total_tx_bytes,
            bucket_15m AS timestamp
        FROM tagged
        GROUP BY host_key, bucket_15m
        ORDER BY timestamp ASC
        "#,
    )
    .bind(host_key)
    .bind(start.timestamp())
    .bind(end.timestamp())
    .fetch_all(pool)
    .await?;
    raws.into_iter().map(MetricsRow::try_from).collect()
}
