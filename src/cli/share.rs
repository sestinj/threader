use anyhow::{bail, Context, Result};

use crate::storage::local::LocalStorage;

pub async fn run(workspace: Option<String>) -> Result<()> {
    let session_id = resolve_current_session()?;

    let token = crate::auth::get_token()
        .await
        .context("Authentication required. Run `threader login`.")?;

    let site_url = std::env::var("THREADER_CONVEX_SITE_URL")
        .unwrap_or_else(|_| "https://ceaseless-shepherd-756.convex.site".to_string());

    let client = reqwest::Client::new();
    let resp = client
        .post(format!("{site_url}/api/sessions/{session_id}/share"))
        .bearer_auth(&token)
        .json(&if let Some(ref ws) = workspace {
            serde_json::json!({ "workspace": ws })
        } else {
            serde_json::json!({ "visibility": "public" })
        })
        .send()
        .await
        .context("Failed to reach Threader cloud")?;

    if !resp.status().is_success() {
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        bail!("Failed to share session ({status}): {text}");
    }

    let body: serde_json::Value = resp.json().await?;
    let slug = body["session"]["shareSlug"]
        .as_str()
        .context("Session has no share slug")?;

    println!("https://threader.sh/s/{slug}");
    Ok(())
}

pub fn resolve_current_session() -> Result<String> {
    // Try PID-based lookup first
    if let Some(claude_pid) = crate::process::find_claude_ancestor_pid() {
        let base = LocalStorage::default_base_dir()?;
        let pid_file = base.join("pid-sessions").join(claude_pid.to_string());
        if let Ok(session_id) = std::fs::read_to_string(&pid_file) {
            let session_id = session_id.trim().to_string();
            if !session_id.is_empty() {
                return Ok(session_id);
            }
        }
    }

    // Fallback: most recently started session for this cwd
    let cwd = std::env::current_dir()?.to_string_lossy().to_string();
    let base = LocalStorage::default_base_dir()?;
    let storage = LocalStorage::new(base);
    let all = storage.list_sessions()?;
    let mut matching: Vec<_> = all
        .iter()
        .filter_map(|id| storage.read_meta(id).ok())
        .filter(|m| m.cwd.as_deref() == Some(cwd.as_str()))
        .collect();

    if matching.is_empty() {
        bail!(
            "No Threader session found. Are you in a Claude Code session with Threader initialized?"
        );
    }

    matching.sort_by(|a, b| b.started_at.cmp(&a.started_at));
    Ok(matching[0].session_id.clone())
}
