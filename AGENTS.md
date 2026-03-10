# Agents Guide

## Build & Test

```sh
cargo build     # Build
cargo test      # Run all tests
cargo run       # Run TUI (requires tmux)
cargo run -- hook          # Run hook mode (reads event from stdin JSON, requires TMUX_PANE)
cargo run -- notify        # Send refresh signal to running TUI
```

The `RUSTUP_TOOLCHAIN` env var may override `rust-toolchain.toml`. If you hit version errors, prefix commands with `unset RUSTUP_TOOLCHAIN &&`.

## Project Structure

```
src/
  main.rs              # CLI entry (clap): TUI / hook / notify subcommands
  app.rs               # TUI app state, async event loop, rendering orchestration
  event.rs             # Keyboard/mouse/timer event handling (crossterm)
  hook.rs              # `chikuwa hook`: stdin JSON → AgentState → IPC
  ipc.rs               # Unix domain socket IPC (client send_state/send_notify, server listener)
  git.rs               # Git branch/PR/repo info with per-path TTL caching
  usage.rs             # Claude API usage polling (OAuth, exponential backoff)
  agent/
    mod.rs
    state.rs           # AgentState, AgentStatus enum, ToolInfo, serialization
  tmux/
    mod.rs
    client.rs          # tmux command execution, tree building, hook registration
    types.rs           # TmuxSession / TmuxWindow / TmuxPane structs
  ui/
    mod.rs
    tree.rs            # Tree view: flatten, render, navigate, mouse hit-test
    status_bar.rs      # Bottom bar: agent counts + usage gauges
    theme.rs           # 3-color palette, NerdFont icons, status styling
```

## Architecture

Single binary, three subcommands:

| Subcommand | Purpose |
|---|---|
| (none) | TUI mode — renders tree view, polls tmux, listens on IPC |
| `hook` | Hook mode — called by Claude Code hooks, sends AgentState via IPC |
| `notify` | Notify mode — called by tmux hooks, sends refresh signal via IPC |

### Data Flow

```
Claude Code ──(hooks)──→ chikuwa hook ──(IPC)──→ chikuwa TUI
tmux ──(hooks)──→ chikuwa notify ──(IPC)──→ chikuwa TUI
tmux ──(list-panes -a, polling every 2s)──←── chikuwa TUI
git ──(rev-parse, gh pr view)──────────────←── chikuwa TUI
Anthropic API ──(usage, polling ~600s)─────←── chikuwa TUI
```

### IPC

Unix domain socket at `$XDG_RUNTIME_DIR/chikuwa.sock` (fallback: `/tmp/chikuwa.sock`).

Two message types:
- AgentState JSON (one line) → `AppEvent::AgentStateUpdate`
- `"notify"` string → `AppEvent::TmuxChanged`

### TUI Async Tasks

The TUI spawns four concurrent tasks:

1. **Event loop** (blocking thread) — keyboard, mouse, 1Hz tick
2. **IPC listener** — AgentState updates + tmux notify signals
3. **Animation ticker** — 150ms spinner frame advance
4. **Usage poller** — 600s base interval, exponential backoff on 429

### Agent State Merging

When an `AgentStateUpdate` arrives:
- `SessionEnd` → remove agent from state map
- Otherwise → merge with existing state:
  - Preserve `session_id` if incoming is `None`
  - `PreToolUse` → append tool to active list
  - `PostToolUse` / `PostToolUseFailure` → remove tool from list
  - Other events → keep existing tools

### Hook Event Mapping

| Claude Code Event | AgentStatus |
|---|---|
| `SessionStart` | `Started` |
| `UserPromptSubmit`, `PreToolUse`, `PostToolUse`, `PostToolUseFailure`, `SubagentStart`, `SubagentStop` | `Running` |
| `Stop` | `Waiting` |
| `PermissionRequest` | `Permission` |
| `Notification` (containing "permission_prompt") | `Permission` |
| `SessionEnd` | `Ended` |

### Tool Detail Extraction (`hook.rs`)

| Tool | Detail Source |
|---|---|
| `Bash` | `tool_input.command` |
| `Read` | `tool_input.file_path` (+ `:offset` if present) |
| `Write`, `Edit` | `tool_input.file_path` |
| `NotebookEdit` | `tool_input.notebook_path` |
| `Grep`, `Glob` | `tool_input.pattern` |
| `WebFetch` | `tool_input.url` |
| `WebSearch` | `tool_input.query` |
| `Task` | `tool_input.description` |

## Key Types

### AgentState (`agent/state.rs`)

```rust
pub enum AgentStatus { Started, Running, Waiting, Permission, Ended }

pub struct ToolInfo {
    pub name: String,
    pub detail: Option<String>,
}

pub struct AgentState {
    pub tmux_pane: String,
    pub session_id: Option<String>,
    pub state: AgentStatus,
    pub updated_at: u64,
    pub hook_event_name: Option<String>,
    pub tool_name: Option<String>,
    pub tool_detail: Option<String>,
    pub tools: Vec<ToolInfo>,
}
```

