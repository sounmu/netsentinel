use chrono::{DateTime, Utc};

use crate::db::DbPool;

use super::rows::{ChartMetricsRow, ChartMetricsRowRaw};
use super::tiers::{CHART_RAW_BOUNDARY_SECS, ROLLUP_BOUNDARY_SECS};

/// Fetch chart-ready metrics for a host within a time range.
///
/// This is intentionally narrower than `fetch_metrics_range`: it keeps the
/// scalar time series and the small chart-only projections for disk,
/// temperature, and Docker graphs, while omitting large snapshot fields that
/// belong to status/detail panels.
pub async fn fetch_chart_metrics_range(
    pool: &DbPool,
    host_key: &str,
    start: DateTime<Utc>,
    end: DateTime<Utc>,
) -> Result<Vec<ChartMetricsRow>, sqlx::Error> {
    let duration = end - start;
    let seconds = duration.num_seconds();

    if seconds <= CHART_RAW_BOUNDARY_SECS {
        // Migration 0005 projects `total_rx_bytes` / `total_tx_bytes` into
        // their own INTEGER columns. Read them directly. Rows inserted
        // before 0005 ran will have NULL for these columns; fall back to
        // the JSON blob in that case so historical queries remain correct
        // until rolling deploys finish and old raw rows age out (3 day
        // retention).
        let raws = sqlx::query_as::<_, ChartMetricsRowRaw>(
            r#"
            SELECT id, host_key, display_name, is_online,
                   cpu_usage_percent, memory_usage_percent,
                   load_1min, load_5min, load_15min,
                   COALESCE(
                       total_rx_bytes,
                       CAST(json_extract(networks, '$.total_rx_bytes') AS INTEGER)
                   ) AS total_rx_bytes,
                   COALESCE(
                       total_tx_bytes,
                       CAST(json_extract(networks, '$.total_tx_bytes') AS INTEGER)
                   ) AS total_tx_bytes,
                   rx_bytes_per_sec,
                   tx_bytes_per_sec,
                   disks,
                   temperatures,
                   docker_stats,
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
        return raws.into_iter().map(ChartMetricsRow::try_from).collect();
    }

    if seconds <= ROLLUP_BOUNDARY_SECS {
        let raws = sqlx::query_as::<_, ChartMetricsRowRaw>(
            r#"
            SELECT
                0 AS id,
                host_key,
                '' AS display_name,
                CAST(is_online AS INTEGER) AS is_online,
                cpu_usage_percent,
                memory_usage_percent,
                load_1min, load_5min, load_15min,
                total_rx_bytes,
                total_tx_bytes,
                avg_rx_bytes_per_sec AS rx_bytes_per_sec,
                avg_tx_bytes_per_sec AS tx_bytes_per_sec,
                disks,
                temperatures,
                docker_stats,
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
        return raws.into_iter().map(ChartMetricsRow::try_from).collect();
    }

    let raws = sqlx::query_as::<_, ChartMetricsRowRaw>(
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
                disks, temperatures, docker_stats,
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
            MAX(total_rx_bytes) AS total_rx_bytes,
            MAX(total_tx_bytes) AS total_tx_bytes,
            CAST(AVG(avg_rx_bytes_per_sec) AS REAL) AS rx_bytes_per_sec,
            CAST(AVG(avg_tx_bytes_per_sec) AS REAL) AS tx_bytes_per_sec,
            MAX(CASE WHEN rn = 1 THEN disks END) AS disks,
            MAX(CASE WHEN rn = 1 THEN temperatures END) AS temperatures,
            MAX(CASE WHEN rn = 1 THEN docker_stats END) AS docker_stats,
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
    raws.into_iter().map(ChartMetricsRow::try_from).collect()
}
