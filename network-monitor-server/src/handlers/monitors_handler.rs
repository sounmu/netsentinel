use std::sync::Arc;

use axum::Json;
use axum::extract::{Path, Query, State};
use serde::Deserialize;

use crate::errors::AppError;
use crate::models::app_state::AppState;
use crate::repositories::{http_monitors_repo, ping_monitors_repo};
use crate::services::auth::AuthGuard;

// ──────────────────────────────────────────────
// HTTP Monitors
// ──────────────────────────────────────────────

/// GET /api/http-monitors
pub async fn list_http_monitors(
    _auth: AuthGuard,
    State(state): State<Arc<AppState>>,
) -> Result<Json<Vec<http_monitors_repo::HttpMonitor>>, AppError> {
    let monitors = http_monitors_repo::get_all(&state.db_pool).await?;
    Ok(Json(monitors))
}

/// POST /api/http-monitors
pub async fn create_http_monitor(
    _auth: AuthGuard,
    State(state): State<Arc<AppState>>,
    Json(body): Json<http_monitors_repo::CreateHttpMonitorRequest>,
) -> Result<Json<http_monitors_repo::HttpMonitor>, AppError> {
    if body.url.is_empty() {
        return Err(AppError::BadRequest("URL is required".to_string()));
    }
    let monitor = http_monitors_repo::create(&state.db_pool, &body).await?;
    tracing::info!(id = monitor.id, url = %monitor.url, "🌐 [HTTP Monitor] Created");
    Ok(Json(monitor))
}

/// PUT /api/http-monitors/{id}
pub async fn update_http_monitor(
    _auth: AuthGuard,
    State(state): State<Arc<AppState>>,
    Path(id): Path<i32>,
    Json(body): Json<http_monitors_repo::UpdateHttpMonitorRequest>,
) -> Result<Json<http_monitors_repo::HttpMonitor>, AppError> {
    let monitor = http_monitors_repo::update(&state.db_pool, id, &body)
        .await?
        .ok_or_else(|| AppError::NotFound(format!("HTTP monitor {} not found", id)))?;
    Ok(Json(monitor))
}

/// DELETE /api/http-monitors/{id}
pub async fn delete_http_monitor(
    _auth: AuthGuard,
    State(state): State<Arc<AppState>>,
    Path(id): Path<i32>,
) -> Result<Json<serde_json::Value>, AppError> {
    let deleted = http_monitors_repo::delete(&state.db_pool, id).await?;
    if !deleted {
        return Err(AppError::NotFound(format!("HTTP monitor {} not found", id)));
    }
    Ok(Json(serde_json::json!({ "deleted": id })))
}

#[derive(Deserialize)]
pub struct ResultsQuery {
    pub limit: Option<i64>,
}

/// GET /api/http-monitors/{id}/results
pub async fn get_http_results(
    _auth: AuthGuard,
    State(state): State<Arc<AppState>>,
    Path(id): Path<i32>,
    Query(query): Query<ResultsQuery>,
) -> Result<Json<Vec<http_monitors_repo::HttpMonitorResult>>, AppError> {
    let limit = query.limit.unwrap_or(50).min(200);
    let results = http_monitors_repo::get_results(&state.db_pool, id, limit).await?;
    Ok(Json(results))
}

/// GET /api/http-monitors/summaries
pub async fn get_http_summaries(
    _auth: AuthGuard,
    State(state): State<Arc<AppState>>,
) -> Result<Json<Vec<http_monitors_repo::HttpMonitorSummary>>, AppError> {
    let summaries = http_monitors_repo::get_summaries(&state.db_pool).await?;
    Ok(Json(summaries))
}

// ──────────────────────────────────────────────
// Ping Monitors
// ──────────────────────────────────────────────

/// GET /api/ping-monitors
pub async fn list_ping_monitors(
    _auth: AuthGuard,
    State(state): State<Arc<AppState>>,
) -> Result<Json<Vec<ping_monitors_repo::PingMonitor>>, AppError> {
    let monitors = ping_monitors_repo::get_all(&state.db_pool).await?;
    Ok(Json(monitors))
}

/// POST /api/ping-monitors
pub async fn create_ping_monitor(
    _auth: AuthGuard,
    State(state): State<Arc<AppState>>,
    Json(body): Json<ping_monitors_repo::CreatePingMonitorRequest>,
) -> Result<Json<ping_monitors_repo::PingMonitor>, AppError> {
    if body.host.is_empty() {
        return Err(AppError::BadRequest("Host is required".to_string()));
    }
    let monitor = ping_monitors_repo::create(&state.db_pool, &body).await?;
    tracing::info!(id = monitor.id, host = %monitor.host, "🏓 [Ping Monitor] Created");
    Ok(Json(monitor))
}

/// PUT /api/ping-monitors/{id}
pub async fn update_ping_monitor(
    _auth: AuthGuard,
    State(state): State<Arc<AppState>>,
    Path(id): Path<i32>,
    Json(body): Json<ping_monitors_repo::UpdatePingMonitorRequest>,
) -> Result<Json<ping_monitors_repo::PingMonitor>, AppError> {
    let monitor = ping_monitors_repo::update(&state.db_pool, id, &body)
        .await?
        .ok_or_else(|| AppError::NotFound(format!("Ping monitor {} not found", id)))?;
    Ok(Json(monitor))
}

/// DELETE /api/ping-monitors/{id}
pub async fn delete_ping_monitor(
    _auth: AuthGuard,
    State(state): State<Arc<AppState>>,
    Path(id): Path<i32>,
) -> Result<Json<serde_json::Value>, AppError> {
    let deleted = ping_monitors_repo::delete(&state.db_pool, id).await?;
    if !deleted {
        return Err(AppError::NotFound(format!("Ping monitor {} not found", id)));
    }
    Ok(Json(serde_json::json!({ "deleted": id })))
}

/// GET /api/ping-monitors/{id}/results
pub async fn get_ping_results(
    _auth: AuthGuard,
    State(state): State<Arc<AppState>>,
    Path(id): Path<i32>,
    Query(query): Query<ResultsQuery>,
) -> Result<Json<Vec<ping_monitors_repo::PingResult>>, AppError> {
    let limit = query.limit.unwrap_or(50).min(200);
    let results = ping_monitors_repo::get_results(&state.db_pool, id, limit).await?;
    Ok(Json(results))
}

/// GET /api/ping-monitors/summaries
pub async fn get_ping_summaries(
    _auth: AuthGuard,
    State(state): State<Arc<AppState>>,
) -> Result<Json<Vec<ping_monitors_repo::PingMonitorSummary>>, AppError> {
    let summaries = ping_monitors_repo::get_summaries(&state.db_pool).await?;
    Ok(Json(summaries))
}
