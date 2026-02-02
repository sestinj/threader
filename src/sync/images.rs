use anyhow::{Context, Result};
use base64::Engine;
use sha2::{Digest, Sha256};
use tracing::{debug, warn};

/// Processes transcript lines to extract, upload, and rewrite image blocks.
pub struct ImageProcessor {
    client: reqwest::Client,
    convex_site_url: String,
}

impl ImageProcessor {
    pub fn new(client: reqwest::Client, convex_site_url: String) -> Self {
        Self {
            client,
            convex_site_url,
        }
    }

    /// Process a single transcript line. If it contains image blocks, upload them
    /// and rewrite the line with URL references. Returns the (possibly modified) line.
    /// Failures are non-blocking: returns the original line with a warning log.
    pub async fn process_line(&self, line: &str, session_id: &str, token: &str) -> String {
        match self.try_process_line(line, session_id, token).await {
            Ok(processed) => processed,
            Err(e) => {
                warn!("Image processing failed, using original line: {}", e);
                line.to_string()
            }
        }
    }

    async fn try_process_line(
        &self,
        line: &str,
        session_id: &str,
        token: &str,
    ) -> Result<String> {
        let mut parsed: serde_json::Value =
            serde_json::from_str(line).context("Failed to parse transcript line as JSON")?;

        let content = match parsed
            .get_mut("message")
            .and_then(|m| m.get_mut("content"))
        {
            Some(c) if c.is_array() => c,
            _ => return Ok(line.to_string()),
        };

        let blocks = content.as_array_mut().unwrap();
        let mut modified = false;

        for block in blocks.iter_mut() {
            if block.get("type").and_then(|t| t.as_str()) != Some("image") {
                continue;
            }

            let source = match block.get("source") {
                Some(s) => s.clone(),
                None => continue,
            };

            let source_type = source.get("type").and_then(|t| t.as_str()).unwrap_or("");

            let (image_bytes, content_type) = match source_type {
                "base64" => {
                    let media_type = source
                        .get("media_type")
                        .and_then(|m| m.as_str())
                        .unwrap_or("image/png")
                        .to_string();
                    let data = match source.get("data").and_then(|d| d.as_str()) {
                        Some(d) => d,
                        None => continue,
                    };
                    let bytes = base64::engine::general_purpose::STANDARD
                        .decode(data)
                        .context("Failed to decode base64 image data")?;
                    (bytes, media_type)
                }
                "file" => {
                    let file_path = match source.get("file_path").and_then(|p| p.as_str()) {
                        Some(p) => p,
                        None => continue,
                    };
                    match std::fs::read(file_path) {
                        Ok(bytes) => {
                            let content_type = mime_from_path(file_path);
                            (bytes, content_type)
                        }
                        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                            warn!("Image file not found: {}", file_path);
                            continue;
                        }
                        Err(e) => {
                            warn!("Failed to read image file {}: {}", file_path, e);
                            continue;
                        }
                    }
                }
                // Already a URL — nothing to do
                "url" => continue,
                _ => continue,
            };

            // Compute SHA-256 hash for content-addressing
            let mut hasher = Sha256::new();
            hasher.update(&image_bytes);
            let hash = format!("{:x}", hasher.finalize());

            // Upload to backend
            match self
                .upload_image(&image_bytes, &content_type, session_id, &hash, token)
                .await
            {
                Ok(url) => {
                    // Rewrite the block to use a URL source
                    *block = serde_json::json!({
                        "type": "image",
                        "source": {
                            "type": "url",
                            "url": url,
                        }
                    });
                    modified = true;
                    debug!("Rewrote image block with URL: {}", url);
                }
                Err(e) => {
                    warn!("Failed to upload image: {}", e);
                    // Leave block as-is
                }
            }
        }

        if modified {
            Ok(serde_json::to_string(&parsed)?)
        } else {
            Ok(line.to_string())
        }
    }

    /// Upload image bytes to the backend and return the public URL.
    async fn upload_image(
        &self,
        image_bytes: &[u8],
        content_type: &str,
        session_id: &str,
        hash: &str,
        token: &str,
    ) -> Result<String> {
        let url = format!("{}/api/images/upload", self.convex_site_url);

        let resp = self
            .client
            .post(&url)
            .bearer_auth(token)
            .header("Content-Type", content_type)
            .header("X-Session-Id", session_id)
            .header("X-Image-Hash", hash)
            .body(image_bytes.to_vec())
            .send()
            .await
            .context("Failed to send image upload request")?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            anyhow::bail!("Image upload failed ({}): {}", status, text);
        }

        let body: serde_json::Value = resp.json().await.context("Failed to parse upload response")?;
        let image_url = body
            .get("url")
            .and_then(|u| u.as_str())
            .context("Missing 'url' in upload response")?
            .to_string();

        Ok(image_url)
    }
}

/// Check if a transcript line contains any image blocks worth processing.
pub fn line_has_images(line: &str) -> bool {
    // Quick check before full JSON parse
    if !line.contains("\"type\":\"image\"") && !line.contains("\"type\": \"image\"") {
        return false;
    }

    if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(line) {
        if let Some(content) = parsed
            .get("message")
            .and_then(|m| m.get("content"))
            .and_then(|c| c.as_array())
        {
            return content.iter().any(|block| {
                if block.get("type").and_then(|t| t.as_str()) != Some("image") {
                    return false;
                }
                // Only process base64 and file sources — URLs are already done
                let source_type = block
                    .get("source")
                    .and_then(|s| s.get("type"))
                    .and_then(|t| t.as_str())
                    .unwrap_or("");
                source_type == "base64" || source_type == "file"
            });
        }
    }
    false
}

fn mime_from_path(path: &str) -> String {
    let lower = path.to_lowercase();
    if lower.ends_with(".png") {
        "image/png".to_string()
    } else if lower.ends_with(".jpg") || lower.ends_with(".jpeg") {
        "image/jpeg".to_string()
    } else if lower.ends_with(".gif") {
        "image/gif".to_string()
    } else if lower.ends_with(".webp") {
        "image/webp".to_string()
    } else if lower.ends_with(".svg") {
        "image/svg+xml".to_string()
    } else {
        "image/png".to_string()
    }
}
