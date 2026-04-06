use axum::extract::FromRequestParts;
use axum::http::request::Parts;

use crate::errors::AppError;
use chrono::Utc;
use chrono_tz::Asia::Seoul;
use jsonwebtoken::{Algorithm, DecodingKey, EncodingKey, Header, Validation, decode, encode};
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

pub static ENCODING_KEY: OnceLock<EncodingKey> = OnceLock::new();
pub static DECODING_KEY: OnceLock<DecodingKey> = OnceLock::new();
/// Global reference to the password_changed_at cache (set once from main.rs).
/// AuthGuard/AdminGuard use this to reject tokens issued before a password change.
static PASSWORD_CHANGED_CACHE: OnceLock<Arc<RwLock<HashMap<i32, i64>>>> = OnceLock::new();

pub fn init_encoding_key(secret: &str) {
    let key = EncodingKey::from_secret(secret.as_bytes());
    let _ = ENCODING_KEY.set(key);
    let dk = DecodingKey::from_secret(secret.as_bytes());
    let _ = DECODING_KEY.set(dk);
}

/// Initialize the password change cache reference (called from main.rs).
pub fn init_password_changed_cache(cache: Arc<RwLock<HashMap<i32, i64>>>) {
    let _ = PASSWORD_CHANGED_CACHE.set(cache);
}

/// Update the password_changed_at timestamp for a user (called on password change).
pub fn update_password_changed_at(user_id: i32, timestamp: i64) {
    if let Some(cache) = PASSWORD_CHANGED_CACHE.get()
        && let Ok(mut map) = cache.write()
    {
        map.insert(user_id, timestamp);
    }
}

/// Check if a user JWT's `iat` is after the last password change.
/// Returns true if the token is still valid (not revoked by password change).
fn is_token_valid_after_password_change(user_id: i32, iat: usize) -> bool {
    let Some(cache) = PASSWORD_CHANGED_CACHE.get() else {
        return true; // Cache not initialized — allow (graceful degradation)
    };
    let Ok(map) = cache.read() else {
        return true; // Lock poisoned — allow
    };
    match map.get(&user_id) {
        Some(&changed_at) => (iat as i64) >= changed_at,
        None => true, // No record — user hasn't changed password, allow
    }
}

/// Validate a JWT token passed as a query parameter (for SSE — EventSource cannot set headers).
/// Accepts both agent JWTs (Claims) and user JWTs (UserClaims).
pub fn check_jwt_query(token: &str) -> bool {
    let Some(dk) = DECODING_KEY.get() else {
        return false;
    };
    let mut agent_validation = Validation::new(Algorithm::HS256);
    agent_validation.set_audience(&["agent"]);
    if decode::<Claims>(token, dk, &agent_validation).is_ok() {
        return true;
    }
    // Fallback: accept agent tokens without aud (legacy agents) via permissive validation
    let mut legacy_validation = Validation::new(Algorithm::HS256);
    legacy_validation.validate_aud = false;
    if let Ok(data) = decode::<Claims>(token, dk, &legacy_validation)
        && data.claims.aud.is_empty()
    {
        return true;
    }
    super::user_auth::decode_user_jwt(token).is_some()
}

pub fn generate_jwt() -> Result<String, AppError> {
    let exp = Utc::now().with_timezone(&Seoul).timestamp() as usize + 60;
    let claims = Claims {
        exp,
        aud: "agent".to_string(),
    };
    let key = ENCODING_KEY
        .get()
        .ok_or_else(|| AppError::Internal("JWT encoding key not initialized".into()))?;
    encode(&Header::new(Algorithm::HS256), &claims, key)
        .map_err(|e| AppError::Internal(format!("JWT encoding failed: {e}")))
}

/// Axum extractor that enforces JWT-based authentication:
///
/// - Agent JWT (HS256, 60s expiry): used by agents during scraping.
/// - User JWT (HS256, 24h expiry): contains sub/username/role, used by web dashboard.
///
/// Either JWT type passing is sufficient. Missing or invalid auth returns 401.
pub struct AuthGuard;

impl<S> FromRequestParts<S> for AuthGuard
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

        // Try agent JWT first (with aud: "agent"), then legacy agent (no aud), then user JWT
        let decoding_key = DECODING_KEY
            .get()
            .ok_or_else(|| AppError::Internal("DECODING_KEY not initialized".to_string()))?;

        let mut agent_validation = Validation::new(Algorithm::HS256);
        agent_validation.set_audience(&["agent"]);
        if decode::<Claims>(token, decoding_key, &agent_validation).is_ok() {
            return Ok(AuthGuard);
        }
        // Legacy agent tokens without aud claim
        let mut legacy_validation = Validation::new(Algorithm::HS256);
        legacy_validation.validate_aud = false;
        if let Ok(data) = decode::<Claims>(token, decoding_key, &legacy_validation)
            && data.claims.aud.is_empty()
        {
            return Ok(AuthGuard);
        }

        // User JWT (different claims structure) — also check password revocation
        if let Some(claims) = super::user_auth::decode_user_jwt(token) {
            if !is_token_valid_after_password_change(claims.sub, claims.iat) {
                return Err(AppError::Unauthorized(
                    "Token revoked (password changed)".to_string(),
                ));
            }
            return Ok(AuthGuard);
        }

        Err(AppError::Unauthorized(
            "Invalid or expired token".to_string(),
        ))
    }
}

/// Axum extractor that enforces admin-only access.
/// Only user JWTs with role == "admin" are accepted. Agent JWTs are rejected.
pub struct AdminGuard;

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

        if !is_token_valid_after_password_change(claims.sub, claims.iat) {
            return Err(AppError::Unauthorized(
                "Token revoked (password changed)".to_string(),
            ));
        }

        if claims.role != "admin" {
            return Err(AppError::Unauthorized("Admin access required".to_string()));
        }

        Ok(AdminGuard)
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
}
