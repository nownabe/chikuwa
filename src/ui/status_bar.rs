use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Frame;

use crate::agent::state::AgentStatus;
use crate::tmux::types::TmuxSession;
use crate::ui::theme;

pub fn render(f: &mut Frame, area: Rect, sessions: &[TmuxSession]) {
    let mut total_agents = 0u32;
    let mut running = 0u32;
    let mut waiting = 0u32;
    let mut permission = 0u32;

    for session in sessions {
        for window in &session.windows {
            for pane in &window.panes {
                if let Some(ref agent) = pane.agent_state {
                    total_agents += 1;
                    match agent.state {
                        AgentStatus::Running => running += 1,
                        AgentStatus::Waiting => waiting += 1,
                        AgentStatus::Permission => permission += 1,
                        _ => {}
                    }
                }
            }
        }
    }

    let mut spans = vec![
        Span::styled(
            format!(" {} agents", total_agents),
            Style::default().fg(theme::COLOR_WHITE),
        ),
        Span::raw(" │ "),
    ];

    if running > 0 {
        spans.push(Span::styled(
            format!("{} {} run", theme::SPINNER_FRAMES[0], running),
            Style::default().fg(theme::status_color(&AgentStatus::Running, true)),
        ));
        spans.push(Span::raw(" "));
    }

    if waiting > 0 {
        spans.push(Span::styled(
            format!("{} {} wait", theme::ICON_WAITING, waiting),
            Style::default().fg(theme::status_color(&AgentStatus::Waiting, true)),
        ));
        spans.push(Span::raw(" "));
    }

    if permission > 0 {
        spans.push(Span::styled(
            format!("{} {} perm", theme::ICON_PERMISSION, permission),
            Style::default().fg(theme::status_color(&AgentStatus::Permission, true)),
        ));
    }

    let paragraph = Paragraph::new(Line::from(spans)).block(
        Block::default()
            .borders(Borders::TOP)
            .border_style(Style::default().fg(Color::DarkGray)),
    );

    f.render_widget(paragraph, area);
}
