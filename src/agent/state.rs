use std::collections::HashMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentStatus {
    Started,
    Running,
    Waiting,
    Permission,
    Ended,
}

impl std::fmt::Display for AgentStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AgentStatus::Started => write!(f, "started"),
            AgentStatus::Running => write!(f, "running"),
            AgentStatus::Waiting => write!(f, "waiting"),
            AgentStatus::Permission => write!(f, "permission"),
            AgentStatus::Ended => write!(f, "ended"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentState {
    pub tmux_pane: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    pub state: AgentStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context_pct: Option<u8>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cost_usd: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project: Option<String>,
    pub updated_at: u64,
}

impl AgentState {
    pub fn new(tmux_pane: String, state: AgentStatus) -> Self {
        Self {
            tmux_pane,
            session_id: None,
            state,
            model: None,
            context_pct: None,
            cost_usd: None,
            project: None,
            updated_at: now(),
        }
    }
}

fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

/// Returns the directory for state files.
pub fn state_dir() -> PathBuf {
    if let Ok(runtime_dir) = std::env::var("XDG_RUNTIME_DIR") {
        PathBuf::from(runtime_dir).join("chikuwa")
    } else {
        PathBuf::from("/tmp/chikuwa")
    }
}

/// Returns the state file path for a given pane ID (e.g. "%5" -> "<dir>/%5.json").
fn state_file(pane_id: &str) -> PathBuf {
    state_dir().join(format!("{}.json", pane_id))
}

/// Read an existing state file for a pane, if it exists.
pub fn read_state(pane_id: &str) -> Result<Option<AgentState>> {
    let path = state_file(pane_id);
    if !path.exists() {
        return Ok(None);
    }
    let content = std::fs::read_to_string(&path)
        .with_context(|| format!("Failed to read state file: {}", path.display()))?;
    let state: AgentState = serde_json::from_str(&content)
        .with_context(|| format!("Failed to parse state file: {}", path.display()))?;
    Ok(Some(state))
}

/// Write a state file for a pane.
pub fn write_state(state: &AgentState) -> Result<()> {
    let dir = state_dir();
    std::fs::create_dir_all(&dir)
        .with_context(|| format!("Failed to create state directory: {}", dir.display()))?;
    let path = state_file(&state.tmux_pane);
    let content = serde_json::to_string_pretty(state)
        .context("Failed to serialize agent state")?;
    std::fs::write(&path, content)
        .with_context(|| format!("Failed to write state file: {}", path.display()))?;
    Ok(())
}

/// Remove the state file for a pane.
pub fn remove_state(pane_id: &str) -> Result<()> {
    let path = state_file(pane_id);
    if path.exists() {
        std::fs::remove_file(&path)
            .with_context(|| format!("Failed to remove state file: {}", path.display()))?;
    }
    Ok(())
}

/// Read all state files in the state directory and return a map of pane_id -> AgentState.
pub fn read_all_states() -> Result<HashMap<String, AgentState>> {
    let dir = state_dir();
    let mut states = HashMap::new();

    if !dir.exists() {
        return Ok(states);
    }

    let entries = std::fs::read_dir(&dir)
        .with_context(|| format!("Failed to read state directory: {}", dir.display()))?;

    for entry in entries {
        let entry = entry?;
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) == Some("json") {
            if let Some(pane_id) = path
                .file_stem()
                .and_then(|s| s.to_str())
                .map(|s| s.to_string())
            {
                match read_state_from_path(&path) {
                    Ok(state) => {
                        states.insert(pane_id, state);
                    }
                    Err(_) => {
                        // Skip malformed state files
                    }
                }
            }
        }
    }

    Ok(states)
}

fn read_state_from_path(path: &Path) -> Result<AgentState> {
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("Failed to read state file: {}", path.display()))?;
    let state: AgentState = serde_json::from_str(&content)
        .with_context(|| format!("Failed to parse state file: {}", path.display()))?;
    Ok(state)
}
