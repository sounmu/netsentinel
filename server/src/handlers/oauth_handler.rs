use std::net::SocketAddr;
use std::sync::Arc;

use axum::Json;
use axum::extract::{ConnectInfo, Query, State};
use axum::http::header::{AUTHORIZATION, HeaderValue, LOCATION, ORIGIN};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use serde::Deserialize;

use crate::errors::AppError;
use crate::handlers::auth_handler::{extract_client_ip, issue_session_response};
use crate::models::app_state::AppState;
use crate::repositories::users_repo;
use crate::services::auth;
use crate::services::oauth;
use crate::services::user_auth;

#[derive(serde::Serialize)]
pub struct OAuthStartResponse {
    pub authorize_url: String,
}

#[derive(Debug, Deserialize)]
pub struct OAuthCallbackQuery {
    code: Option<String>,
    state: Option<String>,
    error: Option<String>,
}

/// GET /api/auth/oauth/google/start — create OAuth state + PKCE and return
/// the Google authorization URL. The browser redirects to this URL.
pub async fn google_start(
    State(state): State<Arc<AppState>>,
    ConnectInfo(peer_addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
) -> Result<Json<OAuthStartResponse>, AppError> {
    if !state.google_oauth.enabled {
        return Err(AppError::BadRequest(
            "Google OAuth is not configured".into(),
        ));
    }
    let ip_str = extract_client_ip(&headers, &peer_addr, state.trusted_proxy_count);
    if let Err(retry_after) = state.login_rate_limiter.check(&ip_str) {
        tracing::warn!(ip = %ip_str, "🔒 [OAuth] Start rate limited");
        return Err(AppError::TooManyRequests(format!(
            "Too many login attempts. Try again in {retry_after} seconds."
        )));
    }

    let origin = headers.get(ORIGIN).and_then(|value| value.to_str().ok());
    let post_login_redirect = state.google_oauth.post_login_redirect_for_origin(origin);
    let link_user_id = optional_user_id_from_authorization(&headers)?;
    let authorize = oauth::build_google_authorize_url(
        &state.google_oauth,
        &state.oauth_state_store,
        post_login_redirect,
        link_user_id,
    )?;
    Ok(Json(OAuthStartResponse {
        authorize_url: authorize.authorize_url,
    }))
}

/// GET /api/auth/oauth/google/callback — verify Google identity, sign in only
/// if that Google subject is already linked, or link it when the OAuth state
/// was issued from an authenticated local session.
pub async fn google_callback(
    State(state): State<Arc<AppState>>,
    ConnectInfo(peer_addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Query(query): Query<OAuthCallbackQuery>,
) -> Result<Response, AppError> {
    if !state.google_oauth.enabled {
        return redirect("/login?error=oauth");
    }
    if query.error.is_some() {
        return redirect("/login?error=oauth");
    }

    let ip_str = extract_client_ip(&headers, &peer_addr, state.trusted_proxy_count);
    if let Err(retry_after) = state.login_rate_limiter.check(&ip_str) {
        tracing::warn!(
            ip = %ip_str,
            retry_after,
            "🔒 [OAuth] Callback rate limited"
        );
        return redirect("/login?error=rate_limited");
    }

    let code = match query.code.as_deref().filter(|code| !code.is_empty()) {
        Some(code) => code,
        None => return redirect("/login?error=oauth"),
    };
    let state_param = match query.state.as_deref().filter(|state| !state.is_empty()) {
        Some(state) => state,
        None => return redirect("/login?error=oauth"),
    };
    let Some(pending) = state.oauth_state_store.consume(state_param) else {
        tracing::warn!("🔐 [OAuth] Missing or expired OAuth state");
        return redirect("/login?error=oauth");
    };
    let post_login_redirect = pending.post_login_redirect.clone();
    let link_user_id = pending.link_user_id;

    let identity =
        match oauth::exchange_google_code(&state.http_client, &state.google_oauth, pending, code)
            .await
        {
            Ok(identity) => identity,
            Err(err) => {
                tracing::warn!(err = ?err, "🔐 [OAuth] Google identity verification failed");
                return redirect_owned(login_redirect(&post_login_redirect, "oauth"));
            }
        };

    let existing_oauth =
        users_repo::find_by_oauth_subject(&state.db_pool, identity.provider, &identity.subject)
            .await?;
    let user = if let Some(existing) = existing_oauth {
        if let Some(link_user_id) = link_user_id
            && existing.id != link_user_id
        {
            tracing::warn!(
                user_id = link_user_id,
                linked_user_id = existing.id,
                "🔐 [OAuth] Rejected link: Google subject is already linked to another user"
            );
            return redirect_owned(login_redirect(&post_login_redirect, "oauth_conflict"));
        }
        existing
    } else if let Some(link_user_id) = link_user_id {
        let existing_user = users_repo::find_by_id(&state.db_pool, link_user_id)
            .await?
            .ok_or_else(|| AppError::Unauthorized("User no longer exists".to_string()))?;
        if existing_user.oauth_subject.is_some() {
            tracing::warn!(
                user_id = link_user_id,
                "🔐 [OAuth] Rejected link: user already has a Google subject"
            );
            return redirect_owned(login_redirect(&post_login_redirect, "oauth_conflict"));
        }
        users_repo::link_google_user(
            &state.db_pool,
            existing_user.id,
            users_repo::GoogleLink {
                provider: identity.provider,
                subject: &identity.subject,
                email: &identity.email,
                display_name: identity.display_name.as_deref(),
                picture_url: identity.picture_url.as_deref(),
                role: existing_user.role.as_str(),
            },
        )
        .await?
    } else {
        let _guard = state.oauth_bootstrap_lock.lock().await;
        let existing_count = users_repo::count_users(&state.db_pool).await?;
        let bootstrap_admin = existing_count == 0
            && state.google_oauth.bootstrap_first_login_as_admin
            && state
                .google_oauth
                .is_bootstrap_email_allowed(&identity.email);
        if !bootstrap_admin {
            tracing::warn!(
                email = %identity.email,
                "🔐 [OAuth] Rejected Google login: account is not linked"
            );
            return redirect_owned(login_redirect(&post_login_redirect, "not_linked"));
        }

        users_repo::upsert_oauth_user(
            &state.db_pool,
            identity.provider,
            &identity.subject,
            &identity.email,
            identity.display_name.as_deref(),
            identity.picture_url.as_deref(),
            "admin",
        )
        .await?
    };

    let mut response = issue_session_response(&state, user, &headers, &ip_str).await?;
    *response.status_mut() = StatusCode::FOUND;
    response.headers_mut().insert(
        LOCATION,
        HeaderValue::from_str(&post_login_redirect)
            .map_err(|e| AppError::Internal(format!("Invalid redirect header: {e}")))?,
    );
    Ok(response)
}

fn optional_user_id_from_authorization(headers: &HeaderMap) -> Result<Option<i32>, AppError> {
    let Some(value) = headers.get(AUTHORIZATION) else {
        return Ok(None);
    };
    let header = value
        .to_str()
        .map_err(|_| AppError::Unauthorized("Invalid Authorization header".to_string()))?;
    let token = header.strip_prefix("Bearer ").ok_or_else(|| {
        AppError::Unauthorized("Authorization header must use Bearer scheme".to_string())
    })?;
    let claims = user_auth::decode_user_jwt(token)
        .ok_or_else(|| AppError::Unauthorized("Invalid or expired token".to_string()))?;
    if !auth::is_token_iat_still_valid(claims.sub, claims.iat) {
        return Err(AppError::Unauthorized("Token revoked".to_string()));
    }
    Ok(Some(claims.sub))
}

fn redirect(location: &'static str) -> Result<Response, AppError> {
    Ok((StatusCode::FOUND, [(LOCATION, location)]).into_response())
}

fn redirect_owned(location: String) -> Result<Response, AppError> {
    let mut response = StatusCode::FOUND.into_response();
    response.headers_mut().insert(
        LOCATION,
        HeaderValue::from_str(&location)
            .map_err(|e| AppError::Internal(format!("Invalid redirect header: {e}")))?,
    );
    Ok(response)
}

fn login_redirect(base: &str, error: &str) -> String {
    if base == "/" {
        return format!("/login?error={error}");
    }
    let base = base.trim_end_matches('/');
    format!("{base}/login?error={error}")
}
