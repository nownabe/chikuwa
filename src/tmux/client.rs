use std::collections::HashMap;

use anyhow::{Context, Result};
use tokio::process::Command;

use super::types::{TmuxPane, TmuxSession, TmuxWindow};
use crate::agent::state::{self, AgentState};

/// Fetch all tmux sessions/windows/panes and build a tree, merging agent states.
pub async fn fetch_tree() -> Result<Vec<TmuxSession>> {
    let raw = list_panes_all().await?;
    let agent_states = state::read_all_states().unwrap_or_default();
    Ok(build_tree(&raw, &agent_states))
}

/// Run `tmux list-panes -a` with a custom format and return raw output.
async fn list_panes_all() -> Result<String> {
    let output = Command::new("tmux")
        .args([
            "list-panes",
            "-a",
            "-F",
            "#{session_name}\t#{session_attached}\t#{window_index}\t#{window_name}\t#{window_active}\t#{pane_id}\t#{pane_index}\t#{pane_current_command}\t#{pane_active}",
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
        if fields.len() < 9 {
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

        let agent_state = agent_states.get(&pane_id).cloned();

        let pane = TmuxPane {
            pane_id,
            pane_index,
            pane_current_command,
            pane_active,
            agent_state,
        };

        let session_idx = if let Some(&idx) = session_map.get(&session_name) {
            idx
        } else {
            let idx = sessions.len();
            sessions.push(TmuxSession {
                session_name: session_name.clone(),
                session_attached,
                windows: Vec::new(),
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
