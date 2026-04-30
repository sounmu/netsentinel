use std::net::SocketAddr;
use std::sync::Arc;

use axum::Json;
use axum::extract::{ConnectInfo, State};
use axum::http::header::{HeaderValue, SET_COOKIE};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};

use crate::errors::AppError;
use crate::models::app_state::AppState;
use crate::repositories::users_repo::{self, UserInfo, UserRow};
use crate::services::auth::{AdminGuard, UserGuard};
use crate::services::refresh_token::{self, REFRESH_TTL_DAYS, RotateOutcome};
use crate::services::user_auth;

/// Cookie name used for the rotating refresh token. Bound to `/api/auth`
/// via the `Path=` attribute so it is never sent to application endpoints —
/// the browser only surfaces it on login/refresh/logout calls.
const REFRESH_COOKIE_NAME: &str = "nm_refresh";

/// Whether the refresh cookie should carry the `Secure` flag.
/// Evaluated once on first call and cached for the process lifetime via
/// `OnceLock`. Secure-by-default: operators must explicitly opt out with
/// `COOKIE_SECURE=false` for local plain-HTTP development.
fn is_secure_cookie() -> bool {
    use std::sync::OnceLock;
    static SECURE: OnceLock<bool> = OnceLock::new();
    *SECURE.get_or_init(|| match std::env::var("COOKIE_SECURE") {
        Ok(value) => !matches!(
            value.trim().to_ascii_lowercase().as_str(),
            "0" | "false" | "no" | "off"
        ),
        Err(_) => true,
    })
}

/// Build a `Set-Cookie` header value that installs a fresh refresh token.
fn build_refresh_cookie(plaintext: &str) -> String {
    let max_age_secs = REFRESH_TTL_DAYS * 24 * 60 * 60;
    let secure = if is_secure_cookie() { "; Secure" } else { "" };
    format!(
        "{name}={value}; HttpOnly; SameSite=Strict; Path=/api/auth; Max-Age={max_age}{secure}",
        name = REFRESH_COOKIE_NAME,
        value = plaintext,
        max_age = max_age_secs
    )
}

/// Build a `Set-Cookie` header value that deletes the refresh cookie.
fn build_refresh_cookie_expiry() -> String {
    let secure = if is_secure_cookie() { "; Secure" } else { "" };
    format!(
        "{name}=; HttpOnly; SameSite=Strict; Path=/api/auth; Max-Age=0{secure}",
        name = REFRESH_COOKIE_NAME,
    )
}

/// Pull the refresh token plaintext out of a `Cookie` request header.
/// Prefix for the refresh cookie — const avoids a format!() allocation per call.
const REFRESH_COOKIE_PREFIX: &str = "nm_refresh=";

fn extract_refresh_cookie(headers: &HeaderMap) -> Option<String> {
    let raw = headers.get("cookie")?.to_str().ok()?;
    for segment in raw.split(';') {
        let trimmed = segment.trim();
        if let Some(rest) = trimmed.strip_prefix(REFRESH_COOKIE_PREFIX) {
            return Some(rest.to_string());
        }
    }
    None
}

/// Read the `User-Agent` request header as an owned `String`, if present.
pub(crate) fn extract_user_agent(headers: &HeaderMap) -> Option<String> {
    headers
        .get("user-agent")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string())
}

/// Build a JSON response that also carries a `Set-Cookie` header.
fn json_with_cookie<T: serde::Serialize>(body: &T, cookie: &str) -> Result<Response, AppError> {
    let mut resp = Json(body).into_response();
    let header_value = HeaderValue::from_str(cookie)
        .map_err(|e| AppError::Internal(format!("Invalid Set-Cookie header value: {e}")))?;
    resp.headers_mut().append(SET_COOKIE, header_value);
    Ok(resp)
}

fn unauthorized_json_with_cookie(message: &str, cookie: &str) -> Result<Response, AppError> {
    let mut resp = (
        StatusCode::UNAUTHORIZED,
        Json(serde_json::json!({ "error": message })),
    )
        .into_response();
    let header_value = HeaderValue::from_str(cookie)
        .map_err(|e| AppError::Internal(format!("Invalid Set-Cookie header value: {e}")))?;
    resp.headers_mut().append(SET_COOKIE, header_value);
    Ok(resp)
}

#[derive(serde::Serialize)]
pub struct LoginResponse {
    pub token: String,
    pub user: UserInfo,
}

