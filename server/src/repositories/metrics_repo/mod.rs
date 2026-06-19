mod chart;
mod range;
mod rows;
mod summary;
mod tiers;
mod uptime;
mod write;

pub use chart::fetch_chart_metrics_range;
pub use range::{fetch_metrics_range, fetch_recent_metrics};
pub use rows::{ChartMetricsRow, HostSummary, MetricsRow, UptimeSummary};
pub use summary::{fetch_batch_uptime_pct, fetch_host_summaries};
pub use tiers::CHART_RAW_BOUNDARY_SECS;
pub use uptime::fetch_uptime;
pub use write::{insert_metrics_batch, insert_offline_metrics_batch};

#[cfg(test)]
use rows::UptimePoint;

#[cfg(test)]
mod sqlite_tests {
    use super::*;
    use crate::db::DbPool;
    use crate::models::agent_metrics::{
        AgentMetrics, DiskInfo, DockerContainer, DockerContainerStats, LoadAverage, NetworkTotal,
        PortStatus, SystemMetrics, TemperatureInfo,
    };
    use chrono::Utc;
    use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
    use std::str::FromStr;

    async fn fresh_pool() -> DbPool {
        let options = SqliteConnectOptions::from_str("sqlite::memory:")
            .unwrap()
            .foreign_keys(false)
            .journal_mode(sqlx::sqlite::SqliteJournalMode::Memory);
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(options)
            .await
            .unwrap();
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();
        // A hosts row is required for fetch_host_summaries; seed one
        // per test via the hosts_repo-compatible raw insert. Test pools
        // run with foreign_keys=false so the FK back to hosts from
        // `metrics` is non-issue.
        sqlx::query(
            "INSERT INTO hosts (host_key, display_name) VALUES ('h1:9101', 'box-1'), ('h2:9101', 'box-2')",
        )
        .execute(&pool)
        .await
        .unwrap();
        pool
    }

    fn synthetic_metrics() -> AgentMetrics {
        AgentMetrics {
            hostname: "box-1".into(),
            timestamp: "2026-01-01T00:00:00Z".into(),
            is_online: true,
            system: SystemMetrics {
                cpu_usage_percent: 42.5,
                memory_total_mb: 8192,
                memory_used_mb: 4096,
                memory_usage_percent: 50.0,
                disks: vec![],
                processes: vec![],
                temperatures: vec![],
                gpus: vec![],
            },
            network: NetworkTotal {
                total_rx_bytes: 1_000_000,
                total_tx_bytes: 500_000,
                ..Default::default()
            },
            network_interfaces: vec![],
            cpu_cores: vec![],
            load_average: LoadAverage {
                one_min: 1.2,
                five_min: 1.5,
                fifteen_min: 2.0,
            },
            docker_containers: vec![] as Vec<DockerContainer>,
            docker_stats: vec![],
            ports: vec![] as Vec<PortStatus>,
            agent_version: "0.3.5".into(),
        }
    }

    #[tokio::test]
    async fn insert_batch_then_fetch_recent() {
        let pool = fresh_pool().await;
        let m = synthetic_metrics();

        insert_metrics_batch(&pool, &[("h1:9101", &m)])
            .await
            .unwrap();
        insert_metrics_batch(&pool, &[("h1:9101", &m)])
            .await
            .unwrap();
        insert_metrics_batch(&pool, &[("h2:9101", &m)])
            .await
            .unwrap();

        let recent = fetch_recent_metrics(&pool, "h1:9101").await.unwrap();
        assert_eq!(recent.len(), 2);
        for row in &recent {
            assert!(row.is_online);
            assert!((row.cpu_usage_percent - 42.5).abs() < 0.01);
            // networks JSON round-tripped from TEXT.
            let net = row.networks.as_ref().unwrap();
            assert_eq!(net["total_rx_bytes"], 1_000_000i64);
        }
    }

    #[tokio::test]
    async fn offline_batch_sets_is_online_false() {
        let pool = fresh_pool().await;
        insert_offline_metrics_batch(&pool, &[("h1:9101", "box-1")])
            .await
            .unwrap();
        let recent = fetch_recent_metrics(&pool, "h1:9101").await.unwrap();
        assert_eq!(recent.len(), 1);
        assert!(!recent[0].is_online);
        assert!(recent[0].networks.is_none());
    }

    #[tokio::test]
    async fn host_summaries_uses_latest_per_host() {
        let pool = fresh_pool().await;
        let m = synthetic_metrics();

        insert_metrics_batch(&pool, &[("h1:9101", &m), ("h2:9101", &m)])
            .await
            .unwrap();

        let summaries = fetch_host_summaries(&pool).await.unwrap();
        assert_eq!(summaries.len(), 2);
        for s in &summaries {
            assert!(s.is_online, "host {} should be online", s.host_key);
            assert!(s.last_seen.is_some());
        }
    }

