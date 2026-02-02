# Threader

Sync your [Claude Code](https://claude.ai) sessions to the cloud for easy sharing, organization, and analysis.

## Install

### Quick install (macOS / Linux)

```sh
curl -fsSL https://threader.sh/install.sh | sh
```

### Homebrew

```sh
brew tap sestinj/tap
brew install threader
```

### From source

```sh
cargo install --git https://github.com/sestinj/threader
```

## Getting started

```sh
threader login    # authenticate via browser
threader init     # install Claude Code hooks
threader start    # start the background daemon
```

That's it. Threader runs invisibly in the background, syncing your sessions to your dashboard at [threader.sh](https://threader.sh).

## Commands

| Command            | Description                              |
| ------------------ | ---------------------------------------- |
| `threader login`   | Authenticate via browser                 |
| `threader logout`  | Clear stored credentials                 |
| `threader init`    | Install hooks and create data dirs       |
| `threader start`   | Start daemon in background               |
| `threader stop`    | Stop the daemon                          |
| `threader status`  | Show daemon status and session info      |
| `threader whoami`  | Show current authenticated user          |

## How it works

Threader installs lightweight hooks into Claude Code that notify a local daemon whenever a session starts, stops, or a message is received. The daemon reads the session data from Claude Code's local storage and syncs it to the cloud.

Sessions are **private by default**. You can share them via workspaces or make them public from the dashboard.

## License

Apache-2.0
