use std::path::PathBuf;

use anyhow::{Context, Result};
use tokio::io::AsyncReadExt;
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::mpsc;
use tracing::{debug, error, info, warn};

use crate::hooks::HookMessage;

/// Unix socket server that receives hook events from the CLI.
pub struct SocketServer {
    socket_path: PathBuf,
}

impl SocketServer {
    pub fn new(socket_path: PathBuf) -> Self {
        Self { socket_path }
    }

    /// Start listening for connections and forward messages to the channel.
    pub async fn run(&self, tx: mpsc::Sender<HookMessage>) -> Result<()> {
        // Remove stale socket file
        if self.socket_path.exists() {
            std::fs::remove_file(&self.socket_path)?;
        }

        let listener = UnixListener::bind(&self.socket_path)
            .with_context(|| format!("Failed to bind socket: {}", self.socket_path.display()))?;

        info!("Listening on {}", self.socket_path.display());

        loop {
            match listener.accept().await {
                Ok((stream, _addr)) => {
                    let tx = tx.clone();
                    tokio::spawn(async move {
                        if let Err(e) = handle_connection(stream, tx).await {
                            warn!("Error handling connection: {}", e);
                        }
                    });
                }
                Err(e) => {
                    error!("Error accepting connection: {}", e);
                }
            }
        }
    }
}

async fn handle_connection(mut stream: UnixStream, tx: mpsc::Sender<HookMessage>) -> Result<()> {
    let mut buf = Vec::new();
    stream.read_to_end(&mut buf).await?;

    let msg: HookMessage = serde_json::from_slice(&buf).context("Failed to parse hook message")?;

    debug!(
        "Received {:?} for session {}",
        msg.event, msg.input.session_id
    );
    tx.send(msg)
        .await
        .context("Failed to send message to session manager")?;

    Ok(())
}
