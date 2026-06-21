use std::sync::Arc;

use axum::Json;
use axum::extract::State;

use crate::errors::AppError;
use crate::handlers::hosts_handler::{MAX_KEY_LEN, MAX_NAME_LEN, validate_host_key_format};
use crate::models::app_state::AppState;
use crate::services::agent_enrollment::{
    ClaimEnrollmentRequest, ClaimEnrollmentResponse, CreateEnrollmentRequest, CreatedEnrollment,
};
use crate::services::auth::AdminGuard;
use crate::services::{agent_enrollment, hosts_snapshot};

/// POST /api/agent-enrollments — create a short-lived install token.
pub async fn create_enrollment(
    admin: AdminGuard,
    State(state): State<Arc<AppState>>,
    Json(body): Json<CreateEnrollmentRequest>,
) -> Result<Json<CreatedEnrollment>, AppError> {
    let enrollment =
        agent_enrollment::create_enrollment(&state.db_pool, admin.claims.sub, body).await?;
    Ok(Json(enrollment))
}

/// POST /api/agent-enrollments/claim — installer exchanges token for host registration.
pub async fn claim_enrollment(
    State(state): State<Arc<AppState>>,
    Json(body): Json<ClaimEnrollmentRequest>,
) -> Result<Json<ClaimEnrollmentResponse>, AppError> {
    if body.host_key.trim().is_empty() {
        return Err(AppError::BadRequest(
            "host_key must not be empty".to_string(),
        ));
    }
    validate_host_key_format(&body.host_key)?;
    if body.host_key.len() > MAX_KEY_LEN {
        return Err(AppError::BadRequest(format!(
            "host_key must not exceed {} characters",
            MAX_KEY_LEN
        )));
    }
    if let Some(display_name) = body.display_name.as_deref()
        && display_name.len() > MAX_NAME_LEN
    {
        return Err(AppError::BadRequest(format!(
            "display_name must not exceed {} characters",
            MAX_NAME_LEN
        )));
    }

    let claimed = agent_enrollment::claim_enrollment(&state.db_pool, body).await?;
    state.pre_populate_status(std::slice::from_ref(&claimed.host));
    hosts_snapshot::refresh(&state.db_pool, &state.hosts_snapshot).await;
    Ok(Json(claimed))
}
