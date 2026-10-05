use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use chrono::{SecondsFormat, Utc};
use futures::StreamExt;
use futures::stream;
use reqwest::Client;

use crate::models::agent_metrics::{
    AgentMetrics, SystemInfoResponse, WIRE_VERSION_HEADER, deserialize_agent_metrics_versioned,
};
use crate::models::app_state::{AlertConfig, AppState, HostRecord};
use crate::models::sse_payloads::{HostStatusPayload, SseBroadcast};
use crate::repositories::{alert_configs_repo, hosts_repo, metrics_repo};
use crate::services::alert_service;
use crate::services::auth::RESPONSE_SIGNATURE_HEADER;
use crate::services::hosts_snapshot;
use crate::services::metrics_service::{self, STATUS_PERIODIC_INTERVAL_SECS};
use chrono::DateTime;

/// Result of a single host scrape — carries data needed for batch DB persistence.
enum ScrapeOutcome {
    /// Scrape succeeded; metrics should be batch-inserted as online.
    Online(Box<AgentMetrics>),
    /// Agent unreachable; an offline record should be batch-inserted.
    Offline,
    /// The agent answered, but the answer cannot be trusted (bad or missing
    /// response signature). Recorded and alerted exactly like `Offline`:
    /// otherwise anyone able to corrupt responses could hold a host at its
    /// last known "online" status with no Host Down alert.
    Rejected(String),
    /// Non-recoverable error (e.g., deserialization); no DB insert needed.
    Failed(String),
}

/// Server version (from Cargo.toml at build time)
const SERVER_VERSION: &str = env!("CARGO_PKG_VERSION");
/// Minimum agent version the server fully supports
const MIN_AGENT_VERSION: &str = "0.1.0";

/// Semantic version comparison: returns true if `a < b`.
/// Handles multi-digit segments correctly (e.g. "0.9.0" < "0.10.0" → true).
fn semver_less_than(a: &str, b: &str) -> bool {
    let parse = |s: &str| -> Vec<u32> {
        s.split('.')
            .filter_map(|seg| seg.parse::<u32>().ok())
            .collect()
    };
    let va = parse(a);
    let vb = parse(b);
    va < vb // Vec<u32> lexicographic comparison on numeric segments
}
/// HTTP request timeout for each agent scrape (seconds)
const SCRAPE_TIMEOUT_SECS: u64 = 5;
/// Cooldown to suppress repeated UP/DOWN alert flapping (seconds)
const FLAP_COOLDOWN_SECS: u64 = 60;
/// Maximum backoff multiplier (2^4 = 16x base interval → 160s at 10s interval)
const MAX_BACKOFF_POWER: u32 = 4;
/// Cap the decoded agent response body before deserialization.
const MAX_AGENT_PAYLOAD_BYTES: usize = 10 * 1024 * 1024;
/// `/system-info` is five short fields; anything larger is not a real agent.
const MAX_SYSTEM_INFO_BYTES: usize = 64 * 1024;
/// Upper bound for any agent-supplied string that is stored or broadcast.
const MAX_AGENT_STRING_BYTES: usize = 256;

/// Read a response body, failing as soon as it exceeds `max_bytes`. The cap
/// applies to the decompressed stream, so a gzip bomb cannot be buffered.
async fn read_capped_body(
    mut resp: reqwest::Response,
    max_bytes: usize,
) -> Result<Vec<u8>, String> {
    let mut bytes = Vec::new();
    loop {
        match resp.chunk().await {
            Ok(Some(chunk)) => {
                if bytes.len().saturating_add(chunk.len()) > max_bytes {
                    return Err(format!("Payload too large: exceeds {max_bytes} bytes"));
                }
                bytes.extend_from_slice(&chunk);
            }
            Ok(None) => return Ok(bytes),
            Err(e) => return Err(format!("Failed to read response body chunk: {e}")),
        }
    }
}

/// Truncate to at most `max_bytes`, on a char boundary.
fn clamp_string(value: &mut String, max_bytes: usize) {
    if value.len() <= max_bytes {
        return;
    }
    let mut end = max_bytes;
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    value.truncate(end);
}

/// Decide whether an agent response may be trusted.
///
/// A valid signature always passes and reports whether this is the first one
/// seen for the host (so the caller can pin it). An invalid signature always
/// fails. A missing signature passes only for hosts that have never signed —
/// agents older than response signing — and fails once the host is pinned.
fn check_response_signature(
    auth: &AgentAuth,
    token: &str,
    body: &[u8],
    header: Option<&str>,
) -> Result<bool, String> {
    use crate::services::auth::ResponseSignature;
    match crate::services::auth::verify_agent_response_signature(&auth.secret, token, body, header)
    {
        ResponseSignature::Valid => Ok(!auth.signs_responses),
        ResponseSignature::Invalid => Err("Response signature mismatch".to_string()),
        ResponseSignature::Missing if auth.signs_responses => {
            Err("Unsigned response from an agent that previously signed its responses".to_string())
        }
        ResponseSignature::Missing => Ok(false),
    }
}

/// Persist and cache the "this agent signs its responses" pin.
async fn pin_response_signing(state: &Arc<AppState>, target: &str) {
    if let Err(e) = hosts_repo::mark_agent_signs_responses(&state.db_pool, target).await {
        tracing::warn!(target = %target, err = %e, "⚠️ [Scraper] Failed to persist response-signing pin");
        return;
    }
    hosts_snapshot::apply_response_signing(&state.hosts_snapshot, target);
    tracing::info!(target = %target, "🔏 [Scraper] Agent signs its responses — unsigned responses are now rejected");
}

/// Credentials for talking to one host.
#[derive(Clone)]
struct AgentAuth {
    /// Per-agent secret, or `JWT_SECRET` for legacy hosts.
    secret: String,
    /// Whether the host is pinned to signed responses.
    signs_responses: bool,
}

/// One outbound request to an agent: the URL and the token minted for it.
struct AgentRequest {
    url: reqwest::Url,
    token: String,
}

