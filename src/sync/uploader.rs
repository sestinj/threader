use std::fs;
use std::path::PathBuf;
use std::time::Duration;

use anyhow::{Context, Result};
use tokio::time;
use tracing::{debug, error, info, warn};

use crate::auth;
use crate::hooks::{QueueAction, QueueEntry};
use crate::storage::local::LocalStorage;
use crate::storage::queue::UploadQueue;
use crate::sync::images::{line_has_images, ImageProcessor};

const UPLOAD_INTERVAL_SECS: u64 = 10;
const MAX_RETRY_ATTEMPTS: u32 = 360;

/// Convex HTTP endpoint base URL.
/// Can be overridden via the THREADER_CONVEX_SITE_URL env var.
fn convex_site_url() -> String {
    std::env::var("THREADER_CONVEX_SITE_URL")
        .unwrap_or_else(|_| "https://ceaseless-shepherd-756.convex.site".to_string())
}

/// Background worker that processes the upload queue.
pub struct BackgroundUploader {
    queue: UploadQueue,
    storage: LocalStorage,
    client: reqwest::Client,
}

impl BackgroundUploader {
    pub fn new(queue: UploadQueue, base_dir: PathBuf) -> Self {
        Self {
            queue,
            storage: LocalStorage::new(base_dir),
            client: reqwest::Client::new(),
        }
    }

    /// Run the upload loop. Processes pending entries periodically.
    pub async fn run(&self) -> Result<()> {
        info!("Background uploader started");

        loop {
            if let Err(e) = self.process_queue().await {
                error!("Error processing upload queue: {}", e);
            }
            time::sleep(Duration::from_secs(UPLOAD_INTERVAL_SECS)).await;
        }
    }

    async fn process_queue(&self) -> Result<()> {
        let entries = self.queue.list_pending()?;
        if entries.is_empty() {
            return Ok(());
        }

        debug!("Processing {} pending uploads", entries.len());

        // Get auth token once per batch
        let token = match auth::get_token().await {
            Ok(t) => {
                debug!("Got auth token (len={})", t.len());
                t
            }
            Err(e) => {
                warn!("Cannot upload: auth error: {}", e);
                return Ok(());
            }
        };

        for (path, mut entry) in entries {
            if entry.attempts >= MAX_RETRY_ATTEMPTS {
                warn!(
                    "Moving queue entry for session {} to dead-letter after {} attempts",
                    entry.session_id, entry.attempts
                );
                self.move_to_failed(&path)?;
                continue;
            }

            match self.upload(&entry, &token).await {
                Ok(()) => {
                    debug!(
                        "Uploaded {:?} for session {}",
                        entry.action, entry.session_id
                    );
                    self.queue.remove(&path)?;
                }
                Err(e) => {
                    warn!(
                        "Upload failed for session {} (attempt {}): {}",
                        entry.session_id,
                        entry.attempts + 1,
                        e
                    );
                    entry.attempts += 1;
                    self.queue.update(&path, &entry)?;
                }
            }
        }

        Ok(())
    }

    /// Move a failed queue entry to the dead-letter directory instead of deleting it.
    fn move_to_failed(&self, path: &PathBuf) -> Result<()> {
        let failed_dir = path
            .parent()
            .context("Queue entry has no parent dir")?
            .parent()
            .context("Queue pending dir has no parent")?
            .join("failed");
        fs::create_dir_all(&failed_dir)?;

        let filename = path
            .file_name()
            .context("Queue entry has no filename")?;
        let dest = failed_dir.join(filename);
        fs::rename(path, &dest)?;
        debug!("Moved failed entry to {}", dest.display());
        Ok(())
    }

    /// Upload a single queue entry to the Convex HTTP endpoint.
    async fn upload(&self, entry: &QueueEntry, token: &str) -> Result<()> {
        match entry.action {
            QueueAction::Create => self.upload_create(entry, token).await,
            QueueAction::Append => {
                // Ensure session exists on server (idempotent create)
                self.upload_create(entry, token).await?;
                self.upload_append(entry, token).await
            }
            QueueAction::Finalize => {
                // Ensure session exists on server (idempotent create)
                self.upload_create(entry, token).await?;
                self.upload_finalize(entry, token).await
            }
        }
    }

