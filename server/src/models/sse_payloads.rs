use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::models::agent_metrics::{
    DiskInfo, DockerContainer, DockerContainerStats, GpuInfo, PortStatus, ProcessInfo,
    TemperatureInfo,
};

/// Network throughput per second — computed server-side as a delta of cumulative byte counters.
///
/// Stored as a single aggregate value instead of a per-interface array:
/// - The agent already sums physical interfaces before sending, so per-interface breakdown is unnecessary.
/// - Reduces SSE payload size.
#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct NetworkRate {
    pub rx_bytes_per_sec: f64,
    pub tx_bytes_per_sec: f64,
    /// Cumulative counters mirrored from the agent's NetworkTotal so live
    /// SSE rows match the REST MetricsRow.networks shape.
    pub total_rx_bytes: u64,
    pub total_tx_bytes: u64,
}

/// Per-interface network throughput (bytes/sec), computed server-side as a delta.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct NetworkInterfaceRate {
    pub name: String,
    pub rx_bytes_per_sec: f64,
    pub tx_bytes_per_sec: f64,
}

/// `event: metrics` payload — dynamic data (CPU, memory, network rate, etc.) sent every scrape cycle
#[derive(Serialize, Clone, Debug)]
pub struct HostMetricsPayload {
    /// Target-URL-based unique identifier — prevents collisions when multiple agents share the same hostname
    pub host_key: String,
    /// Agent-reported hostname — used for UI display only
    pub display_name: String,
    pub is_online: bool,
    pub cpu_usage_percent: f32,
    pub memory_usage_percent: f32,
    pub load_1min: f64,
    pub load_5min: f64,
    pub load_15min: f64,
    /// Aggregate throughput across all physical interfaces (bytes/sec)
    pub network_rate: NetworkRate,
    /// Per-core CPU usage percentages
    pub cpu_cores: Vec<f32>,
    /// Per-interface throughput (bytes/sec)
    pub network_interface_rates: Vec<NetworkInterfaceRate>,
    /// Per-disk usage + I/O throughput (sent every cycle for real-time charts)
    pub disks: Vec<DiskInfo>,
    /// Temperature sensor readings
    pub temperatures: Vec<TemperatureInfo>,
    /// Per-container resource usage (CPU%, memory)
    pub docker_stats: Vec<DockerContainerStats>,
    pub timestamp: String,
}

/// `event: status` payload — semi-static data (Docker containers, port states, etc.)
/// Sent immediately on client connection and re-sent on state change or periodically.
#[derive(Serialize, Clone, Debug)]
pub struct HostStatusPayload {
    /// Target-URL-based unique identifier — prevents hostname collisions
    pub host_key: String,
    /// Agent-reported hostname — used for UI display only
    pub display_name: String,
    /// Effective scrape cadence for this host (seconds).
    pub scrape_interval_secs: u64,
    pub is_online: bool,
    pub last_seen: String,
    pub docker_containers: Vec<DockerContainer>,
    pub ports: Vec<PortStatus>,
    pub disks: Vec<DiskInfo>,
    pub processes: Vec<ProcessInfo>,
    pub temperatures: Vec<TemperatureInfo>,
    pub gpus: Vec<GpuInfo>,
    pub docker_stats: Vec<DockerContainerStats>,
    // ── Static system info (fetched on reconnection + every 24h) ──
    #[serde(skip_serializing_if = "Option::is_none")]
    pub os_info: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cpu_model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub memory_total_mb: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub boot_time: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ip_address: Option<String>,
}

/// Event variants delivered to SSE handlers via a `tokio::sync::broadcast` channel.
///
/// Payloads carry **pre-serialized JSON** (`Arc<str>`) rather than the
/// original Rust structs. The `broadcast::Sender` fan-out is `O(subscribers)`,
/// and the previous shape (`Arc<HostStatusPayload>`) forced every subscriber
/// to call `serde_json::to_string` on its own copy — the JSON encoding cost
/// scaled `subscribers × payload_size` per scrape tick. By moving the encode
/// to the producer, every SSE client just clones the `Arc<str>` and hands it
/// to `axum::sse::Event::data` (which copies the bytes once into the frame).
///
/// `HostStatusPayload` is still cached as `Arc<HostStatusPayload>` in
/// `last_known_status` so the SSE handshake / `Lagged` re-snapshot can do
/// its own one-shot serialization out of band; only the broadcast hot
/// path uses the pre-serialized form.
#[derive(Clone, Debug)]
pub enum SseBroadcast {
    Metrics(Arc<str>),
    Status(Arc<str>),
}

impl SseBroadcast {
    /// Serialize a metrics payload once for all subscribers. Returns `None`
    /// if serialization fails (the producer drops the broadcast in that
    /// case, mirroring the pre-existing `if let Ok(json) = ...` shape).
    pub fn metrics(payload: &HostMetricsPayload) -> Option<Self> {
        serde_json::to_string(payload)
            .ok()
            .map(|s| Self::Metrics(Arc::from(s)))
    }

    pub fn status(payload: &HostStatusPayload) -> Option<Self> {
        serde_json::to_string(payload)
            .ok()
            .map(|s| Self::Status(Arc::from(s)))
    }
}