impl AgentRequest {
    /// Build the request for `path_and_query` on `host_key`. The token is
    /// bound to the request target exactly as it will appear on the wire
    /// (after URL normalisation), which is what the agent hashes on its side.
    fn new(auth: &AgentAuth, host_key: &str, path_and_query: &str) -> Result<Self, String> {
        let url = reqwest::Url::parse(&format!("http://{host_key}{path_and_query}"))
            .map_err(|e| format!("Invalid agent URL: {e}"))?;
        let target = &url[url::Position::BeforePath..url::Position::AfterQuery];
        let token =
            crate::services::auth::generate_agent_jwt_with_secret(&auth.secret, host_key, target)
                .map_err(|e| format!("Failed to mint agent JWT: {e}"))?;
        Ok(Self { url, token })
    }
}

/// Path and query for a metrics scrape of the given ports and containers.
fn metrics_request_target(ports: &[u16], containers: &[String]) -> String {
    let mut params = Vec::new();
    if !ports.is_empty() {
        let ports = ports.iter().map(u16::to_string).collect::<Vec<_>>();
        params.push(format!("ports={}", ports.join(",")));
    }
    if !containers.is_empty() {
        params.push(format!("containers={}", containers.join(",")));
    }
    if params.is_empty() {
        "/metrics".to_string()
    } else {
        format!("/metrics?{}", params.join("&"))
    }
}

/// Credentials for the `/system-info` fetch that follows a scrape. A valid
/// signature on that scrape means the host is pinned as of now, even though
/// the snapshot this context was built from still says otherwise.
fn auth_after_scrape(auth: &AgentAuth, response_signed: bool) -> AgentAuth {
    AgentAuth {
        secret: auth.secret.clone(),
        signs_responses: auth.signs_responses || response_signed,
    }
}

/// Per-host failure tracking for exponential backoff
struct HostBackoff {
    consecutive_failures: u32,
    last_attempt: Instant,
}

/// Starts the pull-model scraper as a background task.
/// Reads target list from the `hosts` DB table and alert rules from `alert_configs` each cycle.
pub fn start_scraper(state: Arc<AppState>) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let client = match Client::builder()
            .timeout(Duration::from_secs(SCRAPE_TIMEOUT_SECS))
            // An agent (or anything answering on its address) must not be
            // able to bounce the hub to another URL, e.g. cloud metadata or
            // a loopback admin port.
            .redirect(reqwest::redirect::Policy::none())
            // Agents close connections idle for 15 s; let go of ours first so
            // a scrape never races the agent's close.
            .pool_idle_timeout(Duration::from_secs(12))
            .build()
        {
            Ok(c) => c,
            Err(e) => {
                tracing::error!(err = ?e, "❌ [Scraper] Failed to build HTTP client");
                return;
            }
        };

        let default_interval_secs = state.scrape_interval_secs;
        tracing::info!(
            default_interval = default_interval_secs,
            scheduler_resolution = 1,
            "🔍 [Scraper] Started (DB-driven)"
        );

        let mut interval = tokio::time::interval(Duration::from_secs(1));
        let _ = interval.tick().await; // skip first immediate tick

        let mut backoff_map: HashMap<String, HostBackoff> = HashMap::new();
        let mut last_scrape_attempt: HashMap<String, Instant> = HashMap::new();

        loop {
            interval.tick().await;
            scrape_all(&client, &state, &mut backoff_map, &mut last_scrape_attempt).await;
        }
    })
}

