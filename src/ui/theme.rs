use ratatui::style::{Color, Style};

use crate::agent::state::AgentStatus;

// 3-color palette
pub const COLOR_WHITE: Color = Color::Rgb(0xff, 0xff, 0xff);
pub const COLOR_PURPLE: Color = Color::Rgb(0x92, 0x93, 0xfe);
pub const COLOR_LIGHT_PURPLE: Color = Color::Rgb(0xb6, 0xb9, 0xff);

// NerdFont icons
pub const ICON_CARET_RIGHT: &str = "\u{f0da}"; //
pub const ICON_FOLDER: &str = "\u{f07b}"; //
pub const ICON_GIT_BRANCH: &str = "\u{e725}"; //
pub const ICON_PR: &str = "\u{f407}"; //
pub const ICON_RUNNING: &str = "\u{f0e7}"; //
pub const ICON_WAITING: &str = "\u{f28b}"; //
pub const ICON_PERMISSION: &str = "\u{f071}"; //
pub const ICON_STARTED: &str = "\u{f04b}"; //

pub fn status_icon(status: &AgentStatus) -> &'static str {
    match status {
        AgentStatus::Running => ICON_RUNNING,
        AgentStatus::Waiting => ICON_WAITING,
        AgentStatus::Permission => ICON_PERMISSION,
        AgentStatus::Started => ICON_STARTED,
        AgentStatus::Ended => ICON_STARTED,
    }
}

pub fn status_color(status: &AgentStatus) -> Color {
    match status {
        AgentStatus::Running => COLOR_WHITE,
        AgentStatus::Permission => COLOR_PURPLE,
        AgentStatus::Waiting => COLOR_LIGHT_PURPLE,
        AgentStatus::Started | AgentStatus::Ended => Color::DarkGray,
    }
}

pub fn status_style(status: &AgentStatus) -> Style {
    Style::default().fg(status_color(status))
}

pub fn dim_style() -> Style {
    Style::default().fg(Color::DarkGray)
}

pub fn branch_style() -> Style {
    Style::default().fg(COLOR_LIGHT_PURPLE)
}

pub fn pr_style() -> Style {
    Style::default().fg(COLOR_PURPLE)
}
