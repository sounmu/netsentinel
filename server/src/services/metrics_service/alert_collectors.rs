use std::time::{Duration, Instant};

use crate::models::agent_metrics::AgentMetrics;
use crate::models::app_state::{AlertConfig, AlertMetricPoint, HostRecord, MetricAlertRule};
use crate::models::sse_payloads::NetworkRate;

use super::alerts::{AlertAction, cooldown_elapsed};

// ──────────────────────────────────────────────
// Alert collection (called inside the lock, no I/O)
// AlertConfig is passed as a parameter so thresholds and cooldowns can be injected dynamically.
// ──────────────────────────────────────────────

pub(super) fn collect_alerts(
    record: &HostRecord,
    hostname: &str,
    alert_config: &AlertConfig,
    metrics: &AgentMetrics,
    network_rate: &NetworkRate,
) -> Vec<AlertAction> {
    let mut actions = Vec::new();

    collect_cpu_alerts(record, hostname, &alert_config.cpu, &mut actions);
    collect_memory_alerts(record, hostname, &alert_config.memory, &mut actions);
    collect_load_alerts(record, hostname, alert_config, metrics, &mut actions);
    collect_port_alerts(record, hostname, metrics, &mut actions);
    collect_disk_alerts(record, hostname, &alert_config.disk, metrics, &mut actions);
    collect_network_alerts(
        record,
        hostname,
        &alert_config.network,
        network_rate,
        &mut actions,
    );
    collect_temperature_alerts(
        record,
        hostname,
        &alert_config.temperature,
        metrics,
        &mut actions,
    );
    collect_gpu_alerts(record, hostname, &alert_config.gpu, metrics, &mut actions);
    collect_docker_alerts(
        record,
        hostname,
        &alert_config.docker,
        metrics,
        &mut actions,
    );

    actions
}

// ── CPU overload check ───────────────────────

pub(super) fn collect_cpu_alerts(
    record: &HostRecord,
    hostname: &str,
    rule: &MetricAlertRule,
    actions: &mut Vec<AlertAction>,
) {
    match sustained_percent_decision(
        record,
        rule,
        record.alert_state.cpu_alerted,
        record.alert_state.last_cpu_alert,
        |p| p.cpu_usage_percent,
    ) {
        Some(SustainedPercentDecision::Overload { current }) => {
            actions.push(AlertAction::CpuOverload {
                hostname: hostname.to_string(),
                sustained_mins: rule.sustained_secs / 60,
                threshold: rule.threshold,
                current,
            });
        }
        Some(SustainedPercentDecision::Recovery { current }) => {
            actions.push(AlertAction::CpuRecovery {
                hostname: hostname.to_string(),
                current,
            });
        }
        None => {}
    }
}

// ── RAM overload check ───────────────────────

pub(super) fn collect_memory_alerts(
    record: &HostRecord,
    hostname: &str,
    rule: &MetricAlertRule,
    actions: &mut Vec<AlertAction>,
) {
    match sustained_percent_decision(
        record,
        rule,
        record.alert_state.memory_alerted,
        record.alert_state.last_memory_alert,
        |p| p.memory_usage_percent,
    ) {
        Some(SustainedPercentDecision::Overload { current }) => {
            actions.push(AlertAction::MemoryOverload {
                hostname: hostname.to_string(),
                sustained_mins: rule.sustained_secs / 60,
                threshold: rule.threshold,
                current,
            });
        }
        Some(SustainedPercentDecision::Recovery { current }) => {
            actions.push(AlertAction::MemoryRecovery {
                hostname: hostname.to_string(),
                current,
            });
        }
        None => {}
    }
}

enum SustainedPercentDecision {
    Overload { current: f32 },
    Recovery { current: f32 },
}

