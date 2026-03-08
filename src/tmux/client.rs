use std::collections::HashMap;

use anyhow::{Context, Result};
use tokio::process::Command;

use super::types::{TmuxPane, TmuxSession, TmuxWindow};
use crate::agent::state::AgentState;

/// Fetch all tmux sessions/windows/panes and build a tree, merging agent states.
pub async fn fetch_tree(agent_states: &HashMap<String, AgentState>) -> Result<Vec<TmuxSession>> {
    let raw = list_panes_all().await?;
    Ok(build_tree(&raw, agent_states))
}

/// Run `tmux list-panes -a` with a custom format and return raw output.
async fn list_panes_all() -> Result<String> {
    let output = Command::new("tmux")
        .args([
            "list-panes",
            "-a",
            "-F",
            "#{session_name}\t#{session_attached}\t#{window_index}\t#{window_name}\t#{window_active}\t#{pane_id}\t#{pane_index}\t#{pane_current_command}\t#{pane_active}\t#{pane_current_path}\t#{pane_title}",
        ])
        .output()
        .await
        .context("Failed to execute tmux list-panes")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("tmux list-panes failed: {}", stderr.trim());
    }

    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

/// Parse raw tmux output into a tree of sessions > windows > panes.
fn build_tree(raw: &str, agent_states: &HashMap<String, AgentState>) -> Vec<TmuxSession> {
    let mut sessions: Vec<TmuxSession> = Vec::new();
    let mut session_map: HashMap<String, usize> = HashMap::new();

    for line in raw.lines() {
        let fields: Vec<&str> = line.split('\t').collect();
        if fields.len() < 11 {
            continue;
        }

        let session_name = fields[0].to_string();
        let session_attached = fields[1] == "1";
        let window_index: u32 = fields[2].parse().unwrap_or(0);
        let window_name = fields[3].to_string();
        let window_active = fields[4] == "1";
        let pane_id = fields[5].to_string();
        let pane_index: u32 = fields[6].parse().unwrap_or(0);
        let pane_current_command = fields[7].to_string();
        let pane_active = fields[8] == "1";
        let pane_current_path = fields[9].to_string();
        let pane_title = fields[10].to_string();

        let agent_state = agent_states.get(&pane_id).cloned();

        let pane = TmuxPane {
            pane_id,
            pane_index,
            pane_current_command,
            pane_current_path,
            pane_title,
            pane_active,
            agent_state,
            git_info: None,
        };

        let session_idx = if let Some(&idx) = session_map.get(&session_name) {
            idx
        } else {
            let idx = sessions.len();
            sessions.push(TmuxSession {
                session_name: session_name.clone(),
                session_attached,
                windows: Vec::new(),
                repo_name: None,
                toplevel: None,
                worktree_name: None,
            });
            session_map.insert(session_name, idx);
            idx
        };

        let session = &mut sessions[session_idx];

        // Find or create window
        if let Some(window) = session
            .windows
            .iter_mut()
            .find(|w| w.window_index == window_index)
        {
            window.panes.push(pane);
        } else {
            session.windows.push(TmuxWindow {
                window_index,
                window_name,
                window_active,
                panes: vec![pane],
            });
        }
    }

    sessions
}

/// Detect the target tmux client (most recently attached).
pub async fn detect_client() -> Result<Option<String>> {
    let output = Command::new("tmux")
        .args(["list-clients", "-F", "#{client_tty}"])
        .output()
        .await
        .context("Failed to execute tmux list-clients")?;

    if !output.status.success() {
        return Ok(None);
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    Ok(stdout.lines().next().map(|s| s.to_string()))
}

/// Switch tmux client to a given target (session, window, or pane).
pub async fn switch_to(client_tty: &str, target: &str) -> Result<()> {
    let output = Command::new("tmux")
        .args(["switch-client", "-c", client_tty, "-t", target])
        .output()
        .await
        .context("Failed to execute tmux switch-client")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("tmux switch-client failed: {}", stderr.trim());
    }

    Ok(())
}

/// Hook names to register for instant tmux change notifications.
const HOOK_NAMES: &[&str] = &[
    "after-select-pane",
    "after-select-window",
    "client-session-changed",
    "session-created",
    "session-closed",
    "session-renamed",
    "window-linked",
    "window-unlinked",
    "window-renamed",
    "pane-exited",
];

/// Hook array index used to avoid conflicts with user hooks.
const HOOK_INDEX: u32 = 42;

/// Register tmux hooks that notify the TUI on structural changes.
pub async fn register_hooks() -> Result<()> {
    let exe = std::env::current_exe()
        .context("Failed to get current executable path")?
        .to_string_lossy()
        .to_string();

    for hook_name in HOOK_NAMES {
        let hook_arg = format!("{}[{}]", hook_name, HOOK_INDEX);
        let cmd = format!("run-shell -b '{} notify'", exe);
        let output = Command::new("tmux")
            .args(["set-hook", "-g", &hook_arg, &cmd])
            .output()
            .await;

        if let Err(e) = output {
            anyhow::bail!("Failed to register tmux hook {}: {}", hook_name, e);
        }
    }

    Ok(())
}