async fn scrape_all(
    client: &Client,
    state: &Arc<AppState>,
    backoff_map: &mut HashMap<String, HostBackoff>,
    last_scrape_attempt: &mut HashMap<String, Instant>,
) {
    // Read hosts + alert_configs from the in-memory snapshot instead of
    // hitting the DB every 10 s. The snapshot is refreshed synchronously
    // on every mutation handler (create/update/delete host, upsert/delete
    // alert config) and also by a 60 s background tick as a backstop.
    // Top-10 review finding #10.
    // Hot read on every cycle. Use the reseed-aware variant so a poisoned
    // RwLock (writer panic) is followed by an immediate background DB reseed
    // instead of serving the recovered (potentially stale) snapshot for up
    // to 60 s until the periodic ticker fires.
    let snapshot = hosts_snapshot::load_or_reseed(&state.db_pool, &state.hosts_snapshot);

    // Pre-register any newly added hosts in last_known_status
    state.pre_populate_status(&snapshot.hosts);

    last_scrape_attempt
        .retain(|host_key, _| snapshot.hosts.iter().any(|host| host.host_key == *host_key));
    backoff_map.retain(|host_key, _| snapshot.hosts.iter().any(|host| host.host_key == *host_key));

    let mut due_contexts = Vec::new();
    for host in &snapshot.hosts {
        let scrape_interval_secs = u64::try_from(host.scrape_interval_secs)
            .ok()
            .filter(|secs| *secs > 0)
            .unwrap_or(state.scrape_interval_secs);
        let host_interval = Duration::from_secs(scrape_interval_secs);

        // Slack on the "is host due?" check to absorb 1-Hz scheduler
        // jitter. Without it the cadence drifts by exactly one tick
        // (1 s) per scrape: the outer loop tick fires at T = 0, 1, 2, …
        // but `last_attempt` is stamped via `Instant::now()` *after*
        // tick fire (T = 0 + ε). At T = host_interval the elapsed comes
        // out as `host_interval − ε`, which is `< host_interval`, so the
        // comparison treats the host as "not yet due" and we skip until
        // T = host_interval + 1. That turned a configured 10 s scrape
        // into an effective ~11 s SSE cadence. 500 ms is generous
        // enough for any realistic clock jitter while still firing
        // strictly before the next interval boundary.
        const SCHEDULER_SLACK: Duration = Duration::from_millis(500);
        if let Some(last_attempt) = last_scrape_attempt.get(&host.host_key)
            && last_attempt.elapsed() + SCHEDULER_SLACK < host_interval
        {
            continue;
        }

        if let Some(backoff) = backoff_map.get(&host.host_key)
            && backoff.consecutive_failures > 0
        {
            let power = backoff.consecutive_failures.min(MAX_BACKOFF_POWER);
            let wait = host_interval * 2u32.pow(power);
            // Same `SCHEDULER_SLACK` rationale as above — the backoff
            // wait is wall-clock derived but checked at 1-Hz tick
            // resolution, so without slack a `wait = 10 s` would also
            // drift by one tick per cycle.
            if backoff.last_attempt.elapsed() + SCHEDULER_SLACK < wait {
                continue;
            }
        }

        // Hosts without a per-agent enrollment secret still authenticate
        // with the shared `JWT_SECRET`. Tokens are minted per request either
        // way (see `AgentRequest`), so one captured token is never valid
        // elsewhere as-is and every response can be tied to its request.
        let Some(secret) = host
            .agent_auth_secret
            .as_deref()
            .or_else(|| crate::services::auth::legacy_agent_secret())
        else {
            tracing::error!(host_key = %host.host_key, "❌ [Scraper] No agent secret available");
            continue;
        };
        let auth = AgentAuth {
            secret: secret.to_string(),
            signs_responses: host.agent_signs_responses,
        };

        last_scrape_attempt.insert(host.host_key.clone(), Instant::now());
        due_contexts.push(ScrapeContext {
            client: client.clone(),
            target: host.host_key.clone(),
            display_name: host.display_name.clone(),
            ports: host.ports.iter().map(|&p| p as u16).collect(),
            containers: host.containers.clone(),
            alert_config: alert_configs_repo::resolve_alert_config(
                &host.host_key,
                host.load_threshold,
                &snapshot.alert_map,
            ),
            state: state.clone(),
            auth,
            system_info_updated_at: host.system_info_updated_at,
            scrape_interval_secs,
        });
    }

    let results = stream::iter(due_contexts.into_iter().map(|ctx| async move {
        let target = ctx.target.clone();
        let display_name = ctx.display_name.clone();
        let result = scrape_one(&ctx).await;
        (target, display_name, result)
    }))
    .buffer_unordered(10)
    .collect::<Vec<_>>()
    .await;

    // ── Collect persist data and update backoff tracking ──
    let mut success_count = 0;
    let mut fail_count = 0;
    let mut online_batch: Vec<(String, AgentMetrics)> = Vec::new();
    let mut offline_batch: Vec<(String, String)> = Vec::new();

    for (url, display_name, outcome) in results {
        match outcome {
            ScrapeOutcome::Online(metrics) => {
                success_count += 1;
                backoff_map.remove(&url);
                online_batch.push((url, *metrics));
            }
            ScrapeOutcome::Offline => {
                fail_count += 1;
                let entry = backoff_map.entry(url.clone()).or_insert(HostBackoff {
                    consecutive_failures: 0,
                    last_attempt: Instant::now(),
                });
                entry.consecutive_failures += 1;
                entry.last_attempt = Instant::now();
                offline_batch.push((url, display_name));
            }
            ScrapeOutcome::Rejected(reason) => {
                tracing::warn!(url = %url, reason = %reason, "🔴 [Scraper] Response rejected — treating host as down");
                fail_count += 1;
                let entry = backoff_map.entry(url.clone()).or_insert(HostBackoff {
                    consecutive_failures: 0,
                    last_attempt: Instant::now(),
                });
                entry.consecutive_failures += 1;
                entry.last_attempt = Instant::now();
                offline_batch.push((url, display_name));
            }
            ScrapeOutcome::Failed(e) => {
                tracing::warn!(url = %url, error = %e, "🔴 [Scraper] Target failed (no DB insert)");
                fail_count += 1;
                let entry = backoff_map.entry(url).or_insert(HostBackoff {
                    consecutive_failures: 0,
                    last_attempt: Instant::now(),
                });
                entry.consecutive_failures += 1;
                entry.last_attempt = Instant::now();
            }
        }
    }

    // ── Batch DB persistence (single transaction per scrape cycle) ──
    if !online_batch.is_empty() || !offline_batch.is_empty() {
        let online_refs: Vec<(&str, &AgentMetrics)> = online_batch
            .iter()
            .map(|(hk, m)| (hk.as_str(), m))
            .collect();
        let offline_refs: Vec<(&str, &str)> = offline_batch
            .iter()
            .map(|(hk, dn)| (hk.as_str(), dn.as_str()))
            .collect();

        let persist_result: Result<(), sqlx::Error> = async {
            let mut tx = state.db_pool.begin().await?;
            metrics_repo::insert_metrics_batch(&mut *tx, &online_refs).await?;
            metrics_repo::insert_offline_metrics_batch(&mut *tx, &offline_refs).await?;
            tx.commit().await?;
            Ok(())
        }
        .await;

        if let Err(e) = persist_result {
            tracing::error!(
                err = ?e,
                online_count = online_batch.len(),
                offline_count = offline_batch.len(),
                "⚠️ [Scraper] Batch metrics transaction failed"
            );
        }
    }

    if fail_count > 0 {
        tracing::info!(
            success = success_count,
            fail = fail_count,
            "📊 [Scraper Summary]"
        );
    }
}

/// Per-host scrape context — groups parameters that flow through
/// `scrape_one` → `handle_success` without long parameter lists.
struct ScrapeContext {
    client: Client,
    target: String,
    display_name: String,
    ports: Vec<u16>,
    containers: Vec<String>,
    alert_config: AlertConfig,
    state: Arc<AppState>,
    auth: AgentAuth,
    system_info_updated_at: Option<DateTime<Utc>>,
    scrape_interval_secs: u64,
}

