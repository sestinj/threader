use std::process::Command;

use anyhow::{Context, Result};
use tracing::{debug, warn};

/// Supported macOS terminal emulators.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Terminal {
    ITerm2,
    Ghostty,
    Alacritty,
    Kitty,
    WezTerm,
    TerminalApp,
}

impl Terminal {
    /// Process name as reported by macOS System Events.
    fn process_name(self) -> &'static str {
        match self {
            Terminal::ITerm2 => "iTerm2",
            Terminal::Ghostty => "Ghostty",
            Terminal::Alacritty => "Alacritty",
            Terminal::Kitty => "kitty",
            Terminal::WezTerm => "WezTerm",
            Terminal::TerminalApp => "Terminal",
        }
    }
}

/// Detection priority order (most preferred first).
const DETECTION_ORDER: &[Terminal] = &[
    Terminal::ITerm2,
    Terminal::Ghostty,
    Terminal::Alacritty,
    Terminal::Kitty,
    Terminal::WezTerm,
    Terminal::TerminalApp,
];

/// Detect the user's running terminal by querying macOS System Events.
pub fn detect_terminal() -> Terminal {
    let script = r#"tell application "System Events" to get name of every application process"#;
    let output = Command::new("osascript").args(["-e", script]).output();

    let processes = match output {
        Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout).to_string(),
        _ => {
            debug!("Failed to query running applications, falling back to Terminal.app");
            return Terminal::TerminalApp;
        }
    };

    for terminal in DETECTION_ORDER {
        if processes.contains(terminal.process_name()) {
            debug!("Detected terminal: {:?}", terminal);
            return *terminal;
        }
    }

    debug!("No known terminal detected, falling back to Terminal.app");
    Terminal::TerminalApp
}

/// Shell-escape a string for use in AppleScript (double backslashes and escape quotes).
fn applescript_escape(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

/// Open a terminal window, cd to `cwd`, and run `command`.
pub fn open_in_terminal(terminal: Terminal, cwd: &str, command: &str) -> Result<()> {
    let result = launch(terminal, cwd, command);
    if result.is_err() && terminal != Terminal::TerminalApp {
        warn!(
            "Failed to launch {:?}, falling back to Terminal.app: {:?}",
            terminal,
            result.as_ref().err()
        );
        return launch(Terminal::TerminalApp, cwd, command);
    }
    result
}

fn launch(terminal: Terminal, cwd: &str, command: &str) -> Result<()> {
    match terminal {
        Terminal::ITerm2 => launch_iterm2(cwd, command),
        Terminal::TerminalApp => launch_terminal_app(cwd, command),
        Terminal::Ghostty => launch_cli("ghostty", &["-e", "bash", "-c"], cwd, command),
        Terminal::Alacritty => {
            launch_cli("alacritty", &["--working-directory"], cwd, command)
        }
        Terminal::Kitty => launch_cli("kitty", &["--directory"], cwd, command),
        Terminal::WezTerm => launch_cli("wezterm", &["start", "--cwd"], cwd, command),
    }
}

fn launch_iterm2(cwd: &str, command: &str) -> Result<()> {
    let escaped_cwd = applescript_escape(cwd);
    let escaped_cmd = applescript_escape(command);
    let script = format!(
        r#"
        tell application "iTerm"
            activate
            set newWindow to (create window with default profile)
            tell current session of newWindow
                write text "cd \"{escaped_cwd}\" && {escaped_cmd}"
            end tell
        end tell
        "#
    );
    run_applescript(&script)
}

fn launch_terminal_app(cwd: &str, command: &str) -> Result<()> {
    let escaped_cwd = applescript_escape(cwd);
    let escaped_cmd = applescript_escape(command);
    let script = format!(
        r#"
        tell application "Terminal"
            activate
            do script "cd \"{escaped_cwd}\" && {escaped_cmd}"
        end tell
        "#
    );
    run_applescript(&script)
}

fn launch_cli(bin: &str, args: &[&str], cwd: &str, command: &str) -> Result<()> {
    let full_cmd = format!("cd '{}' && {}", cwd.replace('\'', "'\\''"), command);

    let mut cmd = Command::new(bin);
    // For alacritty/kitty/wezterm, the working directory flag takes the path,
    // then we pass -e bash -c to run the command.
    // For ghostty, -e bash -c is already in args.
    if bin == "ghostty" {
        cmd.args(args);
        cmd.arg(&full_cmd);
    } else {
        cmd.args(args);
        cmd.arg(cwd);
        cmd.args(["-e", "bash", "-c", &full_cmd]);
    }

    let status = cmd
        .spawn()
        .with_context(|| format!("Failed to spawn {bin}"))?;

    debug!("Launched {bin} with pid {:?}", status.id());
    Ok(())
}

fn run_applescript(script: &str) -> Result<()> {
    let output = Command::new("osascript")
        .args(["-e", script])
        .output()
        .context("Failed to run osascript")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("AppleScript failed: {}", stderr.trim());
    }
    Ok(())
}
