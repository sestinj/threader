use anyhow::Result;
use rusqlite::Connection;
use serde::Deserialize;
use tracing::{debug, warn};

/// Aggregated cost and token data for a session.
#[derive(Debug, Clone, Default)]
pub struct SessionCost {
    pub total_cost_usd: f64,
    pub total_input_tokens: u64,
    pub total_output_tokens: u64,
    pub total_cache_read_tokens: u64,
    pub total_cache_creation_tokens: u64,
}

/// Usage data embedded in assistant message JSON.
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

/// Read aggregated cost data for a session from Claude Code's `__store.db`.
///
/// Opens the database read-only, joins `base_messages` to `assistant_messages`
/// on `uuid`, filters by `session_id`, and sums cost and usage fields.
pub fn read_session_cost(session_id: &str) -> Result<Option<SessionCost>> {
    let claude_dir = dirs::home_dir()
        .ok_or_else(|| anyhow::anyhow!("Cannot determine home directory"))?
        .join(".claude");

    let db_path = claude_dir.join("__store.db");
    if !db_path.exists() {
        debug!("Claude store DB not found at {}", db_path.display());
        return Ok(None);
    }

    let conn = Connection::open_with_flags(
        &db_path,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;

    // Sum cost_usd from assistant_messages for this session
    let cost_result: rusqlite::Result<f64> = conn.query_row(
        "SELECT COALESCE(SUM(a.cost_usd), 0)
         FROM assistant_messages a
         JOIN base_messages b ON a.uuid = b.uuid
         WHERE b.session_id = ?1",
        [session_id],
        |row| row.get(0),
    );

    let total_cost_usd = match cost_result {
        Ok(c) => c,
        Err(e) => {
            warn!("Failed to query cost from store DB: {}", e);
            return Ok(None);
        }
    };

    // Query individual assistant messages to sum usage from the message JSON
    let mut stmt = conn.prepare(
        "SELECT a.message
         FROM assistant_messages a
         JOIN base_messages b ON a.uuid = b.uuid
         WHERE b.session_id = ?1",
    )?;

    let mut total_input_tokens: u64 = 0;
    let mut total_output_tokens: u64 = 0;
    let mut total_cache_read_tokens: u64 = 0;
    let mut total_cache_creation_tokens: u64 = 0;

    let rows = stmt.query_map([session_id], |row| {
        let message_json: String = row.get(0)?;
        Ok(message_json)
    })?;

    for row in rows {
        let message_json = match row {
            Ok(j) => j,
            Err(_) => continue,
        };

        // Parse the message JSON to extract usage
        if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&message_json) {
            if let Some(usage_val) = parsed.get("usage") {
                if let Ok(usage) = serde_json::from_value::<Usage>(usage_val.clone()) {
                    total_input_tokens += usage.input_tokens;
                    total_output_tokens += usage.output_tokens;
                    total_cache_read_tokens += usage.cache_read_input_tokens;
                    total_cache_creation_tokens += usage.cache_creation_input_tokens;
                }
            }
        }
    }

    debug!(
        "Session {} cost: ${:.4}, tokens: {}in/{}out/{}cache_read/{}cache_create",
        session_id,
        total_cost_usd,
        total_input_tokens,
        total_output_tokens,
        total_cache_read_tokens,
        total_cache_creation_tokens
    );

    Ok(Some(SessionCost {
        total_cost_usd,
        total_input_tokens,
        total_output_tokens,
        total_cache_read_tokens,
        total_cache_creation_tokens,
    }))
}
