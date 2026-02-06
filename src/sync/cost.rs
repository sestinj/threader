use std::collections::HashMap;
use std::io::{BufRead, BufReader};
use std::path::Path;

use anyhow::Result;
use serde::Deserialize;
use tracing::debug;

/// Aggregated cost and token data for a session.
#[derive(Debug, Clone, Default)]
pub struct SessionCost {
    pub total_cost_usd: f64,
    pub total_input_tokens: u64,
    pub total_output_tokens: u64,
    pub total_cache_read_tokens: u64,
    pub total_cache_creation_tokens: u64,
}

/// Usage data embedded in an assistant message's API response.
#[derive(Debug, Deserialize, Default)]
struct Usage {
    #[serde(default)]
    input_tokens: u64,
    #[serde(default)]
    output_tokens: u64,
    #[serde(default)]
    cache_read_input_tokens: u64,
    #[serde(default)]
    cache_creation_input_tokens: u64,
}

/// A transcript JSONL line (only the fields we care about).
#[derive(Debug, Deserialize)]
struct TranscriptLine {
    #[serde(default)]
    r#type: Option<String>,
    #[serde(alias = "requestId")]
    #[serde(default)]
    request_id: Option<String>,
    #[serde(default)]
    message: Option<serde_json::Value>,
}

/// Read aggregated token data for a session from its source transcript JSONL.
///
/// Parses assistant message lines, deduplicates by `requestId` (keeping the
/// last/final streaming update per request), and sums usage fields.
pub fn read_session_cost(transcript_path: &Path) -> Result<Option<SessionCost>> {
    if !transcript_path.exists() {
        debug!(
            "Transcript not found at {}",
            transcript_path.display()
        );
        return Ok(None);
    }

    let file = std::fs::File::open(transcript_path)?;
    let reader = BufReader::new(file);

    // Track the latest usage per requestId to deduplicate streaming updates.
    let mut usage_by_request: HashMap<String, Usage> = HashMap::new();

    for line in reader.lines() {
        let line = match line {
            Ok(l) => l,
            Err(_) => continue,
        };

        let parsed: TranscriptLine = match serde_json::from_str(&line) {
            Ok(p) => p,
            Err(_) => continue,
        };

        // Only process assistant message lines
        if parsed.r#type.as_deref() != Some("assistant") {
            continue;
        }

        let request_id = match parsed.request_id {
            Some(rid) if !rid.is_empty() => rid,
            _ => continue,
        };

        let message = match parsed.message {
            Some(m) => m,
            None => continue,
        };

        let usage_val = match message.get("usage") {
            Some(u) => u,
            None => continue,
        };

        if let Ok(usage) = serde_json::from_value::<Usage>(usage_val.clone()) {
            // Always overwrite: the last line for a requestId has the final token counts
            usage_by_request.insert(request_id, usage);
        }
    }

    let mut total_input_tokens: u64 = 0;
    let mut total_output_tokens: u64 = 0;
    let mut total_cache_read_tokens: u64 = 0;
    let mut total_cache_creation_tokens: u64 = 0;

    for usage in usage_by_request.values() {
        total_input_tokens += usage.input_tokens;
        total_output_tokens += usage.output_tokens;
        total_cache_read_tokens += usage.cache_read_input_tokens;
        total_cache_creation_tokens += usage.cache_creation_input_tokens;
    }

    debug!(
        "Session {} tokens: {}in/{}out/{}cache_read/{}cache_create ({} requests)",
        transcript_path.display(),
        total_input_tokens,
        total_output_tokens,
        total_cache_read_tokens,
        total_cache_creation_tokens,
        usage_by_request.len()
    );

    Ok(Some(SessionCost {
        total_cost_usd: 0.0, // Not available from transcript
        total_input_tokens,
        total_output_tokens,
        total_cache_read_tokens,
        total_cache_creation_tokens,
    }))
}
