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

/// Input JSON from Claude Code statusline hook (stdin).
#[derive(Debug, Deserialize)]
struct StatuslineInput {
    #[serde(default)]
    session_id: Option<String>,
    #[serde(default)]
    model: Option<ModelInfo>,
    #[serde(default)]
    context_window: Option<ContextWindow>,
    #[serde(default)]
    cost: Option<CostInfo>,
    #[serde(default)]
    workspace: Option<WorkspaceInfo>,
}

#[derive(Debug, Deserialize)]
struct ModelInfo {
    #[serde(default)]
    display_name: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ContextWindow {
    #[serde(default)]
    used_percentage: Option<f64>,
}

#[derive(Debug, Deserialize)]
struct CostInfo {
    #[serde(default)]
    total_cost_usd: Option<f64>,
}

#[derive(Debug, Deserialize)]
struct WorkspaceInfo {
    #[serde(default)]
    current_dir: Option<String>,
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
        "statusline" => {
            return run_statusline(&pane_id);
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

fn run_statusline(pane_id: &str) -> Result<()> {
    let mut stdin_buf = String::new();
    std::io::stdin().read_to_string(&mut stdin_buf).ok();

    if stdin_buf.trim().is_empty() {
        return Ok(());
    }

    let input: StatuslineInput =
        serde_json::from_str(&stdin_buf).context("Failed to parse statusline input")?;

    let mut agent_state = state::read_state(pane_id)?
        .unwrap_or_else(|| AgentState::new(pane_id.to_string(), AgentStatus::Waiting));

    if let Some(sid) = input.session_id {
        agent_state.session_id = Some(sid);
    }
    if let Some(model) = input.model {
        if let Some(name) = model.display_name {
            agent_state.model = Some(name);
        }
    }
    if let Some(ctx) = input.context_window {
        if let Some(pct) = ctx.used_percentage {
            agent_state.context_pct = Some(pct.round() as u8);
        }
    }
    if let Some(cost) = input.cost {
        if let Some(usd) = cost.total_cost_usd {
            agent_state.cost_usd = Some(usd);
        }
    }
    if let Some(ws) = input.workspace {
        if let Some(dir) = ws.current_dir {
            agent_state.project = Some(dir);
        }
    }

    agent_state.updated_at = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    state::write_state(&agent_state)?;
    Ok(())
}
