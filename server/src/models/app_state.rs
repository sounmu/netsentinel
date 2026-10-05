use std::collections::HashMap;
use std::sync::{Arc, RwLock};

pub use crate::models::alert_runtime::{AlertConfig, MetricAlertRule};
pub use crate::models::metrics_store::{AlertMetricPoint, HostRecord, MetricsStore};
use crate::models::sse_payloads::{HostMetricsPayload, HostStatusPayload, SseBroadcast};
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
    /// Cache of the most recently broadcast per-host **metrics** payload.
    ///
    /// Companion to `last_known_status`. Where the status cache carries the
    /// semi-static shape (disks, containers, ports, system info), this one
    /// carries the live scalars the Overview table reads — CPU%, memory%,
    /// load, network rate. Without it, the SSE handshake only replayed
    /// `status` events, so a freshly-connected dashboard rendered host rows
    /// immediately but left every metric column blank until the next scrape
    /// cycle's `metrics` broadcast (up to one `scrape_interval` of lag, ~10–20 s).
    ///
    /// Replaying the last metrics on handshake makes a new client converge to
    /// the exact state an already-connected client holds: a long-lived client
    /// keeps the last `metrics` in its map even after a host goes offline
    /// (only `status` is re-sent on `handle_down`), and the client-side
    /// `getHostStatus` timestamp safety-net still marks stale rows offline —
    /// so caching the last *online* metrics never resurrects a down host.
    ///
    /// Same `std::sync::RwLock` + `Arc<…>` discipline as `last_known_status`:
    /// no `.await` inside the lock scope; readers drain to a `Vec` of
    /// refcount bumps then serialize outside the critical section.
    pub last_known_metrics: Arc<RwLock<HashMap<String, Arc<HostMetricsPayload>>>>,
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
    /// Ceiling on failed local logins per username across all source IPs.
    /// Much higher than the per-(username, IP) limit so one client cannot
    /// lock an account out, while distributed guessing stays bounded.
    pub login_user_global_rate_limiter: Arc<LoginRateLimiter>,
    /// Number of trusted reverse proxies in front of the server.
    /// When 0, X-Forwarded-For is ignored and the peer socket IP is used.
    /// When >0, the Nth IP from the right of X-Forwarded-For is used.
    pub trusted_proxy_count: usize,
    /// Honour `CF-Connecting-IP` (only meaningful with `trusted_proxy_count > 0`).
    /// Opt-in: any proxy other than Cloudflare passes a client-supplied value
    /// through unchanged, which would let clients pick their rate-limit key.
    pub trust_cf_connecting_ip: bool,
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

#[cfg(test)]
impl AppState {
    /// A fully wired state over an in-memory database, for tests that drive
    /// real code paths (scraper, handlers) instead of isolated helpers.
    pub(crate) async fn for_tests() -> Arc<Self> {
        use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
        use std::str::FromStr;
        use std::time::Duration;

        let options = SqliteConnectOptions::from_str("sqlite::memory:")
            .unwrap()
            .foreign_keys(false);
        let db_pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(options)
            .await
            .unwrap();
        sqlx::migrate!("./migrations").run(&db_pool).await.unwrap();

        let limiter = || Arc::new(LoginRateLimiter::new(1_000, Duration::from_secs(60)));
        let cache_ttl = Duration::from_secs(60);
        let (sse_tx, _) = broadcast::channel(16);

        Arc::new(Self {
            store: Arc::new(RwLock::new(MetricsStore::new())),
            http_client: reqwest::Client::new(),
            google_oauth: Arc::new(GoogleOAuthConfig::from_env().unwrap()),
            oauth_state_store: Arc::new(OAuthStateStore::new()),
            oauth_bootstrap_lock: Arc::new(tokio::sync::Mutex::new(())),
            db_pool,
            scrape_interval_secs: 10,
            max_db_connections: 1,
            sse_tx,
            last_known_status: Arc::new(RwLock::new(HashMap::new())),
            last_known_metrics: Arc::new(RwLock::new(HashMap::new())),
            metrics_query_cache: Arc::new(MetricsQueryCache::new(cache_ttl, 4, 1 << 20)),
            chart_metrics_query_cache: Arc::new(MetricsQueryCache::new(cache_ttl, 4, 1 << 20)),
            login_rate_limiter: limiter(),
            login_user_rate_limiter: limiter(),
            login_user_global_rate_limiter: limiter(),
            trusted_proxy_count: 0,
            trust_cf_connecting_ip: false,
            token_revocation_cutoffs: Arc::new(RwLock::new(HashMap::new())),
            sse_ticket_store: Arc::new(SseTicketStore::new()),
            api_rate_limiter: limiter(),
            public_api_rate_limiter: limiter(),
            sse_connections: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
            max_sse_connections: 8,
            hosts_snapshot: crate::services::hosts_snapshot::empty(),
            monitors_snapshot: crate::services::monitors_snapshot::empty(),
        })
    }
}