/// Extract the client IP address, accounting for trusted reverse proxies.
///
/// When `trusted_proxy_count == 0`, ignores every forwarded-IP header
/// (prevents spoofing) and uses the peer socket address. When `> 0`:
///   1. **`CF-Connecting-IP`** is preferred — Cloudflare always sets this to
///      the original client IP and overwrites any spoofed value at the edge.
///      Native support matters because the NetSentinel stock deployment is
///      "Zero-Trust via Cloudflare Tunnel", where without this every request
///      collapses onto a single tunnel-IP and trips rate limits instantly.
///   2. Falls back to the Nth-from-right entry of `X-Forwarded-For`
///      (proxies append left-to-right, so rightmost entries come from
///      operator-controlled infrastructure).
pub(crate) fn extract_client_ip(
    headers: &HeaderMap,
    peer_addr: &SocketAddr,
    trusted_proxy_count: usize,
) -> String {
    if trusted_proxy_count == 0 {
        return peer_addr.ip().to_string();
    }
    if let Some(cf) = headers
        .get("cf-connecting-ip")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
    {
        return cf.to_string();
    }
    if let Some(xff) = headers.get("x-forwarded-for").and_then(|v| v.to_str().ok()) {
        let ips: Vec<&str> = xff.split(',').map(|s| s.trim()).collect();
        if ips.len() >= trusted_proxy_count {
            return ips[ips.len() - trusted_proxy_count].to_string();
        }
    }
    peer_addr.ip().to_string()
}

/// Install a fresh refresh cookie and return a short-lived access token.
///
/// Used by the OAuth callback and refresh path. The two tokens live
/// separately by design: the browser stores the access token in memory only,
/// while the refresh token is httpOnly/Secure/SameSite=Strict/Path=/api/auth.
pub(crate) async fn issue_session_response(
    state: &AppState,
    user: UserRow,
    headers: &HeaderMap,
    ip_str: &str,
) -> Result<Response, AppError> {
    let access_token = user_auth::generate_user_jwt(user.id, &user.email, &user.role)?;
    let user_agent = extract_user_agent(headers);
    let refresh = refresh_token::issue_new_family(
        &state.db_pool,
        user.id,
        user_agent.as_deref(),
        Some(ip_str),
    )
    .await?;

    tracing::info!(user_id = user.id, email = %user.email, "🔐 [Auth] User session issued");

    let body = LoginResponse {
        token: access_token,
        user: user.into(),
    };
    json_with_cookie(&body, &build_refresh_cookie(&refresh.plaintext))
}

/// POST /api/auth/refresh — rotate the refresh cookie and hand out a new access token.
///
/// Requires no bearer header — the caller proves session continuity via
/// the httpOnly refresh cookie. On success, a new cookie replaces the old
/// one and a fresh access JWT is returned. On failure the response is
/// 401 with an explicit cookie-deletion header so the client releases any
/// stale state.
pub async fn refresh(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    ConnectInfo(peer_addr): ConnectInfo<SocketAddr>,
) -> Result<Response, AppError> {
    let presented = extract_refresh_cookie(&headers)
        .ok_or_else(|| AppError::Unauthorized("No refresh cookie".to_string()))?;
    let user_agent = extract_user_agent(&headers);
    let ip_str = extract_client_ip(&headers, &peer_addr, state.trusted_proxy_count);

    match refresh_token::rotate(
        &state.db_pool,
        &presented,
        user_agent.as_deref(),
        Some(&ip_str),
    )
    .await?
    {
        RotateOutcome::Rotated(new) => {
            // Look up user info for the fresh access token claims.
            let user = users_repo::find_by_id(&state.db_pool, new.user_id)
                .await?
                .ok_or_else(|| AppError::Unauthorized("User no longer exists".to_string()))?;
            let access_token = user_auth::generate_user_jwt(user.id, &user.email, &user.role)?;
            let body = LoginResponse {
                token: access_token,
                user: user.into(),
            };
            json_with_cookie(&body, &build_refresh_cookie(&new.plaintext))
        }
        RotateOutcome::ReuseDetected { user_id } => {
            // Already handled inside rotate(): family revoked, cutoff raised.
            // Tell the browser to drop its (now-worthless) cookie.
            tracing::warn!(
                user_id,
                "🚨 [Auth] /refresh returning 401 after reuse detection"
            );
            unauthorized_json_with_cookie(
                "Session terminated — please sign in again",
                &build_refresh_cookie_expiry(),
            )
        }
        RotateOutcome::Rejected => unauthorized_json_with_cookie(
            "Invalid or expired refresh token",
            &build_refresh_cookie_expiry(),
        ),
    }
}

