mod alert_collectors;
mod alerts;
mod network_rates;
mod status;

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use chrono::{SecondsFormat, Utc};

use self::alert_collectors::collect_alerts;
#[cfg(test)]
use self::alert_collectors::{
    collect_cpu_alerts, collect_disk_alerts, collect_docker_alerts, collect_gpu_alerts,
    collect_load_alerts, collect_memory_alerts, collect_network_alerts, collect_port_alerts,
    collect_temperature_alerts,
};
use self::alerts::update_alert_state_after_send;
#[cfg(test)]
use self::alerts::{AlertAction, cooldown_elapsed};
use self::network_rates::{compute_interface_rates, compute_network_rate};
use self::status::compute_status_hash;
use crate::errors::AppError;
use crate::models::agent_metrics::AgentMetrics;
use crate::models::app_state::{AlertConfig, AlertMetricPoint, AppState, HostRecord};
use crate::models::sse_payloads::{HostMetricsPayload, HostStatusPayload};

/// How long to retain in-memory metric history (10 minutes)
const HISTORY_RETENTION_SECS: u64 = 10 * 60;
/// Minimum interval between periodic forced status SSE broadcasts (2 minutes).
/// Used by both `process_metrics` (online path) and `handle_down` (offline path).
pub const STATUS_PERIODIC_INTERVAL_SECS: u64 = 120;

static LEGACY_FALLBACK_TOTAL: AtomicU64 = AtomicU64::new(0);

pub(crate) fn record_legacy_fallback_used() {
    LEGACY_FALLBACK_TOTAL.fetch_add(1, Ordering::Relaxed);
}

pub(crate) fn legacy_fallback_total() -> u64 {
    LEGACY_FALLBACK_TOTAL.load(Ordering::Relaxed)
}

/// Replace NaN / +/-Infinity / negative values with 0.0.
/// Rate, percentage, and temperature metrics are non-negative by physical
/// meaning, so negative inputs are treated as sensor glitches.
#[inline]
pub(crate) fn sanitize_f64(v: f64) -> f64 {
    if v.is_finite() && v >= 0.0 { v } else { 0.0 }
}

#[inline]
pub(crate) fn sanitize_f32(v: f32) -> f32 {
    if v.is_finite() && v >= 0.0 { v } else { 0.0 }
}

/// Return value of `process_metrics`
pub struct ProcessResult {
    pub log_msg: String,
    /// Payload for `event: metrics` — produced every scrape cycle
    pub metrics_payload: HostMetricsPayload,
    /// Payload for `event: status` — only `Some` when Docker/port state changed,
    /// on the first scrape, or after the 2-minute periodic interval
    pub status_payload: Option<HostStatusPayload>,
}