fn sustained_percent_decision(
    record: &HostRecord,
    rule: &MetricAlertRule,
    is_alerted: bool,
    last_alert: Option<Instant>,
    value_of: impl Fn(&AlertMetricPoint) -> f32,
) -> Option<SustainedPercentDecision> {
    if !rule.enabled || record.alert_history.is_empty() {
        return None;
    }

    let sustained = Duration::from_secs(rule.sustained_secs);
    let mut recent_count = 0;
    let mut all_high = true;
    for point in record
        .alert_history
        .iter()
        .rev()
        .take_while(|p| p.received_at.elapsed() <= sustained)
    {
        recent_count += 1;
        all_high &= value_of(point) as f64 > rule.threshold;
    }

    if recent_count < 2 {
        return None;
    }

    let latest = record.alert_history.back().map(value_of).unwrap_or(0.0);

    if all_high {
        if !is_alerted && cooldown_elapsed(last_alert, rule.cooldown_secs) {
            return Some(SustainedPercentDecision::Overload { current: latest });
        }
    } else if is_alerted {
        return Some(SustainedPercentDecision::Recovery { current: latest });
    }
    None
}

// ── Load average overload check ──────────────

pub(super) fn collect_load_alerts(
    record: &HostRecord,
    hostname: &str,
    alert_config: &AlertConfig,
    metrics: &AgentMetrics,
    actions: &mut Vec<AlertAction>,
) {
    let load_1min = metrics.load_average.one_min;
    let threshold = alert_config.load_threshold;

    if load_1min > threshold {
        if !record.alert_state.load_alerted
            && cooldown_elapsed(
                record.alert_state.last_load_alert,
                alert_config.load_cooldown_secs,
            )
        {
            actions.push(AlertAction::LoadOverload {
                hostname: hostname.to_string(),
                load: load_1min,
                threshold,
            });
        }
    } else if record.alert_state.load_alerted {
        actions.push(AlertAction::LoadRecovery {
            hostname: hostname.to_string(),
            load: load_1min,
        });
    }
}

// ── Port state check ─────────────────────────

pub(super) fn collect_port_alerts(
    record: &HostRecord,
    hostname: &str,
    metrics: &AgentMetrics,
    actions: &mut Vec<AlertAction>,
) {
    for port_status in &metrics.ports {
        let port = port_status.port;

        if !port_status.is_open {
            if !record.alert_state.port_alerted.contains_key(&port) {
                actions.push(AlertAction::PortDown {
                    hostname: hostname.to_string(),
                    port,
                });
            }
        } else if record.alert_state.port_alerted.contains_key(&port) {
            actions.push(AlertAction::PortRecovery {
                hostname: hostname.to_string(),
                port,
            });
        }
    }
}

// ── Disk overload check ─────────────────────

pub(super) fn collect_disk_alerts(
    record: &HostRecord,
    hostname: &str,
    rule: &MetricAlertRule,
    metrics: &AgentMetrics,
    actions: &mut Vec<AlertAction>,
) {
    if !rule.enabled {
        return;
    }

    for disk in &metrics.system.disks {
        let mount = &disk.mount_point;
        let usage = disk.usage_percent;
        let was_alerted = record
            .alert_state
            .disk_alerted
            .get(mount)
            .copied()
            .unwrap_or(false);

        if (usage as f64) > rule.threshold {
            if !was_alerted
                && cooldown_elapsed(record.alert_state.last_disk_alert, rule.cooldown_secs)
            {
                actions.push(AlertAction::DiskOverload {
                    hostname: hostname.to_string(),
                    mount_point: mount.clone(),
                    threshold: rule.threshold,
                    current: usage,
                });
            }
        } else if was_alerted {
            actions.push(AlertAction::DiskRecovery {
                hostname: hostname.to_string(),
                mount_point: mount.clone(),
                current: usage,
            });
        }
    }
}

// ── Network throughput check ────────────────

