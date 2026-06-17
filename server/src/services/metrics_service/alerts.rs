use std::time::{Duration, Instant};

use crate::models::app_state::HostRecord;

/// Type-safe alert result enum.
/// Uses pattern matching instead of string matching for state transitions,
/// preventing silent bugs caused by message format changes.
pub(super) enum AlertAction {
    CpuOverload {
        hostname: String,
        sustained_mins: u64,
        threshold: f64,
        current: f32,
    },
    CpuRecovery {
        hostname: String,
        current: f32,
    },
    MemoryOverload {
        hostname: String,
        sustained_mins: u64,
        threshold: f64,
        current: f32,
    },
    MemoryRecovery {
        hostname: String,
        current: f32,
    },
    LoadOverload {
        hostname: String,
        load: f64,
        threshold: f64,
    },
    LoadRecovery {
        hostname: String,
        load: f64,
    },
    PortDown {
        hostname: String,
        port: u16,
    },
    PortRecovery {
        hostname: String,
        port: u16,
    },
    DiskOverload {
        hostname: String,
        mount_point: String,
        threshold: f64,
        current: f32,
    },
    DiskRecovery {
        hostname: String,
        mount_point: String,
        current: f32,
    },
    NetworkOverload {
        hostname: String,
        bytes_per_sec: f64,
        threshold: f64,
    },
    NetworkRecovery {
        hostname: String,
        bytes_per_sec: f64,
    },
    TemperatureOverload {
        hostname: String,
        sensor: String,
        threshold: f64,
        current: f32,
    },
    TemperatureRecovery {
        hostname: String,
        sensor: String,
        current: f32,
    },
    GpuOverload {
        hostname: String,
        gpu: String,
        threshold: f64,
        current: f32,
    },
    GpuRecovery {
        hostname: String,
        gpu: String,
        current: f32,
    },
    DockerContainerDown {
        hostname: String,
        container: String,
        state: String,
        exit_code: Option<i64>,
        oom_killed: bool,
        restart_count: u64,
    },
    DockerContainerRecovery {
        hostname: String,
        container: String,
    },
}

