use std::sync::Arc;

use axum::Json;
use axum::extract::{Path, State};

use crate::errors::AppError;
use crate::models::app_state::AppState;
use crate::repositories::notification_channels_repo::{
    self, CreateChannelRequest, NotificationChannelRow, UpdateChannelRequest,
};
use crate::services::auth::AuthGuard;

/// GET /api/notification-channels — list all notification channels
pub async fn list_channels(
    _auth: AuthGuard,
    State(state): State<Arc<AppState>>,
) -> Result<Json<Vec<NotificationChannelRow>>, AppError> {
    let channels = notification_channels_repo::get_all(&state.db_pool).await?;
    Ok(Json(channels))
}

/// POST /api/notification-channels — create a new notification channel
pub async fn create_channel(
    _auth: AuthGuard,
    State(state): State<Arc<AppState>>,
    Json(body): Json<CreateChannelRequest>,
) -> Result<Json<NotificationChannelRow>, AppError> {
    if !matches!(body.channel_type.as_str(), "discord" | "slack" | "email") {
        return Err(AppError::BadRequest(format!(
            "Unsupported channel_type: {} (must be 'discord', 'slack', or 'email')",
            body.channel_type
        )));
    }
    let channel = notification_channels_repo::create_channel(&state.db_pool, &body).await?;
    tracing::info!(id = channel.id, channel_type = %body.channel_type, "🔔 [Notification] Channel created");
    Ok(Json(channel))
}

/// PUT /api/notification-channels/{id} — update a notification channel
pub async fn update_channel(
    _auth: AuthGuard,
    State(state): State<Arc<AppState>>,
    Path(id): Path<i32>,
    Json(body): Json<UpdateChannelRequest>,
) -> Result<Json<NotificationChannelRow>, AppError> {
    let channel = notification_channels_repo::update_channel(&state.db_pool, id, &body)
        .await?
        .ok_or_else(|| AppError::NotFound(format!("Notification channel {} not found", id)))?;
    tracing::info!(id = id, "🔔 [Notification] Channel updated");
    Ok(Json(channel))
}

/// DELETE /api/notification-channels/{id} — delete a notification channel
pub async fn delete_channel(
    _auth: AuthGuard,
    State(state): State<Arc<AppState>>,
    Path(id): Path<i32>,
) -> Result<Json<serde_json::Value>, AppError> {
    let deleted = notification_channels_repo::delete_channel(&state.db_pool, id).await?;
    if !deleted {
        return Err(AppError::NotFound(format!(
            "Notification channel {} not found",
            id
        )));
    }
    tracing::info!(id = id, "🔔 [Notification] Channel deleted");
    Ok(Json(serde_json::json!({ "deleted": id })))
}

/// POST /api/notification-channels/{id}/test — send a test message
pub async fn test_channel(
    _auth: AuthGuard,
    State(state): State<Arc<AppState>>,
    Path(id): Path<i32>,
) -> Result<Json<serde_json::Value>, AppError> {
    let channels = notification_channels_repo::get_all(&state.db_pool).await?;
    let channel = channels
        .into_iter()
        .find(|c| c.id == id)
        .ok_or_else(|| AppError::NotFound(format!("Notification channel {} not found", id)))?;

    crate::services::alert_service::test_channel(&state.http_client, &channel)
        .await
        .map_err(AppError::BadRequest)?;

    Ok(Json(serde_json::json!({ "success": true })))
}
