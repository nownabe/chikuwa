#![allow(dead_code)]

use crate::agent::state::AgentState;
use crate::git::GitInfo;

#[derive(Debug, Clone)]
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

#[derive(Debug, Clone)]
pub struct TmuxWindow {
    pub window_index: u32,
    pub window_name: String,
    pub window_active: bool,
    pub panes: Vec<TmuxPane>,
}

#[derive(Debug, Clone)]
pub struct TmuxSession {
    pub session_name: String,
    pub session_attached: bool,
    pub windows: Vec<TmuxWindow>,
    pub repo_name: Option<String>,
    pub toplevel: Option<String>,
}
