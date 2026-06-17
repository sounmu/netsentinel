// ──────────────────────────────────────────────
// Alert config runtime structs
// Loaded from the alert_configs DB table each scrape cycle
// ──────────────────────────────────────────────

#[derive(Debug, Clone, Copy)]
pub struct MetricAlertRule {
    pub enabled: bool,
    pub threshold: f64,
    pub sustained_secs: u64,
    pub cooldown_secs: u64,
}

#[derive(Debug, Clone)]
pub struct AlertConfig {
    pub cpu: MetricAlertRule,
    pub memory: MetricAlertRule,
    pub disk: MetricAlertRule,
    /// Load-average rule loaded from alert_configs (metric_type='load').
    /// When present, this takes precedence over `load_threshold` / `load_cooldown_secs`
    /// below, which are carried forward for back-compat with the per-host `hosts.load_threshold`
    /// column.
    pub load: MetricAlertRule,
    /// Aggregate network throughput rule — threshold is bytes/sec across all physical NICs.
    pub network: MetricAlertRule,
    /// Temperature rule — applied to every sensor in the scrape payload.
    pub temperature: MetricAlertRule,
    /// GPU usage rule — applied to every GPU device.
    pub gpu: MetricAlertRule,
    /// Docker lifecycle rule — fires when a reported container leaves running state.
    pub docker: MetricAlertRule,
    pub load_threshold: f64,
    pub load_cooldown_secs: u64,
}

impl Default for AlertConfig {
    fn default() -> Self {
        Self {
            cpu: MetricAlertRule {
                enabled: true,
                threshold: 80.0,
                sustained_secs: 5 * 60,
                cooldown_secs: 60,
            },
            memory: MetricAlertRule {
                enabled: true,
                threshold: 90.0,
                sustained_secs: 5 * 60,
                cooldown_secs: 60,
            },
            disk: MetricAlertRule {
                enabled: true,
                threshold: 90.0,
                sustained_secs: 0, // Disk alerts fire immediately (no sustained window)
                cooldown_secs: 300,
            },
            load: MetricAlertRule {
                enabled: false,
                threshold: 4.0,
                sustained_secs: 5 * 60,
                cooldown_secs: 300,
            },
            network: MetricAlertRule {
                enabled: false,
                threshold: 500_000_000.0, // 500 MB/s aggregate
                sustained_secs: 5 * 60,
                cooldown_secs: 600,
            },
            temperature: MetricAlertRule {
                enabled: false,
                threshold: 85.0, // °C
                sustained_secs: 2 * 60,
                cooldown_secs: 600,
            },
            gpu: MetricAlertRule {
                enabled: false,
                threshold: 90.0,
                sustained_secs: 5 * 60,
                cooldown_secs: 300,
            },
            docker: MetricAlertRule {
                enabled: false,
                threshold: 1.0,
                sustained_secs: 0,
                cooldown_secs: 300,
            },
            load_threshold: 4.0,
            load_cooldown_secs: 60,
        }
    }
}
