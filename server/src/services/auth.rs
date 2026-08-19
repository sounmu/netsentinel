use axum::extract::FromRequestParts;
use axum::http::request::Parts;
use axum::response::IntoResponse;

use crate::errors::AppError;
use chrono::Utc;
use jsonwebtoken::{Algorithm, DecodingKey, EncodingKey, Header, encode};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, OnceLock, RwLock};

#[derive(Debug, Serialize, Deserialize)]
pub struct Claims {
    pub exp: usize,
    /// Audience claim — "agent" for agent tokens (token type separation)
    #[serde(default)]
    pub aud: String,
}

// Visibility is `pub(crate)` — only `services::user_auth` needs these, and
// both lockers are sized/typed so the surface is small, but narrowing to
// "within this crate only" protects against a well-meaning contributor
// re-exporting them from a test/example crate and leaking key material
// through a dependency graph.
pub(crate) static ENCODING_KEY: OnceLock<EncodingKey> = OnceLock::new();
pub(crate) static DECODING_KEY: OnceLock<DecodingKey> = OnceLock::new();

/// Per-user "tokens issued before this instant are invalid" cutoff cache.
///
/// Written by password changes and explicit revocations (logout / admin kill).
/// The stored timestamp is the earliest `iat` that is still allowed to pass.
/// A new write is kept only if it is strictly later than the existing entry.
static TOKEN_REVOCATION_CACHE: OnceLock<Arc<RwLock<HashMap<i32, i64>>>> = OnceLock::new();

pub fn init_encoding_key(secret: &str) {
    let key = EncodingKey::from_secret(secret.as_bytes());
    let _ = ENCODING_KEY.set(key);
    let dk = DecodingKey::from_secret(secret.as_bytes());
    let _ = DECODING_KEY.set(dk);
}

/// Initialize the token revocation cache reference (called from main.rs).
/// The cache is pre-seeded with password/revocation cutoffs for each user.
pub fn init_token_revocation_cache(cache: Arc<RwLock<HashMap<i32, i64>>>) {
    let _ = TOKEN_REVOCATION_CACHE.set(cache);
}

/// Internal helper: raise the cutoff for `user_id` to `timestamp`, but never
/// lower it.
fn raise_revocation_cutoff(user_id: i32, timestamp: i64) {
    if let Some(cache) = TOKEN_REVOCATION_CACHE.get()
        && let Ok(mut map) = cache.write()
    {
        map.entry(user_id)
            .and_modify(|existing| {
                if timestamp > *existing {
                    *existing = timestamp;
                }
            })
            .or_insert(timestamp);
    }
}

/// Update the cutoff after a password change.
pub fn update_password_changed_at(user_id: i32, timestamp: i64) {
    raise_revocation_cutoff(user_id, timestamp);
}

/// Update the cutoff after an explicit token revocation (logout / admin kill).
pub fn update_tokens_revoked_at(user_id: i32, timestamp: i64) {
    raise_revocation_cutoff(user_id, timestamp);
}

/// Check if a user JWT's `iat` is after the latest revocation event for that
/// user. Returns true if the token is still valid. A missing cache entry means
/// the user has never revoked and every signed token with a valid `iat` is
/// accepted.
///
/// **Fail-secure policy**: this function sits on the security-critical path
/// for every authenticated request. If the cache is not initialized or the
/// `RwLock` is poisoned, the previous implementation returned `true` (let the
/// request through). That is fail-open. A poisoned lock right after logout
/// would silently accept a revoked token for the lifetime of the process.
/// We now reject in both situations and log so the operator can recover.
pub(crate) fn is_token_iat_still_valid(user_id: i32, iat: usize) -> bool {
    let Some(cache) = TOKEN_REVOCATION_CACHE.get() else {
        tracing::error!(
            user_id,
            "🚨 [Auth] Revocation cache uninitialized — rejecting token (fail-secure)"
        );
        return false;
    };
    let map = match cache.read() {
        Ok(m) => m,
        Err(e) => {
            tracing::error!(
                user_id,
                err = %e,
                "🚨 [Auth] Revocation cache poisoned — rejecting token (fail-secure)"
            );
            return false;
        }
    };
    match map.get(&user_id) {
        Some(&cutoff) => (iat as i64) >= cutoff,
        None => true,
    }
}