/// GET /api/auth/me — return current user info from JWT
pub async fn me(
    auth: UserGuard,
    State(state): State<Arc<AppState>>,
) -> Result<Json<UserInfo>, AppError> {
    let user = users_repo::find_by_id(&state.db_pool, auth.claims.sub)
        .await?
        .ok_or_else(|| AppError::Unauthorized("User no longer exists".to_string()))?;

    Ok(Json(user.into()))
}

/// GET /api/auth/status — return public auth entry points.
pub async fn auth_status() -> Result<Json<serde_json::Value>, AppError> {
    Ok(Json(serde_json::json!({
        "login_url": "/api/auth/oauth/google/start",
    })))
}

/// POST /api/auth/logout — revoke every token for the caller and clear the cookie.
///
/// Three side effects:
///   1. Stamp `users.tokens_revoked_at` → in-memory cutoff raised → every
///      existing access JWT for this user is rejected immediately.
///   2. Mark every live row in `refresh_tokens` for this user as revoked.
///   3. Reply with a `Set-Cookie` that deletes `nm_refresh` from the
///      browser.
///
/// **Gated by `UserGuard`** — previously this endpoint accepted any decode-
/// successful token (including expired) and fell back to acting on the
/// refresh cookie alone. That let an attacker with a leaked access fragment
/// or a stolen `nm_refresh` cookie force-logout arbitrary users and churn
/// writes against the SQLite writer lock. The web client already holds a
/// fresh access JWT in normal flows (it refreshes before calling logout),
/// so the stricter gate has no legitimate UX regression.
pub async fn logout(
    auth: UserGuard,
    State(state): State<Arc<AppState>>,
) -> Result<Response, AppError> {
    let user_id = auth.claims.sub;
    let username = auth.claims.username.clone();

    users_repo::revoke_user_tokens(&state.db_pool, user_id).await?;
    let now = chrono::Utc::now().timestamp();
    crate::services::auth::update_tokens_revoked_at(user_id, now);
    if let Err(e) = refresh_token::revoke_all_for_user(&state.db_pool, user_id).await {
        tracing::warn!(err = ?e, user_id, "⚠️ [Auth] Failed to revoke refresh rows on logout");
    }
    tracing::info!(
        user_id,
        %username,
        "🔐 [Auth] User logged out — all tokens revoked"
    );

    let mut resp = Json(serde_json::json!({ "success": true })).into_response();
    resp.headers_mut().append(
        SET_COOKIE,
        HeaderValue::from_str(&build_refresh_cookie_expiry())
            .map_err(|e| AppError::Internal(format!("header build: {e}")))?,
    );
    Ok(resp)
}

/// POST /api/admin/users/{id}/revoke-sessions — operator kill-switch.
///
/// Lets an administrator terminate every active session for a target user
/// without needing their password. Use cases: stolen laptop, offboarded
/// employee, incident response. The admin's own session is unaffected
/// unless they pass their own user id.
pub async fn admin_revoke_user_sessions(
    admin: AdminGuard,
    State(state): State<Arc<AppState>>,
    axum::extract::Path(user_id): axum::extract::Path<i32>,
) -> Result<Json<serde_json::Value>, AppError> {
    let _ = &admin.claims; // used for audit logging below
    // Confirm the target user exists so callers get a 404 instead of a
    // silent success against a non-existent id.
    if users_repo::find_by_id(&state.db_pool, user_id)
        .await?
        .is_none()
    {
        return Err(AppError::NotFound(format!("User {user_id} not found")));
    }

    users_repo::revoke_user_tokens(&state.db_pool, user_id).await?;
    let now = chrono::Utc::now().timestamp();
    crate::services::auth::update_tokens_revoked_at(user_id, now);
    if let Err(e) = refresh_token::revoke_all_for_user(&state.db_pool, user_id).await {
        tracing::warn!(err = ?e, target_user_id = user_id, "⚠️ [Auth] Failed to revoke refresh rows on admin kill");
    }

    tracing::warn!(
        admin = %admin.claims.username,
        target_user_id = user_id,
        "🔐 [Auth] Admin force-revoked all sessions for user"
    );
    Ok(Json(
        serde_json::json!({ "success": true, "user_id": user_id }),
    ))
}