async fn scrape_one(ctx: &ScrapeContext) -> ScrapeOutcome {
    let request = match AgentRequest::new(
        &ctx.auth,
        &ctx.target,
        &metrics_request_target(&ctx.ports, &ctx.containers),
    ) {
        Ok(request) => request,
        Err(e) => return ScrapeOutcome::Failed(e),
    };

    match ctx
        .client
        .get(request.url.clone())
        .header("Authorization", format!("Bearer {}", request.token))
        .send()
        .await
    {
        Ok(resp) if resp.status().is_success() => {
            // Read the agent's advertised wire version before the chunk loop
            // consumes the response. `None` means a pre-versioning agent that
            // sent no header → the decoder falls back to the legacy chain.
            let wire_version = resp
                .headers()
                .get(WIRE_VERSION_HEADER)
                .and_then(|v| v.to_str().ok())
                .and_then(|s| s.trim().parse::<u8>().ok());
            let signature = resp
                .headers()
                .get(RESPONSE_SIGNATURE_HEADER)
                .and_then(|v| v.to_str().ok())
                .map(str::to_owned);
            let bytes = match read_capped_body(resp, MAX_AGENT_PAYLOAD_BYTES).await {
                Ok(bytes) => bytes,
                Err(e) => return ScrapeOutcome::Failed(e),
            };
            match check_response_signature(&ctx.auth, &request.token, &bytes, signature.as_deref())
            {
                Ok(true) => pin_response_signing(&ctx.state, &ctx.target).await,
                Ok(false) => {}
                Err(reason) => {
                    handle_down(
                        &ctx.target,
                        &ctx.display_name,
                        ctx.scrape_interval_secs,
                        &ctx.state,
                        &format!("response rejected ({reason})"),
                    )
                    .await;
                    return ScrapeOutcome::Rejected(reason);
                }
            }
            // A signature header that got this far verified.
            let response_signed = signature.is_some();

            match deserialize_agent_metrics_versioned(&bytes, wire_version) {
                Ok(mut metrics) => {
                    // Defense-in-depth: cap untrusted Vec fields to sane maximums
                    metrics.cpu_cores.truncate(1024);
                    metrics.network_interfaces.truncate(256);
                    metrics.docker_stats.truncate(512);
                    metrics.docker_containers.truncate(512);
                    metrics.ports.truncate(256);
                    metrics.system.processes.truncate(100);
                    metrics.system.disks.truncate(256);
                    metrics.system.temperatures.truncate(256);
                    metrics.system.gpus.truncate(64);

                    sanitize_metrics(&mut metrics);

                    if metrics.agent_version.is_empty() {
                        tracing::warn!(target = %ctx.target, "⚠️ [Scraper] Agent has no version field — consider upgrading");
                    } else if semver_less_than(&metrics.agent_version, MIN_AGENT_VERSION) {
                        tracing::warn!(
                            target = %ctx.target,
                            agent_version = %metrics.agent_version,
                            min_version = MIN_AGENT_VERSION,
                            server_version = SERVER_VERSION,
                            "⚠️ [Scraper] Agent version below minimum — consider upgrading"
                        );
                    }
                    handle_success(metrics, ctx, response_signed).await
                }
                Err(e) => ScrapeOutcome::Failed(format!("Bincode deserialization error: {}", e)),
            }
        }
        Ok(_resp) => {
            handle_down(
                &ctx.target,
                &ctx.display_name,
                ctx.scrape_interval_secs,
                &ctx.state,
                "no response",
            )
            .await;
            ScrapeOutcome::Offline
        }
        Err(_e) => {
            handle_down(
                &ctx.target,
                &ctx.display_name,
                ctx.scrape_interval_secs,
                &ctx.state,
                "no response",
            )
            .await;
            ScrapeOutcome::Offline
        }
    }
}

fn sanitize_metrics(metrics: &mut AgentMetrics) {
    metrics.system.cpu_usage_percent =
        metrics_service::sanitize_f32(metrics.system.cpu_usage_percent);
    metrics.system.memory_usage_percent =
        metrics_service::sanitize_f32(metrics.system.memory_usage_percent);

    metrics.load_average.one_min = metrics_service::sanitize_f64(metrics.load_average.one_min);
    metrics.load_average.five_min = metrics_service::sanitize_f64(metrics.load_average.five_min);
    metrics.load_average.fifteen_min =
        metrics_service::sanitize_f64(metrics.load_average.fifteen_min);

    metrics.network.rx_bytes_per_sec =
        metrics_service::sanitize_f64(metrics.network.rx_bytes_per_sec);
    metrics.network.tx_bytes_per_sec =
        metrics_service::sanitize_f64(metrics.network.tx_bytes_per_sec);

    for disk in &mut metrics.system.disks {
        disk.usage_percent = metrics_service::sanitize_f32(disk.usage_percent);
        disk.read_bytes_per_sec = metrics_service::sanitize_f64(disk.read_bytes_per_sec);
        disk.write_bytes_per_sec = metrics_service::sanitize_f64(disk.write_bytes_per_sec);
        disk.total_gb = metrics_service::sanitize_f64(disk.total_gb);
        disk.available_gb = metrics_service::sanitize_f64(disk.available_gb);
    }

    for core in &mut metrics.cpu_cores {
        *core = metrics_service::sanitize_f32(*core);
    }

    for temperature in &mut metrics.system.temperatures {
        temperature.temperature_c = metrics_service::sanitize_f32(temperature.temperature_c);
    }

    for gpu in &mut metrics.system.gpus {
        if let Some(power_watts) = gpu.power_watts {
            gpu.power_watts = Some(metrics_service::sanitize_f32(power_watts));
        }
        if let Some(power_limit_watts) = gpu.power_limit_watts {
            gpu.power_limit_watts = Some(metrics_service::sanitize_f32(power_limit_watts));
        }
    }

    for stats in &mut metrics.docker_stats {
        stats.cpu_percent = metrics_service::sanitize_f32(stats.cpu_percent);
    }

    for process in &mut metrics.system.processes {
        process.cpu_usage = metrics_service::sanitize_f32(process.cpu_usage);
    }

    clamp_agent_strings(metrics);
}

