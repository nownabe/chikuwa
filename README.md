# chikuwa

A sidebar TUI for monitoring multiple AI agents (Claude Code, etc.) running in tmux sessions at a glance.

```
┌─ project  main ───────────────────┐
│  󱂪 0:claude 󰚩                      │
│    · running                        │
│     Bash: cargo test              │
│     Read: src/main.rs             │
│     main                          │
│  󱂪 1:nvim                           │
│  󱂪 2:zsh  ~/s/g/n/chikuwa          │
├─ other-project  feat ─────────────┤
│  󱂪 0:claude 󰚩                      │
│     waiting                        │
│     feat-branch                   │
│  󱂪 1:zsh                            │
└───────────────────────────────────┘
 3 agents │ · 2 run  1 wait
```

## Features

- **Real-time agent monitoring** — See all running Claude Code agents across tmux sessions with animated status spinners
- **Active tool display** — Shows what each agent is currently doing (e.g., `Bash: cargo test`, `Read: src/main.rs`), including multiple concurrent tools
- **Instant tmux updates** — Registers tmux hooks for immediate response to pane/window/session changes, with periodic polling as a fallback
- **Git integration** — Displays current branch, repo name, and open PR info per session
- **Nvim integration** — Shows the file being edited in nvim panes with relative paths
- **Claude API usage** — Displays 5-hour and 7-day usage gauges in the status bar
- **Keyboard & mouse navigation** — Navigate and switch between tmux windows/panes with vim keys or mouse clicks
- **Status bar** — Summary of all agents (running, waiting, permission) and API usage

## How It Works

A single binary that operates in three modes:

| Mode | Command | Description |
|---|---|---|
| TUI | `chikuwa` | Displays tmux sessions/windows/panes as a tree with real-time agent status |
| Hook | `chikuwa hook` | Called from Claude Code hooks; reads event JSON from stdin to update agent status via IPC |
| Notify | `chikuwa notify` | Called from tmux hooks; signals the TUI to refresh immediately |

```
Claude Code ──(hooks)──→ chikuwa hook ──(IPC)──→ chikuwa (TUI)
tmux ──(hooks)──→ chikuwa notify ──(IPC)──→ chikuwa (TUI)
tmux ──(list-panes -a, polling)────────────←── chikuwa (TUI)
git ──(branch, gh pr)──────────────────────←── chikuwa (TUI)
```

The TUI registers tmux hooks (e.g., `after-select-pane`, `session-created`) on startup. When tmux fires a hook, it runs `chikuwa notify` which signals the TUI to refresh immediately. Periodic polling still runs as a fallback for changes that hooks don't cover (e.g., `pane_current_command`, `pane_current_path`). Hooks are automatically cleaned up on exit.

## Installation

### From crates.io

```sh
cargo install chikuwa
```

### From source

```sh
cargo install --path .
```

### Nix

```sh
nix profile install github:nownabe/chikuwa
```

Pre-built binaries are available via [Cachix](https://app.cachix.org/cache/chikuwa):

```sh
cachix use chikuwa
nix profile install github:nownabe/chikuwa
```

Requires a [Nerd Font](https://www.nerdfonts.com/) for icons.

## Usage

### Starting the TUI

Run outside of tmux (e.g., in a separate terminal pane):

```sh
chikuwa
```

#### Options

| Flag | Description |
|---|---|
| `--store-events` | Log all received hook events to `$XDG_RUNTIME_DIR/chikuwa/events.jsonl` for debugging |

### Key Bindings

| Key | Action |
|---|---|
| `j` / `↓` | Move down |
| `k` / `↑` | Move up |
| `Enter` / `Space` | Toggle session collapse / switch tmux |
| `g` | Jump to top |
| `G` | Jump to bottom |
| `q` / `Ctrl+C` | Quit |

Mouse clicks on tree items are also supported.

### Claude Code Hooks Setup

Add the following to `~/.claude/settings.json`:

```json
{
  "hooks": {
    "SessionStart": [{"hooks": [{"type": "command", "command": "chikuwa hook"}]}],
    "UserPromptSubmit": [{"hooks": [{"type": "command", "command": "chikuwa hook"}]}],
    "PreToolUse": [{"hooks": [{"type": "command", "command": "chikuwa hook"}]}],
    "PostToolUse": [{"hooks": [{"type": "command", "command": "chikuwa hook"}]}],
    "PostToolUseFailure": [{"hooks": [{"type": "command", "command": "chikuwa hook"}]}],
    "PermissionRequest": [{"hooks": [{"type": "command", "command": "chikuwa hook"}]}],
    "Notification": [{"hooks": [{"type": "command", "command": "chikuwa hook"}]}],
    "Stop": [{"hooks": [{"type": "command", "command": "chikuwa hook"}]}],
    "SubagentStart": [{"hooks": [{"type": "command", "command": "chikuwa hook"}]}],
    "SubagentStop": [{"hooks": [{"type": "command", "command": "chikuwa hook"}]}],
    "SessionEnd": [{"hooks": [{"type": "command", "command": "chikuwa hook"}]}]
  }
}
```

The hook reads `hook_event_name`, `tool_name`, and `tool_input` from stdin JSON to determine agent status and active tools.

### tmux Hooks Setup

tmux hooks are automatically managed by the TUI — no manual configuration is needed.

On startup, the TUI registers global tmux hooks (at array index `[42]`) for events like `after-select-pane`, `client-session-changed`, `window-linked`, etc. When tmux fires one of these hooks, it runs `chikuwa notify` in the background to signal an immediate refresh. On exit, the hooks are automatically unregistered.

If you need to manually clean up stale hooks (e.g., after a crash), run:

```sh
tmux show-hooks -g | grep chikuwa   # check for leftover hooks
chikuwa notify                       # or just start and quit the TUI to clean up
```

### Claude API Usage Display

The status bar shows Claude API usage gauges (5-hour and 7-day utilization). This requires being logged in to Claude Code via OAuth — credentials are read from `~/.claude/.credentials.json`. Usage is polled every 10 minutes.

## Development

```sh
cargo build   # Build
cargo test    # Run all tests
cargo run     # Run TUI (requires tmux)
```

See [AGENTS.md](AGENTS.md) for detailed architecture and development guide.

## License

Apache-2.0
