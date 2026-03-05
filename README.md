<p align="center">
  <a href="https://continue.dev">
    <img src=".github/assets/continue-banner.png" width="800" alt="Continue" />
  </a>
</p>

<h1 align="center">Threader</h1>

<p align="center">Sync and share Claude Code threads. Private by default.</p>

<p align="center"><em>An autonomous codebase built by the <a href="https://continue.dev/blueprint">Continue Software Factory</a></em></p>

---

## Why?

Claude Code sessions contain valuable context — decisions, debugging steps, architectural discussions — but they're trapped on your local machine. Threader syncs them to the cloud so you can share, search, and learn from your team's coding sessions.

## Table of Contents

- [Install](#install)
- [Commands](#commands)
- [How it works](#how-it-works)
- [Architecture](#architecture)
- [Documentation](#documentation)
- [Contributing](#contributing)
- [License](#license)

## Install

### Quick install

```sh
curl -fsSL https://threader.sh/install.sh | sh
```

That's it. The install script will create Claude Code hooks and start the daemon for you. Threader runs invisibly in the background, syncing your sessions to your dashboard at [threader.sh](https://threader.sh).

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

## Architecture

- **Client**: Rust daemon that continuously syncs sessions
- **Auth**: Device flow for CLI authentication
- **Storage**: S3-compatible object storage
- **Organization**: Tag-based (workspaces are special tags with sharing permissions)

### Sharing Model

- **Private** (default): Only visible to you
- **Workspace**: Shared with team members via workspace tags
- **Public**: Visible to anyone

## Documentation

- [Technical Design](docs/DESIGN.md) - Client architecture, onboarding flows, backend design
- [Product Vision](docs/VISION.md) - Network effects, long-term product direction
- [Sync Design](docs/SYNC.md) - Transcript sync correctness principles and architecture

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md) for development setup, project structure, and how to submit changes.

## License

Apache-2.0 — see [LICENSE](LICENSE) for details.
Copyright (c) 2025 Continue Dev, Inc.
