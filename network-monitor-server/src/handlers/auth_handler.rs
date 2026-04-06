use std::net::SocketAddr;
use std::sync::Arc;

use axum::Json;
use axum::extract::{ConnectInfo, State};
use axum::http::HeaderMap;
use serde::Deserialize;

use crate::errors::AppError;
use crate::models::app_state::AppState;
use crate::repositories::users_repo::{self, UserInfo};
use crate::services::auth::{AdminGuard, AuthGuard};
use crate::services::user_auth;

#[derive(Deserialize)]
pub struct LoginRequest {
    pub username: String,
    pub password: String,
}

#[derive(serde::Serialize)]
pub struct LoginResponse {
    pub token: String,
    pub user: UserInfo,
}

/// Extract the client IP address, accounting for trusted reverse proxies.
///
/// When `trusted_proxy_count == 0`, ignores X-Forwarded-For (prevents spoofing)
/// and uses the peer socket address. When `> 0`, takes the Nth IP from the
/// **right** of X-Forwarded-For (proxies append left-to-right, so the rightmost
/// entries are from infrastructure the operator controls).
fn extract_client_ip(
    headers: &HeaderMap,
    peer_addr: &SocketAddr,
    trusted_proxy_count: usize,
) -> String {
    if trusted_proxy_count == 0 {
        return peer_addr.ip().to_string();
    }
    if let Some(xff) = headers.get("x-forwarded-for").and_then(|v| v.to_str().ok()) {
        let ips: Vec<&str> = xff.split(',').map(|s| s.trim()).collect();
        if ips.len() >= trusted_proxy_count {
            return ips[ips.len() - trusted_proxy_count].to_string();
        }
    }
    peer_addr.ip().to_string()
}

