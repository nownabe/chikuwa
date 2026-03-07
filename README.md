# chikuwa

A sidebar TUI for monitoring multiple AI agents (Claude Code, etc.) running in tmux sessions at a glance.

```
┌─ chikuwa ─────────┐
│ ▾ 📂 main *        │
│  ├ 0:claude ✦      │
│  └ 1:zsh           │
│ ▾ 📂 feature-x     │
│  └ 0:claude ❯      │
│ ▾ 📂 debug         │
│  ├ 0:claude ✗      │
│  └ 1:zsh           │
│────────────────────│
│ 3 agents │ 1 wait  │
└────────────────────┘
```

## How It Works

A single binary that operates in two modes:

- **`chikuwa`** — TUI mode. Displays tmux sessions/windows/panes as a tree with real-time agent status.
- **`chikuwa hook <event>`** — Hook mode. Called from Claude Code hooks to update state files.

```
Claude Code ──(hooks)──→ chikuwa hook <event> ──→ state files ←── chikuwa (TUI)
                                                  $XDG_RUNTIME_DIR/chikuwa/
tmux ──(list-panes -a)──────────────────────────────────────←── chikuwa (TUI)
```

## Installation

```sh
cargo install --path .
```

## Usage

### Starting the TUI

Run outside of tmux (e.g., in a separate Windows Terminal pane):

```sh
chikuwa
```

### Key Bindings

| Key | Action |
|---|---|
| `j` / `↓` | Move down |
| `k` / `↑` | Move up |
| `Enter` / `Space` | Toggle session collapse / switch tmux |
| `g` | Jump to top |
| `G` | Jump to bottom |
| `q` / `Ctrl+C` | Quit |

### Agent Status Icons

| Icon | Status | Color |
|---|---|---|
| `✦` | Running | Yellow |
| `❯` | Waiting (input required) | Green |
| `⚠` | Permission (approval needed) | Magenta |
| `⏸` | Started | Gray |
| `✗` | Error | Red |

### Claude Code Hooks Setup

Add the following to `~/.claude/settings.json`:

```json
{
  "hooks": {
    "UserPromptSubmit": [{"hooks": [{"type": "command", "command": "chikuwa hook running"}]}],
    "Stop": [{"hooks": [{"type": "command", "command": "chikuwa hook waiting"}]}],
    "Notification": [{"matcher": "*", "hooks": [{"type": "command", "command": "chikuwa hook notification"}]}],
    "SessionStart": [{"hooks": [{"type": "command", "command": "chikuwa hook started"}]}],
    "SessionEnd": [{"hooks": [{"type": "command", "command": "chikuwa hook ended"}]}]
  }
}
```

## Development

```sh
# Build
cargo build

# Test
cargo test

# Run
cargo run
```

## License

MIT
