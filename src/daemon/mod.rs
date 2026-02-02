pub mod socket;

use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Result;
use chrono::Utc;
use tokio::sync::{mpsc, Notify};
use tracing::{error, info, warn};

use crate::hooks::{HookEvent, HookMessage, SessionMeta};
use crate::storage::local::LocalStorage;
use crate::storage::queue::UploadQueue;
use crate::sync::cost;
use crate::sync::cursor::CursorTracker;
use crate::sync::updater::AutoUpdater;
use crate::sync::uploader::BackgroundUploader;

use self::socket::SocketServer;

/// Run the Threader daemon.
pub async fn run(base_dir: PathBuf) -> Result<()> {
    let storage = LocalStorage::new(base_dir.clone());
    storage.init()?;

    let queue = UploadQueue::new(base_dir.clone());
    let uploader_queue = UploadQueue::new(base_dir.clone());
    let socket_server = SocketServer::new(storage.socket_path());

    let (tx, rx) = mpsc::channel::<HookMessage>(256);

    info!("Threader daemon starting");

    // Spawn background uploader
    let uploader = BackgroundUploader::new(uploader_queue, base_dir.clone());
    tokio::spawn(async move {
        if let Err(e) = uploader.run().await {
            error!("Background uploader error: {}", e);
        }
    });

    // Spawn background auto-updater
    let restart_notify = Arc::new(Notify::new());
    let updater = AutoUpdater::new(restart_notify.clone());
    tokio::spawn(async move {
        if let Err(e) = updater.run().await {
            error!("Auto-updater error: {}", e);
        }
    });

    // Spawn socket server
    tokio::spawn(async move {
        if let Err(e) = socket_server.run(tx).await {
            error!("Socket server error: {}", e);
        }
    });

    // Set up SIGTERM handler
    #[cfg(unix)]
    let mut sigterm = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;

    // Process events until shutdown signal
    #[cfg(unix)]
    {
        tokio::select! {
            result = process_events(rx, storage, queue) => result,
            _ = sigterm.recv() => {
                info!("SIGTERM received, shutting down");
                Ok(())
            },
            _ = restart_notify.notified() => {
                info!("Restarting for update");
                Ok(())
            },
        }
    }

    #[cfg(not(unix))]
    {
        tokio::select! {
            result = process_events(rx, storage, queue) => result,
            _ = restart_notify.notified() => {
                info!("Restarting for update");
                Ok(())
            },
        }
    }
}

async fn process_events(
    mut rx: mpsc::Receiver<HookMessage>,
    storage: LocalStorage,
    queue: UploadQueue,
) -> Result<()> {
    info!("Session manager ready, waiting for events");

    while let Some(msg) = rx.recv().await {
        let session_id = &msg.input.session_id;

        let result = match msg.event {
            HookEvent::SessionStart => handle_session_start(&storage, &queue, &msg),
            HookEvent::Stop => handle_stop(&storage, &queue, &msg),
            HookEvent::SessionEnd => handle_session_end(&storage, &queue, &msg),
        };

        if let Err(e) = result {
            error!("Error handling {:?} for session {}: {}", msg.event, session_id, e);
        }
    }

    Ok(())
}

fn handle_session_start(
    storage: &LocalStorage,
    queue: &UploadQueue,
    msg: &HookMessage,
) -> Result<()> {
    let input = &msg.input;
    info!("Session started: {}", input.session_id);

    let meta = SessionMeta {
        session_id: input.session_id.clone(),
        transcript_path: input.transcript_path.clone(),
        cwd: input.cwd.clone(),
        model: input.model.clone(),
        started_at: msg.timestamp,
        ended_at: None,
        end_reason: None,
        tags: vec![],
        total_cost_usd: None,
        total_input_tokens: None,
        total_output_tokens: None,
        total_cache_read_tokens: None,
        total_cache_creation_tokens: None,
    };

    storage.create_session(&meta)?;
    queue.enqueue_create(&input.session_id)?;

    Ok(())
}

/// Ensure a session exists locally, creating it if the daemon missed the SessionStart event.
fn ensure_session(storage: &LocalStorage, queue: &UploadQueue, msg: &HookMessage) -> Result<()> {
    let input = &msg.input;
    if storage.sync_state_path(&input.session_id).exists() {
        return Ok(());
    }
    info!(
        "Late join: creating session {} (missed SessionStart)",
        input.session_id
    );
    handle_session_start(storage, queue, msg)
}

/// Read cost data from Claude Code's store DB and update the session meta.
fn refresh_cost(storage: &LocalStorage, session_id: &str) {
    match cost::read_session_cost(session_id) {
        Ok(Some(c)) => {
            if let Ok(mut meta) = storage.read_meta(session_id) {
                meta.total_cost_usd = Some(c.total_cost_usd);
                meta.total_input_tokens = Some(c.total_input_tokens);
                meta.total_output_tokens = Some(c.total_output_tokens);
                meta.total_cache_read_tokens = Some(c.total_cache_read_tokens);
                meta.total_cache_creation_tokens = Some(c.total_cache_creation_tokens);
                if let Err(e) = storage.update_meta(&meta) {
                    warn!("Failed to update cost meta for {}: {}", session_id, e);
                }
            }
        }
        Ok(None) => {}
        Err(e) => {
            warn!("Failed to read cost for session {}: {}", session_id, e);
        }
    }
}

fn handle_stop(
    storage: &LocalStorage,
    queue: &UploadQueue,
    msg: &HookMessage,
) -> Result<()> {
    let input = &msg.input;
    ensure_session(storage, queue, msg)?;
    let tracker = CursorTracker::new(storage);

    let last_line = tracker.get_position(&input.session_id)?;
    let (new_lines, total_lines) =
        storage.read_transcript_lines(&input.transcript_path, last_line)?;

    if new_lines.is_empty() {
        return Ok(());
    }

    // Append to local copy
    storage.append_transcript(&input.session_id, &new_lines)?;

    // Queue upload for new lines
    queue.enqueue_append(&input.session_id, last_line + 1, total_lines)?;

    // Update cursor
    tracker.advance(&input.session_id, total_lines)?;

    // Read cost data from Claude Code's store DB
    refresh_cost(storage, &input.session_id);

    info!(
        "Synced {} new lines for session {} (lines {}-{})",
        new_lines.len(),
        input.session_id,
        last_line + 1,
        total_lines
    );

    Ok(())
}

fn handle_session_end(
    storage: &LocalStorage,
    queue: &UploadQueue,
    msg: &HookMessage,
) -> Result<()> {
    let input = &msg.input;
    info!("Session ended: {} (reason: {:?})", input.session_id, input.reason);
    ensure_session(storage, queue, msg)?;

    // Sync any remaining lines
    let tracker = CursorTracker::new(storage);
    let last_line = tracker.get_position(&input.session_id)?;
    let (new_lines, total_lines) =
        storage.read_transcript_lines(&input.transcript_path, last_line)?;

    if !new_lines.is_empty() {
        storage.append_transcript(&input.session_id, &new_lines)?;
        queue.enqueue_append(&input.session_id, last_line + 1, total_lines)?;
        tracker.advance(&input.session_id, total_lines)?;
    }

    // Update metadata
    let mut meta = storage.read_meta(&input.session_id)?;
    meta.ended_at = Some(Utc::now());
    meta.end_reason = input.reason.clone();
    storage.update_meta(&meta)?;

    // Read cost data from Claude Code's store DB
    refresh_cost(storage, &input.session_id);

    // Queue finalize
    queue.enqueue_finalize(&input.session_id)?;

    Ok(())
}