fn encode_agent_jwt(key: &EncodingKey) -> Result<String, AppError> {
    let exp = Utc::now().timestamp() as usize + 60;
    let claims = Claims {
        exp,
        aud: "agent".to_string(),
    };
    encode(&Header::new(Algorithm::HS256), &claims, key)
        .map_err(|e| AppError::Internal(format!("JWT encoding failed: {e}")))
}

pub fn generate_jwt() -> Result<String, AppError> {
    let key = ENCODING_KEY
        .get()
        .ok_or_else(|| AppError::Internal("JWT encoding key not initialized".into()))?;
    encode_agent_jwt(key)
}

pub fn generate_agent_jwt_with_secret(secret: &str) -> Result<String, AppError> {
    let key = EncodingKey::from_secret(secret.as_bytes());
    encode_agent_jwt(&key)
}

/// Axum extractor that only accepts **user** JWTs (aud: "user").
/// Agent JWTs are rejected — agents should only be accessed via the scraping path.
/// Carries decoded claims so handlers can access user info without re-parsing the token.
pub struct UserGuard {
    pub claims: super::user_auth::UserClaims,
}

impl<S> FromRequestParts<S> for UserGuard
where
    S: Send + Sync,
{
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        let auth_header = parts
            .headers
            .get("Authorization")
            .and_then(|v| v.to_str().ok())
            .ok_or_else(|| AppError::Unauthorized("Missing Authorization header".to_string()))?;

        let token = auth_header.strip_prefix("Bearer ").ok_or_else(|| {
            AppError::Unauthorized("Authorization header must use Bearer scheme".to_string())
        })?;

        let claims = super::user_auth::decode_user_jwt(token)
            .ok_or_else(|| AppError::Unauthorized("Invalid or expired token".to_string()))?;

        if !is_token_iat_still_valid(claims.sub, claims.iat) {
            return Err(AppError::Unauthorized("Token revoked".to_string()));
        }

        Ok(UserGuard { claims })
    }
}

/// Axum extractor that enforces admin-only access.
/// Only user JWTs with role == "admin" are accepted. Agent JWTs are rejected.
/// Carries decoded claims so handlers can access user info without re-parsing the token.
pub struct AdminGuard {
    pub claims: super::user_auth::UserClaims,
}

impl<S> FromRequestParts<S> for AdminGuard
where
    S: Send + Sync,
{
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        let auth_header = parts
            .headers
            .get("Authorization")
            .and_then(|v| v.to_str().ok())
            .ok_or_else(|| AppError::Unauthorized("Missing Authorization header".to_string()))?;

        let token = auth_header.strip_prefix("Bearer ").ok_or_else(|| {
            AppError::Unauthorized("Authorization header must use Bearer scheme".to_string())
        })?;

        let claims = super::user_auth::decode_user_jwt(token)
            .ok_or_else(|| AppError::Unauthorized("Invalid or expired token".to_string()))?;

        if !is_token_iat_still_valid(claims.sub, claims.iat) {
            return Err(AppError::Unauthorized("Token revoked".to_string()));
        }

        if claims.role != "admin" {
            // 403, not 401 — the token is valid, the role is insufficient.
            // Returning 401 here would cause the web client's 401-handler to
            // wipe the session and force re-login of a correctly-authenticated
            // viewer who merely lacks the admin role.
            return Err(AppError::Forbidden("Admin access required".to_string()));
        }

        Ok(AdminGuard { claims })
    }
}

/// Paths that stay reachable while `must_change_password` is set.
///
/// The set is deliberately tiny: prove who you are, get out, or fix the
/// thing that is blocking you. Everything else — hosts, metrics, alerts,
/// monitors — stays shut, so a temporary password that leaks cannot be used
/// to read or change anything.
fn allowed_while_password_change_required(path: &str) -> bool {
    matches!(
        path,
        "/api/auth/password"
            | "/api/auth/me"
            | "/api/auth/logout"
            | "/api/auth/refresh"
            | "/api/auth/login"
            | "/api/auth/setup"
            | "/api/auth/status"
            | "/api/health"
            | "/api/public/status"
    )
}

