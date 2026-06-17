use std::collections::BTreeMap;

use chrono::{DateTime, Utc};

use crate::db::DbPool;

use super::rows::{UptimePoint, UptimeSummary};

/// Compute daily uptime percentage for a host over the given number of days,
/// grouping days by calendar day in the workspace timezone `tz`.
///
/// The 5-min rollup rows are pulled and grouped **in Rust** rather than in SQL:
/// SQLite has no IANA/DST support and the `'localtime'` modifier is
/// intentionally avoided, so DST-correct calendar-day boundaries are computed
/// via `time_util::day_start_utc`. The lookback window stays a UTC rolling
/// window (`strftime('%s','now')`). With `tz == UTC` the result is identical to
/// the previous UTC-day grouping.
pub async fn fetch_uptime(
    pool: &DbPool,
    host_key: &str,
    days: i32,
    tz: chrono_tz::Tz,
) -> Result<UptimeSummary, sqlx::Error> {
    let rows = sqlx::query_as::<_, (i64, i64, i64)>(
        r#"
        SELECT bucket,
               CAST(is_online AS INTEGER) AS is_online,
               CAST(sample_count AS INTEGER) AS sample_count
        FROM metrics_5min
        WHERE host_key = ?1
          AND bucket >= strftime('%s','now') - ?2 * 86400
        "#,
    )
    .bind(host_key)
    .bind(days)
    .fetch_all(pool)
    .await?;

    // workspace-tz day start (UTC epoch) -> (total_samples, online_samples)
    let mut by_day: BTreeMap<i64, (i64, i64)> = BTreeMap::new();
    let (mut total, mut online) = (0i64, 0i64);
    for (bucket, is_online, sample_count) in rows {
        let day = crate::time_util::day_start_utc(tz, bucket);
        let entry = by_day.entry(day).or_insert((0, 0));
        entry.0 += sample_count;
        total += sample_count;
        if is_online == 1 {
            entry.1 += sample_count;
            online += sample_count;
        }
    }

    // Most-recent day first, matching the previous `ORDER BY day DESC`.
    let daily: Vec<UptimePoint> = by_day
        .into_iter()
        .rev()
        .map(|(day_epoch, (total_count, online_count))| {
            let uptime_pct = if total_count > 0 {
                (online_count as f64 / total_count as f64) * 100.0
            } else {
                0.0
            };
            UptimePoint {
                day: DateTime::<Utc>::from_timestamp(day_epoch, 0)
                    .expect("day-start epoch is always a valid timestamp"),
                total_count,
                online_count,
                uptime_pct,
            }
        })
        .collect();

    let overall_pct = if total > 0 {
        (online as f64 / total as f64) * 100.0
    } else {
        0.0
    };

    Ok(UptimeSummary {
        host_key: host_key.to_string(),
        overall_pct,
        timezone: tz.name().to_string(),
        daily,
    })
}
