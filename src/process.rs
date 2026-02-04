use std::process::Command;

/// Walk up the process tree to find the ancestor whose command name is "claude".
/// Returns the PID of the claude process, or None.
pub fn find_claude_ancestor_pid() -> Option<u32> {
    let mut pid = std::process::id();
    for _ in 0..10 {
        let ppid = get_ppid(pid)?;
        if ppid <= 1 {
            return None;
        }
        if get_process_name(ppid)
            .map(|n| n == "claude")
            .unwrap_or(false)
        {
            return Some(ppid);
        }
        pid = ppid;
    }
    None
}

fn get_ppid(pid: u32) -> Option<u32> {
    let output = Command::new("ps")
        .args(["-o", "ppid=", "-p", &pid.to_string()])
        .output()
        .ok()?;
    String::from_utf8_lossy(&output.stdout)
        .trim()
        .parse()
        .ok()
}

fn get_process_name(pid: u32) -> Option<String> {
    let output = Command::new("ps")
        .args(["-o", "comm=", "-p", &pid.to_string()])
        .output()
        .ok()?;
    let name = String::from_utf8_lossy(&output.stdout).trim().to_string();
    // comm may include full path, extract basename
    name.rsplit('/').next().map(|s| s.to_string())
}
