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
    // The claim is unauthenticated apart from the token, and `host_key` is
    // where the hub will send a request every scrape interval. Addresses that
    // only make sense as "the hub itself" or cloud metadata are never a real
    // agent; an installer that could not detect its LAN IP also lands here.
    if host_key_is_local_only(&body.host_key) {
        return Err(AppError::BadRequest(
            "host_key must be an address the hub can reach this agent on, not a \
             loopback, link-local or unspecified address. Re-run the installer \
             with --bind <reachable-ip>."
                .to_string(),
        ));
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

/// True when the host part of `host_key` is loopback, link-local or
/// unspecified (or the name `localhost`).
fn host_key_is_local_only(host_key: &str) -> bool {
    let host_key = host_key.trim();
    let ip = match host_key.parse::<std::net::SocketAddr>() {
        Ok(addr) => addr.ip(),
        Err(_) => {
            let host = host_key.rsplit_once(':').map_or(host_key, |(h, _)| h);
            let host = host.trim_end_matches('.');
            return host.eq_ignore_ascii_case("localhost")
                || host.to_ascii_lowercase().ends_with(".localhost");
        }
    };
    match ip {
        std::net::IpAddr::V4(v4) => v4.is_loopback() || v4.is_link_local() || v4.is_unspecified(),
        std::net::IpAddr::V6(v6) => {
            if let Some(mapped) = v6.to_ipv4_mapped() {
                return mapped.is_loopback() || mapped.is_link_local() || mapped.is_unspecified();
            }
            v6.is_loopback() || v6.is_unspecified() || (v6.segments()[0] & 0xffc0) == 0xfe80
        }
    }
}

#[cfg(test)]
mod tests {
    use super::host_key_is_local_only;

    #[test]
    fn rejects_addresses_that_only_reach_the_hub_itself() {
        for key in [
            "127.0.0.1:3000",
            "127.8.8.8:9101",
            "169.254.169.254:80",
            "0.0.0.0:9101",
            "[::1]:9101",
            "[::ffff:127.0.0.1]:9101",
            "[fe80::1]:9101",
            "localhost:9101",
            "LocalHost.:9101",
            "db.localhost:9101",
        ] {
            assert!(host_key_is_local_only(key), "{key} should be rejected");
        }
    }

    #[test]
    fn accepts_lan_tailscale_and_named_hosts() {
        for key in [
            "192.168.1.10:9101",
            "10.0.0.5:9101",
            "100.64.0.7:9101",
            "[fd7a:115c:a1e0::1]:9101",
            "nas.lan:9101",
        ] {
            assert!(!host_key_is_local_only(key), "{key} should be accepted");
        }
    }
}
