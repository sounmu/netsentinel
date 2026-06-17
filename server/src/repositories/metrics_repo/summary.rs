use std::collections::HashMap;

use crate::db::DbPool;

use super::rows::HostSummary;

/// Fetch all monitored hosts with their latest online status.
///
/// A host is online iff its most recent metric landed in the past 60 s.
/// Window the subquery to the past 5 minutes so SQLite can skip older
/// chunks entirely.
///
/// Shape note: the previous implementation issued **two correlated scalar
/// subqueries** — one for `is_online`, another for `last_seen` — meaning
/// SQLite re-scanned the `recent` CTE twice per host row. This rewrite
/// picks the latest-per-host row once via `LEFT JOIN` on the `rn = 1`
/// filter of the window function, reusing the same scan for both output
/// columns. Hosts with no recent metric land at `is_online = 0` /
/// `last_seen = NULL` via the LEFT side of the join.
pub async fn fetch_host_summaries(pool: &DbPool) -> Result<Vec<HostSummary>, sqlx::Error> {
    sqlx::query_as::<_, HostSummary>(
        r#"
        SELECT
            h.host_key,
            h.display_name,
            COALESCE(r.is_online = 1 AND r.timestamp > strftime('%s','now') - 60, 0) AS is_online,
            r.timestamp AS last_seen
        FROM hosts h
        LEFT JOIN (
            SELECT host_key, is_online, timestamp,
                   ROW_NUMBER() OVER (PARTITION BY host_key
                                      ORDER BY timestamp DESC, id DESC) AS rn
            FROM metrics
            WHERE timestamp > strftime('%s','now') - 300
        ) r ON r.host_key = h.host_key AND r.rn = 1
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
            host_key,
            CASE
                WHEN SUM(sample_count) > 0
                THEN (CAST(SUM(CASE WHEN is_online = 1 THEN sample_count ELSE 0 END) AS REAL)
                      / CAST(SUM(sample_count) AS REAL)) * 100.0
                ELSE 0.0
            END AS uptime_pct
        FROM metrics_5min
        WHERE bucket >= strftime('%s','now') - ?1 * 86400
        GROUP BY host_key
        "#,
    )
    .bind(days)
    .fetch_all(pool)
    .await?;
    Ok(rows.into_iter().collect())
}