/// Bound every agent-supplied string. These are persisted on each scrape,
/// broadcast over SSE and interpolated into alert messages, so an agent must
/// not be able to grow them without limit.
fn clamp_agent_strings(metrics: &mut AgentMetrics) {
    const MAX: usize = MAX_AGENT_STRING_BYTES;
    clamp_string(&mut metrics.hostname, MAX);
    clamp_string(&mut metrics.timestamp, 64);
    clamp_string(&mut metrics.agent_version, 64);
    for disk in &mut metrics.system.disks {
        clamp_string(&mut disk.name, MAX);
        clamp_string(&mut disk.mount_point, MAX);
    }
    for process in &mut metrics.system.processes {
        clamp_string(&mut process.name, MAX);
    }
    for temperature in &mut metrics.system.temperatures {
        clamp_string(&mut temperature.label, MAX);
    }
    for gpu in &mut metrics.system.gpus {
        clamp_string(&mut gpu.name, MAX);
    }
    for iface in &mut metrics.network_interfaces {
        clamp_string(&mut iface.name, MAX);
    }
    for stats in &mut metrics.docker_stats {
        clamp_string(&mut stats.container_name, MAX);
    }
    for container in &mut metrics.docker_containers {
        clamp_string(&mut container.container_name, MAX);
        clamp_string(&mut container.image, MAX);
        clamp_string(&mut container.state, 64);
        clamp_string(&mut container.status, MAX);
        for label in [
            &mut container.compose_project,
            &mut container.compose_service,
        ]
        .into_iter()
        .flatten()
        {
            clamp_string(label, MAX);
        }
    }
}

// ──────────────────────────────────────────────
// Success path
// ──────────────────────────────────────────────

/// System info refresh interval: 24 hours
const SYSTEM_INFO_REFRESH_SECS: i64 = 24 * 3600;

async fn handle_success(
    mut metrics: AgentMetrics,
    ctx: &ScrapeContext,
    response_signed: bool,
) -> ScrapeOutcome {
    match metrics_service::process_metrics(
        &metrics,
        &ctx.target,
        &ctx.state,
        &ctx.alert_config,
        ctx.scrape_interval_secs,
    )
    .await
    {
        Ok(result) => {
            tracing::info!(target = %ctx.target, "✅ [Scraper] {}", result.log_msg);

            metrics.network.rx_bytes_per_sec = result.metrics_payload.network_rate.rx_bytes_per_sec;
            metrics.network.tx_bytes_per_sec = result.metrics_payload.network_rate.tx_bytes_per_sec;

            // Pre-serialize once at the producer; every subscriber gets a
            // cheap `Arc<str>` clone instead of paying for its own
            // `serde_json::to_string` (see `SseBroadcast` docs).
            let metrics_arc = Arc::new(result.metrics_payload);
            if let Some(ev) = SseBroadcast::metrics(&metrics_arc) {
                let _ = ctx.state.sse_tx.send(ev);
            }
            // Cache the latest metrics so the SSE handshake can replay the
            // live scalars (CPU/RAM/load/network) immediately — without this
            // a freshly-connected dashboard waits a full scrape cycle for the
            // first `metrics` broadcast. SAFETY: no .await while lock is held.
            if let Ok(mut lkm) = ctx.state.last_known_metrics.write() {
                lkm.insert(ctx.target.clone(), Arc::clone(&metrics_arc));
            }

            if let Some(status_payload) = result.status_payload {
                // The `last_known_status` cache still holds the structured
                // payload — `build_initial_events` (handshake / Lagged
                // recovery) and `handle_down` both mutate / re-serialize
                // it independently. Only the broadcast fan-out is
                // pre-serialized.
                let serialized = SseBroadcast::status(&status_payload);
                let arc = Arc::new(status_payload);
                // SAFETY: no .await while lock is held
                if let Ok(mut lks) = ctx.state.last_known_status.write() {
                    lks.insert(ctx.target.clone(), Arc::clone(&arc));
                }
                if let Some(ev) = serialized {
                    let _ = ctx.state.sse_tx.send(ev);
                }
            }
        }
        Err(e) => {
            tracing::error!(target = %ctx.target, err = ?e, "⚠️  [Scraper] process_metrics error");
            return ScrapeOutcome::Failed(format!("process_metrics error: {}", e));
        }
    }

    // Recovery (host back online) alert.
    //
    // Single write-lock acquisition: previously this path peeked under a
    // read lock to decide whether a transition was pending, then reopened
    // as writer when one was. That two-step pattern let another task slip
    // between the read and the write — which we then had to re-check —
    // and it doubled lock entries on the hot path for zero latency win:
    // the write guard itself is cheap, and the recovery branch is rare.
    // Taking write once up front eliminates the TOCTOU and one whole
    // `store` critical section per scrape cycle.
    let recovery_msg = {
        // SAFETY: no .await while lock is held
        let mut store = match ctx.state.store.write() {
            Ok(s) => s,
            Err(e) => {
                tracing::warn!(err = %e, "⚠️ [Scraper] Store write lock poisoned in recovery check");
                return ScrapeOutcome::Online(Box::new(metrics));
            }
        };
        match store.hosts.get_mut(ctx.target.as_str()) {
            Some(record) => {
                mark_recovery_if_cooldown_passed(record, &metrics.hostname, Instant::now())
            }
            None => None,
        }
    };
    let was_offline = recovery_msg.is_some();

    // Recovery alert: fan out to webhooks + alert_history write on a detached
    // task. `send_alert` can spend hundreds of ms on external HTTP, and the
    // caller is inside the scraper's `buffer_unordered(10)` stream — blocking
    // here steals a concurrency slot for the remainder of the cycle.
    if let Some(msg) = recovery_msg {
        let http = ctx.state.http_client.clone();
        let pool = ctx.state.db_pool.clone();
        let target_owned = ctx.target.clone();
        tokio::spawn(async move {
            alert_service::send_alert(&http, &pool, &msg).await;
            if let Err(e) = crate::repositories::alert_history_repo::insert_alert(
                &pool,
                &target_owned,
                "host_recovery",
                &msg,
            )
            .await
            {
                tracing::error!(err = ?e, "⚠️ [AlertHistory] Failed to log host_recovery");
            }
        });
    }

    // ── System info fetch (on reconnection or stale > 24h) ──
    let sys_info_stale = ctx.system_info_updated_at.is_none_or(|t| {
        Utc::now().signed_duration_since(t).num_seconds() > SYSTEM_INFO_REFRESH_SECS
    });

    if was_offline || sys_info_stale {
        let target_owned = ctx.target.clone();
        let client = ctx.client.clone();
        let auth = auth_after_scrape(&ctx.auth, response_signed);
        let state = Arc::clone(&ctx.state);
        tokio::spawn(async move {
            fetch_and_store_system_info(&client, &target_owned, &auth, &state).await;
        });
    }

    ScrapeOutcome::Online(Box::new(metrics))
}

