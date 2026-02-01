pub mod hook;
pub mod init;

use anyhow::{Context, Result};
use chrono::Utc;
use clap::{Parser, Subcommand};

use crate::hooks::HookEvent;
use crate::storage::local::LocalStorage;

#[derive(Parser)]
#[command(name = "threader", about = "Sync Claude Code sessions to the cloud", version)]
pub struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Start the Threader daemon in the background
    Start,

    /// Stop the Threader daemon
    Stop,

    /// Run the Threader daemon in the foreground
    Daemon,

    /// Handle a Claude Code hook event
    Hook {
        #[command(subcommand)]
        event: HookCommand,
    },

    /// Initialize Threader (create dirs, install hooks)
    Init,

    /// Log in to Threader via browser
    Login,

    /// Log out and clear stored credentials
    Logout,

    /// Show daemon status and session info
    Status,

    /// Show current authenticated user
    Whoami,

}

#[derive(Subcommand)]
enum HookCommand {
    /// Handle SessionStart event
    SessionStart,
    /// Handle Stop event (Claude finished responding)
    Stop,
    /// Handle SessionEnd event
    SessionEnd,
}

impl Cli {
    pub async fn run(self) -> Result<()> {
        match self.command {
            Command::Start => {
                let base_dir = LocalStorage::default_base_dir()?;
                start_daemon(&base_dir)
            }
            Command::Stop => {
                let base_dir = LocalStorage::default_base_dir()?;
                stop_daemon(&base_dir)
            }
            Command::Daemon => {
                let base_dir = LocalStorage::default_base_dir()?;
                // Write PID file so `threader stop` and `threader status` can find us
                let pid_path = base_dir.join("threader.pid");
                std::fs::write(&pid_path, std::process::id().to_string())?;
                let result = crate::daemon::run(base_dir.clone()).await;
                // Clean up PID file on exit
                let _ = std::fs::remove_file(&pid_path);
                result
            }
            Command::Hook { event } => {
                let hook_event = match event {
                    HookCommand::SessionStart => HookEvent::SessionStart,
                    HookCommand::Stop => HookEvent::Stop,
                    HookCommand::SessionEnd => HookEvent::SessionEnd,
                };
                hook::handle_hook(hook_event)
            }
            Command::Init => init::run_init(),
            Command::Login => {
                match crate::auth::device_flow::login().await {
                    Ok(creds) => {
                        let who = creds.email.as_deref().unwrap_or(&creds.user_id);
                        println!("Logged in as {who}");
                        Ok(())
                    }
                    Err(e) => {
                        eprintln!("Login failed: {e}");
                        std::process::exit(1);
                    }
                }
            }
            Command::Logout => {
                crate::auth::logout().map_err(|e| anyhow::anyhow!("{e}"))?;
                println!("Logged out");
                Ok(())
            }
            Command::Status => {
                let base_dir = LocalStorage::default_base_dir()?;
                show_status(base_dir)
            }
            Command::Whoami => show_whoami(),
        }
    }
}

/// Read the PID from the PID file, if it exists and the process is alive.
fn read_daemon_pid(base_dir: &std::path::Path) -> Option<u32> {
    let pid_path = base_dir.join("threader.pid");
    let pid_str = std::fs::read_to_string(&pid_path).ok()?;
    let pid: u32 = pid_str.trim().parse().ok()?;
    // Check if the process is still alive (signal 0 = no signal, just check existence)
    let alive = unsafe { libc::kill(pid as i32, 0) == 0 };
    if alive {
        Some(pid)
    } else {
        // Stale PID file — clean up
        let _ = std::fs::remove_file(&pid_path);
        None
    }
}

/// Check if the daemon is actually running by trying to connect to its socket.
fn is_daemon_running(base_dir: &std::path::Path, socket_path: &std::path::Path) -> bool {
    // First check PID file
    if read_daemon_pid(base_dir).is_some() {
        return true;
    }
    // Fallback: try connecting to socket
    if !socket_path.exists() {
        return false;
    }
    std::os::unix::net::UnixStream::connect(socket_path).is_ok()
}

