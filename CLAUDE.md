# Threader

Threader is a daemon that runs locally and syncs Claude Code sessions to the cloud for easy sharing, organization, and analysis.

## Core Principles

1. **Frictionless onboarding** - Time to value is under 60 seconds: copy a curl command, sign in, send one message, see your dashboard
2. **Invisible by default** - Private-first, with optional invisible mode so developers don't need to disclose usage
3. **Zero learning curve** - Developers shouldn't have to think about how to use it; it just works
4. **Self-hostable** - Easy to run your own instance or set up for a team
5. **Resilient daemon** - Runs continuously without intervention; doesn't turn off unless explicitly stopped

## Architecture Overview

- **Client**: Rust daemon that continuously syncs sessions
- **Auth**: Device flow for CLI authentication
- **Storage**: S3-compatible object storage
- **Organization**: Tag-based (workspaces are special tags with sharing permissions)

## Sharing Model

- **Private** (default): Only visible to you
- **Workspace**: Shared with team members via workspace tags
- **Public**: Visible to anyone

## Documentation

- [Technical Design](docs/DESIGN.md) - Client architecture, onboarding flows, backend design
- [Product Vision](docs/VISION.md) - Network effects, long-term product direction
- [Sync Design](docs/SYNC.md) - Transcript sync correctness principles and architecture
