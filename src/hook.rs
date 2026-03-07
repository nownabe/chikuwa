use std::io::Read;

use anyhow::{Context, Result};
use serde::Deserialize;

use crate::agent::state::{AgentState, AgentStatus};
use crate::ipc;

/// Input JSON from Claude Code hooks (stdin).
#[derive(Debug, Deserialize)]
struct HookInput {
    #[serde(default)]
    session_id: Option<String>,
}

/// Run the hook subcommand: read stdin, send state via IPC.
pub async fn run(event: &str) -> Result<()> {
    let pane_id = std::env::var("TMUX_PANE")
        .context("TMUX_PANE environment variable not set (not running inside tmux?)")?;

    if event == "ended" {
        let state = AgentState::new(pane_id, AgentStatus::Ended);
        ipc::send_state(&state).await?;
        return Ok(());
    }

    let status = match event {
        "started" => AgentStatus::Started,
        "running" => AgentStatus::Running,
        "waiting" => AgentStatus::Waiting,
        "permission" => AgentStatus::Permission,
        "notification" => {
            return run_notification(&pane_id).await;
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

    let mut agent_state = AgentState::new(pane_id, status);

    if let Some(input) = input {
        agent_state.session_id = input.session_id;
    }

    ipc::send_state(&agent_state).await?;
    Ok(())
}

async fn run_notification(pane_id: &str) -> Result<()> {
    let mut stdin_buf = String::new();
    std::io::stdin().read_to_string(&mut stdin_buf).ok();

    let is_permission = stdin_buf.contains("permission_prompt");

    let status = if is_permission {
        AgentStatus::Permission
    } else {
        AgentStatus::Running
    };

    let state = AgentState::new(pane_id.to_string(), status);
    ipc::send_state(&state).await?;
    Ok(())
}
