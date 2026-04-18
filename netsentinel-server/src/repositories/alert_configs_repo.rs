use std::collections::HashMap;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;

use crate::models::app_state::{AlertConfig, MetricAlertRule};

/// Alert metric type — compile-time exhaustive matching.
///
/// The first three variants (Cpu/Memory/Disk) are the legacy metrics whose
/// thresholds the scraper evaluates every cycle. The remaining variants were
/// introduced to let operators store rules for sensors that the agent already
/// reports (Load/Network/Temperature/Gpu). Their scraper-side evaluation is
/// scheduled for a later milestone — today the server just persists and
/// surfaces them so the UI can show a complete rule catalog.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, sqlx::Type)]
#[sqlx(type_name = "TEXT", rename_all = "lowercase")]
#[serde(rename_all = "lowercase")]
pub enum MetricType {
    Cpu,
    Memory,
    Disk,
    Load,
    Network,
    Temperature,
    Gpu,
}

impl std::fmt::Display for MetricType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MetricType::Cpu => write!(f, "cpu"),
            MetricType::Memory => write!(f, "memory"),
            MetricType::Disk => write!(f, "disk"),
            MetricType::Load => write!(f, "load"),
            MetricType::Network => write!(f, "network"),
            MetricType::Temperature => write!(f, "temperature"),
            MetricType::Gpu => write!(f, "gpu"),
        }
    }
}

/// Row struct for the `alert_configs` table.
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct AlertConfigRow {
    pub id: i32,
    pub host_key: Option<String>,
    pub metric_type: MetricType,
    pub enabled: bool,
    pub threshold: f64,
    pub sustained_secs: i32,
    pub cooldown_secs: i32,
    pub updated_at: DateTime<Utc>,
    /// Optional scope within a metric type — e.g. a specific sensor label,
    /// interface name, or GPU index. `NULL` means the rule applies to the
    /// whole metric category on the host.
    #[serde(default)]
    pub sub_key: Option<String>,
}

/// Fetch global default alert configs (host_key IS NULL).
pub async fn get_global_configs(pool: &PgPool) -> Result<Vec<AlertConfigRow>, sqlx::Error> {
    sqlx::query_as::<_, AlertConfigRow>(
        "SELECT * FROM alert_configs WHERE host_key IS NULL ORDER BY metric_type, sub_key",
    )
    .fetch_all(pool)
    .await
}

/// Fetch per-host alert config overrides only.
pub async fn get_host_configs(
    pool: &PgPool,
    host_key: &str,
) -> Result<Vec<AlertConfigRow>, sqlx::Error> {
    sqlx::query_as::<_, AlertConfigRow>(
        "SELECT * FROM alert_configs WHERE host_key = $1 ORDER BY metric_type, sub_key",
    )
    .bind(host_key)
    .fetch_all(pool)
    .await
}

/// Load all alert configs and build a per-host AlertConfig map.
///
/// Only the legacy Cpu/Memory/Disk variants feed `MetricAlertRule` here;
/// the extended metric types are stored but not yet evaluated by the
/// scraper, so they are intentionally ignored when materialising runtime
/// rules.
pub async fn load_all_as_map(pool: &PgPool) -> Result<HashMap<String, AlertConfig>, sqlx::Error> {
    let rows = sqlx::query_as::<_, AlertConfigRow>(
        "SELECT * FROM alert_configs ORDER BY host_key NULLS FIRST, metric_type",
    )
    .fetch_all(pool)
    .await?;

    let mut global_cpu = default_cpu_rule();
    let mut global_mem = default_memory_rule();
    let mut global_disk = default_disk_rule();

    for row in &rows {
        if row.host_key.is_none() && row.sub_key.is_none() {
            match row.metric_type {
                MetricType::Cpu => global_cpu = row_to_rule(row),
                MetricType::Memory => global_mem = row_to_rule(row),
                MetricType::Disk => global_disk = row_to_rule(row),
                _ => {}
            }
        }
    }

    let mut map = HashMap::new();
    let mut host_overrides: HashMap<(&str, MetricType), MetricAlertRule> = HashMap::new();
    let mut host_keys_set = std::collections::HashSet::new();

    for row in &rows {
        if let Some(ref hk) = row.host_key {
            // Only consider whole-metric overrides (sub_key IS NULL) for the
            // runtime rule map; sub-scoped rules are addressed by the handler
            // that consumes them.
            if row.sub_key.is_none() {
                host_overrides.insert((hk.as_str(), row.metric_type), row_to_rule(row));
            }
            host_keys_set.insert(hk.as_str());
        }
    }

    for hk in host_keys_set {
        let cpu = host_overrides
            .get(&(hk, MetricType::Cpu))
            .copied()
            .unwrap_or(global_cpu);
        let mem = host_overrides
            .get(&(hk, MetricType::Memory))
            .copied()
            .unwrap_or(global_mem);
        let disk = host_overrides
            .get(&(hk, MetricType::Disk))
            .copied()
            .unwrap_or(global_disk);

        map.insert(
            hk.to_string(),
            AlertConfig {
                cpu,
                memory: mem,
                disk,
                load_threshold: 4.0,
                load_cooldown_secs: 60,
            },
        );
    }

    map.insert(
        "__global__".to_string(),
        AlertConfig {
            cpu: global_cpu,
            memory: global_mem,
            disk: global_disk,
            load_threshold: 4.0,
            load_cooldown_secs: 60,
        },
    );

    Ok(map)
}

