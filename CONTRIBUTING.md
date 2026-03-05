# Contributing to Threader

Thank you for your interest in contributing to Threader! This guide will help you get started.

## Development Setup

### Prerequisites

- [Rust](https://rustup.rs/) (stable toolchain)
- Git

### Getting Started

```sh
git clone https://github.com/sestinj/threader.git
cd threader
cargo build
cargo test
```

### Running Locally

```sh
cargo run -- start    # Start the daemon
cargo run -- status   # Check status
```

## Project Structure

```
src/
  agents/     # Agent-related functionality
  auth/       # Authentication (device flow)
  cli/        # CLI command definitions (clap)
  daemon/     # Background daemon process
  hooks/      # Claude Code hook installation
  storage/    # Local storage management
  sync/       # Session sync logic
  main.rs     # Entry point
  git.rs      # Git utilities
  process.rs  # Process management
```

## Making Changes

### Commit Messages

Use [Conventional Commits](https://www.conventionalcommits.org/):

- `feat:` — New feature
- `fix:` — Bug fix
- `docs:` — Documentation only
- `refactor:` — Code change that neither fixes a bug nor adds a feature
- `test:` — Adding or updating tests
- `chore:` — Maintenance tasks

### Pull Request Process

1. Fork the repository and create a feature branch from `main`
2. Make your changes and ensure `cargo check` and `cargo test` pass
3. Write a clear PR description explaining **what** changed and **why**
4. Request review — PRs require at least one approving review before merge

### Code Style

This project uses `rustfmt` for formatting. Run `cargo fmt` before committing.

## Reporting Issues

Use [GitHub Issues](https://github.com/sestinj/threader/issues) to report bugs or request features. Please use the provided issue templates.

## Questions?

Open a [discussion](https://github.com/sestinj/threader/discussions) or reach out in the issue tracker.