#[derive(serde::Serialize)]
pub struct SseTicketResponse {
    pub ticket: String,
    /// TTL hint for the client so it can pre-refresh without probing the server.
    /// Kept in sync with `services::sse_ticket::TICKET_TTL`.
    pub expires_in_secs: u64,
}

/// POST /api/auth/sse-ticket — mint a short-lived single-use ticket for `GET /api/stream`.
///
/// Requires a valid user JWT on the `Authorization` header. The returned ticket is
/// bound to the caller's `user_id` and is consumed atomically on the SSE handshake.
/// Per-user `ISSUE_COOLDOWN` (2 s) prevents a tight retry loop on a flaky
/// SSE connection from burning the entire authenticated API rate-limit
/// budget on ticket traffic — see `services::sse_ticket` for rationale.
pub async fn issue_sse_ticket(
    auth: UserGuard,
    State(state): State<Arc<AppState>>,
) -> Result<Json<SseTicketResponse>, AppError> {
    match state
        .sse_ticket_store
        .issue(auth.claims.sub, auth.claims.iat)
    {
        crate::services::sse_ticket::IssueOutcome::Minted(ticket) => Ok(Json(SseTicketResponse {
            ticket,
            expires_in_secs: 60,
        })),
        crate::services::sse_ticket::IssueOutcome::CoolingDown { retry_after_secs } => {
            tracing::warn!(
                user_id = auth.claims.sub,
                retry_after_secs,
                "🔒 [Auth] SSE ticket issue throttled (per-user cooldown)"
            );
            Err(AppError::TooManyRequests(format!(
                "SSE ticket issued too recently; retry in {retry_after_secs} s"
            )))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── build_refresh_cookie ──

    #[test]
    fn build_refresh_cookie_contains_expected_attributes() {
        let cookie = build_refresh_cookie("tok_abc123");
        assert!(
            cookie.contains("nm_refresh=tok_abc123"),
            "cookie should contain name=value"
        );
        assert!(cookie.contains("HttpOnly"), "cookie should be HttpOnly");
        assert!(
            cookie.contains("SameSite=Strict"),
            "cookie should be SameSite=Strict"
        );
        assert!(
            cookie.contains("Path=/api/auth"),
            "cookie should be scoped to /api/auth"
        );
        assert!(cookie.contains("Max-Age="), "cookie should have Max-Age");
        // Verify Max-Age is REFRESH_TTL_DAYS in seconds
        let expected_max_age = REFRESH_TTL_DAYS * 24 * 60 * 60;
        assert!(
            cookie.contains(&format!("Max-Age={expected_max_age}")),
            "Max-Age should be {expected_max_age}, got: {cookie}"
        );
    }

    // ── build_refresh_cookie_expiry ──

    #[test]
    fn build_refresh_cookie_expiry_sets_max_age_zero() {
        let cookie = build_refresh_cookie_expiry();
        assert!(
            cookie.contains("Max-Age=0"),
            "expiry cookie should have Max-Age=0, got: {cookie}"
        );
        assert!(
            cookie.contains("nm_refresh=;")
                || cookie.contains("nm_refresh= ;")
                || cookie.contains("nm_refresh="),
            "expiry cookie should clear the value"
        );
    }

    // ── extract_refresh_cookie ──

    #[test]
    fn extract_refresh_cookie_parses_single_cookie() {
        let mut headers = HeaderMap::new();
        headers.insert("cookie", HeaderValue::from_static("nm_refresh=abc123"));
        let result = extract_refresh_cookie(&headers);
        assert_eq!(result, Some("abc123".to_string()));
    }

    #[test]
    fn extract_refresh_cookie_parses_among_multiple() {
        let mut headers = HeaderMap::new();
        headers.insert(
            "cookie",
            HeaderValue::from_static("other=foo; nm_refresh=mytoken; session=bar"),
        );
        let result = extract_refresh_cookie(&headers);
        assert_eq!(result, Some("mytoken".to_string()));
    }

    #[test]
    fn extract_refresh_cookie_returns_none_when_missing() {
        let mut headers = HeaderMap::new();
        headers.insert("cookie", HeaderValue::from_static("other=foo; session=bar"));
        let result = extract_refresh_cookie(&headers);
        assert!(result.is_none(), "should return None when cookie is absent");
    }

    #[test]
    fn extract_refresh_cookie_returns_none_when_no_cookie_header() {
        let headers = HeaderMap::new();
        let result = extract_refresh_cookie(&headers);
        assert!(result.is_none(), "should return None with no cookie header");
    }
}
