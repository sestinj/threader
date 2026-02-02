# Supported Coding Agents

| Agent | Status | Hook Type | Transcript | Notes |
|-------|--------|-----------|------------|-------|
| Claude Code | Supported | Session hooks | JSONL streaming | Full support |
| Cursor | In Progress | Editor hooks | JSON stdin | |
| Windsurf | Planned | TBD | TBD | |
| Codex | Planned | Notification | JSON args | |
| OpenCode | Planned | TBD | TBD | |
| Pi | Planned | TBD | TBD | |
| Amp | Planned | TBD | Markdown | |

## How It Works

Each agent gets a module in the daemon (`src/agents/`) that implements the `Agent` trait:

- **`detect()`** — Check if the agent is installed on the system
- **`install()`** — Install threader hooks into the agent's configuration
- **`parse_hook_input()`** — Parse the agent's native hook format into a common `HookInput`

When you run `threader init`, all installed agents are auto-detected and hooks are installed for each.

Sessions are tagged with the agent that produced them (e.g. `agent: "claude-code"`). The raw transcript format from each agent is stored as-is and parsed on the frontend by agent-specific parsers.
