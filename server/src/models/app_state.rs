use std::collections::HashMap;
use std::sync::{Arc, RwLock};

pub use crate::models::alert_runtime::{AlertConfig, MetricAlertRule};
pub use crate::models::metrics_store::{AlertMetricPoint, HostRecord, MetricsStore};
use crate::models::sse_payloads::{HostStatusPayload, SseBroadcast};
use crate::repositories::hosts_repo::HostRow;
use crate::repositories::metrics_repo::{ChartMetricsRow, MetricsRow};
use crate::services::hosts_snapshot::SharedHostsSnapshot;
pub use crate::services::metrics_cache::{
    CacheWeight, MetricsQueryCache, metrics_cache_key, should_cache_metrics_range,
};
use crate::services::monitors_snapshot::SharedMonitorsSnapshot;
use crate::services::oauth::GoogleOAuthConfig;
use crate::services::oauth_state_store::OAuthStateStore;
pub use crate::services::rate_limiter::LoginRateLimiter;
use crate::services::sse_ticket::SseTicketStore;
use tokio::sync::broadcast;

// ──────────────────────────────────────────────
// Application shared state
// ──────────────────────────────────────────────

/// Top-level state struct injected into the Axum router.
/// Fully DB-driven — no config.yaml dependency at runtime.
#[derive(Clone)]
pub struct AppState {
    /// In-memory store for per-host metric history and alert state
    pub store: SharedStore,
    /// Shared HTTP client reused for alert notifications and external OAuth calls.
    pub http_client: reqwest::Client,
    /// Google OAuth client configuration loaded from environment at startup.
    pub google_oauth: Arc<GoogleOAuthConfig>,
    /// Short-lived one-use OAuth state + PKCE verifier store.
    pub oauth_state_store: Arc<OAuthStateStore>,
    /// Serializes the first-login admin bootstrap path so two concurrent
    /// Google callbacks cannot both observe an empty users table.
    pub oauth_bootstrap_lock: Arc<tokio::sync::Mutex<()>>,
    /// Database connection pool. NetSentinel is SQLite-only; the alias
    /// keeps downstream modules from importing sqlx internals directly.
    pub db_pool: crate::db::DbPool,
    /// Global scrape interval in seconds (from env var or default 10)
    pub scrape_interval_secs: u64,
    /// Configured sqlx pool size, used by fan-out handlers to avoid
    /// out-concurrencying the SQLite connection pool.
    pub max_db_connections: u32,
    /// SSE event broadcast channel sender
    pub sse_tx: broadcast::Sender<SseBroadcast>,
    /// Cache of the most recently sent per-host status payload.
    ///
    /// Uses `std::sync::RwLock` (not `tokio::sync::RwLock`) deliberately:
    /// lock scopes are micro-duration data shuffles with **no `.await` inside**,
    /// so the lower per-access overhead of std RwLock beats tokio's cooperative
    /// scheduling cost. Do not add `.await` calls inside lock scopes.
    /// Values are `Arc<HostStatusPayload>` so `build_initial_events`
    /// (SSE handshake + `Lagged` re-sync) can drain the map to a `Vec`
    /// of cheap reference-count bumps under the read lock, then serialize
    /// each payload **outside** the critical section. Writers either
    /// insert a freshly-built `Arc::new(...)` or swap in a new `Arc`
    /// via `Arc::make_mut` for in-place field updates.
    pub last_known_status: Arc<RwLock<HashMap<String, Arc<HostStatusPayload>>>>,
    /// TTL cache for full long-range metric queries (avoids repeated DB scans for same range)
    pub metrics_query_cache: Arc<MetricsQueryCache<MetricsRow>>,
    /// TTL cache for lightweight chart long-range queries.
    pub chart_metrics_query_cache: Arc<MetricsQueryCache<ChartMetricsRow>>,
    /// Per-IP OAuth login rate limiter for start/callback traffic.
    /// Default 30 per 5 min — sized so a small NAT / Cloudflare-tunnel
    /// deployment with several concurrent dashboards does not lock itself out
    /// during normal Google redirect retries.
    pub login_rate_limiter: Arc<LoginRateLimiter>,
    /// Per-username local login limiter. Keeps password guessing against one
    /// account bounded even when the attacker rotates source IPs.
    pub login_user_rate_limiter: Arc<LoginRateLimiter>,
    /// Number of trusted reverse proxies in front of the server.
    /// When 0, X-Forwarded-For is ignored and the peer socket IP is used.
    /// When >0, the Nth IP from the right of X-Forwarded-For is used.
    pub trusted_proxy_count: usize,
    /// Unified "tokens before this instant are invalid" cache keyed by
    /// `user_id`. Fed by password changes and explicit user/admin revocations
    /// (`users.password_changed_at`, `users.tokens_revoked_at`) — see
    /// `services::auth` for the verification path.
    pub token_revocation_cutoffs: Arc<RwLock<HashMap<i32, i64>>>,
    /// Single-use opaque ticket store for the SSE handshake.
    /// See `services::sse_ticket` for rationale.
    pub sse_ticket_store: Arc<SseTicketStore>,
    /// Per-IP rate limiter for all API endpoints. More generous than the
    /// OAuth limiter. Prevents any single IP from overwhelming the server
    /// with rapid-fire requests.
    pub api_rate_limiter: Arc<LoginRateLimiter>,
    /// Tighter per-IP limiter for **unauthenticated** endpoints
    /// (`/api/auth/oauth/google/*|status`, `/api/public/status`, `/api/health`).
    /// Without a separate bucket, abusive unauthenticated traffic would eat
    /// into the same budget the authenticated SPA uses for SWR polling +
    /// SSE retry, forcing the authenticated shell to return 429 while the
    /// abuse is ongoing.
    pub public_api_rate_limiter: Arc<LoginRateLimiter>,
    /// Global cap on concurrent SSE connections. Each `/api/stream` stream
    /// holds a `broadcast::Receiver`, a `last_known_status` snapshot, and
    /// an `auth_check` interval — unbounded growth turns one misbehaving
    /// client into a memory exhaustion vector. Controlled by
    /// `MAX_SSE_CONNECTIONS` env var.
    pub sse_connections: Arc<std::sync::atomic::AtomicUsize>,
    /// Upper bound the connection counter is compared against.
    pub max_sse_connections: usize,
    /// Cached view of the `hosts` + `alert_configs` tables used by the
    /// scraper hot path. See `services::hosts_snapshot` for the refresh
    /// protocol (invalidation on mutation handlers + 60 s background tick).
    /// This replaced per-scrape `SELECT * FROM hosts` + `SELECT * FROM alert_configs`
    /// round-trips (Top-10 review finding #10).
    pub hosts_snapshot: SharedHostsSnapshot,
    /// Cached view of the enabled HTTP / Ping monitor sets used by
    /// `monitor_scraper`. Replaces the per-sweep
    /// `SELECT … FROM http_monitors WHERE enabled = 1` + ping equivalent
    /// (Top-10 review #9). Refreshed synchronously on monitor mutation
    /// handlers and every 60 s as a backstop.
    pub monitors_snapshot: SharedMonitorsSnapshot,
}