    #[tokio::test]
    async fn host_summaries_returns_offline_for_hosts_without_recent_metrics() {
        // Regression pin for the LEFT JOIN rewrite: a host that exists in
        // `hosts` but has no row in `metrics` within the 5-minute window
        // must still appear in the summary with `is_online = false` and
        // `last_seen = None`. The previous correlated-subquery shape
        // happened to return the row via `COALESCE(..., 0)`; the LEFT JOIN
        // shape must match that contract.
        let pool = fresh_pool().await;
        // Insert a row for h1 only — h2 has no metrics.
        insert_metrics_batch(&pool, &[("h1:9101", &synthetic_metrics())])
            .await
            .unwrap();

        let summaries = fetch_host_summaries(&pool).await.unwrap();
        assert_eq!(summaries.len(), 2, "both hosts must be listed");
        let h2 = summaries
            .iter()
            .find(|s| s.host_key == "h2:9101")
            .expect("h2 present");
        assert!(!h2.is_online, "h2 has no metrics → offline");
        assert!(h2.last_seen.is_none());
    }

    #[tokio::test]
    async fn host_summaries_respects_per_host_scrape_interval() {
        let pool = fresh_pool().await;
        sqlx::query("UPDATE hosts SET scrape_interval_secs = 120 WHERE host_key = 'h2:9101'")
            .execute(&pool)
            .await
            .unwrap();

        let m = synthetic_metrics();
        insert_metrics_batch(&pool, &[("h1:9101", &m), ("h2:9101", &m)])
            .await
            .unwrap();
        sqlx::query(
            "UPDATE metrics SET timestamp = strftime('%s','now') - 180 \
             WHERE host_key IN ('h1:9101', 'h2:9101')",
        )
        .execute(&pool)
        .await
        .unwrap();

        let summaries = fetch_host_summaries(&pool).await.unwrap();
        let h1 = summaries
            .iter()
            .find(|s| s.host_key == "h1:9101")
            .expect("h1 present");
        let h2 = summaries
            .iter()
            .find(|s| s.host_key == "h2:9101")
            .expect("h2 present");

        assert!(!h1.is_online, "default 10s scrape is stale after 180s");
        assert!(h2.is_online, "120s scrape remains fresh for 3 intervals");
    }

    #[tokio::test]
    async fn metrics_range_raw_tier_returns_rows_within_window() {
        let pool = fresh_pool().await;
        let m = synthetic_metrics();
        insert_metrics_batch(&pool, &[("h1:9101", &m)])
            .await
            .unwrap();

        let now = Utc::now();
        let rows = fetch_metrics_range(
            &pool,
            "h1:9101",
            now - chrono::Duration::minutes(30),
            now + chrono::Duration::minutes(30),
        )
        .await
        .unwrap();
        assert_eq!(rows.len(), 1);
        // Trimmed JSON columns are NULL per the "raw tier minus heavy
        // columns" projection.
        assert!(rows[0].docker_containers.is_none());
        assert!(rows[0].ports.is_none());
        // Full-detail columns survive.
        assert!(rows[0].networks.is_some());
    }

    #[tokio::test]
    async fn chart_metrics_range_returns_lightweight_projection() {
        let pool = fresh_pool().await;
        let mut m = synthetic_metrics();
        m.system.disks = vec![DiskInfo {
            name: "disk0".into(),
            mount_point: "/".into(),
            total_gb: 100.0,
            available_gb: 40.0,
            usage_percent: 60.0,
            read_bytes_per_sec: 123.0,
            write_bytes_per_sec: 456.0,
        }];
        m.system.temperatures = vec![TemperatureInfo {
            label: "CPU".into(),
            temperature_c: 55.0,
        }];
        m.docker_stats = vec![DockerContainerStats {
            container_name: "app".into(),
            cpu_percent: 7.5,
            memory_usage_mb: 128,
            memory_limit_mb: 1024,
            net_rx_bytes: 99,
            net_tx_bytes: 100,
            block_read_bytes: 0,
            block_write_bytes: 0,
        }];
        m.network.rx_bytes_per_sec = 11.0;
        m.network.tx_bytes_per_sec = 22.0;

        insert_metrics_batch(&pool, &[("h1:9101", &m)])
            .await
            .unwrap();

        let now = Utc::now();
        let rows = fetch_chart_metrics_range(
            &pool,
            "h1:9101",
            now - chrono::Duration::minutes(30),
            now + chrono::Duration::minutes(30),
        )
        .await
        .unwrap();

        assert_eq!(rows.len(), 1);
        let row = &rows[0];
        assert_eq!(row.disks[0].mount_point, "/");
        assert!((row.disks[0].usage_percent - 60.0).abs() < 0.01);
        assert_eq!(row.temperatures[0].label, "CPU");
        assert_eq!(row.docker_stats[0].container_name, "app");
        assert!((row.docker_stats[0].cpu_percent - 7.5).abs() < 0.01);
        assert_eq!(row.networks.as_ref().unwrap().total_rx_bytes, 1_000_000);
        assert_eq!(row.networks.as_ref().unwrap().rx_bytes_per_sec, 11.0);
    }