fn mark_recovery_if_cooldown_passed(
    record: &mut HostRecord,
    hostname: &str,
    now: Instant,
) -> Option<String> {
    if !record.alert_state.offline_alerted {
        return None;
    }

    let cooldown_passed = record
        .alert_state
        .last_recovery_alert
        .is_none_or(|t| now.duration_since(t) > Duration::from_secs(FLAP_COOLDOWN_SECS));

    if !cooldown_passed {
        return None;
    }

    record.alert_state.offline_alerted = false;
    record.alert_state.last_recovery_alert = Some(now);
    Some(format!(
        "✅ **[Host Recovery]** `{hostname}` — agent is back online."
    ))
}

/// Fetch system info from the agent and persist to DB + in-memory status.
async fn fetch_and_store_system_info(
    client: &Client,
    target: &str,
    auth: &AgentAuth,
    state: &Arc<AppState>,
) {
    let request = match AgentRequest::new(auth, target, "/system-info") {
        Ok(request) => request,
        Err(e) => {
            tracing::warn!(target = %target, err = %e, "⚠️ [SystemInfo] Could not build request");
            return;
        }
    };
    let resp = match client
        .get(request.url.clone())
        .header("Authorization", format!("Bearer {}", request.token))
        .timeout(Duration::from_secs(SCRAPE_TIMEOUT_SECS))
        .send()
        .await
    {
        Ok(r) if r.status().is_success() => r,
        Ok(r) => {
            tracing::warn!(target = %target, status = %r.status(), "⚠️ [SystemInfo] Non-success response");
            return;
        }
        Err(e) => {
            tracing::warn!(target = %target, err = %e, "⚠️ [SystemInfo] Request failed (agent may not support /system-info)");
            return;
        }
    };

    let signature = resp
        .headers()
        .get(RESPONSE_SIGNATURE_HEADER)
        .and_then(|v| v.to_str().ok())
        .map(str::to_owned);
    let bytes = match read_capped_body(resp, MAX_SYSTEM_INFO_BYTES).await {
        Ok(bytes) => bytes,
        Err(e) => {
            tracing::warn!(target = %target, err = %e, "⚠️ [SystemInfo] Response rejected");
            return;
        }
    };
    if let Err(e) = check_response_signature(auth, &request.token, &bytes, signature.as_deref()) {
        tracing::warn!(target = %target, err = %e, "⚠️ [SystemInfo] Response rejected");
        return;
    }
    let mut info: SystemInfoResponse = match serde_json::from_slice(&bytes) {
        Ok(i) => i,
        Err(e) => {
            tracing::warn!(target = %target, err = %e, "⚠️ [SystemInfo] JSON parse failed");
            return;
        }
    };
    clamp_string(&mut info.os, MAX_AGENT_STRING_BYTES);
    clamp_string(&mut info.cpu_model, MAX_AGENT_STRING_BYTES);
    clamp_string(&mut info.ip_address, 64);

    // Persist to DB
    if let Err(e) = hosts_repo::update_system_info(
        &state.db_pool,
        target,
        &info.os,
        &info.cpu_model,
        info.memory_total_mb as i64,
        info.boot_time as i64,
        &info.ip_address,
    )
    .await
    {
        tracing::warn!(target = %target, err = %e, "⚠️ [SystemInfo] DB update failed");
        return;
    }

    // Update in-memory SSE status
    if let Ok(mut lks) = state.last_known_status.write()
        && let Some(arc) = lks.get_mut(target)
    {
        let status = Arc::make_mut(arc);
        status.os_info = Some(info.os.clone());
        status.cpu_model = Some(info.cpu_model.clone());
        status.memory_total_mb = Some(info.memory_total_mb as i64);
        status.boot_time = Some(info.boot_time as i64);
        status.ip_address = Some(info.ip_address.clone());
    }

    hosts_snapshot::apply_system_info(&state.hosts_snapshot, target, &info);

    tracing::info!(target = %target, "✅ [SystemInfo] Updated");
}

// ──────────────────────────────────────────────
// Failure path
// ──────────────────────────────────────────────