/// Core business logic for processing scraped metric data.
///
/// Lock minimization strategy:
/// 1) Under write lock: only lightweight AlertMetricPoint push + SSE payload assembly
/// 2) Alert evaluation iterates only alert_history (Copy type) — no heap allocations
/// 3) Discord I/O always happens after the lock is released
#[tracing::instrument(skip(metrics, state, alert_config))]
pub async fn process_metrics(
    metrics: &AgentMetrics,
    target: &str,
    state: &AppState,
    alert_config: &AlertConfig,
    scrape_interval_secs: u64,
) -> Result<ProcessResult, AppError> {
    tracing::debug!(hostname = %metrics.hostname, is_online = %metrics.is_online, "Processing metrics (overview)");
    tracing::trace!(metrics = ?metrics, "Detailed metrics JSON data");

    let http_client = state.http_client.clone();
    let hostname = metrics.hostname.clone();

    // Allocate once outside the lock to avoid redundant .to_string()/.clone() inside it.
    let target_str = target.to_string();
    // Prefer the agent-provided timestamp when it parses as RFC 3339; fall
    // back to the server's own wall-clock otherwise. Agents now emit UTC
    // RFC 3339 (v0.3.3); older agents that still send a KST wall-clock
    // string silently fall through to the server fallback — the old contract
    // dropped the field entirely, so no behavior regresses.
    let server_ts = chrono::DateTime::parse_from_rfc3339(&metrics.timestamp)
        .map(|dt| {
            dt.with_timezone(&Utc)
                .to_rfc3339_opts(SecondsFormat::Millis, true)
        })
        .unwrap_or_else(|_| Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true));

    // ── Lock region: only lightweight data manipulation ──
    // AlertMetricPoint is a Copy type (~20 B), so pushing inside the lock is trivially cheap.
    // Vec clones for HostStatusPayload are deferred until after the lock is released.
    let (alert_actions, history_count, metrics_payload, needs_status, alert_eval_now) = {
        let mut store = state.store.write().map_err(|e| {
            AppError::Internal(format!("Failed to acquire store write lock: {}", e))
        })?;

        let record = store
            .hosts
            .entry(target_str.clone())
            .or_insert_with(|| HostRecord::new(hostname.clone()));

        record.last_known_hostname.clone_from(&hostname);

        // ── Push lightweight metric point + evict stale entries ──
        let now = Instant::now();
        let point = AlertMetricPoint {
            received_at: now,
            cpu_usage_percent: metrics.system.cpu_usage_percent,
            memory_usage_percent: metrics.system.memory_usage_percent,
        };
        record.push_alert_point(point, Duration::from_secs(HISTORY_RETENTION_SECS));
        let history_count = record.alert_history.len();

        // ── Compute per-second network throughput (aggregate + per-interface) ──
        let network_rate = compute_network_rate(&metrics.network, &mut record.network_prev);
        let network_interface_rates = compute_interface_rates(
            &metrics.network_interfaces,
            &mut record.network_interface_prev,
        );

        // ── Determine if status SSE payload is needed (decision only, no allocation) ──
        let new_hash = compute_status_hash(
            &metrics.docker_containers,
            &metrics.ports,
            &metrics.system.disks,
        );
        let periodic_elapsed = record.last_status_sent.is_none_or(|t| {
            now.checked_duration_since(t).is_some_and(|elapsed| {
                elapsed >= Duration::from_secs(STATUS_PERIODIC_INTERVAL_SECS)
            })
        });
        let hash_changed = record.prev_status_hash != Some(new_hash);

        let needs_status = hash_changed || periodic_elapsed;
        if needs_status {
            record.prev_status_hash = Some(new_hash);
            record.last_status_sent = Some(now);
        }

        // ── Build metrics SSE payload ──
        let metrics_payload = HostMetricsPayload {
            host_key: target_str,
            display_name: hostname.clone(),
            is_online: metrics.is_online,
            cpu_usage_percent: metrics.system.cpu_usage_percent,
            memory_usage_percent: metrics.system.memory_usage_percent,
            load_1min: metrics.load_average.one_min,
            load_5min: metrics.load_average.five_min,
            load_15min: metrics.load_average.fifteen_min,
            network_rate,
            cpu_cores: metrics.cpu_cores.clone(),
            network_interface_rates,
            disks: metrics.system.disks.clone(),
            temperatures: metrics.system.temperatures.clone(),
            docker_stats: metrics.docker_stats.clone(),
            timestamp: server_ts.clone(),
        };

        // ── Evaluate alert conditions (iterates alert_history only, no I/O) ──
        let alert_actions = collect_alerts(
            record,
            &hostname,
            alert_config,
            metrics,
            &metrics_payload.network_rate,
            now,
        );

        tracing::info!(
            target = %target,
            hostname = %hostname,
            count = history_count,
            "📊 [Store] Recorded metrics"
        );

        (
            alert_actions,
            history_count,
            metrics_payload,
            needs_status,
            now,
        )
        // ← RwLockWriteGuard is dropped here, releasing the lock immediately
    };

    // ── Build status payload OUTSIDE the lock (Vec clones happen here, no contention) ──
    let status_payload = if needs_status {
        // Carry forward system info from existing status (populated by fetch_and_store_system_info)
        let prev_sys = state.last_known_status.read().ok().and_then(|lks| {
            lks.get(target).map(|s| {
                (
                    s.os_info.clone(),
                    s.cpu_model.clone(),
                    s.memory_total_mb,
                    s.boot_time,
                    s.ip_address.clone(),
                )
            })
        });
        let (os_info, cpu_model, memory_total_mb, boot_time, ip_address) =
            prev_sys.unwrap_or((None, None, None, None, None));

        Some(HostStatusPayload {
            host_key: metrics_payload.host_key.clone(),
            display_name: hostname.clone(),
            scrape_interval_secs,
            is_online: metrics.is_online,
            last_seen: server_ts,
            docker_containers: metrics.docker_containers.clone(),
            ports: metrics.ports.clone(),
            disks: metrics.system.disks.clone(),
            processes: metrics.system.processes.clone(),
            temperatures: metrics.system.temperatures.clone(),
            gpus: metrics.system.gpus.clone(),
            docker_stats: metrics.docker_stats.clone(),
            os_info,
            cpu_model,
            memory_total_mb,
            boot_time,
            ip_address,
        })
    } else {
        None
    };

    // Update alert state immediately (brief write lock) before spawning delivery
    if !alert_actions.is_empty() {
        let mut store = state.store.write().map_err(|e| {
            AppError::Internal(format!("Failed to acquire store write lock: {}", e))
        })?;
        if let Some(record) = store.hosts.get_mut(target) {
            update_alert_state_after_send(record, &alert_actions, alert_eval_now);
        }
    }

    // ── Fire-and-forget alert delivery (spawned, non-blocking) ──
    // Alert delivery can take hundreds of milliseconds — spawn it so it doesn't
    // block the scraper from processing the next host.
    //
    // History is persisted via a **single batched INSERT** at the end of the
    // spawn; one host can emit CPU+Memory+Disk+Temperature overloads in the
    // same cycle, which on the pre-batch code path produced N separate writer-
    // lock acquisitions each contending with the scrape-cycle batch INSERT.
    if !alert_actions.is_empty() {
        let messages: Vec<(String, String)> = alert_actions
            .iter()
            .map(|a| (a.alert_type_str().to_string(), a.to_message()))
            .collect();
        let http_client = http_client.clone();
        let db_pool = state.db_pool.clone();
        let target_owned = target.to_string();
        tokio::spawn(async move {
            for (_alert_type, message) in &messages {
                crate::services::alert_service::send_alert(&http_client, &db_pool, message).await;
            }
            let rows: Vec<(&str, &str, &str)> = messages
                .iter()
                .map(|(ty, msg)| (target_owned.as_str(), ty.as_str(), msg.as_str()))
                .collect();
            if let Err(e) =
                crate::repositories::alert_history_repo::insert_alerts_batch(&db_pool, &rows).await
            {
                tracing::error!(err = ?e, count = rows.len(), "⚠️ [AlertHistory] Failed to batch-log alerts");
            }
        });
    }

    // DB persistence is deferred — the caller (scraper) collects ProcessResults
    // and commits all metrics writes in a single transaction per scrape cycle.

    Ok(ProcessResult {
        log_msg: format!(
            "Data from {} ({}) processed successfully (history: {})",
            metrics.hostname, target, history_count
        ),
        metrics_payload,
        status_payload,
    })
}

