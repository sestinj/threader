use std::process::Command;

/// Run `git remote get-url origin` in `cwd` and parse `owner/repo` from the result.
pub fn resolve_repo(cwd: &str) -> Option<String> {
    let output = Command::new("git")
        .args(["remote", "get-url", "origin"])
        .current_dir(cwd)
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let url = String::from_utf8(output.stdout).ok()?;
    parse_owner_repo(url.trim())
}

/// Extract `owner/repo` from a git remote URL.
///
/// Handles:
/// - SSH:   `git@github.com:owner/repo.git`
/// - HTTPS: `https://github.com/owner/repo.git`
/// - HTTPS: `https://github.com/owner/repo`
fn parse_owner_repo(url: &str) -> Option<String> {
    // SSH: git@host:owner/repo.git
    if let Some(path) = url
        .strip_prefix("git@")
        .and_then(|s| s.split_once(':').map(|(_, p)| p))
    {
        let path = path.strip_suffix(".git").unwrap_or(path);
        if path.contains('/') {
            return Some(path.to_string());
        }
    }

    // HTTPS/HTTP: https://host/owner/repo.git
    if let Ok(parsed) = url::Url::parse(url) {
        let path = parsed.path().trim_start_matches('/');
        let path = path.strip_suffix(".git").unwrap_or(path);
        // Expect at least owner/repo (2 segments)
        if path.matches('/').count() >= 1 {
            return Some(path.to_string());
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ssh_url() {
        assert_eq!(
            parse_owner_repo("git@github.com:sestinj/threader.git"),
            Some("sestinj/threader".to_string())
        );
    }

    #[test]
    fn https_url_with_git_suffix() {
        assert_eq!(
            parse_owner_repo("https://github.com/sestinj/threader.git"),
            Some("sestinj/threader".to_string())
        );
    }

    #[test]
    fn https_url_without_git_suffix() {
        assert_eq!(
            parse_owner_repo("https://github.com/sestinj/threader"),
            Some("sestinj/threader".to_string())
        );
    }

    #[test]
    fn ssh_no_suffix() {
        assert_eq!(
            parse_owner_repo("git@github.com:owner/repo"),
            Some("owner/repo".to_string())
        );
    }

    #[test]
    fn invalid_url() {
        assert_eq!(parse_owner_repo("not-a-url"), None);
    }
}
