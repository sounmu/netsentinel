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
    /// Let the claim replace the secret of a host that is already registered
    /// (reinstalling an agent). Off by default so a leaked install command
    /// cannot be used to take over an existing host.
    #[serde(default)]
    pub allow_existing_host: bool,
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
            (label, token_hash, expires_at, created_by_user_id, allow_existing_host)
        VALUES (?1, ?2, ?3, ?4, ?5)
        "#,
    )
    .bind(request.label)
    .bind(hash)
    .bind(expires_at.timestamp())
    .bind(user_id)
    .bind(request.allow_existing_host)
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

    let allow_existing_host: Option<bool> = sqlx::query_scalar(
        r#"
        UPDATE agent_enrollment_tokens
        SET used_at = ?2,
            used_by_host_key = ?3
        WHERE token_hash = ?1
          AND used_at IS NULL
          AND expires_at >= ?2
        RETURNING allow_existing_host
        "#,
    )
    .bind(token_hash(request.token.trim()))
    .bind(now)
    .bind(host_key)
    .fetch_optional(&mut *tx)
    .await?;

    let Some(allow_existing_host) = allow_existing_host else {
        return Err(AppError::Unauthorized(
            "Enrollment token is invalid, expired, or already used".to_string(),
        ));
    };

    // Without this, any unused token could rotate the secret of an arbitrary
    // registered host: the real agent would start failing auth and whoever
    // answers on that address would be trusted instead. Returning here drops
    // the transaction, so the token is not consumed and stays usable.
    if !allow_existing_host {
        let exists: Option<i64> = sqlx::query_scalar("SELECT 1 FROM hosts WHERE host_key = ?1")
            .bind(host_key)
            .fetch_optional(&mut *tx)
            .await?;
        if exists.is_some() {
            return Err(AppError::Conflict(format!(
                "Host {host_key} is already registered. To reinstall its agent, create \
                 the install command with \"Re-enroll an existing host\" enabled."
            )));
        }
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
            agent_signs_responses = 0,
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

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
    use std::str::FromStr;

    async fn fresh_pool() -> DbPool {
        let options = SqliteConnectOptions::from_str("sqlite::memory:")
            .unwrap()
            .foreign_keys(false)
            .journal_mode(sqlx::sqlite::SqliteJournalMode::Memory);
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(options)
            .await
            .unwrap();
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();
        pool
    }

    async fn new_token(pool: &DbPool, allow_existing_host: bool) -> String {
        create_enrollment(
            pool,
            1,
            CreateEnrollmentRequest {
                label: None,
                ttl_secs: None,
                allow_existing_host,
            },
        )
        .await
        .unwrap()
        .token
    }

    fn claim(token: &str, host_key: &str) -> ClaimEnrollmentRequest {
        ClaimEnrollmentRequest {
            token: token.to_string(),
            host_key: host_key.to_string(),
            display_name: Some("box".to_string()),
            network_mode: None,
        }
    }

    #[tokio::test]
    async fn claim_cannot_take_over_an_existing_host_by_default() {
        let pool = fresh_pool().await;
        let first = new_token(&pool, false).await;
        let original = claim_enrollment(&pool, claim(&first, "192.168.1.10:9101"))
            .await
            .unwrap();

        let second = new_token(&pool, false).await;
        let err = claim_enrollment(&pool, claim(&second, "192.168.1.10:9101"))
            .await
            .unwrap_err();
        assert!(matches!(err, AppError::Conflict(_)));

        let host = hosts_repo::get_host(&pool, "192.168.1.10:9101")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            host.agent_auth_secret.as_deref(),
            Some(original.agent_auth_secret.as_str()),
            "the existing secret must be untouched"
        );

        // The rejected claim did not burn the token: it still works for a new host.
        claim_enrollment(&pool, claim(&second, "192.168.1.11:9101"))
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn reenroll_token_rotates_secret_and_clears_signing_pin() {
        let pool = fresh_pool().await;
        let first = new_token(&pool, false).await;
        let original = claim_enrollment(&pool, claim(&first, "192.168.1.10:9101"))
            .await
            .unwrap();
        hosts_repo::mark_agent_signs_responses(&pool, "192.168.1.10:9101")
            .await
            .unwrap();

        let reenroll = new_token(&pool, true).await;
        let replaced = claim_enrollment(&pool, claim(&reenroll, "192.168.1.10:9101"))
            .await
            .unwrap();
        assert_ne!(replaced.agent_auth_secret, original.agent_auth_secret);
        assert!(!replaced.host.agent_signs_responses);
    }

    #[tokio::test]
    async fn token_is_single_use() {
        let pool = fresh_pool().await;
        let token = new_token(&pool, true).await;
        claim_enrollment(&pool, claim(&token, "192.168.1.10:9101"))
            .await
            .unwrap();
        let err = claim_enrollment(&pool, claim(&token, "192.168.1.12:9101"))
            .await
            .unwrap_err();
        assert!(matches!(err, AppError::Unauthorized(_)));
    }
}