impl AlertAction {
    /// Returns a short string identifier for this alert type (used for DB logging).
    pub fn alert_type_str(&self) -> &'static str {
        match self {
            Self::CpuOverload { .. } => "cpu_overload",
            Self::CpuRecovery { .. } => "cpu_recovery",
            Self::MemoryOverload { .. } => "memory_overload",
            Self::MemoryRecovery { .. } => "memory_recovery",
            Self::LoadOverload { .. } => "load_overload",
            Self::LoadRecovery { .. } => "load_recovery",
            Self::PortDown { .. } => "port_down",
            Self::PortRecovery { .. } => "port_recovery",
            Self::DiskOverload { .. } => "disk_overload",
            Self::DiskRecovery { .. } => "disk_recovery",
            Self::NetworkOverload { .. } => "network_overload",
            Self::NetworkRecovery { .. } => "network_recovery",
            Self::TemperatureOverload { .. } => "temperature_overload",
            Self::TemperatureRecovery { .. } => "temperature_recovery",
            Self::GpuOverload { .. } => "gpu_overload",
            Self::GpuRecovery { .. } => "gpu_recovery",
            Self::DockerContainerDown { .. } => "docker_down",
            Self::DockerContainerRecovery { .. } => "docker_recovery",
        }
    }

    /// Formats a Discord notification message for this alert action.
    pub fn to_message(&self) -> String {
        match self {
            Self::CpuOverload {
                hostname,
                sustained_mins,
                threshold,
                current,
            } => format!(
                "🔥 **[CPU Overload]** Host `{}` — CPU usage has been above {:.1}% for the past {} minute(s). (current: {:.1}%)",
                hostname, threshold, sustained_mins, current
            ),
            Self::CpuRecovery { hostname, current } => format!(
                "✅ **[CPU Recovery]** Host `{}` — CPU usage has returned to normal. (current: {:.1}%)",
                hostname, current
            ),
            Self::MemoryOverload {
                hostname,
                sustained_mins,
                threshold,
                current,
            } => format!(
                "🔥 **[Memory Overload]** Host `{}` — Memory usage has been above {:.1}% for the past {} minute(s). (current: {:.1}%)",
                hostname, threshold, sustained_mins, current
            ),
            Self::MemoryRecovery { hostname, current } => format!(
                "✅ **[Memory Recovery]** Host `{}` — Memory usage has returned to normal. (current: {:.1}%)",
                hostname, current
            ),
            Self::LoadOverload {
                hostname,
                load,
                threshold,
            } => format!(
                "⚡ **[High Load]** Host `{}` — Load Average (1 min) is {:.2}, exceeding threshold {:.1}!",
                hostname, load, threshold
            ),
            Self::LoadRecovery { hostname, load } => format!(
                "✅ **[Load Recovery]** Host `{}` — Load Average (1 min) has returned to normal at {:.2}.",
                hostname, load
            ),
            Self::PortDown { hostname, port } => format!(
                "🚫 **[Port Down]** Host `{}` — port `{}` is not responding (closed).",
                hostname, port
            ),
            Self::PortRecovery { hostname, port } => format!(
                "✅ **[Port Recovery]** Host `{}` — port `{}` is open again.",
                hostname, port
            ),
            Self::DiskOverload {
                hostname,
                mount_point,
                threshold,
                current,
            } => format!(
                "💾 **[Disk Full]** Host `{}` — disk `{}` usage is {:.1}%, exceeding threshold {:.1}%!",
                hostname, mount_point, current, threshold
            ),
            Self::DiskRecovery {
                hostname,
                mount_point,
                current,
            } => format!(
                "✅ **[Disk Recovery]** Host `{}` — disk `{}` usage has returned to normal. (current: {:.1}%)",
                hostname, mount_point, current
            ),
            Self::NetworkOverload {
                hostname,
                bytes_per_sec,
                threshold,
            } => format!(
                "📡 **[Network Overload]** Host `{}` — aggregate network throughput is {:.1} MB/s (threshold {:.1} MB/s).",
                hostname,
                bytes_per_sec / 1_000_000.0,
                threshold / 1_000_000.0
            ),
            Self::NetworkRecovery {
                hostname,
                bytes_per_sec,
            } => format!(
                "✅ **[Network Recovery]** Host `{}` — network throughput has returned to normal. (current: {:.1} MB/s)",
                hostname,
                bytes_per_sec / 1_000_000.0
            ),
            Self::TemperatureOverload {
                hostname,
                sensor,
                threshold,
                current,
            } => format!(
                "🌡️ **[Temperature Overload]** Host `{}` — sensor `{}` reads {:.1}°C (threshold {:.1}°C).",
                hostname, sensor, current, threshold
            ),
            Self::TemperatureRecovery {
                hostname,
                sensor,
                current,
            } => format!(
                "✅ **[Temperature Recovery]** Host `{}` — sensor `{}` returned to {:.1}°C.",
                hostname, sensor, current
            ),
            Self::GpuOverload {
                hostname,
                gpu,
                threshold,
                current,
            } => format!(
                "🎮 **[GPU Overload]** Host `{}` — GPU `{}` usage is {:.1}% (threshold {:.1}%).",
                hostname, gpu, current, threshold
            ),
            Self::GpuRecovery {
                hostname,
                gpu,
                current,
            } => format!(
                "✅ **[GPU Recovery]** Host `{}` — GPU `{}` returned to {:.1}%.",
                hostname, gpu, current
            ),
            Self::DockerContainerDown {
                hostname,
                container,
                state,
                exit_code,
                oom_killed,
                restart_count,
            } => {
                let reason = match (exit_code, oom_killed) {
                    (Some(code), true) => format!("exit code {code}, OOMKilled"),
                    (Some(code), false) => format!("exit code {code}"),
                    (None, true) => "OOMKilled".to_string(),
                    (None, false) => "no exit code".to_string(),
                };
                format!(
                    "🐳 **[Docker Container Down]** Host `{}` — container `{}` is `{}` ({}, restarts: {}).",
                    hostname, container, state, reason, restart_count
                )
            }
            Self::DockerContainerRecovery {
                hostname,
                container,
            } => format!(
                "✅ **[Docker Container Recovery]** Host `{}` — container `{}` is running again.",
                hostname, container
            ),
        }
    }
}

