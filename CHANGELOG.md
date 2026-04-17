# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/),
and this project adheres to [Semantic Versioning](https://semver.org/).

## [0.3.1] — 2026-04-17

Patch release fixing two defects introduced by the v0.3.0 bump that escaped the pre-release checks. No runtime behavior changes; a fresh install that previously failed (`npm ci`, `cargo test`) now succeeds.

### Fixed

- **`npm ci` fails with `EUSAGE`** after v0.3.0 — the v0.3.0 version-bump regex substituted `"version": "1.2.0"` globally inside `netsentinel-web/package-lock.json`, hitting five unrelated transitive dependencies whose `resolved` URLs still pointed to their real versions (`@emnapi/wasi-threads`, `gopd`, `has-proto`, `run-parallel` at `1.2.0`; `safe-array-concat` at `1.1.3`). The lock file's own self-consistency check failed. `npm install` papered over it locally by re-deriving the tree; `npm ci` (used by CI and fresh clones) rejected the lock outright. Restored each entry's `version` field from the version encoded in its `resolved` URL — no dependency version was actually changed.
- **`cargo test` fails with `E0308`** in `alert_configs_handler.rs` after the `MetricType` / `ChannelType` enum migration (v0.3.0 commit `4242bd6`). The test helper `make_request()` was still constructing `UpsertAlertRequest { metric_type: String, ... }` via `.to_string()`, while the production struct had moved to `metric_type: MetricType`. The failure was masked by incremental-build caches during `cargo check` and `cargo clippy --all-targets` — only a fresh `cargo test` (which links a new test binary) surfaced the type error. Test helper now takes `MetricType`; obsolete `test_invalid_metric_type` removed since the invalid branch is no longer expressible in Rust once the enum is closed (serde rejects unknown variants before the handler runs).

### Changed

- Version bumped to `0.3.1` across `netsentinel-server/Cargo.toml`, `netsentinel-agent/Cargo.toml`, `netsentinel-web/package.json`, and the two lock file root entries. Cargo.lock files refreshed accordingly.

### Notes for downstream

- If you successfully installed v0.3.0 via `npm install` (no error shown), you are already running on the exact same tarballs v0.3.1 locks — the fix is purely metadata. A fresh `npm ci` on v0.3.1 installs the identical node_modules tree.
- No DB migrations, no config changes, no API changes. Drop-in upgrade.

## [0.3.0] — 2026-04-17

Second release under the **NetSentinel** name (post-rename from `network-monitor`, baseline v0.2.0 ≡ v1.2.0). Focus: **expanded metrics surface, real-time SSE richness, M3-aligned UI**, and a broad **security + code-quality sweep**.

### ⚠️ Compatibility note

- Agent ↔ Server bincode schema gains four new fields (`cpu_cores`, `network_interfaces`, `docker_stats`, disk I/O rates). New fields carry `#[serde(default)]`, so **older agents keep working** (graceful degradation — new UI elements render empty until the agent is upgraded). Upgrade agents to see per-core CPU, per-interface network rates, disk I/O, and per-container stats.
- New database migrations **012** (expanded metrics columns), **013** (host system info), and **014** (rebuilt `metrics_5min` CA with JSONB snapshots) run automatically on server startup. Migration 014 `DROP`s and recreates the continuous aggregate; backfill reseeds in the background over ~90 days of history.

### Added

**Metrics collection (agent + server)**
- **Per-core CPU usage** via `sysinfo::Cpus` — exposed as `cpu_cores` array in `AgentMetrics` and surfaced in UI as a color-coded bar grid (`CpuCoreGrid`).
- **Per-interface network traffic** alongside the existing aggregate totals, emitting per-interface RX/TX rates in the SSE `metrics` event.
- **Disk I/O read/write bytes per second** (Linux sysfs delta, macOS falls back to 0).
- **Per-container Docker resource stats** via bollard's one-shot `stats` API: CPU %, memory usage/limit MB, network RX/TX per running container. Streamed in the SSE `status` event.
- **Host system info** — OS, CPU model, memory total, boot time, IP address. Migration `013_host_system_info.sql` adds columns to `hosts`, populated by `/system-info` endpoint on reconnection and every 24 h thereafter.

**Real-time SSE richness**
- SSE `metrics` event now streams `disks`, `temperatures`, and `docker_stats` every scrape cycle (previously only on status change), so temperature/disk/Docker charts update live instead of waiting for DB polls.
- Synthetic SSE rows backfill those same fields into chart buffers so there is no `null` gap between DB rows and live data.

**Database schema (TimescaleDB)**
- Migration `012_expanded_metrics.sql` — `cpu_cores`, `network_interfaces`, `docker_stats` JSONB columns on the `metrics` hypertable.
- Migration `013_host_system_info.sql` — OS/CPU/memory/boot-time/IP columns on `hosts` (idempotent `IF NOT EXISTS`).
- Migration `014_ca_v3.sql` — rebuilt `metrics_5min` continuous aggregate that additionally stores `last(disks, timestamp)`, `last(temperatures, timestamp)`, `last(gpus, timestamp)`, `last(docker_stats, timestamp)`. Long-range charts (>6 h) now keep full per-element granularity instead of the previous scalar-only aggregates.

**Frontend components**
- `CpuCoreGrid`, `NetworkInterfaceTable`, `DockerGrid`, `DockerCharts`, and a redesigned `DiskUsageBar` (read/write indicators under the capacity bar).
- Top **Navbar** replaces the sidebar — horizontal nav on desktop, hamburger + dropdown on mobile, all icon buttons carry `aria-label`.
- New **M3-aligned design tokens**: shape scale (`--md-sys-shape-corner-xs..full`), motion (duration + easing curves), 4 px spacing grid, state-layer hover (color-mix 8 %).
- Proper M3 toggle switch (`.switch`, `role=switch`, `aria-checked`) replaces the crude 32 × 18 div toggle on the alerts page.
- Complete M3 design system documentation in `DESIGN.md` — tonal palette (light/dark), typography scale (Display/Headline 400), elevation via surface-container tokens, canonical breakpoints 600/839/1200, motion tokens, component patterns (buttons, cards, tables, toggles, inputs, dialog, snackbar, progress), data-visualization guidance, WCAG AA checklist, and a phased migration plan.

**Authentication / security primitives**
- `UserGuard` — extractor that only accepts user JWTs (`aud: "user"`) on all read-only user-facing endpoints, rejecting agent JWTs outright.
- `AdminGuard` now also guards the notification-channel **list** endpoint (was previously read-for-all) and every channel mutation — required because the list response contains SMTP credentials.
- `DefaultBodyLimit(256 KB)` on the router to cap JSON payloads (SS-01).
- `Strict-Transport-Security` header added alongside the existing `X-Content-Type-Options` / `X-Frame-Options` / `Referrer-Policy` set.
- Agent now enforces a `JWT_SECRET` minimum of 32 characters (matches server) and refuses to start otherwise.
- Agent log retention — `LOG_RETENTION_DAYS` (default 180) via `tracing-appender` Builder API prevents unbounded disk growth.

### Changed

**Scraper / request pipeline (server)**
- `scrape_one` and `handle_success` parameters consolidated into a **`ScrapeContext`** struct (owns `client`, `target`, `display_name`, `ports`, `containers`, `alert_config`, `state`, `jwt_token`, `system_info_updated_at`, `is_known_host`). Eliminates two `#[allow(clippy::too_many_arguments)]` overrides.
- `ensure_host_registered` now **skips known hosts** via a `let ... && cond` chain — removes one `INSERT ON CONFLICT` per host per scrape cycle.
- Batch metrics dispatch uses `buffer_unordered(5)` instead of `join_all`, capping concurrent DB inserts at half the default pool size.
- `list_hosts` and `load_all_as_map` now run **in parallel via `tokio::join!`** during scrape init.
- Stringly-typed `channel_type` / `metric_type` columns replaced by `ChannelType` / `MetricType` Rust enums (`sqlx::Type + serde`). Unknown variants are rejected at deserialize-time — handler-level validation removed as dead code.
- Alert delivery now **fire-and-forget**: the scraper no longer awaits outbound webhook calls (SP-09), so a slow Discord or Slack receiver can't back up the scrape loop.
- `MetricAlertRule` derives `Copy`; `.cloned()` → `.copied()` on iterator chains (SI-15).

**Argon2 + auth pipeline**
- Password hash / verify calls wrapped in `tokio::spawn_blocking` (SI-01) — prevents CPU-bound Argon2 from blocking the runtime executor.
- `UserGuard` / `AdminGuard` now **carry decoded claims** in the request extension, eliminating 4× JWT re-parse per request (SI-04).
- Setup endpoint (initial admin creation) wraps the check + insert in a **database transaction** (SS-02) — closes the TOCTOU race where two concurrent setup requests could both see an empty users table.
- `extract_refresh_cookie` uses a `const` prefix instead of `format!`, saving one allocation per request (SI-10).
- `create_user()` made executor-generic (`impl Executor`) so it works with both `&Pool` and `&mut Transaction`.
- Admin session-revoke audit log now includes the admin's username (not just id).

**Frontend state + rendering**
- `ThemeProvider` / `I18nProvider` read `localStorage` inside `useEffect` rather than during render (WB-01/WB-02) — eliminates SSR/CSR hydration mismatch.
- `AlertsPage`, `HostAlertOverride`, `DashboardWidgets` replaced render-time `setState` with `useEffect` (WB-07) — fixes "cannot update during render" React warnings.
- Client password policy aligned with server: minimum 8 chars + uppercase + lowercase + digit + special char (W-07).
- `<html lang>` attribute now syncs with the current i18n locale via `useEffect` (W-20).
- Tooltip rendering in multi-series charts now sorts entries by value descending so the largest series appears first.

**Documentation**
- `DESIGN.md` rewritten as a full M3-adapted design system reference (see Added).
- `README.md` updated to document `UserGuard` / `AdminGuard` split, new SSE fields, host system-info columns, and the new `logout` / `sse-ticket` / `revoke-sessions` endpoints.

### Fixed

**Security**
- **SSRF bypass via IPv4-mapped IPv6** (`::ffff:127.0.0.1`) — `url_validator` now unwraps IPv4-mapped addresses before the private-IP check (SS-02).
- `host_key` format strictly validated as `host:port` — no path, query, fragment, or `@` characters allowed (SS-06). Prevents path injection when the scraper builds `http://{host_key}/metrics`.
- Login rate limit now returns **429 TooManyRequests** instead of 400 BadRequest (SS-05) — matches semantics and enables client retry-after logic.
- Password length **capped at 128 chars** to prevent Argon2 DoS (SS-04) — a malicious 1 MB password would otherwise cost seconds of CPU per verification.
- API rate limiter switched to `extract_client_ip(TRUSTED_PROXY_COUNT)` so X-Forwarded-File isn't trusted by default (SS-03).
- `change_password` now uses `UserGuard` (was `AdminGuard`) — **viewers can rotate their own password** without admin intervention (SS-07).
- Alert history `offset` parameter capped at 10 000 (SS-10) — bounds the deep-scan cost an authenticated caller can impose.
- `display_name` in `ensure_host_registered` is only updated when the stored value is empty (SS-15) — prevents a compromised agent from relabeling another host.
- Legacy tokens without `aud` claim now emit a **warning log** (SS-14), flagging for eventual removal of the backward-compat branch.

**Agent**
- `ports` query param capped at **100 entries** to prevent DoS via unbounded TCP connect floods (A-04).
- Top-10 process selection switched to `select_nth_unstable` — **O(n)** instead of the previous O(n log n) sort (A-12).
- `AGENT_VERSION` extracted to `const` — no per-request allocation of the version string (A-11).
- Disk I/O previous-values cache migrated from `Mutex<Option<HashMap>>` to `LazyLock<Mutex<HashMap>>` — eliminates the option + init race (A-07).
- Port 0 filtered from `parse_comma_separated_ports` (AG-09).
- JWT audience comment corrected — `set_audience` **does** reject tokens without `aud` (AG-06).

**Frontend**
- RX/TX and Read/Write labels disappeared from network / disk-I/O chart tooltips — the formatter was returning an empty string as the series name. Now the original Recharts series name is preserved.
- Form label associations (`htmlFor` / `id`) added on setup, agents, alerts, and monitors pages (WB-05). `MiniField` changed from `<div>` to `<label>`.
- `DateTimePicker` closes on `Escape` key (WB-06).
- `NotificationChannelsSection`, `HttpMonitors`, `PingMonitors` wrapped network calls in `try/catch` + `toast.error()` (WB-03/WB-04) — failures were previously silent.

**Server correctness / resilience**
- `list_hosts` and `load_all_as_map` parallelization fixed a head-of-line wait that was causing first-scrape-after-startup to skip hosts.
- `AppError::From<sqlx::Error>` now formats with `{err:#}` to include the full error chain in server-side logs (SI-03) — previously only the top-level error surfaced.
- Email channel now logs a warning when SMTP credentials are empty (SI-08) instead of silently delivering nothing.

### Security

- Agent-JWT-to-user-endpoint escalation path closed: `AuthGuard` (accepted both audiences) removed entirely; every user endpoint is `UserGuard` or `AdminGuard`. A compromised agent token can no longer read dashboard metrics, hosts, alerts, or notification channels.
- All points already listed under Fixed > Security (SS-01 .. SS-15) remain, consolidated by issue ID for traceability. Internal review documents: `docs/review-20260414.md`, `docs/review-20260414-v2.md`, `docs/review-20260415.md`.

### Performance

- **Sysinfo statics cached** in `LazyLock<Mutex<_>>` (AG-03): `System`, `Networks`, `Components` are now allocated once and mutated in place instead of constructed fresh every 10 s scrape cycle. Reduces memory fragmentation on long-running agents. (Disks still refreshed each cycle — mount points can change.)
- **Docker event + stats reconnection** uses exponential backoff (5 s → 300 s cap) instead of fixed 5 s delays (AG-02). Resets on first successful reconnect.
- **Metrics query cache**: cache hits now return `Arc<Vec<MetricsRow>>` **directly** instead of going through `Arc::unwrap_or_clone` (which always cloned because the cache itself held one Arc). Removes one `Vec` clone per cache hit on every `/api/metrics/{host_key}` (SP-04).
- **Stale login-attempt entries** evicted by a background task every 5 min via `LoginRateLimiter::evict_stale()` (SP-05). Previously the map grew unbounded with one entry per unique IP for the full 5-min window after its last miss.
- `tokio::join!` used to parallelize scraper init (SP-03) and agent-side sysinfo/ports/Docker-cache reads.

### Refactored

- Scraper: `ScrapeContext` struct (see Changed — scraper pipeline). Eliminates 9-10 parameter signatures and two `too_many_arguments` allow attributes.
- Channel / metric type enums replace raw strings in DB + API layers.
- Docker event handler flattened from nested `match` to `let-else` idiom (A-13), consistent with the project's Rust edition 2024 clippy profile.
- Safety comments added on `std::sync::RwLock` acquisitions inside `handle_success` documenting the no-`.await`-while-held invariant.

### Contributors / reviewers

Automated code review loop (server-security, server-idiom, contract, agent, and web reviewers) was run twice during this cycle; each finding is tagged in commit bodies (`SS-xx`, `SI-xx`, `SP-xx`, `A-xx`, `AG-xx`, `W-xx`, `WB-xx`) and cross-referenced in `docs/review-20260414*.md` and `docs/review-20260415.md`.

## [1.0.0] — 2026-04-05

### Added
- AdminGuard extractor — mutation endpoints require admin role
- Login rate limiting (10 attempts per 5 minutes per IP via X-Forwarded-For)
- Password change endpoint (PUT /api/auth/password)
- Health check endpoint (GET /api/health — verifies DB connectivity, returns server version)
- Graceful shutdown for server and agent (SIGTERM/SIGINT signal handling)
- Monitor failure alerting — HTTP/Ping monitor failures trigger notifications with 5-min cooldown
- Agent version field (agent_version) for server-agent compatibility tracking
- X-API-Version: 1 response header on all API responses
- Scraper exponential backoff for unresponsive hosts (10s → 160s cap)
- React ErrorBoundary wrapping main layout
- Skip-to-content link and focus-visible ring for keyboard accessibility
- aria-live region for SSE connection status
- 30+ i18n translation keys (EN/KO) for sidebar, agents, alerts, dashboard, ports
- CHANGELOG.md with git-cliff configuration for automated generation
- sqlx migrations (5 numbered SQL files replacing code-based init_db)

### Changed
- Authentication simplified to two-track: Agent JWT + User JWT (removed static API key)
- Chart colors now use CSS variables for proper dark mode support
- Server Dockerfile runs as non-root 'monitor' user
- Docker log rotation added to all services (10MB x 3 files)
- Deploy health check upgraded from / to /api/health (verifies DB)

### Fixed
- Replaced .expect() panics in auth.rs/user_auth.rs with proper AppError returns
- Frontend auto-redirects to /login on 401 (expired token)
- Input validation added for alert configs, monitors, and notification channels
- Uptime calculation always showing 100% — offline periods now write is_online=false metric records

### Security
- 90-day retention policies for alert_history, http_monitor_results, ping_results (TimescaleDB hypertables)

## [0.1.0] — 2026-04-04

### Added
- Full-stack network monitoring: Rust agent (CPU, memory, disk, GPU, Docker, ports) + Rust/Axum server + Next.js dashboard
- Real-time metrics via SSE and SWR polling
- TimescaleDB hypertable with 90-day retention and 5-minute continuous aggregates
- Multi-channel alerts: Discord, Slack, Email with per-host overrides and cooldown
- HTTP endpoint and Ping (TCP) external monitoring
- User authentication (Argon2id + JWT) with admin/viewer roles
- Customizable dashboard with pinnable widgets
- i18n support (English / Korean)
- Dark mode with CSS variable theming
- PWA support with service worker
- Prometheus `/metrics` export endpoint
- Public status page (`/status`)
- CI/CD: PR-triggered lint/test/build + manual deploy via SSH rsync + native ARM64 build
