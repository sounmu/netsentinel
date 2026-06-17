use std::collections::{HashMap, VecDeque};
use std::time::{Duration, Instant};

// ──────────────────────────────────────────────
// Lightweight alert data point cache
// ──────────────────────────────────────────────

#[derive(Debug, Clone, Copy)]
pub struct AlertMetricPoint {
    pub received_at: Instant,
    pub cpu_usage_percent: f32,
    pub memory_usage_percent: f32,
}

/// Top-level in-memory metrics store
pub struct MetricsStore {
    pub hosts: HashMap<String, HostRecord>,
}

impl MetricsStore {
    pub fn new() -> Self {
        Self {
            hosts: HashMap::new(),
        }
    }
}

/// Per-host alert history, alert state, and SSE-related state
pub struct HostRecord {
    pub last_known_hostname: String,
    pub alert_history: VecDeque<AlertMetricPoint>,
    pub alert_state: AlertState,
    pub network_prev: Option<(u64, u64, Instant)>,
    /// Per-interface previous byte counters for rate calculation
    pub network_interface_prev: HashMap<String, (u64, u64, Instant)>,
    pub prev_status_hash: Option<u64>,
    pub last_status_sent: Option<Instant>,
}

impl HostRecord {
    pub fn new(hostname: String) -> Self {
        Self {
            last_known_hostname: hostname,
            alert_history: VecDeque::new(),
            alert_state: AlertState::new(),
            network_prev: None,
            network_interface_prev: HashMap::new(),
            prev_status_hash: None,
            last_status_sent: None,
        }
    }

    pub fn push_alert_point(&mut self, point: AlertMetricPoint, retention: Duration) {
        self.alert_history.push_back(point);
        while let Some(front) = self.alert_history.front() {
            if front.received_at.elapsed() > retention {
                self.alert_history.pop_front();
            } else {
                break;
            }
        }
    }
}

pub struct AlertState {
    pub offline_alerted: bool,
    pub cpu_alerted: bool,
    pub memory_alerted: bool,
    pub load_alerted: bool,
    pub network_alerted: bool,
    /// Per-mount-point disk alert state (keyed by mount_point string)
    pub disk_alerted: HashMap<String, bool>,
    /// Per-sensor temperature alert state (keyed by sensor label)
    pub temperature_alerted: HashMap<String, bool>,
    /// Per-GPU alert state (keyed by GPU name or index)
    pub gpu_alerted: HashMap<String, bool>,
    /// Per-container Docker lifecycle alert state (keyed by container name)
    pub docker_alerted: HashMap<String, bool>,
    pub last_offline_alert: Option<Instant>,
    pub last_recovery_alert: Option<Instant>,
    pub last_cpu_alert: Option<Instant>,
    pub last_memory_alert: Option<Instant>,
    pub last_load_alert: Option<Instant>,
    pub last_disk_alert: Option<Instant>,
    pub last_network_alert: Option<Instant>,
    pub last_temperature_alert: Option<Instant>,
    pub last_gpu_alert: Option<Instant>,
    pub last_docker_alert: Option<Instant>,
    pub port_alerted: HashMap<u16, Instant>,
}

impl AlertState {
    pub fn new() -> Self {
        Self {
            offline_alerted: false,
            cpu_alerted: false,
            memory_alerted: false,
            load_alerted: false,
            network_alerted: false,
            disk_alerted: HashMap::new(),
            temperature_alerted: HashMap::new(),
            gpu_alerted: HashMap::new(),
            docker_alerted: HashMap::new(),
            last_offline_alert: None,
            last_recovery_alert: None,
            last_cpu_alert: None,
            last_memory_alert: None,
            last_load_alert: None,
            last_disk_alert: None,
            last_network_alert: None,
            last_temperature_alert: None,
            last_gpu_alert: None,
            last_docker_alert: None,
            port_alerted: HashMap::new(),
        }
    }
}
