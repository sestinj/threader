pub mod device_flow;
pub mod storage;

use chrono::{DateTime, TimeZone, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Credentials {
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub expires_at: Option<DateTime<Utc>>,
    pub user_id: String,
    pub email: Option<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum AuthError {
    #[error("not logged in")]
    NotLoggedIn,
    #[error("device flow expired")]
    Expired,
    #[error("access denied")]
    AccessDenied,
    #[error("token refresh failed: {0}")]
    RefreshFailed(String),
    #[error("storage error: {0}")]
    Storage(String),
    #[error("http error: {0}")]
    Http(#[from] reqwest::Error),
}

const CLIENT_ID: &str = "client_01KFE40Z1FZ1NJQKHTNNPPWZ3C";

/// Get a valid access token, refreshing if needed.
pub async fn get_token() -> Result<String, AuthError> {
    let creds = storage::load().map_err(|e| AuthError::Storage(e.to_string()))?;
    let creds = creds.ok_or(AuthError::NotLoggedIn)?;

    // Check actual JWT expiry (more reliable than stored expires_at which
    // may be wrong from older versions)
    let jwt_exp = parse_jwt_expiry(&creds.access_token);
    let expired = Utc::now() >= jwt_exp;

    if !expired {
        return Ok(creds.access_token);
    }

    // Token expired, try refresh
    if let Some(ref refresh_token) = creds.refresh_token {
        return refresh(refresh_token).await;
    }
    Err(AuthError::NotLoggedIn)
}

async fn refresh(refresh_token: &str) -> Result<String, AuthError> {
    let client = reqwest::Client::new();
    let resp = client
        .post("https://api.workos.com/user_management/authenticate")
        .form(&[
            ("grant_type", "refresh_token"),
            ("client_id", CLIENT_ID),
            ("refresh_token", refresh_token),
        ])
        .send()
        .await?;

    if !resp.status().is_success() {
        let text = resp.text().await.unwrap_or_default();
        return Err(AuthError::RefreshFailed(text));
    }

    let auth_resp: device_flow::AuthResponse = resp.json().await?;
    let creds = Credentials {
        access_token: auth_resp.access_token.clone(),
        refresh_token: Some(auth_resp.refresh_token),
        expires_at: Some(parse_jwt_expiry(&auth_resp.access_token)),
        user_id: auth_resp.user.id,
        email: auth_resp.user.email,
    };
    storage::store(&creds).map_err(|e| AuthError::Storage(e.to_string()))?;
    Ok(creds.access_token)
}

pub fn logout() -> Result<(), AuthError> {
    storage::delete().map_err(|e| AuthError::Storage(e.to_string()))
}

/// Extract the `exp` claim from a JWT without verifying the signature.
/// Falls back to now + 4 minutes if parsing fails.
pub fn parse_jwt_expiry(token: &str) -> DateTime<Utc> {
    let fallback = Utc::now() + chrono::Duration::minutes(4);

    let parts: Vec<&str> = token.split('.').collect();
    if parts.len() != 3 {
        return fallback;
    }

    use base64::Engine;
    let engine = base64::engine::general_purpose::URL_SAFE_NO_PAD;
    let payload = match engine.decode(parts[1]) {
        Ok(p) => p,
        Err(_) => return fallback,
    };

    #[derive(Deserialize)]
    struct Claims {
        exp: Option<i64>,
    }

    match serde_json::from_slice::<Claims>(&payload) {
        Ok(claims) => claims
            .exp
            .and_then(|exp| Utc.timestamp_opt(exp, 0).single())
            .unwrap_or(fallback),
        Err(_) => fallback,
    }
}
