use std::path::Path;
use std::process::Command;

use anyhow::{Context, Result};
use tracing::info;

use super::hydrate;
use crate::storage::local::LocalStorage;

/// Resume a session from the CLI. Looks up the session locally first, then
/// falls back to downloading from cloud. Runs `claude --resume` in the current terminal.
pub async fn resume_session(session_id: &str) -> Result<()> {
    let cwd = resolve_session_cwd(session_id).await?;

    // Validate cwd exists on disk
    if !Path::new(&cwd).is_dir() {
        anyhow::bail!(
            "The working directory does not exist on this machine:\n{cwd}\n\n\
             This session was started in a directory that doesn't exist locally."
        );
    }

    // Hydrate session transcript (downloads from cloud if not present locally)
    info!("Hydrating session {session_id} for cwd {cwd}");
    hydrate::hydrate_session(session_id, &cwd).await?;

    // Exec claude --resume in the current terminal
    let status = Command::new("claude")
        .arg("--resume")
        .arg(session_id)
        .current_dir(&cwd)
        .status()
        .context("Failed to run `claude --resume`. Is Claude Code installed?")?;

    if !status.success() {
        std::process::exit(status.code().unwrap_or(1));
    }

    Ok(())
}

/// Resolve the cwd for a session: check local storage first, then fetch from cloud.
async fn resolve_session_cwd(session_id: &str) -> Result<String> {
    // Try local storage first
    let base_dir = LocalStorage::default_base_dir()?;
    let storage = LocalStorage::new(base_dir);

    if let Ok(meta) = storage.read_meta(session_id) {
        if let Some(cwd) = meta.cwd {
            info!("Found session {session_id} locally with cwd {cwd}");
            return Ok(cwd);
        }
    }

    // Fall back to cloud
    info!("Session {session_id} not found locally, fetching from cloud...");

    let token = crate::auth::get_token()
        .await
        .context("Authentication required to look up session")?;

    let site_url = std::env::var("THREADER_CONVEX_SITE_URL")
        .unwrap_or_else(|_| "https://ceaseless-shepherd-756.convex.site".to_string());

    let url = format!("{site_url}/api/sessions/{session_id}");
    let client = reqwest::Client::new();
    let resp = client
        .get(&url)
        .bearer_auth(&token)
        .send()
        .await
        .context("Failed to fetch session from cloud")?;

    if !resp.status().is_success() {
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        anyhow::bail!("Session not found ({status}): {text}");
    }

    let body: serde_json::Value = resp
        .json()
        .await
        .context("Failed to parse session response")?;

    body["session"]["cwd"]
        .as_str()
        .map(|s| s.to_string())
        .context("Session has no working directory (cwd) set")
}
