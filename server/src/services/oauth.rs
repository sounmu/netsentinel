use std::collections::HashSet;

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use jsonwebtoken::{Algorithm, DecodingKey, Validation, decode, decode_header};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use url::Url;

use crate::errors::AppError;
use crate::services::oauth_state_store::{OAuthStateStore, PendingOAuthState, random_url_token};
use crate::services::user_auth::JWT_CLOCK_SKEW_LEEWAY_SECS;

const GOOGLE_AUTH_URL: &str = "https://accounts.google.com/o/oauth2/v2/auth";
const GOOGLE_TOKEN_URL: &str = "https://oauth2.googleapis.com/token";
const GOOGLE_JWKS_URL: &str = "https://www.googleapis.com/oauth2/v3/certs";
const GOOGLE_PROVIDER: &str = "google";

#[derive(Debug, Clone)]
pub struct GoogleOAuthConfig {
    pub enabled: bool,
    pub client_id: String,
    client_secret: String,
    redirect_uri: String,
    admin_emails: HashSet<String>,
    allowed_post_login_origins: HashSet<String>,
    pub bootstrap_first_login_as_admin: bool,
}

#[derive(Debug, Clone)]
pub struct OAuthAuthorize {
    pub authorize_url: String,
}

#[derive(Debug, Clone)]
pub struct GoogleIdentity {
    pub provider: &'static str,
    pub subject: String,
    pub email: String,
    pub display_name: Option<String>,
    pub picture_url: Option<String>,
}

impl GoogleOAuthConfig {
    pub fn from_env() -> anyhow::Result<Self> {
        let client_id = std::env::var("GOOGLE_OAUTH_CLIENT_ID").unwrap_or_default();
        let client_secret = std::env::var("GOOGLE_OAUTH_CLIENT_SECRET").unwrap_or_default();
        let redirect_uri = std::env::var("GOOGLE_OAUTH_REDIRECT_URI").unwrap_or_default();
        let any_configured =
            !client_id.is_empty() || !client_secret.is_empty() || !redirect_uri.is_empty();
        if any_configured
            && (client_id.is_empty() || client_secret.is_empty() || redirect_uri.is_empty())
        {
            anyhow::bail!(
                "Google OAuth is partially configured. Set GOOGLE_OAUTH_CLIENT_ID, \
                 GOOGLE_OAUTH_CLIENT_SECRET, and GOOGLE_OAUTH_REDIRECT_URI together, \
                 or unset all three to use local login only."
            );
        }
        let admin_emails = std::env::var("OAUTH_ADMIN_EMAILS")
            .unwrap_or_default()
            .split(',')
            .map(|email| email.trim().to_ascii_lowercase())
            .filter(|email| !email.is_empty())
            .collect();
        let mut allowed_post_login_origins: HashSet<String> = std::env::var("ALLOWED_ORIGINS")
            .unwrap_or_else(|_| "http://localhost:3001".to_string())
            .split(',')
            .map(|origin| origin.trim().trim_end_matches('/').to_string())
            .filter(|origin| !origin.is_empty())
            .collect();
        if let Some(origin) = origin_from_url(&redirect_uri) {
            allowed_post_login_origins.insert(origin);
        }
        let bootstrap_first_login_as_admin = std::env::var("OAUTH_BOOTSTRAP_FIRST_LOGIN_AS_ADMIN")
            .ok()
            .map(|value| {
                !matches!(
                    value.trim().to_ascii_lowercase().as_str(),
                    "0" | "false" | "no" | "off"
                )
            })
            .unwrap_or(true);

        Ok(Self {
            enabled: any_configured,
            client_id,
            client_secret,
            redirect_uri,
            admin_emails,
            allowed_post_login_origins,
            bootstrap_first_login_as_admin,
        })
    }

    pub fn is_admin_email(&self, email: &str) -> bool {
        self.admin_emails.contains(&email.to_ascii_lowercase())
    }

