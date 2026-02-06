pub mod socket;

use std::fs;
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
use crate::sync::poller::TranscriptPoller;
use crate::sync::uploader::BackgroundUploader;

use self::socket::SocketServer;

/// Run the Threader daemon.
pub async fn run(base_dir: PathBuf) -> Result<()> {
    let storage = LocalStorage::new(base_dir.clone());
    storage.init()?;

    let queue = UploadQueue::new(base_dir.clone());
    let uploader_queue = UploadQueue::new(base_dir.clone());
    let socket_server = SocketServer::new(storage.socket_path());

    let (tx, mut rx) = mpsc::channel::<HookMessage>(256);

    info!("Threader daemon starting");

    // Set Sentry user context from stored credentials (if logged in)
    if let Ok(Some(creds)) = crate::auth::storage::load() {
        sentry::configure_scope(|scope| {
            scope.set_user(Some(sentry::User {
                id: Some(creds.user_id.clone()),
                email: creds.email.clone(),
                ..Default::default()
            }));
        });
    }

    // Replay any spooled messages from when the daemon was down
    replay_spool(&storage, &queue);

    // Catch up any active sessions that may have advanced while daemon was down
    catch_up_active_sessions(&storage, &queue);

    // Spawn background uploader
    let uploader = BackgroundUploader::new(uploader_queue, base_dir.clone());
    tokio::spawn(async move {
        if let Err(e) = uploader.run().await {
            error!("Background uploader error: {}", e);
        }
    });

    // Spawn transcript poller for faster sync of active sessions
    let poller = TranscriptPoller::new(base_dir.clone());
    tokio::spawn(async move {
        if let Err(e) = poller.run().await {
            error!("Transcript poller error: {}", e);
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
    let shutdown_reason;
    #[cfg(unix)]
    {
        shutdown_reason = loop {
            tokio::select! {
                msg = rx.recv() => {
                    match msg {
                        Some(msg) => handle_event(&storage, &queue, &msg),
                        None => break "channel closed",
                    }
                },
                _ = sigterm.recv() => break "SIGTERM",
                _ = restart_notify.notified() => break "update restart",
            }
        };
    }

    #[cfg(not(unix))]
    {
        shutdown_reason = loop {
            tokio::select! {
                msg = rx.recv() => {
                    match msg {
                        Some(msg) => handle_event(&storage, &queue, &msg),
                        None => break "channel closed",
                    }
                },
                _ = restart_notify.notified() => break "update restart",
            }
        };
    }

    // Drain any remaining messages in the channel before exiting
    info!("{}, draining remaining events", shutdown_reason);
    rx.close();
    let mut drained = 0;
    while let Ok(msg) = rx.try_recv() {
        handle_event(&storage, &queue, &msg);
        drained += 1;
    }
    if drained > 0 {
        info!("Drained {} events before shutdown", drained);
    }

    Ok(())
}

/// Replay spooled hook messages that were saved when the daemon was unreachable.
fn replay_spool(storage: &LocalStorage, queue: &UploadQueue) {
    let spool_dir = storage.spool_dir();
    if !spool_dir.exists() {
        return;
    }

    let mut entries: Vec<_> = match fs::read_dir(&spool_dir) {
        Ok(rd) => rd.filter_map(|e| e.ok()).collect(),
        Err(e) => {
            warn!("Failed to read spool directory: {}", e);
            return;
        }
    };
    // Sort by filename (timestamp-prefixed) to replay in order
    entries.sort_by_key(|e| e.file_name());

    let mut replayed = 0;
    for entry in &entries {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        match fs::read_to_string(&path) {
            Ok(data) => match serde_json::from_str::<HookMessage>(&data) {
                Ok(msg) => {
                    let session_id = &msg.input.session_id;
                    let result = match msg.event {
                        HookEvent::SessionStart => handle_session_start(storage, queue, &msg),
                        HookEvent::Stop => handle_stop(storage, queue, &msg),
                        HookEvent::SessionEnd => handle_session_end(storage, queue, &msg),
                    };
                    if let Err(e) = result {
                        error!("Error replaying spooled {:?} for {}: {}", msg.event, session_id, e);
                    } else {
                        replayed += 1;
                    }
                    // Remove the spool file regardless (avoid infinite replay of bad messages)
                    let _ = fs::remove_file(&path);
                }
                Err(e) => {
                    warn!("Skipping malformed spool entry {}: {}", path.display(), e);
                    let _ = fs::remove_file(&path);
                }
            },
            Err(e) => {
                warn!("Failed to read spool entry {}: {}", path.display(), e);
            }
        }
    }

    if replayed > 0 {
        info!("Replayed {} spooled messages", replayed);
    }
}

/// Scan active sessions and sync any lines that were missed while daemon was down.
fn catch_up_active_sessions(storage: &LocalStorage, queue: &UploadQueue) {
    let active = match storage.list_active_sessions() {
        Ok(a) => a,
        Err(e) => {
            warn!("Failed to list active sessions for catch-up: {}", e);
            return;
        }
    };

    let tracker = CursorTracker::new(storage);
    let mut caught_up = 0;

    for meta in &active {
        let session_id = &meta.session_id;
        let last_line = match tracker.get_position(session_id) {
            Ok(l) => l,
            Err(_) => continue,
        };

        let (new_lines, total_lines) = match storage.read_transcript_lines(&meta.transcript_path, last_line) {
            Ok(r) => r,
            Err(_) => continue,
        };

        if new_lines.is_empty() {
            continue;
        }

        if let Err(e) = storage.append_transcript(session_id, &new_lines) {
            warn!("Catch-up: failed to append transcript for {}: {}", session_id, e);
            continue;
        }

        if let Err(e) = queue.enqueue_append(session_id, last_line, total_lines) {
            warn!("Catch-up: failed to enqueue append for {}: {}", session_id, e);
            continue;
        }

        if let Err(e) = tracker.advance(session_id, total_lines) {
            warn!("Catch-up: failed to advance cursor for {}: {}", session_id, e);
            continue;
        }

        info!(
            "Catch-up: synced {} missed lines for session {} (lines {}-{})",
            new_lines.len(),
            session_id,
            last_line,
            total_lines
        );
        caught_up += 1;
    }

    if caught_up > 0 {
        info!("Caught up {} active sessions", caught_up);
    }
}

/// Process a single hook event, logging any errors.
fn handle_event(storage: &LocalStorage, queue: &UploadQueue, msg: &HookMessage) {
    let session_id = &msg.input.session_id;

    let result = match msg.event {
        HookEvent::SessionStart => handle_session_start(storage, queue, msg),
        HookEvent::Stop => handle_stop(storage, queue, msg),
        HookEvent::SessionEnd => handle_session_end(storage, queue, msg),
    };

    if let Err(e) = result {
        error!("Error handling {:?} for session {}: {}", msg.event, session_id, e);
    }
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
        repo: input.repo.clone(),
        agent: msg.agent.clone(),
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

/// Read token data from the source transcript JSONL and update the session meta.
fn refresh_cost(storage: &LocalStorage, session_id: &str) {
    let meta = match storage.read_meta(session_id) {
        Ok(m) => m,
        Err(_) => return,
    };

    match cost::read_session_cost(&meta.transcript_path) {
        Ok(Some(c)) => {
            let mut meta = meta;
            meta.total_cost_usd = Some(c.total_cost_usd);
            meta.total_input_tokens = Some(c.total_input_tokens);
            meta.total_output_tokens = Some(c.total_output_tokens);
            meta.total_cache_read_tokens = Some(c.total_cache_read_tokens);
            meta.total_cache_creation_tokens = Some(c.total_cache_creation_tokens);
            if let Err(e) = storage.update_meta(&meta) {
                warn!("Failed to update cost meta for {}: {}", session_id, e);
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
    queue.enqueue_append(&input.session_id, last_line, total_lines)?;

    // Update cursor
    tracker.advance(&input.session_id, total_lines)?;

    // Read cost data from Claude Code's store DB
    refresh_cost(storage, &input.session_id);

    info!(
        "Synced {} new lines for session {} (lines {}-{})",
        new_lines.len(),
        input.session_id,
        last_line,
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
        queue.enqueue_append(&input.session_id, last_line, total_lines)?;
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