/// Refuse the API while the caller owes us a password change.
///
/// Runs for every request. Unauthenticated ones pass straight through — the
/// route's own `UserGuard` decides those. Authenticated ones cost one
/// primary-key lookup, which is the honest price of reading the flag from the
/// database rather than trusting a claim baked into a JWT that was minted
/// before the reset.
pub async fn require_password_change(
    axum::extract::State(state): axum::extract::State<
        std::sync::Arc<crate::models::app_state::AppState>,
    >,
    request: axum::extract::Request,
    next: axum::middleware::Next,
) -> axum::response::Response {
    let path = request.uri().path();
    if allowed_while_password_change_required(path) {
        return next.run(request).await;
    }

    let claims = request
        .headers()
        .get("Authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|h| h.strip_prefix("Bearer "))
        .and_then(super::user_auth::decode_user_jwt);

    let Some(claims) = claims else {
        return next.run(request).await;
    };

    match crate::repositories::users_repo::find_by_id(&state.db_pool, claims.sub).await {
        Ok(Some(user)) if user.must_change_password => {
            // A distinct code so the client can route to the change-password
            // screen instead of treating this as a generic permission error.
            crate::errors::AppError::Forbidden(
                "password_change_required: set a new password before using the dashboard"
                    .to_string(),
            )
            .into_response()
        }
        // A lookup failure must not open the gate, but it also must not lock
        // out a healthy instance on a transient DB blip: the route's own guard
        // still runs behind us, so falling through is safe here.
        _ => next.run(request).await,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use jsonwebtoken::{DecodingKey, Validation, decode};

    // OnceLock is set once per process, so all tests in this suite share the same secret.
    const TEST_SECRET: &str = "test-secret-for-unit-tests";

    fn test_decoding_key() -> DecodingKey {
        DecodingKey::from_secret(TEST_SECRET.as_bytes())
    }

    fn test_validation() -> Validation {
        let mut v = Validation::new(Algorithm::HS256);
        v.validate_exp = false;
        v.set_audience(&["agent"]);
        v
    }

    #[test]
    fn test_generate_jwt_produces_three_part_token() {
        init_encoding_key(TEST_SECRET);
        let token = generate_jwt().expect("JWT generation failed");
        assert!(!token.is_empty());
        assert_eq!(
            token.split('.').count(),
            3,
            "JWT must be in header.payload.signature format"
        );
    }

    #[test]
    fn test_generated_jwt_is_decodable_with_correct_secret() {
        init_encoding_key(TEST_SECRET);
        let token = generate_jwt().expect("JWT generation failed");
        let result = decode::<Claims>(&token, &test_decoding_key(), &test_validation());
        assert!(
            result.is_ok(),
            "Should be decodable with the correct secret"
        );
    }

    #[test]
    fn test_jwt_signed_with_wrong_secret_fails_validation() {
        // Use encode/decode directly to avoid OnceLock global state — keeps this test isolated.
        use jsonwebtoken::{EncodingKey, Header, encode};
        let token = encode(
            &Header::new(Algorithm::HS256),
            &Claims {
                exp: usize::MAX,
                aud: "agent".to_string(),
            },
            &EncodingKey::from_secret(b"correct-secret"),
        )
        .expect("Token creation failed");

        let mut wrong_validation = test_validation();
        wrong_validation.validate_exp = false;
        let result = decode::<Claims>(
            &token,
            &DecodingKey::from_secret(b"wrong-secret"),
            &wrong_validation,
        );
        assert!(
            result.is_err(),
            "Validation must fail with the wrong secret"
        );
    }

    #[test]
    fn test_generated_jwt_exp_is_in_future() {
        use chrono::Utc;
        init_encoding_key(TEST_SECRET);
        let token = generate_jwt().expect("JWT generation failed");
        let data = decode::<Claims>(&token, &test_decoding_key(), &test_validation())
            .expect("Decoding failed");
        let now = Utc::now().timestamp() as usize;
        assert!(
            data.claims.exp > now,
            "exp must be in the future (token expires ~60 seconds from now)"
        );
    }

    // ── Secret rotation contract ─────────────────
    // Rotating JWT_SECRET (which in practice means restarting the server with
    // a new secret) must invalidate every previously-issued token. The
    // guarantee comes from jsonwebtoken's HMAC signature verification. The
    // equivalent contract tests for the user-JWT path live in
    // `services::user_auth::tests`; they cover both the `aud: "user"` branch
    // and the legacy no-aud fallback. When `check_jwt_query` was removed in
    // favour of single-use SSE tickets, its rotation tests were deleted
    // rather than ported — the `user_auth` tests already cover the same
    // code path (`decode_user_jwt`) and there is no longer a query-parameter
    // JWT acceptance point to exercise.
}