fn start_daemon(base_dir: &std::path::Path) -> Result<()> {
    let storage = LocalStorage::new(base_dir.to_path_buf());

    // Check if already running
    if is_daemon_running(base_dir, &storage.socket_path()) {
        let pid = read_daemon_pid(base_dir);
        if let Some(pid) = pid {
            println!("Threader daemon is already running (pid {pid})");
        } else {
            println!("Threader daemon is already running");
        }
        return Ok(());
    }

    // Find our own binary
    let exe = std::env::current_exe().context("could not determine threader binary path")?;

    // Spawn `threader daemon` as a detached background process
    let log_path = base_dir.join("logs").join("daemon.log");
    std::fs::create_dir_all(log_path.parent().unwrap())?;

    let log_file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)
        .with_context(|| format!("could not open log file: {}", log_path.display()))?;
    let log_stderr = log_file.try_clone()?;

    let child = std::process::Command::new(&exe)
        .arg("daemon")
        .stdin(std::process::Stdio::null())
        .stdout(log_file)
        .stderr(log_stderr)
        .spawn()
        .context("failed to start daemon process")?;

    println!("Threader daemon started (pid {})", child.id());
    println!("Logs: {}", log_path.display());

    Ok(())
}

fn stop_daemon(base_dir: &std::path::Path) -> Result<()> {
    let storage = LocalStorage::new(base_dir.to_path_buf());

    if let Some(pid) = read_daemon_pid(base_dir) {
        unsafe {
            libc::kill(pid as i32, libc::SIGTERM);
        }
        println!("Stopped Threader daemon (pid {pid})");
        let _ = std::fs::remove_file(base_dir.join("threader.pid"));
        // Clean up stale socket
        let _ = std::fs::remove_file(storage.socket_path());
        return Ok(());
    }

    // No PID file, but maybe socket is alive
    if is_daemon_running(base_dir, &storage.socket_path()) {
        eprintln!("Daemon appears to be running but no PID file found.");
        eprintln!("Remove the socket manually: {}", storage.socket_path().display());
        std::process::exit(1);
    }

    println!("Threader daemon is not running");
    Ok(())
}

fn show_status(base_dir: std::path::PathBuf) -> Result<()> {
    let storage = LocalStorage::new(base_dir.clone());
    let queue = crate::storage::queue::UploadQueue::new(base_dir.clone());

    let sessions = storage.list_sessions()?;
    let pending = queue.pending_count()?;
    let daemon_running = is_daemon_running(&base_dir, &storage.socket_path());
    let daemon_pid = read_daemon_pid(&base_dir);

    let auth_status = match crate::auth::storage::load() {
        Ok(Some(creds)) => {
            let who = creds.email.as_deref().unwrap_or(&creds.user_id);
            let expired = creds
                .expires_at
                .map(|exp| Utc::now() >= exp)
                .unwrap_or(false);
            if expired {
                format!("authenticated as {who} (token expired, will refresh)")
            } else {
                format!("authenticated as {who}")
            }
        }
        _ => "not logged in".to_string(),
    };

    println!("Threader Status");
    println!("───────────────");
    println!("Auth: {auth_status}");
    println!(
        "Daemon: {}",
        if daemon_running {
            match daemon_pid {
                Some(pid) => format!("running (pid {pid})"),
                None => "running".to_string(),
            }
        } else {
            "stopped".to_string()
        }
    );
    println!("Sessions: {}", sessions.len());
    println!("Pending uploads: {}", pending);
    println!("Data dir: {}", base_dir.display());

    Ok(())
}

fn show_whoami() -> Result<()> {
    match crate::auth::storage::load() {
        Ok(Some(creds)) => {
            let who = creds.email.as_deref().unwrap_or(&creds.user_id);
            println!("{who}");
        }
        _ => {
            eprintln!("Not logged in. Run `threader login` to authenticate.");
            std::process::exit(1);
        }
    }
    Ok(())
}
