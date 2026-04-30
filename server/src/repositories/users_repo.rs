use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::db::DbPool;

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct UserRow {
    pub id: i32,
    pub oauth_provider: String,
    pub oauth_subject: String,
    pub email: String,
    pub display_name: Option<String>,
    pub picture_url: Option<String>,
    pub role: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Public user info.
#[derive(Debug, Serialize)]
pub struct UserInfo {
    pub id: i32,
    /// Backward-compatible display identifier used by older frontend code.
    /// For OAuth users this is the verified lowercase email address.
    pub username: String,
    pub email: String,
    pub display_name: Option<String>,
    pub picture_url: Option<String>,
    pub role: String,
}

impl From<UserRow> for UserInfo {
    fn from(row: UserRow) -> Self {
        Self {
            id: row.id,
            username: row.email.clone(),
            email: row.email,
            display_name: row.display_name,
            picture_url: row.picture_url,
            role: row.role,
        }
    }
}

pub async fn count_users(pool: &DbPool) -> Result<i64, sqlx::Error> {
    let (count,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM users")
        .fetch_one(pool)
        .await?;
    Ok(count)
}

pub async fn find_by_id(pool: &DbPool, user_id: i32) -> Result<Option<UserRow>, sqlx::Error> {
    sqlx::query_as::<_, UserRow>(
        r#"
        SELECT id, oauth_provider, oauth_subject, email, display_name, picture_url,
               role, created_at, updated_at
        FROM users
        WHERE id = ?1
        "#,
    )
    .bind(user_id)
    .fetch_optional(pool)
    .await
}

/// Create or refresh an OAuth user profile. Role is intentionally supplied by
/// the caller after applying the bootstrap/admin-email policy.
pub async fn upsert_oauth_user<'e, E: sqlx::Executor<'e, Database = sqlx::Sqlite>>(
    executor: E,
    provider: &str,
    subject: &str,
    email: &str,
    display_name: Option<&str>,
    picture_url: Option<&str>,
    role: &str,
) -> Result<UserRow, sqlx::Error> {
    sqlx::query_as::<_, UserRow>(
        r#"
        INSERT INTO users (oauth_provider, oauth_subject, email, display_name, picture_url, role)
        VALUES (?1, ?2, ?3, ?4, ?5, ?6)
        ON CONFLICT(oauth_provider, oauth_subject) DO UPDATE SET
            email = excluded.email,
            display_name = excluded.display_name,
            picture_url = excluded.picture_url,
            role = excluded.role,
            updated_at = strftime('%s','now')
        RETURNING id, oauth_provider, oauth_subject, email, display_name, picture_url,
                  role, created_at, updated_at
        "#,
    )
    .bind(provider)
    .bind(subject)
    .bind(email)
    .bind(display_name)
    .bind(picture_url)
    .bind(role)
    .fetch_one(executor)
    .await
}

/// Stamp `tokens_revoked_at = now` for a user. Called on logout and admin
/// session-kill. Any JWT whose `iat` predates this row is invalidated.
pub async fn revoke_user_tokens(pool: &DbPool, user_id: i32) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE users SET tokens_revoked_at = strftime('%s','now'), \
         updated_at = strftime('%s','now') WHERE id = ?1",
    )
    .bind(user_id)
    .execute(pool)
    .await?;
    Ok(())
}

/// Load tokens_revoked_at timestamps for users who have one set (startup cache).
/// Users who have never had a revocation return no row, so the map is sparse.
pub async fn load_tokens_revoked_at(
    pool: &DbPool,
) -> Result<std::collections::HashMap<i32, i64>, sqlx::Error> {
    let rows: Vec<(i32, i64)> = sqlx::query_as(
        "SELECT id, tokens_revoked_at FROM users WHERE tokens_revoked_at IS NOT NULL",
    )
    .fetch_all(pool)
    .await?;
    Ok(rows.into_iter().collect())
}

#[cfg(test)]
mod sqlite_tests {
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

    #[tokio::test]
    async fn upsert_then_find_roundtrips_user_row() {
        let pool = fresh_pool().await;

        assert_eq!(count_users(&pool).await.unwrap(), 0);

        let user = upsert_oauth_user(
            &pool,
            "google",
            "sub-1",
            "alice@example.com",
            Some("Alice"),
            Some("https://example.com/alice.png"),
            "admin",
        )
        .await
        .unwrap();
        assert_eq!(user.email, "alice@example.com");
        assert_eq!(user.display_name.as_deref(), Some("Alice"));
        assert_eq!(user.role, "admin");
        assert!(user.id >= 1);

        assert_eq!(count_users(&pool).await.unwrap(), 1);

        let by_id = find_by_id(&pool, user.id).await.unwrap().unwrap();
        assert_eq!(by_id.email, "alice@example.com");
        assert_eq!(
            by_id.picture_url.as_deref(),
            Some("https://example.com/alice.png")
        );

        let updated = upsert_oauth_user(
            &pool,
            "google",
            "sub-1",
            "alice.renamed@example.com",
            Some("Alice Renamed"),
            None,
            "admin",
        )
        .await
        .unwrap();
        assert_eq!(updated.id, user.id);
        assert_eq!(updated.email, "alice.renamed@example.com");
        assert_eq!(count_users(&pool).await.unwrap(), 1);
    }

    #[tokio::test]
    async fn revocation_timestamps_load_back_as_maps() {
        let pool = fresh_pool().await;
        let a = upsert_oauth_user(&pool, "google", "a", "a@example.com", None, None, "admin")
            .await
            .unwrap();
        let b = upsert_oauth_user(&pool, "google", "b", "b@example.com", None, None, "viewer")
            .await
            .unwrap();

        // No revocations yet — map should be empty.
        assert!(load_tokens_revoked_at(&pool).await.unwrap().is_empty());

        revoke_user_tokens(&pool, a.id).await.unwrap();
        revoke_user_tokens(&pool, b.id).await.unwrap();

        let revoked = load_tokens_revoked_at(&pool).await.unwrap();
        assert_eq!(revoked.len(), 2);
        assert!(revoked.contains_key(&a.id));
        assert!(revoked.contains_key(&b.id));
    }
}
