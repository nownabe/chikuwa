use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Frame;

use crate::agent::state::AgentStatus;
use crate::tmux::types::TmuxSession;
use crate::ui::theme;
use crate::usage::Usage;

const GAUGE_WIDTH: usize = 10;

fn gauge_spans(label: &str, utilization: f64) -> Vec<Span<'static>> {
    let pct = (utilization * 100.0).round() as u32;
    let filled = ((utilization * GAUGE_WIDTH as f64).round() as usize).min(GAUGE_WIDTH);
    let empty = GAUGE_WIDTH - filled;
    let color = theme::usage_color(utilization);

    vec![
        Span::styled(
            format!("{} ", label),
            Style::default().fg(theme::COLOR_PURPLE),
        ),
        Span::styled("\u{2588}".repeat(filled), Style::default().fg(color)),
        Span::styled(
            "\u{2591}".repeat(empty),
            Style::default().fg(theme::COLOR_PURPLE),
        ),
        Span::styled(format!(" {:>3}%", pct), Style::default().fg(color)),
    ]
}

fn format_remaining(secs: u64) -> String {
    if secs >= 60 {
        format!("{}m", secs / 60)
    } else {
        format!("{}s", secs)
    }
}

pub fn render(
    f: &mut Frame,
    area: Rect,
    sessions: &[TmuxSession],
    usage: Option<Result<&Usage, &String>>,
    usage_remaining_secs: Option<u64>,
) {
    let mut running = 0u32;
    let mut waiting = 0u32;
    let mut permission = 0u32;

    for session in sessions {
        for window in &session.windows {
            for pane in &window.panes {
                if let Some(ref agent) = pane.agent_state {
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

    // Line 1: agent counts
    let mut line1_spans: Vec<Span> = vec![Span::raw(" ")];

    if running > 0 {
        line1_spans.push(Span::styled(
            format!("{} {} run", theme::SPINNER_FRAMES[0], running),
            Style::default().fg(theme::status_color(&AgentStatus::Running, true)),
        ));
        line1_spans.push(Span::raw(" "));
    }

    if waiting > 0 {
        line1_spans.push(Span::styled(
            format!("{} {} wait", theme::ICON_WAITING, waiting),
            Style::default().fg(theme::status_color(&AgentStatus::Waiting, true)),
        ));
        line1_spans.push(Span::raw(" "));
    }

    if permission > 0 {
        line1_spans.push(Span::styled(
            format!("{} {} perm", theme::ICON_PERMISSION, permission),
            Style::default().fg(theme::status_color(&AgentStatus::Permission, true)),
        ));
    }

    // Line 2: usage gauges or error
    let gray = Style::default().fg(Color::Rgb(0x7a, 0x7a, 0x7a));
    let remaining_span = usage_remaining_secs.map(|s| {
        Span::styled(format!("  next in {}", format_remaining(s)), gray)
    });
    let line2 = match usage {
        Some(Ok(usage)) => {
            let mut spans: Vec<Span> = vec![Span::raw(" ")];
            spans.extend(gauge_spans("5h", usage.five_hour));
            spans.push(Span::raw("  "));
            spans.extend(gauge_spans("7d", usage.seven_day));
            if let Some(span) = remaining_span {
                spans.push(span);
            }
            Line::from(spans)
        }
        Some(Err(msg)) => {
            let mut spans = vec![
                Span::raw(" "),
                Span::styled(msg.to_string(), gray),
            ];
            if let Some(span) = remaining_span {
                spans.push(span);
            }
            Line::from(spans)
        }
        None => Line::from(""),
    };

    let paragraph = Paragraph::new(vec![Line::from(line1_spans), line2]).block(
        Block::default()
            .borders(Borders::TOP)
            .border_style(Style::default().fg(Color::DarkGray)),
    );

    f.render_widget(paragraph, area);
}
