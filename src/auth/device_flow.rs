use serde::Deserialize;
use tracing::{info, warn};

use super::{AuthError, Credentials, CLIENT_ID};

#[derive(Debug, Deserialize)]
struct DeviceAuthResponse {
    device_code: String,
    user_code: String,
    verification_uri: String,
    verification_uri_complete: Option<String>,
    expires_in: u64,
    interval: u64,
}

#[derive(Debug, Deserialize)]
pub struct AuthResponse {
    pub access_token: String,
    pub refresh_token: String,
    pub user: AuthUser,
}

#[derive(Debug, Deserialize)]
pub struct AuthUser {
    pub id: String,
    pub email: Option<String>,
    pub first_name: Option<String>,
    pub last_name: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ErrorResponse {
    error: String,
}

pub async fn login() -> Result<Credentials, AuthError> {
    let client = reqwest::Client::new();

    // Step 1: Initiate device flow
    let resp = client
        .post("https://api.workos.com/user_management/authorize/device")
        .form(&[("client_id", CLIENT_ID), ("screen_hint", "sign-up")])
        .send()
        .await?;

    if !resp.status().is_success() {
        let text = resp.text().await.unwrap_or_default();
        return Err(AuthError::RefreshFailed(format!(
            "failed to initiate device flow: {text}"
        )));
    }

    let device: DeviceAuthResponse = resp.json().await?;

    // Step 2: Display code and open browser
    println!();
    println!("  Open this URL: {}", device.verification_uri);
    println!("  Enter code:    {}", device.user_code);
    println!();

    let open_url = device
        .verification_uri_complete
        .as_deref()
        .unwrap_or(&device.verification_uri);
    if open::that(open_url).is_err() {
        info!("could not open browser automatically");
    }

    // Step 3: Poll for completion
    let mut interval = device.interval;
    let deadline = tokio::time::Instant::now()
        + tokio::time::Duration::from_secs(device.expires_in);

    loop {
        tokio::time::sleep(tokio::time::Duration::from_secs(interval)).await;

        if tokio::time::Instant::now() >= deadline {
            return Err(AuthError::Expired);
        }

        let resp = client
            .post("https://api.workos.com/user_management/authenticate")
            .form(&[
                (
                    "grant_type",
                    "urn:ietf:params:oauth:grant-type:device_code",
                ),
                ("device_code", &device.device_code),
                ("client_id", CLIENT_ID),
            ])
            .send()
            .await?;

        let status = resp.status();
        let body = resp.text().await?;

        if status.is_success() {
            let auth: AuthResponse = serde_json::from_str(&body)
                .map_err(|e| AuthError::RefreshFailed(e.to_string()))?;

            let creds = Credentials {
                access_token: auth.access_token.clone(),
                refresh_token: Some(auth.refresh_token),
                expires_at: Some(super::parse_jwt_expiry(&auth.access_token)),
                user_id: auth.user.id,
                email: auth.user.email,
            };

            super::storage::store(&creds)
                .map_err(|e| AuthError::Storage(e.to_string()))?;

            return Ok(creds);
        }

        // Parse error response
        let err: ErrorResponse = match serde_json::from_str(&body) {
            Ok(e) => e,
            Err(_) => {
                warn!("unexpected poll response: {body}");
                continue;
            }
        };

        match err.error.as_str() {
            "authorization_pending" => continue,
            "slow_down" => {
                interval += 5;
                continue;
            }
            "access_denied" => return Err(AuthError::AccessDenied),
            "expired_token" => return Err(AuthError::Expired),
            other => {
                warn!("unexpected error during poll: {other}");
                continue;
            }
        }
    }
}