    pub fn post_login_redirect_for_origin(&self, origin: Option<&str>) -> String {
        let Some(origin) = origin.map(|value| value.trim().trim_end_matches('/')) else {
            return "/".to_string();
        };
        if self.allowed_post_login_origins.contains(origin) {
            return format!("{origin}/");
        }
        tracing::warn!(
            origin,
            "🔐 [OAuth] Ignoring untrusted post-login redirect origin"
        );
        "/".to_string()
    }
}

pub fn build_google_authorize_url(
    config: &GoogleOAuthConfig,
    state_store: &OAuthStateStore,
    post_login_redirect: String,
) -> Result<OAuthAuthorize, AppError> {
    let code_verifier = random_url_token();
    let code_challenge = pkce_s256(&code_verifier);
    let nonce = random_url_token();
    let state = state_store.issue(code_verifier, nonce.clone(), post_login_redirect);

    let mut url = Url::parse(GOOGLE_AUTH_URL)
        .map_err(|e| AppError::Internal(format!("Invalid Google auth URL: {e}")))?;
    url.query_pairs_mut()
        .append_pair("client_id", &config.client_id)
        .append_pair("redirect_uri", &config.redirect_uri)
        .append_pair("response_type", "code")
        .append_pair("scope", "openid email profile")
        .append_pair("state", &state)
        .append_pair("code_challenge", &code_challenge)
        .append_pair("code_challenge_method", "S256")
        .append_pair("nonce", &nonce)
        .append_pair("prompt", "select_account");

    Ok(OAuthAuthorize {
        authorize_url: url.into(),
    })
}

fn origin_from_url(value: &str) -> Option<String> {
    let url = Url::parse(value).ok()?;
    let host = url.host_str()?;
    let mut origin = format!("{}://{}", url.scheme(), host);
    if let Some(port) = url.port() {
        origin.push(':');
        origin.push_str(&port.to_string());
    }
    Some(origin)
}

pub async fn exchange_google_code(
    client: &reqwest::Client,
    config: &GoogleOAuthConfig,
    pending: PendingOAuthState,
    code: &str,
) -> Result<GoogleIdentity, AppError> {
    let token = client
        .post(GOOGLE_TOKEN_URL)
        .form(&[
            ("grant_type", "authorization_code"),
            ("code", code),
            ("redirect_uri", config.redirect_uri.as_str()),
            ("client_id", config.client_id.as_str()),
            ("client_secret", config.client_secret.as_str()),
            ("code_verifier", pending.code_verifier.as_str()),
        ])
        .send()
        .await
        .map_err(|e| AppError::Unauthorized(format!("Google token exchange failed: {e}")))?;

    if !token.status().is_success() {
        tracing::warn!(
            status = %token.status(),
            "🔐 [OAuth] Google token exchange rejected"
        );
        return Err(AppError::Unauthorized(
            "Google OAuth token exchange failed".into(),
        ));
    }

    let token_body = token
        .json::<GoogleTokenResponse>()
        .await
        .map_err(|e| AppError::Unauthorized(format!("Invalid Google token response: {e}")))?;

    verify_google_id_token(client, config, &token_body.id_token, &pending.nonce).await
}

fn pkce_s256(code_verifier: &str) -> String {
    let digest = Sha256::digest(code_verifier.as_bytes());
    URL_SAFE_NO_PAD.encode(digest)
}