    #[tokio::test]
    async fn chart_metrics_range_uses_rollup_after_one_hour() {
        let pool = fresh_pool().await;
        let now = Utc::now();
        let bucket = (now.timestamp() / 300) * 300;

        sqlx::query(
            r#"
            INSERT INTO metrics_5min (
                host_key, bucket, cpu_usage_percent, memory_usage_percent,
                load_1min, load_5min, load_15min, is_online, sample_count,
                total_rx_bytes, total_tx_bytes, disks, temperatures, docker_stats,
                avg_rx_bytes_per_sec, avg_tx_bytes_per_sec
            )
            VALUES (
                'h1:9101', ?1, 12.5, 34.5,
                0.1, 0.2, 0.3, 1, 30,
                12345, 67890,
                '[{"name":"disk0","mount_point":"/","total_gb":100,"available_gb":50,"usage_percent":50,"read_bytes_per_sec":1,"write_bytes_per_sec":2}]',
                '[{"label":"CPU","temperature_c":44}]',
                '[{"container_name":"app","cpu_percent":3.5,"memory_usage_mb":64,"memory_limit_mb":1024,"net_rx_bytes":1,"net_tx_bytes":2}]',
                111.0, 222.0
            )
            "#,
        )
        .bind(bucket)
        .execute(&pool)
        .await
        .unwrap();

        let rows = fetch_chart_metrics_range(
            &pool,
            "h1:9101",
            now - chrono::Duration::hours(2),
            now + chrono::Duration::hours(2),
        )
        .await
        .unwrap();

        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].id, 0, "rollup rows are synthetic chart rows");
        assert!((rows[0].cpu_usage_percent - 12.5).abs() < 0.01);
        assert_eq!(rows[0].networks.as_ref().unwrap().rx_bytes_per_sec, 111.0);
        assert_eq!(rows[0].disks[0].mount_point, "/");
    }

    #[tokio::test]
    async fn chart_metrics_range_wide_re_aggregates_into_15min_buckets() {
        // > 14 d windows route through the window-function CTE and pick the
        // last-in-bucket JSON snapshot via `MAX(CASE WHEN rn = 1 ...)`. This
        // path is the easiest to break (CTE column shape, alias drift,
        // SQLite window-function support) and the cheapest to regression-pin.
        let pool = fresh_pool().await;

        // Pin "now" to a 15-min boundary so both inserted rows fall inside
        // the same 15-min re-aggregation bucket and the `rn = 1` selector
        // has a deterministic winner.
        let bucket_anchor = (Utc::now().timestamp() / 900) * 900;
        let bucket_first = bucket_anchor; // older 5-min bucket
        let bucket_last = bucket_anchor + 300; // newer 5-min bucket within the same 15-min window

        // Older 5-min bucket: lower CPU, alternative disk JSON.
        sqlx::query(
            r#"
            INSERT INTO metrics_5min (
                host_key, bucket, cpu_usage_percent, memory_usage_percent,
                load_1min, load_5min, load_15min, is_online, sample_count,
                total_rx_bytes, total_tx_bytes, disks, temperatures, docker_stats,
                avg_rx_bytes_per_sec, avg_tx_bytes_per_sec
            )
            VALUES (
                'h1:9101', ?1, 10.0, 20.0,
                0.1, 0.2, 0.3, 1, 30,
                100, 200,
                '[{"name":"older","mount_point":"/older","total_gb":50,"available_gb":25,"usage_percent":50,"read_bytes_per_sec":1,"write_bytes_per_sec":2}]',
                '[{"label":"OLD","temperature_c":30}]',
                '[{"container_name":"old","cpu_percent":1.0,"memory_usage_mb":10,"memory_limit_mb":256,"net_rx_bytes":1,"net_tx_bytes":2}]',
                100.0, 200.0
            )
            "#,
        )
        .bind(bucket_first)
        .execute(&pool)
        .await
        .unwrap();

        // Newer 5-min bucket inside the same 15-min window: higher CPU,
        // distinct disk/temperature/docker JSON.  The wide branch must
        // surface *this* row's JSON snapshots (rn = 1).
        sqlx::query(
            r#"
            INSERT INTO metrics_5min (
                host_key, bucket, cpu_usage_percent, memory_usage_percent,
                load_1min, load_5min, load_15min, is_online, sample_count,
                total_rx_bytes, total_tx_bytes, disks, temperatures, docker_stats,
                avg_rx_bytes_per_sec, avg_tx_bytes_per_sec
            )
            VALUES (
                'h1:9101', ?1, 30.0, 60.0,
                0.4, 0.5, 0.6, 1, 30,
                500, 700,
                '[{"name":"newer","mount_point":"/newer","total_gb":100,"available_gb":40,"usage_percent":60,"read_bytes_per_sec":3,"write_bytes_per_sec":4}]',
                '[{"label":"NEW","temperature_c":55}]',
                '[{"container_name":"new","cpu_percent":7.5,"memory_usage_mb":128,"memory_limit_mb":1024,"net_rx_bytes":9,"net_tx_bytes":10}]',
                300.0, 400.0
            )
            "#,
        )
        .bind(bucket_last)
        .execute(&pool)
        .await
        .unwrap();

        // 30-day window forces the wide branch (>14 d).
        let now = Utc::now();
        let rows = fetch_chart_metrics_range(
            &pool,
            "h1:9101",
            now - chrono::Duration::days(30),
            now + chrono::Duration::days(1),
        )
        .await
        .unwrap();

        // Both 5-min rows fall in the same 15-min bucket → exactly one
        // output row.
        assert_eq!(
            rows.len(),
            1,
            "two 5-min rows collapse into one 15-min bucket"
        );
        let row = &rows[0];

        // Scalars are AVG over both rows.
        assert!(
            (row.cpu_usage_percent - 20.0).abs() < 0.01,
            "CPU should be the average of 10 and 30, got {}",
            row.cpu_usage_percent
        );
        assert!(
            (row.memory_usage_percent - 40.0).abs() < 0.01,
            "memory should be the average of 20 and 60, got {}",
            row.memory_usage_percent
        );

        // Cumulative counters are MAX (latest absolute value).
        let net = row.networks.as_ref().expect("network rates synthesized");
        assert_eq!(net.total_rx_bytes, 500);
        assert_eq!(net.total_tx_bytes, 700);

        // JSON snapshots come from the rn=1 row (the *newer* bucket).
        assert_eq!(row.disks.len(), 1);
        assert_eq!(row.disks[0].mount_point, "/newer");
        assert_eq!(row.temperatures[0].label, "NEW");
        assert_eq!(row.docker_stats[0].container_name, "new");
    }

    #[tokio::test]
    async fn uptime_returns_empty_until_rollup_worker_runs() {
        // Without the rollup worker running, uptime queries see an empty
        // aggregate. Batch uptime still returns known hosts with 0.0 so
        // public status does not need to special-case missing map entries.
        let pool = fresh_pool().await;
        let batch = fetch_batch_uptime_pct(&pool, 7).await.unwrap();
        assert_eq!(batch.len(), 2);
        assert_eq!(batch.get("h1:9101"), Some(&0.0));
        assert_eq!(batch.get("h2:9101"), Some(&0.0));

        let summary = fetch_uptime(&pool, "h1:9101", 7, chrono_tz::Tz::UTC)
            .await
            .unwrap();
        assert_eq!(summary.overall_pct, 0.0);
        assert!(summary.daily.is_empty());
    }
}