/// POST /api/auth/login — authenticate with username/password, returns JWT
pub async fn login(
    State(state): State<Arc<AppState>>,
    ConnectInfo(peer_addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Json(body): Json<LoginRequest>,
) -> Result<Json<LoginResponse>, AppError> {
    // Rate limit by client IP (secure extraction, immune to X-Forwarded-For spoofing)
    let ip = extract_client_ip(&headers, &peer_addr, state.trusted_proxy_count);

    if let Err(retry_after) = state.login_rate_limiter.check(&ip) {
        tracing::warn!(ip = %ip, "🔒 [Auth] Login rate limited");
        return Err(AppError::BadRequest(format!(
            "Too many login attempts. Try again in {retry_after} seconds."
        )));
    }

    let user = users_repo::find_by_username(&state.db_pool, &body.username)
        .await?
        .ok_or_else(|| AppError::Unauthorized("Invalid username or password".to_string()))?;

    if !user_auth::verify_password(&body.password, &user.password_hash) {
        return Err(AppError::Unauthorized(
            "Invalid username or password".to_string(),
        ));
    }

    let token = user_auth::generate_user_jwt(user.id, &user.username, &user.role)?;

    tracing::info!(username = %user.username, "🔐 [Auth] User logged in");

    Ok(Json(LoginResponse {
        token,
        user: user.into(),
    }))
}

/// GET /api/auth/me — return current user info from JWT
pub async fn me(
    _auth: AuthGuard,
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Json<UserInfo>, AppError> {
    let token = headers
        .get("Authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|h| h.strip_prefix("Bearer "))
        .ok_or_else(|| AppError::Unauthorized("Missing token".to_string()))?;

    let claims = user_auth::decode_user_jwt(token)
        .ok_or_else(|| AppError::Unauthorized("Invalid user token".to_string()))?;

    let user = users_repo::find_by_username(&state.db_pool, &claims.username)
        .await?
        .ok_or_else(|| AppError::Unauthorized("User no longer exists".to_string()))?;

    Ok(Json(user.into()))
}

#[derive(Deserialize)]
pub struct SetupRequest {
    pub username: String,
    pub password: String,
}

/// POST /api/auth/setup — create initial admin account (only when no users exist)
pub async fn setup(
    State(state): State<Arc<AppState>>,
    Json(body): Json<SetupRequest>,
) -> Result<Json<LoginResponse>, AppError> {
    let count = users_repo::count_users(&state.db_pool).await?;
    if count > 0 {
        return Err(AppError::BadRequest(
            "Setup already completed. Use login instead.".to_string(),
        ));
    }

    if body.username.is_empty() {
        return Err(AppError::BadRequest("Username is required".to_string()));
    }
    validate_password(&body.password)?;

    let password_hash = user_auth::hash_password(&body.password)
        .map_err(|e| AppError::Internal(format!("Failed to hash password: {}", e)))?;

    let user = users_repo::create_user(&state.db_pool, &body.username, &password_hash, "admin")
        .await
        .map_err(|e| AppError::Internal(format!("Failed to create user: {}", e)))?;

    let token = user_auth::generate_user_jwt(user.id, &user.username, &user.role)?;

    tracing::info!(username = %user.username, "🔐 [Auth] Initial admin account created");

    Ok(Json(LoginResponse {
        token,
        user: user.into(),
    }))
}

/// GET /api/auth/status — check if setup is needed (no auth required)
pub async fn auth_status(
    State(state): State<Arc<AppState>>,
) -> Result<Json<serde_json::Value>, AppError> {
    let count = users_repo::count_users(&state.db_pool).await?;
    Ok(Json(serde_json::json!({
        "setup_required": count == 0,
    })))
}

#[derive(Deserialize)]
pub struct ChangePasswordRequest {
    pub current_password: String,
    pub new_password: String,
}

/// PUT /api/auth/password — change current user's password (admin only)
pub async fn change_password(
    _admin: AdminGuard,
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(body): Json<ChangePasswordRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    validate_password(&body.new_password)?;

    let token = headers
        .get("Authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|h| h.strip_prefix("Bearer "))
        .ok_or_else(|| AppError::Unauthorized("Missing token".to_string()))?;

    let claims = user_auth::decode_user_jwt(token)
        .ok_or_else(|| AppError::Unauthorized("Invalid token".to_string()))?;

    let user = users_repo::find_by_username(&state.db_pool, &claims.username)
        .await?
        .ok_or_else(|| AppError::Unauthorized("User not found".to_string()))?;

    if !user_auth::verify_password(&body.current_password, &user.password_hash) {
        return Err(AppError::Unauthorized(
            "Current password is incorrect".to_string(),
        ));
    }

    let new_hash = user_auth::hash_password(&body.new_password)
        .map_err(|e| AppError::Internal(format!("Failed to hash password: {e}")))?;

    users_repo::update_password(&state.db_pool, user.id, &new_hash).await?;

    // Invalidate all existing tokens by updating password_changed_at cache
    let now = chrono::Utc::now().timestamp();
    crate::services::auth::update_password_changed_at(user.id, now);

    tracing::info!(username = %user.username, "🔐 [Auth] Password changed — existing tokens revoked");
    Ok(Json(serde_json::json!({ "success": true })))
}

/// Validate password strength: min 8 chars, uppercase, lowercase, digit, special char.
fn validate_password(password: &str) -> Result<(), AppError> {
    if password.len() < 8 {
        return Err(AppError::BadRequest(
            "Password must be at least 8 characters".to_string(),
        ));
    }
    let has_upper = password.chars().any(|c| c.is_uppercase());
    let has_lower = password.chars().any(|c| c.is_lowercase());
    let has_digit = password.chars().any(|c| c.is_ascii_digit());
    let has_special = password.chars().any(|c| !c.is_alphanumeric());
    if !has_upper || !has_lower || !has_digit || !has_special {
        return Err(AppError::BadRequest(
            "Password must contain uppercase, lowercase, digit, and special character".to_string(),
        ));
    }
    Ok(())
}
