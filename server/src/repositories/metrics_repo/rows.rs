use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::models::agent_metrics::{DiskInfo, DockerContainerStats, TemperatureInfo};

// All API timestamps are serialized as canonical **UTC RFC 3339** (e.g.
// `2026-06-16T11:20:30Z`) via chrono's default `DateTime<Utc>` Serialize impl —
// identical to every other repository in this crate. The frontend treats the
// wire value as UTC and re-localizes to the browser's timezone for display, so
// the server never bakes a fixed offset (previously `+09:00`) into the wire.

// ──────────────────────────────────────────────
// Select (GET /api/metrics/:host_key)
// ──────────────────────────────────────────────

/// Row returned to the dashboard for chart rendering
#[derive(Serialize, Deserialize, Clone, sqlx::FromRow)]
pub struct MetricsRow {
    pub id: i64,
    /// Target-URL-based unique identifier
    pub host_key: String,
    /// UI display name (OS hostname reported by the agent)
    pub display_name: String,
    pub is_online: bool,
    pub cpu_usage_percent: f32,
    pub memory_usage_percent: f32,
    pub load_1min: f32,
    pub load_5min: f32,
    pub load_15min: f32,
    pub networks: Option<Value>,
    pub docker_containers: Option<Value>,
    pub ports: Option<Value>,
    pub disks: Option<Value>,
    pub processes: Option<Value>,
    pub temperatures: Option<Value>,
    pub gpus: Option<Value>,
    pub cpu_cores: Option<Value>,
    pub network_interfaces: Option<Value>,
    pub docker_stats: Option<Value>,
    pub timestamp: DateTime<Utc>,
}

