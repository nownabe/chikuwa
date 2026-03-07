use std::io::Read;

use anyhow::{Context, Result};
use serde::Deserialize;

use crate::agent::state::{self, AgentState, AgentStatus};

/// Input JSON from Claude Code hooks (stdin).
#[derive(Debug, Deserialize)]
struct HookInput {
    #[serde(default)]
    session_id: Option<String>,
}

/// Run the hook subcommand: read stdin, update state file.
pub fn run(event: &str) -> Result<()> {
    let pane_id = std::env::var("TMUX_PANE")
        .context("TMUX_PANE environment variable not set (not running inside tmux?)")?;

    match event {
        "ended" => {
            state::remove_state(&pane_id)?;
            return Ok(());
        }
        _ => {}
    }

    let status = match event {
        "started" => AgentStatus::Started,
        "running" => AgentStatus::Running,
        "waiting" => AgentStatus::Waiting,
        "permission" => AgentStatus::Permission,
        "notification" => {
            return run_notification(&pane_id);
        }
        other => anyhow::bail!("Unknown hook event: {}", other),
    };

    let mut stdin_buf = String::new();
    std::io::stdin().read_to_string(&mut stdin_buf).ok();

    let input: Option<HookInput> = if stdin_buf.trim().is_empty() {
        None
    } else {
        serde_json::from_str(&stdin_buf).ok()
    };

    // Read existing state or create new
    let mut agent_state = state::read_state(&pane_id)?
        .unwrap_or_else(|| AgentState::new(pane_id.clone(), status.clone()));

    agent_state.state = status;
    agent_state.updated_at = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    if let Some(input) = input {
        if input.session_id.is_some() {
            agent_state.session_id = input.session_id;
        }
    }

    state::write_state(&agent_state)?;
    Ok(())
}

fn run_notification(pane_id: &str) -> Result<()> {
    let mut stdin_buf = String::new();
    std::io::stdin().read_to_string(&mut stdin_buf).ok();

    // Check if this is a permission notification
    let is_permission = stdin_buf.contains("permission_prompt");

    let mut agent_state = state::read_state(pane_id)?
        .unwrap_or_else(|| AgentState::new(pane_id.to_string(), AgentStatus::Running));

    if is_permission {
        agent_state.state = AgentStatus::Permission;
    }

    agent_state.updated_at = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    state::write_state(&agent_state)?;
    Ok(())
}