/// Resolve the effective AlertConfig for a host (host override → global fallback).
pub fn resolve_alert_config(
    host_key: &str,
    load_threshold: f64,
    alert_map: &HashMap<String, AlertConfig>,
) -> AlertConfig {
    let base = alert_map
        .get(host_key)
        .or_else(|| alert_map.get("__global__"))
        .cloned()
        .unwrap_or_default();

    AlertConfig {
        load_threshold,
        ..base
    }
}

/// Request body for upserting an alert config entry.
#[derive(Debug, Deserialize)]
pub struct UpsertAlertRequest {
    pub metric_type: MetricType,
    pub enabled: bool,
    pub threshold: f64,
    pub sustained_secs: i32,
    pub cooldown_secs: i32,
    /// Optional scope within a metric type (sensor / interface / device).
    #[serde(default)]
    pub sub_key: Option<String>,
}

/// Upsert a global or per-host alert config row.
pub async fn upsert_alert_config(
    pool: &PgPool,
    host_key: Option<&str>,
    req: &UpsertAlertRequest,
) -> Result<AlertConfigRow, sqlx::Error> {
    sqlx::query_as::<_, AlertConfigRow>(
        r#"
        INSERT INTO alert_configs
            (host_key, metric_type, sub_key, enabled, threshold, sustained_secs, cooldown_secs, updated_at)
        VALUES ($1, $2, $3, $4, $5, $6, $7, NOW())
        ON CONFLICT (host_key, metric_type, sub_key)
        DO UPDATE SET
            enabled = EXCLUDED.enabled,
            threshold = EXCLUDED.threshold,
            sustained_secs = EXCLUDED.sustained_secs,
            cooldown_secs = EXCLUDED.cooldown_secs,
            updated_at = NOW()
        RETURNING *
        "#,
    )
    .bind(host_key)
    .bind(req.metric_type)
    .bind(req.sub_key.as_deref())
    .bind(req.enabled)
    .bind(req.threshold)
    .bind(req.sustained_secs)
    .bind(req.cooldown_secs)
    .fetch_one(pool)
    .await
}

/// Apply the same set of overrides to every host in one transaction.
pub async fn bulk_upsert_host_configs(
    pool: &PgPool,
    host_keys: &[String],
    requests: &[UpsertAlertRequest],
) -> Result<Vec<AlertConfigRow>, sqlx::Error> {
    let mut tx = pool.begin().await?;
    let mut rows = Vec::with_capacity(host_keys.len() * requests.len());

    for hk in host_keys {
        for req in requests {
            let row = sqlx::query_as::<_, AlertConfigRow>(
                r#"
                INSERT INTO alert_configs
                    (host_key, metric_type, sub_key, enabled, threshold, sustained_secs, cooldown_secs, updated_at)
                VALUES ($1, $2, $3, $4, $5, $6, $7, NOW())
                ON CONFLICT (host_key, metric_type, sub_key)
                DO UPDATE SET
                    enabled = EXCLUDED.enabled,
                    threshold = EXCLUDED.threshold,
                    sustained_secs = EXCLUDED.sustained_secs,
                    cooldown_secs = EXCLUDED.cooldown_secs,
                    updated_at = NOW()
                RETURNING *
                "#,
            )
            .bind(hk)
            .bind(req.metric_type)
            .bind(req.sub_key.as_deref())
            .bind(req.enabled)
            .bind(req.threshold)
            .bind(req.sustained_secs)
            .bind(req.cooldown_secs)
            .fetch_one(&mut *tx)
            .await?;
            rows.push(row);
        }
    }

    tx.commit().await?;
    Ok(rows)
}

/// Delete all per-host alert config overrides (host reverts to global defaults).
pub async fn delete_host_configs(pool: &PgPool, host_key: &str) -> Result<bool, sqlx::Error> {
    let result = sqlx::query("DELETE FROM alert_configs WHERE host_key = $1")
        .bind(host_key)
        .execute(pool)
        .await?;
    Ok(result.rows_affected() > 0)
}

fn row_to_rule(row: &AlertConfigRow) -> MetricAlertRule {
    MetricAlertRule {
        enabled: row.enabled,
        threshold: row.threshold,
        sustained_secs: row.sustained_secs as u64,
        cooldown_secs: row.cooldown_secs as u64,
    }
}

fn default_cpu_rule() -> MetricAlertRule {
    MetricAlertRule {
        enabled: true,
        threshold: 80.0,
        sustained_secs: 300,
        cooldown_secs: 60,
    }
}

fn default_memory_rule() -> MetricAlertRule {
    MetricAlertRule {
        enabled: true,
        threshold: 90.0,
        sustained_secs: 300,
        cooldown_secs: 60,
    }
}

fn default_disk_rule() -> MetricAlertRule {
    MetricAlertRule {
        enabled: true,
        threshold: 90.0,
        sustained_secs: 0,
        cooldown_secs: 300,
    }
}
