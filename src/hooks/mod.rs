use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Events received from Claude Code hooks.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "event")]
pub enum HookEvent {
    SessionStart,
    Stop,
    SessionEnd,
}

/// Input data provided by Claude Code to each hook via stdin.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HookInput {
    pub session_id: String,
    pub transcript_path: PathBuf,
    #[serde(default)]
    pub cwd: Option<String>,
    #[serde(default)]
    pub model: Option<String>,
    /// For SessionEnd: reason the session ended.
    #[serde(default)]
    pub reason: Option<String>,
}

/// Message sent from the hook CLI to the daemon via Unix socket.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HookMessage {
    pub event: HookEvent,
    pub input: HookInput,
    pub timestamp: DateTime<Utc>,
}

/// Session metadata stored locally.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionMeta {
    pub session_id: String,
    pub transcript_path: PathBuf,
    #[serde(default)]
    pub cwd: Option<String>,
    #[serde(default)]
    pub model: Option<String>,
    pub started_at: DateTime<Utc>,
    #[serde(default)]
    pub ended_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub end_reason: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub total_cost_usd: Option<f64>,
    #[serde(default)]
    pub total_input_tokens: Option<u64>,
    #[serde(default)]
    pub total_output_tokens: Option<u64>,
    #[serde(default)]
    pub total_cache_read_tokens: Option<u64>,
    #[serde(default)]
    pub total_cache_creation_tokens: Option<u64>,
}

/// Tracks how far we've synced a session's transcript.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncState {
    pub last_synced_line: u64,
    pub last_synced_at: DateTime<Utc>,
    pub upload_status: UploadStatus,
    pub retry_count: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum UploadStatus {
    Pending,
    InProgress,
    Synced,
    Failed,
}

/// An entry in the persistent upload queue.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueueEntry {
    pub session_id: String,
    pub action: QueueAction,
    pub lines_start: u64,
    pub lines_end: u64,
    pub created_at: DateTime<Utc>,
    pub attempts: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum QueueAction {
    Create,
    Append,
    Finalize,
}
