use argon2::password_hash::rand_core::{OsRng, RngCore};
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::db::DbPool;
use crate::errors::AppError;
use crate::repositories::hosts_repo::{self, HostRow};

const TOKEN_BYTES: usize = 32;
const AGENT_SECRET_BYTES: usize = 32;
const DEFAULT_TTL_SECS: i64 = 15 * 60;
const MIN_TTL_SECS: i64 = 60;
const MAX_TTL_SECS: i64 = 24 * 60 * 60;

#[derive(Debug, Deserialize)]
pub struct CreateEnrollmentRequest {
    pub label: Option<String>,
    pub ttl_secs: Option<i64>,
}

#[derive(Debug, Serialize)]
pub struct CreatedEnrollment {
    pub token: String,
    pub expires_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize)]
pub struct ClaimEnrollmentRequest {
    pub token: String,
    pub host_key: String,
    pub display_name: Option<String>,
    pub network_mode: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct ClaimEnrollmentResponse {
    pub host_key: String,
    pub agent_auth_secret: String,
    pub host: HostRow,
}

fn random_url_secret(prefix: &str, bytes_len: usize) -> String {
    let mut raw = vec![0_u8; bytes_len];
    OsRng.fill_bytes(&mut raw);
    format!("{prefix}{}", URL_SAFE_NO_PAD.encode(raw))
}

fn token_hash(token: &str) -> Vec<u8> {
    Sha256::digest(token.as_bytes()).to_vec()
}

pub async fn create_enrollment(
    pool: &DbPool,
    user_id: i32,
    request: CreateEnrollmentRequest,
) -> Result<CreatedEnrollment, AppError> {
    let ttl_secs = request.ttl_secs.unwrap_or(DEFAULT_TTL_SECS);
    if !(MIN_TTL_SECS..=MAX_TTL_SECS).contains(&ttl_secs) {
        return Err(AppError::BadRequest(format!(
            "ttl_secs must be between {MIN_TTL_SECS} and {MAX_TTL_SECS}"
        )));
    }

    let token = random_url_secret("nsenr_", TOKEN_BYTES);
    let hash = token_hash(&token);
    let expires_at = Utc::now() + chrono::Duration::seconds(ttl_secs);
    sqlx::query(
        r#"
        INSERT INTO agent_enrollment_tokens
            (label, token_hash, expires_at, created_by_user_id)
        VALUES (?1, ?2, ?3, ?4)
        "#,
    )
    .bind(request.label)
    .bind(hash)
    .bind(expires_at.timestamp())
    .bind(user_id)
    .execute(pool)
    .await?;

    Ok(CreatedEnrollment { token, expires_at })
}

pub async fn claim_enrollment(
    pool: &DbPool,
    request: ClaimEnrollmentRequest,
) -> Result<ClaimEnrollmentResponse, AppError> {
    let host_key = request.host_key.trim();
    let display_name = request
        .display_name
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or(host_key);
    let now = Utc::now().timestamp();
    let auth_secret = random_url_secret("nsauth_", AGENT_SECRET_BYTES);
    let default_ports = serde_json::to_string(&vec![80_i32, 443_i32])
        .expect("static default ports always serialize");
    let empty_containers =
        serde_json::to_string(&Vec::<String>::new()).expect("empty vec always serializes");

    let mut tx = pool.begin().await?;

    let consumed_id: Option<i64> = sqlx::query_scalar(
        r#"
        UPDATE agent_enrollment_tokens
        SET used_at = ?2,
            used_by_host_key = ?3
        WHERE token_hash = ?1
          AND used_at IS NULL
          AND expires_at >= ?2
        RETURNING id
        "#,
    )
    .bind(token_hash(request.token.trim()))
    .bind(now)
    .bind(host_key)
    .fetch_optional(&mut *tx)
    .await?;

    if consumed_id.is_none() {
        return Err(AppError::Unauthorized(
            "Enrollment token is invalid, expired, or already used".to_string(),
        ));
    }

    sqlx::query(
        r#"
        INSERT INTO hosts
            (host_key, display_name, scrape_interval_secs, load_threshold,
             ports, containers, agent_auth_secret)
        VALUES (?1, ?2, 10, 4.0, ?3, ?4, ?5)
        ON CONFLICT(host_key) DO UPDATE SET
            display_name = excluded.display_name,
            agent_auth_secret = excluded.agent_auth_secret,
            updated_at = strftime('%s','now')
        "#,
    )
    .bind(host_key)
    .bind(display_name)
    .bind(default_ports)
    .bind(empty_containers)
    .bind(&auth_secret)
    .execute(&mut *tx)
    .await?;

    tx.commit().await?;

    let host = hosts_repo::get_host(pool, host_key)
        .await?
        .ok_or_else(|| AppError::Internal("Claimed host was not persisted".to_string()))?;

    tracing::info!(
        host_key = %host_key,
        network_mode = ?request.network_mode,
        "🪪 [AgentEnrollment] Agent enrollment claimed"
    );

    Ok(ClaimEnrollmentResponse {
        host_key: host_key.to_string(),
        agent_auth_secret: auth_secret,
        host,
    })
}