#[derive(Serialize, Deserialize, Clone, sqlx::FromRow)]
pub struct ChartNetwork {
    pub total_rx_bytes: u64,
    pub total_tx_bytes: u64,
    pub rx_bytes_per_sec: f64,
    pub tx_bytes_per_sec: f64,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct ChartDiskInfo {
    pub name: String,
    pub mount_point: String,
    pub usage_percent: f32,
    pub read_bytes_per_sec: f64,
    pub write_bytes_per_sec: f64,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct ChartDockerStats {
    pub container_name: String,
    pub cpu_percent: f32,
    pub memory_usage_mb: u64,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct ChartMetricsRow {
    pub id: i64,
    pub host_key: String,
    pub display_name: String,
    pub is_online: bool,
    pub cpu_usage_percent: f32,
    pub memory_usage_percent: f32,
    pub load_1min: f32,
    pub load_5min: f32,
    pub load_15min: f32,
    pub networks: Option<ChartNetwork>,
    pub disks: Vec<ChartDiskInfo>,
    pub temperatures: Vec<TemperatureInfo>,
    pub docker_stats: Vec<ChartDockerStats>,
    pub timestamp: DateTime<Utc>,
}

// ──────────────────────────────────────────────
// Select (GET /api/hosts)
// ──────────────────────────────────────────────

/// Host summary shown in the frontend sidebar
#[derive(Serialize, Deserialize, sqlx::FromRow)]
pub struct HostSummary {
    /// Target-URL-based unique identifier
    pub host_key: String,
    /// UI display name
    pub display_name: String,
    pub is_online: bool,
    pub last_seen: Option<DateTime<Utc>>,
}

// ──────────────────────────────────────────────
// Uptime (GET /api/uptime/:host_key)
// ──────────────────────────────────────────────

/// Daily uptime data point
#[derive(Serialize, Deserialize)]
pub struct UptimePoint {
    /// UTC instant of the start of this calendar day **in the workspace
    /// timezone** (see [`UptimeSummary::timezone`]). Serialized as RFC 3339
    /// `…Z`; the frontend formats it in that timezone for day labels.
    pub day: DateTime<Utc>,
    pub total_count: i64,
    pub online_count: i64,
    pub uptime_pct: f64,
}

/// Overall uptime summary for a host
#[derive(Serialize)]
pub struct UptimeSummary {
    pub host_key: String,
    pub overall_pct: f64,
    /// IANA name of the workspace timezone the daily buckets were grouped by
    /// (e.g. `UTC`, `Asia/Seoul`). Lets the client label `day` unambiguously.
    pub timezone: String,
    pub daily: Vec<UptimePoint>,
}

// Raw-metrics write + read paths. The 6h–14d and >14d tiers of
// `fetch_metrics_range` query the `metrics_5min` rollup table — the
// rollup worker (services/rollup_worker.rs) populates that table on a
// schedule.
//
// `MetricsRowRaw` holds JSON columns as `Option<String>` and decodes
// them into `serde_json::Value` at the boundary via `TryFrom`.

#[derive(sqlx::FromRow)]
pub(super) struct MetricsRowRaw {
    id: i64,
    host_key: String,
    display_name: String,
    is_online: Option<bool>,
    cpu_usage_percent: f32,
    memory_usage_percent: f32,
    load_1min: f32,
    load_5min: f32,
    load_15min: f32,
    networks: Option<String>,
    docker_containers: Option<String>,
    ports: Option<String>,
    disks: Option<String>,
    processes: Option<String>,
    temperatures: Option<String>,
    gpus: Option<String>,
    cpu_cores: Option<String>,
    network_interfaces: Option<String>,
    docker_stats: Option<String>,
    rx_bytes_per_sec: Option<f64>,
    tx_bytes_per_sec: Option<f64>,
    /// Scalar totals sourced from `metrics_5min` in the rollup/wide
    /// branches. Populated as `Some(_)` only when `networks` is `None`
    /// (i.e. the SQL did not pre-build the JSON via `json_object`); the
    /// `TryFrom` synthesizes the networks object in Rust.
    total_rx_bytes: Option<i64>,
    total_tx_bytes: Option<i64>,
    timestamp: DateTime<Utc>,
}

fn parse_opt_json(s: Option<String>) -> Result<Option<Value>, sqlx::Error> {
    match s {
        Some(text) => serde_json::from_str(&text)
            .map(Some)
            .map_err(|e| sqlx::Error::Decode(Box::new(e))),
        None => Ok(None),
    }
}

impl TryFrom<MetricsRowRaw> for MetricsRow {
    type Error = sqlx::Error;

    fn try_from(raw: MetricsRowRaw) -> Result<Self, Self::Error> {
        let mut networks = parse_opt_json(raw.networks)?;

        // Synthesize the `networks` JSON object on the Rust side when the
        // rollup / wide-aggregation branches supply scalar totals instead.
        // Skipping SQLite's `json_object(...)` per-row call saves the
        // planner's string-building pass inside the query — measurable on
        // 30-day windows at >14d.
        //
        // Gate on `total_*_bytes` specifically — only the rollup branches
        // populate those, never the raw branches. This preserves the
        // contract that offline rows from `insert_offline_metrics_batch`
        // (which bind `rx_bytes_per_sec = 0.0` but leave `networks` NULL)
        // surface as `networks = None`, not a misleading
        // `{"rx_bytes_per_sec": 0, "tx_bytes_per_sec": 0}` object.
        if networks.is_none() && (raw.total_rx_bytes.is_some() || raw.total_tx_bytes.is_some()) {
            let mut map = serde_json::Map::with_capacity(4);
            if let Some(v) = raw.total_rx_bytes {
                map.insert("total_rx_bytes".into(), Value::from(v));
            }
            if let Some(v) = raw.total_tx_bytes {
                map.insert("total_tx_bytes".into(), Value::from(v));
            }
            if let Some(v) = raw.rx_bytes_per_sec {
                map.insert("rx_bytes_per_sec".into(), Value::from(v));
            }
            if let Some(v) = raw.tx_bytes_per_sec {
                map.insert("tx_bytes_per_sec".into(), Value::from(v));
            }
            networks = Some(Value::Object(map));
        } else if let Some(Value::Object(ref mut map)) = networks {
            // Raw-branch path: `networks` was read as JSON text from
            // `metrics.networks`; merge the scalar rate columns so the
            // shape matches the rollup branches above.
            if let Some(rx) = raw.rx_bytes_per_sec {
                map.insert("rx_bytes_per_sec".to_string(), Value::from(rx));
            }
            if let Some(tx) = raw.tx_bytes_per_sec {
                map.insert("tx_bytes_per_sec".to_string(), Value::from(tx));
            }
        }

        Ok(Self {
            id: raw.id,
            host_key: raw.host_key,
            display_name: raw.display_name,
            is_online: raw.is_online.unwrap_or(false),
            cpu_usage_percent: raw.cpu_usage_percent,
            memory_usage_percent: raw.memory_usage_percent,
            load_1min: raw.load_1min,
            load_5min: raw.load_5min,
            load_15min: raw.load_15min,
            networks,
            docker_containers: parse_opt_json(raw.docker_containers)?,
            ports: parse_opt_json(raw.ports)?,
            disks: parse_opt_json(raw.disks)?,
            processes: parse_opt_json(raw.processes)?,
            temperatures: parse_opt_json(raw.temperatures)?,
            gpus: parse_opt_json(raw.gpus)?,
            cpu_cores: parse_opt_json(raw.cpu_cores)?,
            network_interfaces: parse_opt_json(raw.network_interfaces)?,
            docker_stats: parse_opt_json(raw.docker_stats)?,
            timestamp: raw.timestamp,
        })
    }
}

#[derive(sqlx::FromRow)]
pub(super) struct ChartMetricsRowRaw {
    id: i64,
    host_key: String,
    display_name: String,
    is_online: Option<bool>,
    cpu_usage_percent: f32,
    memory_usage_percent: f32,
    load_1min: f32,
    load_5min: f32,
    load_15min: f32,
    total_rx_bytes: Option<i64>,
    total_tx_bytes: Option<i64>,
    rx_bytes_per_sec: Option<f64>,
    tx_bytes_per_sec: Option<f64>,
    disks: Option<String>,
    temperatures: Option<String>,
    docker_stats: Option<String>,
    timestamp: DateTime<Utc>,
}

fn parse_json_vec<T>(s: Option<String>) -> Result<Vec<T>, sqlx::Error>
where
    T: for<'de> Deserialize<'de>,
{
    match s {
        Some(text) => serde_json::from_str(&text).map_err(|e| sqlx::Error::Decode(Box::new(e))),
        None => Ok(Vec::new()),
    }
}

impl TryFrom<ChartMetricsRowRaw> for ChartMetricsRow {
    type Error = sqlx::Error;

    fn try_from(raw: ChartMetricsRowRaw) -> Result<Self, Self::Error> {
        let networks = match (raw.total_rx_bytes, raw.total_tx_bytes) {
            (Some(rx_total), Some(tx_total)) => Some(ChartNetwork {
                total_rx_bytes: rx_total.max(0) as u64,
                total_tx_bytes: tx_total.max(0) as u64,
                rx_bytes_per_sec: raw.rx_bytes_per_sec.unwrap_or(0.0),
                tx_bytes_per_sec: raw.tx_bytes_per_sec.unwrap_or(0.0),
            }),
            _ => None,
        };

        let disks: Vec<DiskInfo> = parse_json_vec(raw.disks)?;
        let docker_stats: Vec<DockerContainerStats> = parse_json_vec(raw.docker_stats)?;

        Ok(Self {
            id: raw.id,
            host_key: raw.host_key,
            display_name: raw.display_name,
            is_online: raw.is_online.unwrap_or(false),
            cpu_usage_percent: raw.cpu_usage_percent,
            memory_usage_percent: raw.memory_usage_percent,
            load_1min: raw.load_1min,
            load_5min: raw.load_5min,
            load_15min: raw.load_15min,
            networks,
            disks: disks
                .into_iter()
                .map(|d| ChartDiskInfo {
                    name: d.name,
                    mount_point: d.mount_point,
                    usage_percent: d.usage_percent,
                    read_bytes_per_sec: d.read_bytes_per_sec,
                    write_bytes_per_sec: d.write_bytes_per_sec,
                })
                .collect(),
            temperatures: parse_json_vec(raw.temperatures)?,
            docker_stats: docker_stats
                .into_iter()
                .map(|s| ChartDockerStats {
                    container_name: s.container_name,
                    cpu_percent: s.cpu_percent,
                    memory_usage_mb: s.memory_usage_mb,
                })
                .collect(),
            timestamp: raw.timestamp,
        })
    }
}