// ──────────────────────────────────────────────
// Shared utilities
// ──────────────────────────────────────────────

/// Returns true if the cooldown period has elapsed. Cooldown duration is injected for flexibility.
pub(super) fn cooldown_elapsed(
    last_alert: Option<Instant>,
    cooldown_secs: u64,
    now: Instant,
) -> bool {
    last_alert.is_none_or(|t| {
        now.checked_duration_since(t)
            .is_some_and(|elapsed| elapsed >= Duration::from_secs(cooldown_secs))
    })
}

pub(super) fn update_alert_state_after_send(
    record: &mut HostRecord,
    actions: &[AlertAction],
    now: Instant,
) {
    for action in actions {
        match action {
            AlertAction::CpuOverload { .. } => {
                record.alert_state.cpu_alerted = true;
                record.alert_state.last_cpu_alert = Some(now);
            }
            AlertAction::CpuRecovery { .. } => {
                record.alert_state.cpu_alerted = false;
            }
            AlertAction::MemoryOverload { .. } => {
                record.alert_state.memory_alerted = true;
                record.alert_state.last_memory_alert = Some(now);
            }
            AlertAction::MemoryRecovery { .. } => {
                record.alert_state.memory_alerted = false;
            }
            AlertAction::LoadOverload { .. } => {
                record.alert_state.load_alerted = true;
                record.alert_state.last_load_alert = Some(now);
            }
            AlertAction::LoadRecovery { .. } => {
                record.alert_state.load_alerted = false;
            }
            AlertAction::PortDown { port, .. } => {
                record.alert_state.port_alerted.insert(*port, now);
            }
            AlertAction::PortRecovery { port, .. } => {
                record.alert_state.port_alerted.remove(port);
            }
            AlertAction::DiskOverload { mount_point, .. } => {
                record
                    .alert_state
                    .disk_alerted
                    .insert(mount_point.clone(), true);
                record.alert_state.last_disk_alert = Some(now);
            }
            AlertAction::DiskRecovery { mount_point, .. } => {
                record.alert_state.disk_alerted.remove(mount_point);
            }
            AlertAction::NetworkOverload { .. } => {
                record.alert_state.network_alerted = true;
                record.alert_state.last_network_alert = Some(now);
            }
            AlertAction::NetworkRecovery { .. } => {
                record.alert_state.network_alerted = false;
            }
            AlertAction::TemperatureOverload { sensor, .. } => {
                record
                    .alert_state
                    .temperature_alerted
                    .insert(sensor.clone(), true);
                record.alert_state.last_temperature_alert = Some(now);
            }
            AlertAction::TemperatureRecovery { sensor, .. } => {
                record.alert_state.temperature_alerted.remove(sensor);
            }
            AlertAction::GpuOverload { gpu, .. } => {
                record.alert_state.gpu_alerted.insert(gpu.clone(), true);
                record.alert_state.last_gpu_alert = Some(now);
            }
            AlertAction::GpuRecovery { gpu, .. } => {
                record.alert_state.gpu_alerted.remove(gpu);
            }
            AlertAction::DockerContainerDown { container, .. } => {
                record
                    .alert_state
                    .docker_alerted
                    .insert(container.clone(), true);
                record.alert_state.last_docker_alert = Some(now);
            }
            AlertAction::DockerContainerRecovery { container, .. } => {
                record.alert_state.docker_alerted.remove(container);
            }
        }
    }
}
