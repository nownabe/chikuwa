# Agents Guide

## Build & Test

```sh
cargo build     # Build
cargo test      # Run all tests
cargo run       # Run TUI (requires tmux)
cargo run -- hook          # Run hook mode (reads event from stdin JSON, requires TMUX_PANE)
```

The `RUSTUP_TOOLCHAIN` env var may override `rust-toolchain.toml`. If you hit version errors, prefix commands with `unset RUSTUP_TOOLCHAIN &&`.

## Project Structure

```
src/
  main.rs              # CLI entry point (clap subcommands)
  app.rs               # TUI app state and main event loop
  event.rs             # Keyboard/timer event handling
  hook.rs              # `chikuwa hook` subcommand (stdin JSON → IPC state)
  agent/
    state.rs           # AgentState struct, state file read/write
  tmux/
    client.rs          # tmux command execution, output parsing, tree building
    types.rs           # TmuxSession/TmuxWindow/TmuxPane structs
  ui/
    tree.rs            # Tree view widget (flatten + render)
    status_bar.rs      # Bottom status bar
    theme.rs           # Colors, icons, styles
```

## Architecture

Two modes in a single binary:

- **TUI mode** (`chikuwa`): Polls `tmux list-panes -a` every 2 seconds, reads state files from `$XDG_RUNTIME_DIR/chikuwa/`, and renders a tree view with ratatui.
- **Hook mode** (`chikuwa hook`): Reads JSON from stdin, determines event type via `hook_event_name` field, and sends state via IPC. Called by Claude Code hooks.

State files are JSON at `$XDG_RUNTIME_DIR/chikuwa/<TMUX_PANE>.json` (fallback: `/tmp/chikuwa/`).

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

## Conventions

- Keep dependencies minimal. Prefer standard library where possible.
- All public logic should have unit tests in `#[cfg(test)]` modules within the same file.
- Use `anyhow::Result` for error handling throughout.
- UI rendering uses ratatui. Styles and icons are centralized in `ui/theme.rs`.