impl AppState {
    /// Pre-populate last_known_status from the hosts table on startup.
    /// Ensures SSE clients see all configured hosts immediately upon connection.
    pub fn pre_populate_status(&self, hosts: &[HostRow]) {
        let mut lks = self.last_known_status.write().unwrap_or_else(|e| {
            tracing::warn!("⚠️ [Status] RwLock poisoned during pre_populate_status, recovering");
            e.into_inner()
        });
        for host in hosts {
            lks.entry(host.host_key.clone()).or_insert_with(|| {
                Arc::new(HostStatusPayload {
                    host_key: host.host_key.clone(),
                    display_name: host.display_name.clone(),
                    scrape_interval_secs: u64::try_from(host.scrape_interval_secs)
                        .ok()
                        .filter(|secs| *secs > 0)
                        .unwrap_or(self.scrape_interval_secs),
                    is_online: false,
                    last_seen: String::new(),
                    docker_containers: vec![],
                    ports: vec![],
                    disks: vec![],
                    processes: vec![],
                    temperatures: vec![],
                    gpus: vec![],
                    docker_stats: vec![],
                    os_info: host.os_info.clone(),
                    cpu_model: host.cpu_model.clone(),
                    memory_total_mb: host.memory_total_mb,
                    boot_time: host.boot_time,
                    ip_address: host.ip_address.clone(),
                })
            });
        }
    }
}

/// Thread-safe shared store type alias (RwLock-guarded)
pub type SharedStore = Arc<RwLock<MetricsStore>>;