// ──────────────────────────────────────────────
// Unit tests
// ──────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::agent_metrics::{
        AgentMetrics, LoadAverage, NetworkInterfaceInfo, NetworkTotal, PortStatus, SystemMetrics,
    };
    use crate::models::app_state::{AlertConfig, AlertMetricPoint, HostRecord, MetricAlertRule};
    use crate::models::sse_payloads::NetworkRate;

    const TEST_HOSTNAME: &str = "test-host";

    #[test]
    fn sanitize_f64_nan_zero_inf_neg() {
        assert_eq!(sanitize_f64(f64::NAN), 0.0);
        assert_eq!(sanitize_f64(f64::NEG_INFINITY), 0.0);
        assert_eq!(sanitize_f64(f64::INFINITY), 0.0);
        assert_eq!(sanitize_f64(-1.5), 0.0);
        assert_eq!(sanitize_f64(1.5), 1.5);

        assert_eq!(sanitize_f32(f32::NAN), 0.0);
        assert_eq!(sanitize_f32(f32::NEG_INFINITY), 0.0);
        assert_eq!(sanitize_f32(f32::INFINITY), 0.0);
        assert_eq!(sanitize_f32(-1.5), 0.0);
        assert_eq!(sanitize_f32(1.5), 1.5);
    }

    fn make_metrics(load: f64, cpu: f32, ports: Vec<PortStatus>) -> AgentMetrics {
        AgentMetrics {
            hostname: TEST_HOSTNAME.to_string(),
            timestamp: "2024-01-01T00:00:00Z".to_string(),
            is_online: true,
            system: SystemMetrics {
                cpu_usage_percent: cpu,
                memory_total_mb: 8000,
                memory_used_mb: 4000,
                memory_usage_percent: 50.0,
                disks: vec![],
                processes: vec![],
                temperatures: vec![],
                gpus: vec![],
            },
            network: NetworkTotal::default(),
            load_average: LoadAverage {
                one_min: load,
                five_min: 0.0,
                fifteen_min: 0.0,
            },
            docker_containers: vec![],
            ports,
            agent_version: "0.1.0".to_string(),
            cpu_cores: vec![],
            network_interfaces: vec![],
            docker_stats: vec![],
        }
    }

    fn make_record() -> HostRecord {
        HostRecord::new(TEST_HOSTNAME.to_string())
    }

    fn make_record_with_cpu_history(cpu: f32, count: usize, alerted: bool) -> HostRecord {
        let mut record = make_record();
        record.alert_state.cpu_alerted = alerted;
        for _ in 0..count {
            record.alert_history.push_back(AlertMetricPoint {
                received_at: Instant::now(),
                cpu_usage_percent: cpu,
                memory_usage_percent: 50.0,
            });
        }
        record
    }

    fn make_record_with_memory_history(mem: f32, count: usize, alerted: bool) -> HostRecord {
        let mut record = make_record();
        record.alert_state.memory_alerted = alerted;
        for _ in 0..count {
            record.alert_history.push_back(AlertMetricPoint {
                received_at: Instant::now(),
                cpu_usage_percent: 30.0,
                memory_usage_percent: mem,
            });
        }
        record
    }

    fn default_cpu_rule() -> MetricAlertRule {
        MetricAlertRule {
            enabled: true,
            threshold: 80.0,
            sustained_secs: 5 * 60,
            cooldown_secs: 60,
        }
    }

    fn default_memory_rule() -> MetricAlertRule {
        MetricAlertRule {
            enabled: true,
            threshold: 90.0,
            sustained_secs: 5 * 60,
            cooldown_secs: 60,
        }
    }

    fn default_alert_config() -> AlertConfig {
        AlertConfig::default()
    }

    // ── cooldown_elapsed ─────────────────────────

    #[test]
    fn test_cooldown_elapsed_never_alerted_is_always_ready() {
        assert!(cooldown_elapsed(None, 60, Instant::now()));
    }

    #[test]
    fn test_cooldown_elapsed_just_sent_is_not_ready() {
        let just_now = Instant::now();
        assert!(!cooldown_elapsed(Some(just_now), 60, Instant::now()));
    }

    #[test]
    fn test_cooldown_elapsed_uses_injected_now() {
        let sent_at = Instant::now();
        assert!(!cooldown_elapsed(
            Some(sent_at),
            60,
            sent_at + Duration::from_secs(59)
        ));
        assert!(cooldown_elapsed(
            Some(sent_at),
            60,
            sent_at + Duration::from_secs(60)
        ));
    }

    // ── compute_status_hash ───────────────────────

    #[test]
    fn test_status_hash_same_input_same_hash() {
        use crate::models::agent_metrics::{DockerContainer, PortStatus};
        let containers = vec![DockerContainer {
            container_name: "nginx".to_string(),
            image: "nginx:latest".to_string(),
            state: "running".to_string(),
            status: "Up 2 hours".to_string(),
            oom_killed: false,
            exit_code: None,
            restart_count: 0,
            compose_project: None,
            compose_service: None,
            health_status: None,
        }];
        let ports = vec![PortStatus {
            port: 80,
            is_open: true,
        }];
        let h1 = compute_status_hash(&containers, &ports, &[]);
        let h2 = compute_status_hash(&containers, &ports, &[]);
        assert_eq!(h1, h2);
    }

    #[test]
    fn test_status_hash_different_state_different_hash() {
        use crate::models::agent_metrics::{DockerContainer, PortStatus};
        let running = vec![DockerContainer {
            container_name: "nginx".to_string(),
            image: "nginx:latest".to_string(),
            state: "running".to_string(),
            status: "Up 2 hours".to_string(),
            oom_killed: false,
            exit_code: None,
            restart_count: 0,
            compose_project: None,
            compose_service: None,
            health_status: None,
        }];
        let exited = vec![DockerContainer {
            container_name: "nginx".to_string(),
            image: "nginx:latest".to_string(),
            state: "exited".to_string(),
            status: "Exited (1) 5 minutes ago".to_string(),
            oom_killed: false,
            exit_code: Some(1),
            restart_count: 0,
            compose_project: None,
            compose_service: None,
            health_status: None,
        }];
        let ports = vec![PortStatus {
            port: 80,
            is_open: true,
        }];
        assert_ne!(
            compute_status_hash(&running, &ports, &[]),
            compute_status_hash(&exited, &ports, &[])
        );
    }

    // ── collect_load_alerts ──────────────────────

    #[test]
    fn test_load_alert_fires_above_threshold() {
        let record = make_record();
        let metrics = make_metrics(5.0, 10.0, vec![]);
        let config = default_alert_config();
        let mut actions = Vec::new();
        collect_load_alerts(
            &record,
            TEST_HOSTNAME,
            &config,
            &metrics,
            Instant::now(),
            &mut actions,
        );
        assert_eq!(actions.len(), 1);
        assert!(matches!(actions[0], AlertAction::LoadOverload { .. }));
    }

    #[test]
    fn test_load_alert_silent_below_threshold() {
        let record = make_record();
        let metrics = make_metrics(1.0, 10.0, vec![]);
        let config = default_alert_config();
        let mut actions = Vec::new();
        collect_load_alerts(
            &record,
            TEST_HOSTNAME,
            &config,
            &metrics,
            Instant::now(),
            &mut actions,
        );
        assert!(actions.is_empty());
    }

    #[test]
    fn test_load_alert_no_duplicate_when_already_alerted() {
        let mut record = make_record();
        record.alert_state.load_alerted = true;
        let metrics = make_metrics(5.0, 10.0, vec![]);
        let config = default_alert_config();
        let mut actions = Vec::new();
        collect_load_alerts(
            &record,
            TEST_HOSTNAME,
            &config,
            &metrics,
            Instant::now(),
            &mut actions,
        );
        assert!(actions.is_empty());
    }

    #[test]
    fn test_load_alert_recovery_fires_when_was_alerted() {
        let mut record = make_record();
        record.alert_state.load_alerted = true;
        let metrics = make_metrics(1.0, 10.0, vec![]);
        let config = default_alert_config();
        let mut actions = Vec::new();
        collect_load_alerts(
            &record,
            TEST_HOSTNAME,
            &config,
            &metrics,
            Instant::now(),
            &mut actions,
        );
        assert_eq!(actions.len(), 1);
        assert!(matches!(actions[0], AlertAction::LoadRecovery { .. }));
    }

    // ── collect_cpu_alerts ───────────────────────

    #[test]
    fn test_cpu_alert_fires_when_sustained_high() {
        let record = make_record_with_cpu_history(90.0, 3, false);
        let rule = default_cpu_rule();
        let mut actions = Vec::new();
        collect_cpu_alerts(&record, TEST_HOSTNAME, &rule, Instant::now(), &mut actions);
        assert_eq!(actions.len(), 1);
        assert!(matches!(actions[0], AlertAction::CpuOverload { .. }));
    }

    #[test]
    fn test_cpu_alert_silent_when_cpu_normal() {
        let record = make_record_with_cpu_history(50.0, 3, false);
        let rule = default_cpu_rule();
        let mut actions = Vec::new();
        collect_cpu_alerts(&record, TEST_HOSTNAME, &rule, Instant::now(), &mut actions);
        assert!(actions.is_empty());
    }

    #[test]
    fn test_cpu_alert_no_duplicate_when_already_alerted() {
        let record = make_record_with_cpu_history(90.0, 3, true);
        let rule = default_cpu_rule();
        let mut actions = Vec::new();
        collect_cpu_alerts(&record, TEST_HOSTNAME, &rule, Instant::now(), &mut actions);
        assert!(actions.is_empty());
    }

    #[test]
    fn test_cpu_recovery_fires_when_was_alerted() {
        let record = make_record_with_cpu_history(50.0, 3, true);
        let rule = default_cpu_rule();
        let mut actions = Vec::new();
        collect_cpu_alerts(&record, TEST_HOSTNAME, &rule, Instant::now(), &mut actions);
        assert_eq!(actions.len(), 1);
        assert!(matches!(actions[0], AlertAction::CpuRecovery { .. }));
    }

    #[test]
    fn test_cpu_alert_silent_with_too_few_history() {
        let record = make_record_with_cpu_history(90.0, 1, false);
        let rule = default_cpu_rule();
        let mut actions = Vec::new();
        collect_cpu_alerts(&record, TEST_HOSTNAME, &rule, Instant::now(), &mut actions);
        assert!(actions.is_empty());
    }

    #[test]
    fn test_cpu_alert_silent_with_empty_history() {
        let record = make_record();
        let rule = default_cpu_rule();
        let mut actions = Vec::new();
        collect_cpu_alerts(&record, TEST_HOSTNAME, &rule, Instant::now(), &mut actions);
        assert!(actions.is_empty());
    }

    #[test]
    fn test_cpu_alert_respects_disabled_rule() {
        let record = make_record_with_cpu_history(90.0, 3, false);
        let mut rule = default_cpu_rule();
        rule.enabled = false;
        let mut actions = Vec::new();
        collect_cpu_alerts(&record, TEST_HOSTNAME, &rule, Instant::now(), &mut actions);
        assert!(
            actions.is_empty(),
            "A disabled rule must not generate alerts"
        );
    }

    // ── collect_memory_alerts ────────────────────

    #[test]
    fn test_memory_alert_fires_when_sustained_high() {
        let record = make_record_with_memory_history(95.0, 3, false);
        let rule = default_memory_rule();
        let mut actions = Vec::new();
        collect_memory_alerts(&record, TEST_HOSTNAME, &rule, Instant::now(), &mut actions);
        assert_eq!(actions.len(), 1);
        assert!(matches!(actions[0], AlertAction::MemoryOverload { .. }));
    }

    #[test]
    fn test_memory_alert_silent_when_normal() {
        let record = make_record_with_memory_history(50.0, 3, false);
        let rule = default_memory_rule();
        let mut actions = Vec::new();
        collect_memory_alerts(&record, TEST_HOSTNAME, &rule, Instant::now(), &mut actions);
        assert!(actions.is_empty());
    }

    #[test]
    fn test_memory_recovery_fires_when_was_alerted() {
        let record = make_record_with_memory_history(50.0, 3, true);
        let rule = default_memory_rule();
        let mut actions = Vec::new();
        collect_memory_alerts(&record, TEST_HOSTNAME, &rule, Instant::now(), &mut actions);
        assert_eq!(actions.len(), 1);
        assert!(matches!(actions[0], AlertAction::MemoryRecovery { .. }));
    }

    #[test]
    fn test_memory_alert_respects_disabled_rule() {
        let record = make_record_with_memory_history(95.0, 3, false);
        let mut rule = default_memory_rule();
        rule.enabled = false;
        let mut actions = Vec::new();
        collect_memory_alerts(&record, TEST_HOSTNAME, &rule, Instant::now(), &mut actions);
        assert!(
            actions.is_empty(),
            "A disabled rule must not generate alerts"
        );
    }

    // ── collect_port_alerts ──────────────────────

    #[test]
    fn test_port_down_fires_first_time() {
        let record = make_record();
        let metrics = make_metrics(
            1.0,
            10.0,
            vec![PortStatus {
                port: 80,
                is_open: false,
            }],
        );
        let mut actions = Vec::new();
        collect_port_alerts(&record, TEST_HOSTNAME, &metrics, &mut actions);
        assert_eq!(actions.len(), 1);
        assert!(matches!(actions[0], AlertAction::PortDown { port: 80, .. }));
    }

    #[test]
    fn test_port_down_no_duplicate_alert() {
        let mut record = make_record();
        record.alert_state.port_alerted.insert(80, Instant::now());
        let metrics = make_metrics(
            1.0,
            10.0,
            vec![PortStatus {
                port: 80,
                is_open: false,
            }],
        );
        let mut actions = Vec::new();
        collect_port_alerts(&record, TEST_HOSTNAME, &metrics, &mut actions);
        assert!(actions.is_empty());
    }

    #[test]
    fn test_port_recovery_fires_when_was_down() {
        let mut record = make_record();
        record.alert_state.port_alerted.insert(8080, Instant::now());
        let metrics = make_metrics(
            1.0,
            10.0,
            vec![PortStatus {
                port: 8080,
                is_open: true,
            }],
        );
        let mut actions = Vec::new();
        collect_port_alerts(&record, TEST_HOSTNAME, &metrics, &mut actions);
        assert_eq!(actions.len(), 1);
        assert!(matches!(
            actions[0],
            AlertAction::PortRecovery { port: 8080, .. }
        ));
    }

    #[test]
    fn test_port_open_silent_when_always_open() {
        let record = make_record();
        let metrics = make_metrics(
            1.0,
            10.0,
            vec![PortStatus {
                port: 443,
                is_open: true,
            }],
        );
        let mut actions = Vec::new();
        collect_port_alerts(&record, TEST_HOSTNAME, &metrics, &mut actions);
        assert!(actions.is_empty());
    }

    // ── update_alert_state_after_send ────────────

    #[test]
    fn test_state_update_cpu_overload() {
        let mut record = make_record();
        let actions = vec![AlertAction::CpuOverload {
            hostname: "test".to_string(),
            sustained_mins: 5,
            threshold: 80.0,
            current: 90.0,
        }];
        update_alert_state_after_send(&mut record, &actions, Instant::now());
        assert!(record.alert_state.cpu_alerted);
        assert!(record.alert_state.last_cpu_alert.is_some());
    }

    #[test]
    fn test_state_update_cpu_recovery() {
        let mut record = make_record();
        record.alert_state.cpu_alerted = true;
        let actions = vec![AlertAction::CpuRecovery {
            hostname: "test".to_string(),
            current: 50.0,
        }];
        update_alert_state_after_send(&mut record, &actions, Instant::now());
        assert!(!record.alert_state.cpu_alerted);
    }

    #[test]
    fn test_state_update_memory_overload() {
        let mut record = make_record();
        let actions = vec![AlertAction::MemoryOverload {
            hostname: "test".to_string(),
            sustained_mins: 5,
            threshold: 90.0,
            current: 95.0,
        }];
        update_alert_state_after_send(&mut record, &actions, Instant::now());
        assert!(record.alert_state.memory_alerted);
        assert!(record.alert_state.last_memory_alert.is_some());
    }

    #[test]
    fn test_state_update_memory_recovery() {
        let mut record = make_record();
        record.alert_state.memory_alerted = true;
        let actions = vec![AlertAction::MemoryRecovery {
            hostname: "test".to_string(),
            current: 50.0,
        }];
        update_alert_state_after_send(&mut record, &actions, Instant::now());
        assert!(!record.alert_state.memory_alerted);
    }

    #[test]
    fn test_state_update_port_down_inserts_entry() {
        let mut record = make_record();
        let actions = vec![AlertAction::PortDown {
            hostname: "test-host".to_string(),
            port: 80,
        }];
        update_alert_state_after_send(&mut record, &actions, Instant::now());
        assert!(record.alert_state.port_alerted.contains_key(&80));
    }

    #[test]
    fn test_state_update_port_recovery_removes_entry() {
        let mut record = make_record();
        record.alert_state.port_alerted.insert(443, Instant::now());
        let actions = vec![AlertAction::PortRecovery {
            hostname: "test-host".to_string(),
            port: 443,
        }];
        update_alert_state_after_send(&mut record, &actions, Instant::now());
        assert!(!record.alert_state.port_alerted.contains_key(&443));
    }

    // ── compute_network_rate ────────────────────

    #[test]
    fn test_network_rate_first_call_returns_zero() {
        let net = NetworkTotal {
            total_rx_bytes: 1000,
            total_tx_bytes: 2000,
            ..Default::default()
        };
        let mut prev = None;
        let rate = compute_network_rate(&net, &mut prev);
        assert_eq!(rate.rx_bytes_per_sec, 0.0);
        assert_eq!(rate.tx_bytes_per_sec, 0.0);
        assert!(prev.is_some());
    }

    #[test]
    fn test_network_rate_second_call_computes_delta() {
        let mut prev = Some((1000u64, 2000u64, Instant::now() - Duration::from_secs(1)));
        let net = NetworkTotal {
            total_rx_bytes: 2000,
            total_tx_bytes: 4000,
            ..Default::default()
        };
        let rate = compute_network_rate(&net, &mut prev);
        // 1000 bytes in ~1 second
        assert!(rate.rx_bytes_per_sec > 900.0 && rate.rx_bytes_per_sec < 1100.0);
        assert!(rate.tx_bytes_per_sec > 1900.0 && rate.tx_bytes_per_sec < 2100.0);
    }

    #[test]
    fn test_network_rate_counter_reset_saturating() {
        // Simulates counter reset after reboot
        let mut prev = Some((5000u64, 5000u64, Instant::now() - Duration::from_secs(1)));
        let net = NetworkTotal {
            total_rx_bytes: 100,
            total_tx_bytes: 100,
            ..Default::default()
        };
        let rate = compute_network_rate(&net, &mut prev);
        // saturating_sub: 100 - 5000 = 0
        assert_eq!(rate.rx_bytes_per_sec, 0.0);
        assert_eq!(rate.tx_bytes_per_sec, 0.0);
    }

    #[test]
    fn test_network_rate_prefers_agent_reported_value() {
        // New-agent path: agent has already computed the rate. Server must
        // trust it even if the (prev → current) delta it could compute
        // itself would differ.
        let mut prev = Some((1000u64, 2000u64, Instant::now() - Duration::from_secs(1)));
        let net = NetworkTotal {
            total_rx_bytes: 2000,
            total_tx_bytes: 4000,
            rx_bytes_per_sec: 12_345.0,
            tx_bytes_per_sec: 54_321.0,
            rate_fields_present: true,
        };
        let rate = compute_network_rate(&net, &mut prev);
        assert_eq!(rate.rx_bytes_per_sec, 12_345.0);
        assert_eq!(rate.tx_bytes_per_sec, 54_321.0);
    }

    #[test]
    fn test_network_rate_preserves_agent_reported_zero() {
        let mut prev = Some((1000u64, 2000u64, Instant::now() - Duration::from_secs(1)));
        let net = NetworkTotal {
            total_rx_bytes: 2000,
            total_tx_bytes: 4000,
            rx_bytes_per_sec: 0.0,
            tx_bytes_per_sec: 0.0,
            rate_fields_present: true,
        };
        let rate = compute_network_rate(&net, &mut prev);
        assert_eq!(rate.rx_bytes_per_sec, 0.0);
        assert_eq!(rate.tx_bytes_per_sec, 0.0);
    }

    // ── compute_interface_rates ─────────────────

    #[test]
    fn test_interface_rates_first_call_returns_zero() {
        let interfaces = vec![NetworkInterfaceInfo {
            name: "eth0".to_string(),
            rx_bytes: 1000,
            tx_bytes: 2000,
        }];
        let mut prev_map = std::collections::HashMap::new();
        let rates = compute_interface_rates(&interfaces, &mut prev_map);
        assert_eq!(rates.len(), 1);
        assert_eq!(rates[0].name, "eth0");
        assert_eq!(rates[0].rx_bytes_per_sec, 0.0);
        assert!(prev_map.contains_key("eth0"));
    }

    #[test]
    fn test_interface_rates_delta_computation() {
        let mut prev_map = std::collections::HashMap::new();
        prev_map.insert(
            "eth0".to_string(),
            (1000u64, 2000u64, Instant::now() - Duration::from_secs(1)),
        );
        let interfaces = vec![NetworkInterfaceInfo {
            name: "eth0".to_string(),
            rx_bytes: 2000,
            tx_bytes: 4000,
        }];
        let rates = compute_interface_rates(&interfaces, &mut prev_map);
        assert!(rates[0].rx_bytes_per_sec > 900.0);
        assert!(rates[0].tx_bytes_per_sec > 1900.0);
    }

    // ── collect_disk_alerts ─────────────────────

    #[test]
    fn test_disk_alert_fires_above_threshold() {
        use crate::models::agent_metrics::DiskInfo;
        let record = make_record();
        let rule = MetricAlertRule {
            enabled: true,
            threshold: 90.0,
            sustained_secs: 0,
            cooldown_secs: 300,
        };
        let mut metrics = make_metrics(1.0, 10.0, vec![]);
        metrics.system.disks = vec![DiskInfo {
            name: "sda1".to_string(),
            mount_point: "/".to_string(),
            total_gb: 100.0,
            available_gb: 5.0,
            usage_percent: 95.0,
            read_bytes_per_sec: 0.0,
            write_bytes_per_sec: 0.0,
        }];
        let mut actions = Vec::new();
        collect_disk_alerts(
            &record,
            TEST_HOSTNAME,
            &rule,
            &metrics,
            Instant::now(),
            &mut actions,
        );
        assert_eq!(actions.len(), 1);
        assert!(matches!(actions[0], AlertAction::DiskOverload { .. }));
    }

    #[test]
    fn test_disk_alert_silent_below_threshold() {
        use crate::models::agent_metrics::DiskInfo;
        let record = make_record();
        let rule = MetricAlertRule {
            enabled: true,
            threshold: 90.0,
            sustained_secs: 0,
            cooldown_secs: 300,
        };
        let mut metrics = make_metrics(1.0, 10.0, vec![]);
        metrics.system.disks = vec![DiskInfo {
            name: "sda1".to_string(),
            mount_point: "/".to_string(),
            total_gb: 100.0,
            available_gb: 50.0,
            usage_percent: 50.0,
            read_bytes_per_sec: 0.0,
            write_bytes_per_sec: 0.0,
        }];
        let mut actions = Vec::new();
        collect_disk_alerts(
            &record,
            TEST_HOSTNAME,
            &rule,
            &metrics,
            Instant::now(),
            &mut actions,
        );
        assert!(actions.is_empty());
    }

    #[test]
    fn test_disk_alert_no_duplicate() {
        use crate::models::agent_metrics::DiskInfo;
        let mut record = make_record();
        record
            .alert_state
            .disk_alerted
            .insert("/".to_string(), true);
        let rule = MetricAlertRule {
            enabled: true,
            threshold: 90.0,
            sustained_secs: 0,
            cooldown_secs: 300,
        };
        let mut metrics = make_metrics(1.0, 10.0, vec![]);
        metrics.system.disks = vec![DiskInfo {
            name: "sda1".to_string(),
            mount_point: "/".to_string(),
            total_gb: 100.0,
            available_gb: 5.0,
            usage_percent: 95.0,
            read_bytes_per_sec: 0.0,
            write_bytes_per_sec: 0.0,
        }];
        let mut actions = Vec::new();
        collect_disk_alerts(
            &record,
            TEST_HOSTNAME,
            &rule,
            &metrics,
            Instant::now(),
            &mut actions,
        );
        assert!(actions.is_empty());
    }

    #[test]
    fn test_disk_recovery_fires() {
        use crate::models::agent_metrics::DiskInfo;
        let mut record = make_record();
        record
            .alert_state
            .disk_alerted
            .insert("/".to_string(), true);
        let rule = MetricAlertRule {
            enabled: true,
            threshold: 90.0,
            sustained_secs: 0,
            cooldown_secs: 300,
        };
        let mut metrics = make_metrics(1.0, 10.0, vec![]);
        metrics.system.disks = vec![DiskInfo {
            name: "sda1".to_string(),
            mount_point: "/".to_string(),
            total_gb: 100.0,
            available_gb: 50.0,
            usage_percent: 50.0,
            read_bytes_per_sec: 0.0,
            write_bytes_per_sec: 0.0,
        }];
        let mut actions = Vec::new();
        collect_disk_alerts(
            &record,
            TEST_HOSTNAME,
            &rule,
            &metrics,
            Instant::now(),
            &mut actions,
        );
        assert_eq!(actions.len(), 1);
        assert!(matches!(actions[0], AlertAction::DiskRecovery { .. }));
    }

    #[test]
    fn test_disk_alert_disabled_rule() {
        use crate::models::agent_metrics::DiskInfo;
        let record = make_record();
        let rule = MetricAlertRule {
            enabled: false,
            threshold: 90.0,
            sustained_secs: 0,
            cooldown_secs: 300,
        };
        let mut metrics = make_metrics(1.0, 10.0, vec![]);
        metrics.system.disks = vec![DiskInfo {
            name: "sda1".to_string(),
            mount_point: "/".to_string(),
            total_gb: 100.0,
            available_gb: 5.0,
            usage_percent: 95.0,
            read_bytes_per_sec: 0.0,
            write_bytes_per_sec: 0.0,
        }];
        let mut actions = Vec::new();
        collect_disk_alerts(
            &record,
            TEST_HOSTNAME,
            &rule,
            &metrics,
            Instant::now(),
            &mut actions,
        );
        assert!(actions.is_empty());
    }

    // ── Network / Temperature / GPU ──────────────

    fn enabled_rule(threshold: f64) -> MetricAlertRule {
        MetricAlertRule {
            enabled: true,
            threshold,
            sustained_secs: 0,
            cooldown_secs: 0,
        }
    }

    #[test]
    fn test_network_alert_fires_when_rate_exceeds_threshold() {
        let record = make_record();
        let rule = enabled_rule(100.0); // 100 B/s
        let rate = NetworkRate {
            rx_bytes_per_sec: 80.0,
            tx_bytes_per_sec: 80.0,
            total_rx_bytes: 0,
            total_tx_bytes: 0,
        };
        let mut actions = Vec::new();
        collect_network_alerts(
            &record,
            TEST_HOSTNAME,
            &rule,
            &rate,
            Instant::now(),
            &mut actions,
        );
        assert_eq!(actions.len(), 1);
        assert!(matches!(actions[0], AlertAction::NetworkOverload { .. }));
    }

    #[test]
    fn test_network_alert_silent_below_threshold() {
        let record = make_record();
        let rule = enabled_rule(1_000_000.0);
        let rate = NetworkRate {
            rx_bytes_per_sec: 10.0,
            tx_bytes_per_sec: 10.0,
            total_rx_bytes: 0,
            total_tx_bytes: 0,
        };
        let mut actions = Vec::new();
        collect_network_alerts(
            &record,
            TEST_HOSTNAME,
            &rule,
            &rate,
            Instant::now(),
            &mut actions,
        );
        assert!(actions.is_empty());
    }

    #[test]
    fn test_network_alert_recovery_fires_when_was_alerted() {
        let mut record = make_record();
        record.alert_state.network_alerted = true;
        let rule = enabled_rule(1_000_000.0);
        let rate = NetworkRate {
            rx_bytes_per_sec: 10.0,
            tx_bytes_per_sec: 10.0,
            total_rx_bytes: 0,
            total_tx_bytes: 0,
        };
        let mut actions = Vec::new();
        collect_network_alerts(
            &record,
            TEST_HOSTNAME,
            &rule,
            &rate,
            Instant::now(),
            &mut actions,
        );
        assert_eq!(actions.len(), 1);
        assert!(matches!(actions[0], AlertAction::NetworkRecovery { .. }));
    }

    #[test]
    fn test_temperature_alert_fires_per_sensor() {
        use crate::models::agent_metrics::TemperatureInfo;
        let record = make_record();
        let rule = enabled_rule(80.0);
        let mut metrics = make_metrics(1.0, 10.0, vec![]);
        metrics.system.temperatures = vec![
            TemperatureInfo {
                label: "cpu".to_string(),
                temperature_c: 90.0,
            },
            TemperatureInfo {
                label: "gpu".to_string(),
                temperature_c: 50.0,
            },
        ];
        let mut actions = Vec::new();
        collect_temperature_alerts(
            &record,
            TEST_HOSTNAME,
            &rule,
            &metrics,
            Instant::now(),
            &mut actions,
        );
        assert_eq!(actions.len(), 1);
        match &actions[0] {
            AlertAction::TemperatureOverload { sensor, .. } => assert_eq!(sensor, "cpu"),
            _ => panic!("expected TemperatureOverload"),
        }
    }

    #[test]
    fn test_temperature_alert_disabled_rule() {
        use crate::models::agent_metrics::TemperatureInfo;
        let record = make_record();
        let rule = MetricAlertRule {
            enabled: false,
            ..enabled_rule(50.0)
        };
        let mut metrics = make_metrics(1.0, 10.0, vec![]);
        metrics.system.temperatures = vec![TemperatureInfo {
            label: "cpu".to_string(),
            temperature_c: 100.0,
        }];
        let mut actions = Vec::new();
        collect_temperature_alerts(
            &record,
            TEST_HOSTNAME,
            &rule,
            &metrics,
            Instant::now(),
            &mut actions,
        );
        assert!(actions.is_empty());
    }

    #[test]
    fn test_temperature_alert_recovery_per_sensor() {
        use crate::models::agent_metrics::TemperatureInfo;
        let mut record = make_record();
        record
            .alert_state
            .temperature_alerted
            .insert("cpu".to_string(), true);
        let rule = enabled_rule(80.0);
        let mut metrics = make_metrics(1.0, 10.0, vec![]);
        metrics.system.temperatures = vec![TemperatureInfo {
            label: "cpu".to_string(),
            temperature_c: 40.0,
        }];
        let mut actions = Vec::new();
        collect_temperature_alerts(
            &record,
            TEST_HOSTNAME,
            &rule,
            &metrics,
            Instant::now(),
            &mut actions,
        );
        assert_eq!(actions.len(), 1);
        assert!(matches!(
            actions[0],
            AlertAction::TemperatureRecovery { .. }
        ));
    }

    #[test]
    fn test_gpu_alert_fires_per_device() {
        use crate::models::agent_metrics::GpuInfo;
        let record = make_record();
        let rule = enabled_rule(90.0);
        let mut metrics = make_metrics(1.0, 10.0, vec![]);
        metrics.system.gpus = vec![GpuInfo {
            name: "RTX 4090".to_string(),
            gpu_usage_percent: 95,
            memory_used_mb: 0,
            memory_total_mb: 0,
            temperature_c: 0,
            power_watts: None,
            frequency_mhz: None,
        }];
        let mut actions = Vec::new();
        collect_gpu_alerts(
            &record,
            TEST_HOSTNAME,
            &rule,
            &metrics,
            Instant::now(),
            &mut actions,
        );
        assert_eq!(actions.len(), 1);
        assert!(matches!(actions[0], AlertAction::GpuOverload { .. }));
    }

    #[test]
    fn test_gpu_alert_recovery_clears_state() {
        use crate::models::agent_metrics::GpuInfo;
        let mut record = make_record();
        record
            .alert_state
            .gpu_alerted
            .insert("RTX 4090".to_string(), true);
        let rule = enabled_rule(90.0);
        let mut metrics = make_metrics(1.0, 10.0, vec![]);
        metrics.system.gpus = vec![GpuInfo {
            name: "RTX 4090".to_string(),
            gpu_usage_percent: 40,
            memory_used_mb: 0,
            memory_total_mb: 0,
            temperature_c: 0,
            power_watts: None,
            frequency_mhz: None,
        }];
        let mut actions = Vec::new();
        collect_gpu_alerts(
            &record,
            TEST_HOSTNAME,
            &rule,
            &metrics,
            Instant::now(),
            &mut actions,
        );
        assert_eq!(actions.len(), 1);
        assert!(matches!(actions[0], AlertAction::GpuRecovery { .. }));
    }

    #[test]
    fn test_docker_down_alert_includes_exit_details() {
        use crate::models::agent_metrics::DockerContainer;
        let record = make_record();
        let rule = enabled_rule(1.0);
        let mut metrics = make_metrics(1.0, 10.0, vec![]);
        metrics.docker_containers = vec![DockerContainer {
            container_name: "api".into(),
            image: "api:latest".into(),
            state: "exited".into(),
            status: "Exited (137)".into(),
            oom_killed: true,
            exit_code: Some(137),
            restart_count: 3,
            compose_project: Some("prod".into()),
            compose_service: Some("api".into()),
            health_status: None,
        }];
        let mut actions = Vec::new();
        collect_docker_alerts(
            &record,
            TEST_HOSTNAME,
            &rule,
            &metrics,
            Instant::now(),
            &mut actions,
        );

        assert_eq!(actions.len(), 1);
        match &actions[0] {
            AlertAction::DockerContainerDown {
                container,
                exit_code,
                oom_killed,
                restart_count,
                ..
            } => {
                assert_eq!(container, "api");
                assert_eq!(*exit_code, Some(137));
                assert!(*oom_killed);
                assert_eq!(*restart_count, 3);
            }
            _ => panic!("expected DockerContainerDown"),
        }
    }

    #[test]
    fn test_docker_recovery_fires_when_was_down() {
        use crate::models::agent_metrics::DockerContainer;
        let mut record = make_record();
        record.alert_state.docker_alerted.insert("api".into(), true);
        let rule = enabled_rule(1.0);
        let mut metrics = make_metrics(1.0, 10.0, vec![]);
        metrics.docker_containers = vec![DockerContainer {
            container_name: "api".into(),
            image: "api:latest".into(),
            state: "running".into(),
            status: "Up".into(),
            oom_killed: false,
            exit_code: Some(0),
            restart_count: 1,
            compose_project: None,
            compose_service: None,
            health_status: Some("healthy".into()),
        }];
        let mut actions = Vec::new();
        collect_docker_alerts(
            &record,
            TEST_HOSTNAME,
            &rule,
            &metrics,
            Instant::now(),
            &mut actions,
        );

        assert_eq!(actions.len(), 1);
        assert!(matches!(
            actions[0],
            AlertAction::DockerContainerRecovery { .. }
        ));
    }
}