    async fn upload_create(&self, entry: &QueueEntry, token: &str) -> Result<()> {
        let meta = self.storage.read_meta(&entry.session_id)?;

        // Build body, omitting None fields (Convex rejects null for optional string fields)
        let mut body = serde_json::json!({
            "session_id": meta.session_id,
            "started_at": meta.started_at.to_rfc3339(),
        });
        if let Some(ref model) = meta.model {
            body["model"] = serde_json::json!(model);
        }
        if let Some(ref cwd) = meta.cwd {
            body["cwd"] = serde_json::json!(cwd);
        }
        if let Some(ref agent) = meta.agent {
            body["agent"] = serde_json::json!(agent);
        }
        if let Some(ref repo) = meta.repo {
            body["repo"] = serde_json::json!(repo);
        }

        let url = format!("{}/api/sessions", convex_site_url());
        info!("Creating session {} at {}", entry.session_id, url);

        let resp = self
            .client
            .post(&url)
            .bearer_auth(token)
            .json(&body)
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            anyhow::bail!("Create session failed ({}): {}", status, text);
        }

        info!("Created session {} on server", entry.session_id);
        Ok(())
    }

    async fn upload_append(&self, entry: &QueueEntry, token: &str) -> Result<()> {
        // Read the lines from local transcript
        let transcript_path = self.storage.transcript_path(&entry.session_id);
        let content = std::fs::read_to_string(&transcript_path)
            .with_context(|| format!("Failed to read transcript for {}", entry.session_id))?;

        let all_lines: Vec<&str> = content.lines().collect();

        // Extract the range of lines to send
        let start = entry.lines_start as usize;
        let end = (entry.lines_end as usize).min(all_lines.len());
        if start >= all_lines.len() || start >= end {
            debug!("No lines to append for session {}", entry.session_id);
            return Ok(());
        }

        let lines_to_send: Vec<&str> = all_lines[start..end].to_vec();

        // Process lines through image processor — rewrite image blocks with URLs
        let image_processor =
            ImageProcessor::new(self.client.clone(), convex_site_url());
        let mut processed_lines: Vec<String> = Vec::with_capacity(lines_to_send.len());
        for line in &lines_to_send {
            if line_has_images(line) {
                let processed = image_processor
                    .process_line(line, &entry.session_id, token)
                    .await;
                processed_lines.push(processed);
            } else {
                processed_lines.push(line.to_string());
            }
        }

        let lines_str = processed_lines.join("\n");
        let line_count = processed_lines.len();

        let meta = self.storage.read_meta(&entry.session_id)?;
        let mut body = serde_json::json!({
            "lines": lines_str,
            "line_count": line_count,
            "start_line": start,
        });
        if let Some(cost) = meta.total_cost_usd {
            body["total_cost_usd"] = serde_json::json!(cost);
        }
        if let Some(tokens) = meta.total_input_tokens {
            body["total_input_tokens"] = serde_json::json!(tokens);
        }
        if let Some(tokens) = meta.total_output_tokens {
            body["total_output_tokens"] = serde_json::json!(tokens);
        }
        if let Some(tokens) = meta.total_cache_read_tokens {
            body["total_cache_read_tokens"] = serde_json::json!(tokens);
        }
        if let Some(tokens) = meta.total_cache_creation_tokens {
            body["total_cache_creation_tokens"] = serde_json::json!(tokens);
        }

        let resp = self
            .client
            .post(format!(
                "{}/api/sessions/{}/append",
                convex_site_url(), entry.session_id
            ))
            .bearer_auth(token)
            .json(&body)
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            anyhow::bail!("Append lines failed ({}): {}", status, text);
        }

        debug!(
            "Appended {} lines for session {}",
            line_count, entry.session_id
        );
        Ok(())
    }

    async fn upload_finalize(&self, entry: &QueueEntry, token: &str) -> Result<()> {
        let meta = self.storage.read_meta(&entry.session_id)?;

        let mut body = serde_json::json!({});
        if let Some(ref reason) = meta.end_reason {
            body["end_reason"] = serde_json::json!(reason);
        }
        if let Some(ended_at) = meta.ended_at {
            body["ended_at"] = serde_json::json!(ended_at.to_rfc3339());
        }
        if let Some(cost) = meta.total_cost_usd {
            body["total_cost_usd"] = serde_json::json!(cost);
        }
        if let Some(tokens) = meta.total_input_tokens {
            body["total_input_tokens"] = serde_json::json!(tokens);
        }
        if let Some(tokens) = meta.total_output_tokens {
            body["total_output_tokens"] = serde_json::json!(tokens);
        }
        if let Some(tokens) = meta.total_cache_read_tokens {
            body["total_cache_read_tokens"] = serde_json::json!(tokens);
        }
        if let Some(tokens) = meta.total_cache_creation_tokens {
            body["total_cache_creation_tokens"] = serde_json::json!(tokens);
        }

        let resp = self
            .client
            .post(format!(
                "{}/api/sessions/{}/finalize",
                convex_site_url(), entry.session_id
            ))
            .bearer_auth(token)
            .json(&body)
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            anyhow::bail!("Finalize session failed ({}): {}", status, text);
        }

        debug!("Finalized session {} on server", entry.session_id);
        Ok(())
    }
}
