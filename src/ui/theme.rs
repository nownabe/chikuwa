use ratatui::style::{Color, Modifier, Style};

use crate::agent::state::AgentStatus;

pub const ICON_RUNNING: &str = "✦";
pub const ICON_WAITING: &str = "❯";
pub const ICON_PERMISSION: &str = "⚠";
pub const ICON_STARTED: &str = "⏸";
pub const ICON_SESSION: &str = "📂";
pub const ICON_ERROR: &str = "✗";

pub fn status_icon(status: &AgentStatus) -> &'static str {
    match status {
        AgentStatus::Running => ICON_RUNNING,
        AgentStatus::Waiting => ICON_WAITING,
        AgentStatus::Permission => ICON_PERMISSION,
        AgentStatus::Started => ICON_STARTED,
        AgentStatus::Ended => ICON_ERROR,
    }
}

pub fn status_color(status: &AgentStatus) -> Color {
    match status {
        AgentStatus::Running => Color::Yellow,
        AgentStatus::Waiting => Color::Green,
        AgentStatus::Permission => Color::Magenta,
        AgentStatus::Started => Color::DarkGray,
        AgentStatus::Ended => Color::Red,
    }
}

pub fn status_style(status: &AgentStatus) -> Style {
    Style::default().fg(status_color(status))
}

pub fn selected_style() -> Style {
    Style::default()
        .bg(Color::DarkGray)
        .add_modifier(Modifier::BOLD)
}

pub fn header_style() -> Style {
    Style::default()
        .fg(Color::Cyan)
        .add_modifier(Modifier::BOLD)
}

pub fn session_style(attached: bool) -> Style {
    if attached {
        Style::default()
            .fg(Color::White)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::DarkGray)
    }
}

pub fn dim_style() -> Style {
    Style::default().fg(Color::DarkGray)
}