/// Unregister tmux hooks. Ignores errors (hooks may not exist).
pub async fn unregister_hooks() {
    for hook_name in HOOK_NAMES {
        let hook_arg = format!("{}[{}]", hook_name, HOOK_INDEX);
        let _ = Command::new("tmux")
            .args(["set-hook", "-gu", &hook_arg])
            .output()
            .await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::state::{AgentState, AgentStatus};

    #[test]
    fn test_build_tree_single_session_single_window() {
        let raw = "main\t1\t0\tzsh\t1\t%0\t0\tbash\t1\t/home/user\tuser@host\n";
        let tree = build_tree(raw, &HashMap::new());

        assert_eq!(tree.len(), 1);
        assert_eq!(tree[0].session_name, "main");
        assert!(tree[0].session_attached);
        assert_eq!(tree[0].windows.len(), 1);
        assert_eq!(tree[0].windows[0].window_name, "zsh");
        assert_eq!(tree[0].windows[0].panes.len(), 1);
        assert_eq!(tree[0].windows[0].panes[0].pane_current_command, "bash");
        assert_eq!(tree[0].windows[0].panes[0].pane_current_path, "/home/user");
        assert_eq!(tree[0].windows[0].panes[0].pane_title, "user@host");
    }

    #[test]
    fn test_build_tree_multiple_sessions() {
        let raw = "main\t1\t0\tzsh\t1\t%0\t0\tbash\t1\t/home\t\n\
                    dev\t0\t0\tvim\t1\t%1\t0\tvim\t1\t/tmp\t\n";
        let tree = build_tree(raw, &HashMap::new());

        assert_eq!(tree.len(), 2);
        assert_eq!(tree[0].session_name, "main");
        assert!(tree[0].session_attached);
        assert_eq!(tree[1].session_name, "dev");
        assert!(!tree[1].session_attached);
    }

    #[test]
    fn test_build_tree_multiple_windows() {
        let raw = "main\t1\t0\tzsh\t1\t%0\t0\tbash\t1\t/home\t\n\
                    main\t1\t1\tvim\t0\t%1\t0\tvim\t1\t/tmp\t\n";
        let tree = build_tree(raw, &HashMap::new());

        assert_eq!(tree.len(), 1);
        assert_eq!(tree[0].windows.len(), 2);
        assert_eq!(tree[0].windows[0].window_name, "zsh");
        assert_eq!(tree[0].windows[1].window_name, "vim");
    }

    #[test]
    fn test_build_tree_multiple_panes() {
        let raw = "main\t1\t0\tzsh\t1\t%0\t0\tbash\t1\t/home\t\n\
                    main\t1\t0\tzsh\t1\t%1\t1\tvim\t0\t/tmp\t\n";
        let tree = build_tree(raw, &HashMap::new());

        assert_eq!(tree.len(), 1);
        assert_eq!(tree[0].windows.len(), 1);
        assert_eq!(tree[0].windows[0].panes.len(), 2);
        assert_eq!(tree[0].windows[0].panes[0].pane_id, "%0");
        assert_eq!(tree[0].windows[0].panes[1].pane_id, "%1");
    }

    #[test]
    fn test_build_tree_with_agent_state() {
        let raw = "main\t1\t0\tclaude\t1\t%0\t0\tnode\t1\t/project\t\n";
        let mut agents = HashMap::new();
        agents.insert(
            "%0".to_string(),
            AgentState {
                tmux_pane: "%0".to_string(),
                session_id: Some("sess1".to_string()),
                state: AgentStatus::Running,
                updated_at: 100,
                hook_event_name: None,
                tool_name: None,
                tool_detail: None,
                tools: Vec::new(),
            },
        );

        let tree = build_tree(raw, &agents);
        let pane = &tree[0].windows[0].panes[0];
        assert!(pane.agent_state.is_some());
        assert_eq!(
            pane.agent_state.as_ref().unwrap().state,
            AgentStatus::Running
        );
        assert_eq!(pane.pane_current_path, "/project");
    }

    #[test]
    fn test_build_tree_skips_short_lines() {
        let raw = "bad\tline\n\
                    main\t1\t0\tzsh\t1\t%0\t0\tbash\t1\t/home\t\n";
        let tree = build_tree(raw, &HashMap::new());
        assert_eq!(tree.len(), 1);
    }

    #[test]
    fn test_build_tree_empty_input() {
        let tree = build_tree("", &HashMap::new());
        assert!(tree.is_empty());
    }
}
