use std::collections::HashMap;

use crate::db::DbPool;

use super::rows::HostSummary;

/// Fetch all monitored hosts with their latest online status.
///
/// A host is online iff its most recent online metric is still fresh for that
/// host's configured scrape cadence. The default 10 s scrape keeps the old
/// 60 s grace window; slower per-host scrapes get a 3-interval window so they
/// do not appear offline between expected samples.
/// The query probes the latest recent row per host through
/// `idx_metrics_host_time_id_online`. That keeps the hot dashboard/status
/// path bounded by host count and the 5-minute window instead of materialising
/// a window over the full raw metrics index.
pub async fn fetch_host_summaries(pool: &DbPool) -> Result<Vec<HostSummary>, sqlx::Error> {
    sqlx::query_as::<_, HostSummary>(
        r#"
        SELECT
            h.host_key,
            h.display_name,
            COALESCE(
                m.is_online = 1
                AND m.timestamp > strftime('%s','now') - max(60, h.scrape_interval_secs * 3),
                0
            ) AS is_online,
            m.timestamp AS last_seen
        FROM hosts h
        LEFT JOIN metrics m ON m.id = (
            SELECT x.id
            FROM metrics x
            WHERE x.host_key = h.host_key
              AND x.timestamp > strftime('%s','now') - max(300, h.scrape_interval_secs * 3)
            ORDER BY x.timestamp DESC, x.id DESC
            LIMIT 1
        )
        ORDER BY h.host_key
        "#,
    )
    .fetch_all(pool)
    .await
}

/// Fetch N-day overall uptime percentage for all hosts in a single query.
/// Returns a HashMap<host_key, uptime_pct> — used by public_status to avoid N+1 queries.
pub async fn fetch_batch_uptime_pct(
    pool: &DbPool,
    days: i32,
) -> Result<HashMap<String, f64>, sqlx::Error> {
    let rows: Vec<(String, f64)> = sqlx::query_as(
        r#"
        SELECT
            h.host_key,
            CASE
                WHEN COALESCE(SUM(m.sample_count), 0) > 0
                THEN (CAST(SUM(CASE WHEN m.is_online = 1 THEN m.sample_count ELSE 0 END) AS REAL)
                      / CAST(SUM(m.sample_count) AS REAL)) * 100.0
                ELSE 0.0
            END AS uptime_pct
        FROM hosts h
        LEFT JOIN metrics_5min m
          ON m.host_key = h.host_key
         AND m.bucket >= strftime('%s','now') - ?1 * 86400
        GROUP BY h.host_key
        "#,
    )
    .bind(days)
    .fetch_all(pool)
    .await?;
    Ok(rows.into_iter().collect())
}