async fn handle_down(
    target: &str,
    display_name: &str,
    scrape_interval_secs: u64,
    state: &Arc<AppState>,
    reason: &str,
) {
    let now = Instant::now();
    let host_key = target.to_string();

    // DB persistence is deferred — the caller (scrape_all) collects offline hosts
    // and commits all metrics writes in a single transaction per scrape cycle.

    // ── Phase 1: store write lock (lightweight — alert state only) ──
    let (alert_msg, hostname, should_broadcast) = {
        let mut store = match state.store.write() {
            Ok(s) => s,
            Err(e) => {
                tracing::warn!(err = %e, "⚠️ [Scraper] Store write lock poisoned in handle_down");
                return;
            }
        };

        let hostname = store
            .hosts
            .get(target)
            .map(|r| r.last_known_hostname.clone())
            .unwrap_or_else(|| display_name.to_string());

        let record = store
            .hosts
            .entry(target.to_string())
            .or_insert_with(|| HostRecord::new(hostname.clone()));

        // Throttle status broadcasts for offline hosts — same pattern as handle_success().
        // Without this, N offline hosts generate N unnecessary SSE events every scrape cycle.
        let periodic_elapsed = record
            .last_status_sent
            .is_none_or(|t| t.elapsed() >= Duration::from_secs(STATUS_PERIODIC_INTERVAL_SECS));
        if periodic_elapsed {
            record.last_status_sent = Some(now);
        }

        let alert = if record.alert_state.offline_alerted {
            None
        } else {
            let last_offline = record.alert_state.last_offline_alert;
            let cooldown_passed =
                last_offline.is_none_or(|t| t.elapsed() > Duration::from_secs(FLAP_COOLDOWN_SECS));

            if cooldown_passed {
                record.alert_state.offline_alerted = true;
                record.alert_state.last_offline_alert = Some(now);
                Some(format!(
                    "🔴 **[Host Down]** `{}` (target: `{}`) — {}",
                    hostname, target, reason
                ))
            } else {
                None
            }
        };

        // Broadcast on first offline (alert fired) or periodic interval
        let should_broadcast = alert.is_some() || periodic_elapsed;

        (alert, hostname, should_broadcast)
        // ← store write lock released here
    };

    // ── Phase 2: last_known_status update + SSE broadcast (no store lock held) ──
    if should_broadcast {
        let server_ts = Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true);

        if let Ok(mut lks) = state.last_known_status.write() {
            let arc = lks.entry(host_key.clone()).or_insert_with(|| {
                Arc::new(HostStatusPayload {
                    host_key: host_key.clone(),
                    display_name: hostname.clone(),
                    scrape_interval_secs,
                    is_online: false,
                    last_seen: String::new(),
                    docker_containers: vec![],
                    ports: vec![],
                    disks: vec![],
                    processes: vec![],
                    temperatures: vec![],
                    gpus: vec![],
                    docker_stats: vec![],
                    os_info: None,
                    cpu_model: None,
                    memory_total_mb: None,
                    boot_time: None,
                    ip_address: None,
                })
            });
            // `Arc::make_mut` is cheap (no-op) while the Arc is uniquely owned —
            // the common case when no SSE subscriber is currently holding the
            // previous broadcast. It clones only when a slow consumer still
            // references the prior payload, which is exactly when we need to
            // avoid mutating a value other tasks are reading.
            let status = Arc::make_mut(arc);
            status.scrape_interval_secs = scrape_interval_secs;
            status.is_online = false;
            status.last_seen = server_ts;
            status.processes = vec![];
            // Serialize once for the broadcast fan-out; the cache still
            // holds the structured `Arc<HostStatusPayload>` for handshake
            // re-snapshots.
            if let Some(ev) = SseBroadcast::status(status) {
                let _ = state.sse_tx.send(ev);
            }
        }
    }

    // ── Phase 3: alert delivery (async I/O, no locks held) ──
    // Fire-and-forget: same rationale as the recovery path — webhook latency
    // should not be charged against the scraper's concurrency budget.
    if let Some(msg) = alert_msg {
        let http = state.http_client.clone();
        let pool = state.db_pool.clone();
        let hk = host_key.clone();
        tokio::spawn(async move {
            alert_service::send_alert(&http, &pool, &msg).await;
            if let Err(e) =
                crate::repositories::alert_history_repo::insert_alert(&pool, &hk, "host_down", &msg)
                    .await
            {
                tracing::error!(err = ?e, "⚠️ [AlertHistory] Failed to log host_down");
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TOKEN: &str = "scrape-token";

    fn auth(signs_responses: bool) -> AgentAuth {
        AgentAuth {
            secret: "agent-secret-agent-secret-agent-secret".to_string(),
            signs_responses,
        }
    }

    fn signature(auth: &AgentAuth, body: &[u8]) -> String {
        use hmac::{Hmac, Mac};
        let mut mac = Hmac::<sha2::Sha256>::new_from_slice(auth.secret.as_bytes()).unwrap();
        mac.update(TOKEN.as_bytes());
        mac.update(&[0]);
        mac.update(body);
        let hex: String = mac
            .finalize()
            .into_bytes()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        format!("v1={hex}")
    }

    #[test]
    fn unsigned_response_is_accepted_only_before_the_host_is_pinned() {
        assert_eq!(
            check_response_signature(&auth(false), TOKEN, b"body", None),
            Ok(false)
        );
        assert!(check_response_signature(&auth(true), TOKEN, b"body", None).is_err());
    }

    #[test]
    fn valid_signature_pins_once_and_tampering_is_always_rejected() {
        let unpinned = auth(false);
        let sig = signature(&unpinned, b"body");
        // First valid signature asks the caller to pin; later ones do not.
        assert_eq!(
            check_response_signature(&unpinned, TOKEN, b"body", Some(&sig)),
            Ok(true)
        );
        assert_eq!(
            check_response_signature(&auth(true), TOKEN, b"body", Some(&sig)),
            Ok(false)
        );
        // A bad signature fails even for a host that never signed before.
        assert!(check_response_signature(&unpinned, TOKEN, b"tampered", Some(&sig)).is_err());
    }

    // ── Regression tests against a fake agent ───────────────────────

    /// Serve `app` on an ephemeral loopback port and return its `host:port`.
    async fn spawn_fake_agent(app: axum::Router) -> String {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        addr.to_string()
    }

    fn context(state: &Arc<AppState>, target: &str, signs_responses: bool) -> ScrapeContext {
        ScrapeContext {
            client: Client::new(),
            target: target.to_string(),
            display_name: "box".to_string(),
            ports: vec![80],
            containers: vec![],
            alert_config: alert_configs_repo::resolve_alert_config(target, 4.0, &HashMap::new()),
            state: Arc::clone(state),
            auth: auth(signs_responses),
            system_info_updated_at: Some(Utc::now()),
            scrape_interval_secs: 10,
        }
    }

    fn host_is_marked_down(state: &Arc<AppState>, target: &str) -> bool {
        let alerted = state
            .store
            .read()
            .unwrap()
            .hosts
            .get(target)
            .is_some_and(|record| record.alert_state.offline_alerted);
        let offline = state
            .last_known_status
            .read()
            .unwrap()
            .get(target)
            .is_some_and(|status| !status.is_online);
        alerted && offline
    }

    #[tokio::test]
    async fn response_with_bad_signature_marks_the_host_down() {
        use axum::routing::get;
        let target = spawn_fake_agent(axum::Router::new().route(
            "/metrics",
            get(|| async {
                (
                    [(RESPONSE_SIGNATURE_HEADER, format!("v1={}", "00".repeat(32)))],
                    "forged",
                )
            }),
        ))
        .await;
        let state = AppState::for_tests().await;

        let outcome = scrape_one(&context(&state, &target, false)).await;

        // Before: `Failed`, which only logged — no offline record, no alert.
        assert!(matches!(outcome, ScrapeOutcome::Rejected(_)));
        assert!(host_is_marked_down(&state, &target));
    }

    #[tokio::test]
    async fn unsigned_response_from_a_pinned_host_marks_the_host_down() {
        use axum::routing::get;
        let target =
            spawn_fake_agent(axum::Router::new().route("/metrics", get(|| async { "unsigned" })))
                .await;
        let state = AppState::for_tests().await;

        let outcome = scrape_one(&context(&state, &target, true)).await;

        assert!(matches!(outcome, ScrapeOutcome::Rejected(_)));
        assert!(host_is_marked_down(&state, &target));
    }

    const SYSTEM_INFO_JSON: &str = r#"{"os":"FakeOS","cpu_model":"Fake CPU","memory_total_mb":1,"boot_time":1,"ip_address":"192.0.2.1"}"#;

    async fn stored_os_info(state: &Arc<AppState>, target: &str) -> Option<String> {
        hosts_repo::get_host(&state.db_pool, target)
            .await
            .unwrap()
            .unwrap()
            .os_info
    }

    async fn register_host(state: &Arc<AppState>, target: &str) {
        hosts_repo::create_host(
            &state.db_pool,
            &hosts_repo::CreateHostRequest {
                host_key: target.to_string(),
                display_name: "box".to_string(),
                scrape_interval_secs: 10,
                load_threshold: 4.0,
                ports: vec![],
                containers: vec![],
            },
        )
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn system_info_after_the_pinning_scrape_must_be_signed() {
        use axum::routing::get;
        let target = spawn_fake_agent(
            axum::Router::new().route("/system-info", get(|| async { SYSTEM_INFO_JSON })),
        )
        .await;
        let state = AppState::for_tests().await;
        register_host(&state, &target).await;

        // The scrape context was built before the host was pinned, but its
        // response carried a valid signature: the follow-up fetch must
        // already enforce signatures.
        let pinned_now = auth_after_scrape(&auth(false), true);
        assert!(pinned_now.signs_responses);
        fetch_and_store_system_info(&Client::new(), &target, &pinned_now, &state).await;
        assert_eq!(stored_os_info(&state, &target).await, None);

        // Control: a host that has never signed still gets its info stored,
        // so the assertion above is about the signature and nothing else.
        let never_signed = auth_after_scrape(&auth(false), false);
        assert!(!never_signed.signs_responses);
        fetch_and_store_system_info(&Client::new(), &target, &never_signed, &state).await;
        assert_eq!(
            stored_os_info(&state, &target).await.as_deref(),
            Some("FakeOS")
        );
    }

    #[test]
    fn metrics_request_target_lists_only_what_is_monitored() {
        assert_eq!(metrics_request_target(&[], &[]), "/metrics");
        assert_eq!(
            metrics_request_target(&[80, 443], &[]),
            "/metrics?ports=80,443"
        );
        assert_eq!(
            metrics_request_target(&[80], &["web".to_string(), "db".to_string()]),
            "/metrics?ports=80&containers=web,db"
        );
    }

    #[test]
    fn request_token_is_bound_to_the_target_as_sent_on_the_wire() {
        use crate::services::auth::{Claims, request_target_digest};
        use jsonwebtoken::{Algorithm, DecodingKey, Validation, decode};

        let auth = auth(false);
        let rq_of = |path_and_query: &str| {
            let request = AgentRequest::new(&auth, "10.0.0.1:9101", path_and_query).unwrap();
            let mut validation = Validation::new(Algorithm::HS256);
            validation.set_audience(&["agent"]);
            let claims = decode::<Claims>(
                &request.token,
                &DecodingKey::from_secret(auth.secret.as_bytes()),
                &validation,
            )
            .unwrap()
            .claims;
            (request.url, claims.rq)
        };

        let (url, rq) = rq_of("/metrics?ports=80,443");
        assert_eq!(url.as_str(), "http://10.0.0.1:9101/metrics?ports=80,443");
        assert_eq!(rq, request_target_digest("/metrics?ports=80,443"));

        // Changing the monitored ports in transit no longer matches the token.
        assert_ne!(rq, rq_of("/metrics?ports=80").1);

        // The digest covers the normalised form the agent actually receives.
        let (url, rq) = rq_of("/metrics?containers=my app");
        assert_eq!(url.query(), Some("containers=my%20app"));
        assert_eq!(rq, request_target_digest("/metrics?containers=my%20app"));
    }

    #[test]
    fn clamp_string_truncates_on_a_char_boundary() {
        let mut ascii = "a".repeat(300);
        clamp_string(&mut ascii, 256);
        assert_eq!(ascii.len(), 256);

        // 'é' is two bytes; cutting at an odd byte must back off, not panic.
        let mut multibyte = "é".repeat(10);
        clamp_string(&mut multibyte, 5);
        assert_eq!(multibyte, "éé");

        let mut short = "ok".to_string();
        clamp_string(&mut short, 256);
        assert_eq!(short, "ok");
    }

    #[test]
    fn recovery_cooldown_uses_last_recovery_alert() {
        let now = Instant::now();
        let mut record = HostRecord::new("test-host".to_string());
        record.alert_state.offline_alerted = true;
        record.alert_state.last_offline_alert = Some(now);

        let first = mark_recovery_if_cooldown_passed(&mut record, "test-host", now);
        assert!(first.is_some(), "first recovery after host down must send");
        assert!(!record.alert_state.offline_alerted);
        assert_eq!(record.alert_state.last_recovery_alert, Some(now));

        record.alert_state.offline_alerted = true;
        let duplicate = mark_recovery_if_cooldown_passed(
            &mut record,
            "test-host",
            now + Duration::from_secs(30),
        );
        assert!(
            duplicate.is_none(),
            "second recovery inside cooldown must be suppressed"
        );
        assert!(record.alert_state.offline_alerted);

        let after_cooldown = mark_recovery_if_cooldown_passed(
            &mut record,
            "test-host",
            now + Duration::from_secs(FLAP_COOLDOWN_SECS + 1),
        );
        assert!(
            after_cooldown.is_some(),
            "recovery after cooldown must send again"
        );
        assert!(!record.alert_state.offline_alerted);
    }
}
