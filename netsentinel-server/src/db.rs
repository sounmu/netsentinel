//! SQLite database entry point.
//!
//! The process opens exactly one SQLite pool and runs the embedded
//! migrations against it. `main.rs` calls `connect()` and
//! `run_migrations()` and stores the result on `AppState` through
//! the `DbPool` alias. Retiring the Postgres path collapses a dozen
//! branching `#[cfg]` blocks per repo — the single-backend tree is
//! strictly easier to reason about and is what self-hosters actually
//! deploy.

use anyhow::Context;

/// Pool alias reserved for the day we ever grow a second backend —
/// today it is always a SQLite pool. Downstream modules import
/// `crate::db::DbPool` rather than `sqlx::SqlitePool` so future
/// swaps remain a one-line change in this file.
pub type DbPool = sqlx::SqlitePool;

/// Open the SQLite database, applying the pragma set chosen in
/// docs/SQLITE_MIGRATION.md §3. The file is created if missing —
/// first-run UX assumes an operator runs the binary against an
/// empty path and expects it to "just work".
pub async fn connect(
    database_url: &str,
    max_connections: u32,
    _statement_timeout_secs: u64,
) -> anyhow::Result<DbPool> {
    use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
    use std::str::FromStr;

    let connect_options = SqliteConnectOptions::from_str(database_url)
        .context("Invalid DATABASE_URL — SQLite expects `sqlite://path` or `sqlite::memory:`")?
        .create_if_missing(true)
        .foreign_keys(true)
        .journal_mode(sqlx::sqlite::SqliteJournalMode::Wal)
        .synchronous(sqlx::sqlite::SqliteSynchronous::Normal)
        .busy_timeout(std::time::Duration::from_secs(5))
        // 256 MiB mmap, 64 MiB page cache — see §3.
        .pragma("mmap_size", "268435456")
        .pragma("cache_size", "-65536")
        .pragma("temp_store", "MEMORY")
        .pragma("wal_autocheckpoint", "1000");

    let pool = SqlitePoolOptions::new()
        .max_connections(max_connections)
        .min_connections(1)
        .acquire_timeout(std::time::Duration::from_secs(5))
        .connect_with(connect_options)
        .await
        .context("Failed to open SQLite database")?;

    tracing::info!("✅ [DB] Connected to SQLite (WAL mode).");
    Ok(pool)
}

pub async fn run_migrations(pool: &DbPool) -> anyhow::Result<()> {
    sqlx::migrate!("./migrations")
        .run(pool)
        .await
        .context("Failed to run SQLite migrations")?;
    tracing::info!("✅ [DB] SQLite migrations applied.");
    Ok(())
}