### Tmux Types (`tmux/types.rs`)

```rust
pub struct TmuxSession {
    pub session_name: String,
    pub session_attached: bool,
    pub windows: Vec<TmuxWindow>,
    pub repo_name: Option<String>,
    pub toplevel: Option<String>,
    pub worktree_name: Option<String>,
}

pub struct TmuxWindow {
    pub window_index: u32,
    pub window_name: String,
    pub window_active: bool,
    pub panes: Vec<TmuxPane>,
}

pub struct TmuxPane {
    pub pane_id: String,
    pub pane_index: u32,
    pub pane_current_command: String,
    pub pane_current_path: String,
    pub pane_title: String,
    pub pane_active: bool,
    pub agent_state: Option<AgentState>,
    pub git_info: Option<GitInfo>,
}
```

### Git (`git.rs`)

```rust
pub struct GitInfo {
    pub branch: Option<String>,
    pub pr: Option<PrInfo>,
    pub repo_name: Option<String>,
    pub toplevel: Option<String>,
    pub worktree_name: Option<String>,
}
```

Cache TTLs: branch 2s, PR 60s, repo/toplevel/worktree cached once per path.

### Usage (`usage.rs`)

```rust
pub struct Usage { pub five_hour: f64, pub seven_day: f64 }
```

Endpoint: `https://api.anthropic.com/api/oauth/usage`
Auth: Bearer token from `~/.claude/.credentials.json` (`claudeAiOauth.accessToken`)

## Tmux Integration

### Tree Building (`tmux/client.rs`)

Executes `tmux list-panes -a -F "<format>"` with 11 tab-separated fields. Parses output into `Vec<TmuxSession>` with deduplication and agent state merging.

### Tmux Hooks

10 hooks registered at array index `[42]`:
`after-select-pane`, `after-select-window`, `client-session-changed`, `session-created`, `session-closed`, `session-renamed`, `window-linked`, `window-unlinked`, `window-renamed`, `pane-exited`

Each runs `chikuwa notify` in background via `run-shell -b`.
Auto-registered on TUI start, auto-unregistered on exit.

### Navigation

`detect_client()` finds the most recently attached tmux client TTY.
`switch_to(client_tty, target)` switches the client to a `session:window.pane` target.

## UI Rendering

### Tree View (`ui/tree.rs`)

Flattening: sessions → windows → panes. Single-pane windows embed pane details in the Window item. Multi-pane windows show Window + individual Pane items.

Visual rows include: session borders (top/bottom), item main row, agent status sub-lines (tools), git info sub-lines (branch + PR title with word wrapping).

Session title bar has a wave/shine animation when an agent is Running (40-step cycle, purple → white → purple).

### Path Abbreviation

Long paths are shortened: `/home/user/src/github.com/nownabe/chikuwa` → `~/s/g/n/chikuwa`. Preserves first and last components, abbreviates intermediates, stops when ≤ 30 chars.

### Nvim Integration

Detects `pane_current_command == "nvim"`, extracts filename from `pane_title` (handles legacy format `"file (dir) - Nvim"` and NerdFont icon prefix format). Caches titles to handle plugin UI takeover. Computes relative path from git toplevel.

## Pre-commit Checklist

Before committing, always run:

1. `cargo fmt` — format code
2. `cargo clippy` — check for lint warnings
3. `cargo test` — run all tests

## Commit Messages

Follow [Conventional Commits](https://www.conventionalcommits.org/). Common types:

- `feat` — new feature
- `fix` — bug fix
- `style` — visual/formatting changes (no behavior change)
- `refactor` — code restructuring (no feature or fix)
- `perf` — performance improvement
- `test` — add/update tests
- `docs` — documentation only
- `chore` — build, CI, tooling, etc.

## Color Palette (CRITICAL)

The TUI uses **only** three colors: **white**, **light purple**, and **purple** (defined in `ui/theme.rs` as `COLOR_WHITE`, `COLOR_LIGHT_PURPLE`, `COLOR_PURPLE`). All UI elements — text, icons, gauges, highlights — MUST use only these three colors. Never introduce additional colors (no green, red, yellow, etc.).

Exception: `COLOR_YELLOW` is used solely for the bolt icon (`ICON_BOLT`).

## Conventions

- Keep dependencies minimal. Prefer standard library where possible.
- All public logic should have unit tests in `#[cfg(test)]` modules within the same file.
- Use `anyhow::Result` for error handling throughout.
- UI rendering uses ratatui. Styles and icons are centralized in `ui/theme.rs`.
- Async runtime is tokio. The event loop runs on a blocking thread; everything else is async.

## CI/CD

- **PR checks** (`pr.yaml`): format (`cargo fmt --check`), lint (`cargo clippy -- -D warnings`), test (`cargo test`)
- **Release** (`release.yaml`): release-plz versioning, binary upload, flake.nix sync
- Rust toolchain: stable (from `rust-toolchain.toml`)