async fn verify_google_id_token(
    client: &reqwest::Client,
    config: &GoogleOAuthConfig,
    id_token: &str,
    expected_nonce: &str,
) -> Result<GoogleIdentity, AppError> {
    let header = decode_header(id_token)
        .map_err(|e| AppError::Unauthorized(format!("Invalid Google ID token header: {e}")))?;
    let kid = header
        .kid
        .ok_or_else(|| AppError::Unauthorized("Google ID token missing key id".into()))?;

    let jwks = client
        .get(GOOGLE_JWKS_URL)
        .send()
        .await
        .map_err(|e| AppError::Unauthorized(format!("Google JWKS fetch failed: {e}")))?
        .json::<GoogleJwks>()
        .await
        .map_err(|e| AppError::Unauthorized(format!("Invalid Google JWKS: {e}")))?;

    let jwk = jwks
        .keys
        .into_iter()
        .find(|key| key.kid == kid && key.kty == "RSA")
        .ok_or_else(|| AppError::Unauthorized("No matching Google signing key".into()))?;
    let decoding_key = DecodingKey::from_rsa_components(&jwk.n, &jwk.e)
        .map_err(|e| AppError::Unauthorized(format!("Invalid Google signing key: {e}")))?;

    let mut validation = Validation::new(Algorithm::RS256);
    validation.set_audience(&[config.client_id.as_str()]);
    validation.leeway = JWT_CLOCK_SKEW_LEEWAY_SECS;
    let claims = decode::<GoogleIdClaims>(id_token, &decoding_key, &validation)
        .map_err(|e| AppError::Unauthorized(format!("Google ID token rejected: {e}")))?
        .claims;

    if claims.iss != "https://accounts.google.com" && claims.iss != "accounts.google.com" {
        return Err(AppError::Unauthorized("Unexpected Google issuer".into()));
    }
    if claims.aud != config.client_id {
        return Err(AppError::Unauthorized("Unexpected Google audience".into()));
    }
    if claims.exp == 0 {
        return Err(AppError::Unauthorized(
            "Google token missing expiration".into(),
        ));
    }
    if claims.nonce.as_deref() != Some(expected_nonce) {
        return Err(AppError::Unauthorized("Google nonce mismatch".into()));
    }
    if !claims.email_verified {
        return Err(AppError::Unauthorized(
            "Google email is not verified".into(),
        ));
    }

    Ok(GoogleIdentity {
        provider: GOOGLE_PROVIDER,
        subject: claims.sub,
        email: claims.email.to_ascii_lowercase(),
        display_name: claims.name,
        picture_url: claims.picture,
    })
}

#[derive(Debug, Deserialize)]
struct GoogleTokenResponse {
    id_token: String,
}

#[derive(Debug, Deserialize)]
struct GoogleJwks {
    keys: Vec<GoogleJwk>,
}

#[derive(Debug, Deserialize)]
struct GoogleJwk {
    kid: String,
    kty: String,
    n: String,
    e: String,
}

#[derive(Debug, Deserialize)]
struct GoogleIdClaims {
    iss: String,
    aud: String,
    sub: String,
    email: String,
    email_verified: bool,
    exp: usize,
    nonce: Option<String>,
    name: Option<String>,
    picture: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::oauth_state_store::OAuthStateStore;

    #[test]
    fn authorize_url_contains_pkce_and_nonce() {
        let config = GoogleOAuthConfig {
            enabled: true,
            client_id: "client-id".to_string(),
            client_secret: "secret".to_string(),
            redirect_uri: "https://example.com/api/auth/oauth/google/callback".to_string(),
            admin_emails: HashSet::new(),
            allowed_post_login_origins: HashSet::new(),
            bootstrap_first_login_as_admin: true,
        };
        let store = OAuthStateStore::new();

        let auth = build_google_authorize_url(&config, &store, "/".to_string()).unwrap();
        let url = Url::parse(&auth.authorize_url).unwrap();
        let query: HashSet<(String, String)> = url.query_pairs().into_owned().collect();

        assert!(query.contains(&("response_type".to_string(), "code".to_string())));
        assert!(query.contains(&("code_challenge_method".to_string(), "S256".to_string())));
        assert!(
            query
                .iter()
                .any(|(key, value)| key == "state" && !value.is_empty())
        );
        assert!(
            query
                .iter()
                .any(|(key, value)| key == "nonce" && !value.is_empty())
        );
    }
}