pub(super) fn collect_network_alerts(
    record: &HostRecord,
    hostname: &str,
    rule: &MetricAlertRule,
    rate: &NetworkRate,
    actions: &mut Vec<AlertAction>,
) {
    if !rule.enabled {
        return;
    }

    let aggregate = rate.rx_bytes_per_sec + rate.tx_bytes_per_sec;
    if aggregate > rule.threshold {
        if !record.alert_state.network_alerted
            && cooldown_elapsed(record.alert_state.last_network_alert, rule.cooldown_secs)
        {
            actions.push(AlertAction::NetworkOverload {
                hostname: hostname.to_string(),
                bytes_per_sec: aggregate,
                threshold: rule.threshold,
            });
        }
    } else if record.alert_state.network_alerted {
        actions.push(AlertAction::NetworkRecovery {
            hostname: hostname.to_string(),
            bytes_per_sec: aggregate,
        });
    }
}

// ── Temperature check (per sensor) ──────────

pub(super) fn collect_temperature_alerts(
    record: &HostRecord,
    hostname: &str,
    rule: &MetricAlertRule,
    metrics: &AgentMetrics,
    actions: &mut Vec<AlertAction>,
) {
    if !rule.enabled {
        return;
    }

    for sensor in &metrics.system.temperatures {
        let label = &sensor.label;
        let current = sensor.temperature_c;
        let was_alerted = record
            .alert_state
            .temperature_alerted
            .get(label)
            .copied()
            .unwrap_or(false);

        if (current as f64) > rule.threshold {
            if !was_alerted
                && cooldown_elapsed(
                    record.alert_state.last_temperature_alert,
                    rule.cooldown_secs,
                )
            {
                actions.push(AlertAction::TemperatureOverload {
                    hostname: hostname.to_string(),
                    sensor: label.clone(),
                    threshold: rule.threshold,
                    current,
                });
            }
        } else if was_alerted {
            actions.push(AlertAction::TemperatureRecovery {
                hostname: hostname.to_string(),
                sensor: label.clone(),
                current,
            });
        }
    }
}

// ── GPU check (per device) ──────────────────

pub(super) fn collect_gpu_alerts(
    record: &HostRecord,
    hostname: &str,
    rule: &MetricAlertRule,
    metrics: &AgentMetrics,
    actions: &mut Vec<AlertAction>,
) {
    if !rule.enabled {
        return;
    }

    for gpu in &metrics.system.gpus {
        let name = &gpu.name;
        let current = gpu.gpu_usage_percent as f32;
        let was_alerted = record
            .alert_state
            .gpu_alerted
            .get(name)
            .copied()
            .unwrap_or(false);

        if (current as f64) > rule.threshold {
            if !was_alerted
                && cooldown_elapsed(record.alert_state.last_gpu_alert, rule.cooldown_secs)
            {
                actions.push(AlertAction::GpuOverload {
                    hostname: hostname.to_string(),
                    gpu: name.clone(),
                    threshold: rule.threshold,
                    current,
                });
            }
        } else if was_alerted {
            actions.push(AlertAction::GpuRecovery {
                hostname: hostname.to_string(),
                gpu: name.clone(),
                current,
            });
        }
    }
}

// ── Docker lifecycle check ──────────────────

pub(super) fn collect_docker_alerts(
    record: &HostRecord,
    hostname: &str,
    rule: &MetricAlertRule,
    metrics: &AgentMetrics,
    actions: &mut Vec<AlertAction>,
) {
    if !rule.enabled {
        return;
    }

    for container in &metrics.docker_containers {
        let name = &container.container_name;
        let is_running = container.state == "running";
        let was_alerted = record
            .alert_state
            .docker_alerted
            .get(name)
            .copied()
            .unwrap_or(false);

        if !is_running {
            if !was_alerted
                && cooldown_elapsed(record.alert_state.last_docker_alert, rule.cooldown_secs)
            {
                actions.push(AlertAction::DockerContainerDown {
                    hostname: hostname.to_string(),
                    container: name.clone(),
                    state: container.state.clone(),
                    exit_code: container.exit_code,
                    oom_killed: container.oom_killed,
                    restart_count: container.restart_count,
                });
            }
        } else if was_alerted {
            actions.push(AlertAction::DockerContainerRecovery {
                hostname: hostname.to_string(),
                container: name.clone(),
            });
        }
    }
}
