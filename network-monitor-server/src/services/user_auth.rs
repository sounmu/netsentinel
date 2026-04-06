use argon2::password_hash::SaltString;
use argon2::password_hash::rand_core::OsRng;
use argon2::{Argon2, PasswordHash, PasswordHasher, PasswordVerifier};
use jsonwebtoken::{Algorithm, Header, Validation, decode, encode};
use serde::{Deserialize, Serialize};

use super::auth::{DECODING_KEY, ENCODING_KEY};
use crate::errors::AppError;

/// JWT claims for authenticated web users.
/// Distinguished from agent Claims by the presence of `sub` (user ID).
#[derive(Debug, Serialize, Deserialize)]
pub struct UserClaims {
    /// User ID
    pub sub: i32,
    pub username: String,
    pub role: String,
    /// Issued-at (Unix timestamp) — used for token revocation on password change
    #[serde(default)]
    pub iat: usize,
    /// Expiration (Unix timestamp)
    pub exp: usize,
    /// Audience claim — "user" for user tokens (token type separation)
    #[serde(default)]
    pub aud: String,
}

/// Hash a plaintext password with Argon2id
pub fn hash_password(password: &str) -> Result<String, argon2::password_hash::Error> {
    let salt = SaltString::generate(&mut OsRng);
    let argon2 = Argon2::default();
    let hash = argon2.hash_password(password.as_bytes(), &salt)?;
    Ok(hash.to_string())
}

/// Verify a plaintext password against a stored Argon2 hash
pub fn verify_password(password: &str, hash: &str) -> bool {
    let parsed = match PasswordHash::new(hash) {
        Ok(h) => h,
        Err(_) => return false,
    };
    Argon2::default()
        .verify_password(password.as_bytes(), &parsed)
        .is_ok()
}

/// Generate a user JWT (24-hour expiry)
pub fn generate_user_jwt(user_id: i32, username: &str, role: &str) -> Result<String, AppError> {
    let now = chrono::Utc::now().timestamp() as usize;
    let claims = UserClaims {
        sub: user_id,
        username: username.to_string(),
        role: role.to_string(),
        iat: now,
        exp: now + 24 * 60 * 60,
        aud: "user".to_string(),
    };
    let key = ENCODING_KEY
        .get()
        .ok_or_else(|| AppError::Internal("JWT encoding key not initialized".into()))?;
    encode(&Header::new(Algorithm::HS256), &claims, key)
        .map_err(|e| AppError::Internal(format!("JWT encoding failed: {e}")))
}

/// Decode and validate a user JWT, returning claims if valid.
/// Accepts tokens with `aud: "user"` and legacy tokens without `aud` (backward compat).
pub fn decode_user_jwt(token: &str) -> Option<UserClaims> {
    let dk = DECODING_KEY.get()?;
    // Try with aud: "user" first
    let mut user_validation = Validation::new(Algorithm::HS256);
    user_validation.set_audience(&["user"]);
    if let Ok(data) = decode::<UserClaims>(token, dk, &user_validation) {
        return Some(data.claims);
    }
    // Fallback: legacy tokens without aud claim
    let mut legacy_validation = Validation::new(Algorithm::HS256);
    legacy_validation.validate_aud = false;
    decode::<UserClaims>(token, dk, &legacy_validation)
        .ok()
        .filter(|data| data.claims.aud.is_empty()) // Only accept if no aud (legacy)
        .map(|data| data.claims)
}