#[cfg(test)]
mod serialization_tests {
    use super::*;
    use chrono::{TimeZone, Utc};

    /// API timestamps must serialize as canonical UTC RFC 3339 (`…Z`), never a
    /// fixed offset like `+09:00`. Guards against re-introducing KST/local-time
    /// serialization on the wire.
    #[test]
    fn timestamps_serialize_as_utc_rfc3339_z() {
        let dt = Utc.with_ymd_and_hms(2026, 6, 16, 11, 20, 30).unwrap();

        let point = UptimePoint {
            day: dt,
            total_count: 10,
            online_count: 9,
            uptime_pct: 90.0,
        };
        let json = serde_json::to_string(&point).unwrap();
        assert!(json.contains("\"2026-06-16T11:20:30Z\""), "got {json}");
        assert!(
            !json.contains("+09:00"),
            "must not emit a fixed offset: {json}"
        );

        let some = HostSummary {
            host_key: "h:9101".into(),
            display_name: "box".into(),
            is_online: true,
            last_seen: Some(dt),
        };
        let j2 = serde_json::to_string(&some).unwrap();
        assert!(j2.contains("\"2026-06-16T11:20:30Z\""), "got {j2}");
        assert!(!j2.contains("+09:00"), "must not emit a fixed offset: {j2}");

        let none = HostSummary {
            host_key: "h:9101".into(),
            display_name: "box".into(),
            is_online: false,
            last_seen: None,
        };
        let j3 = serde_json::to_string(&none).unwrap();
        assert!(j3.contains("\"last_seen\":null"), "got {j3}");
    }
}
