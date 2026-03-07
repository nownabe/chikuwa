use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Frame;

use crate::agent::state::AgentState;
use crate::tmux::types::{TmuxPane, TmuxSession};
use crate::ui::theme;

/// A flattened tree item for rendering and selection.
#[derive(Debug, Clone)]
pub enum TreeItem {
    Session {
        name: String,
        attached: bool,
        collapsed: bool,
    },
    Window {
        session_name: String,
        window_index: u32,
        window_name: String,
        is_last: bool,
        /// Agent state if this window has exactly one pane with an agent.
        agent_state: Option<AgentState>,
    },
    Pane {
        session_name: String,
        window_index: u32,
        pane: TmuxPane,
        is_last_window: bool,
        is_last_pane: bool,
    },
}

impl TreeItem {
    /// Build a target string for tmux switch-client.
    pub fn tmux_target(&self) -> String {
        match self {
            TreeItem::Session { name, .. } => name.clone(),
            TreeItem::Window {
                session_name,
                window_index,
                ..
            } => format!("{}:{}", session_name, window_index),
            TreeItem::Pane {
                session_name,
                window_index,
                pane,
                ..
            } => format!("{}:{}.{}", session_name, window_index, pane.pane_index),
        }
    }
}

/// Flatten the session tree into a list of TreeItems for rendering.
pub fn flatten(
    sessions: &[TmuxSession],
    collapsed_sessions: &std::collections::HashSet<String>,
) -> Vec<TreeItem> {
    let mut items = Vec::new();

    for session in sessions {
        let collapsed = collapsed_sessions.contains(&session.session_name);
        items.push(TreeItem::Session {
            name: session.session_name.clone(),
            attached: session.session_attached,
            collapsed,
        });

        if collapsed {
            continue;
        }

        let window_count = session.windows.len();
        for (wi, window) in session.windows.iter().enumerate() {
            let is_last_window = wi == window_count - 1;

            // If single pane, embed agent state in the Window item
            let agent_state = if window.panes.len() == 1 {
                window.panes[0].agent_state.clone()
            } else {
                None
            };

            items.push(TreeItem::Window {
                session_name: session.session_name.clone(),
                window_index: window.window_index,
                window_name: window.window_name.clone(),
                is_last: is_last_window,
                agent_state,
            });

            // Only show individual panes if there's more than one
            if window.panes.len() > 1 {
                let pane_count = window.panes.len();
                for (pi, pane) in window.panes.iter().enumerate() {
                    let is_last_pane = pi == pane_count - 1;
                    items.push(TreeItem::Pane {
                        session_name: session.session_name.clone(),
                        window_index: window.window_index,
                        pane: pane.clone(),
                        is_last_window,
                        is_last_pane,
                    });
                }
            }
        }
    }

    items
}

/// Render the tree view.
pub fn render(
    f: &mut Frame,
    area: Rect,
    items: &[TreeItem],
    selected: usize,
    scroll_offset: usize,
) {
    let block = Block::default()
        .title(" chikuwa ")
        .title_style(theme::header_style())
        .borders(Borders::ALL)
        .border_style(Style::default().fg(ratatui::style::Color::DarkGray));

    let inner = block.inner(area);
    f.render_widget(block, area);

    let visible_height = inner.height as usize;
    let visible_items: Vec<Line> = items
        .iter()
        .enumerate()
        .skip(scroll_offset)
        .take(visible_height)
        .map(|(idx, item)| render_item(item, idx == selected))
        .collect();

    let paragraph = Paragraph::new(visible_items);
    f.render_widget(paragraph, inner);
}

fn render_item(item: &TreeItem, selected: bool) -> Line<'static> {
    let base_style = if selected {
        theme::selected_style()
    } else {
        Style::default()
    };

    match item {
        TreeItem::Session {
            name,
            attached,
            collapsed,
        } => {
            let icon = if *collapsed { "▸" } else { "▾" };
            let marker = if *attached { " *" } else { "" };
            let style = if selected {
                theme::selected_style()
            } else {
                theme::session_style(*attached)
            };
            Line::from(vec![Span::styled(
                format!("{} {} {}{}", icon, theme::ICON_SESSION, name, marker),
                style,
            )])
        }
        TreeItem::Window {
            window_index,
            window_name,
            is_last,
            agent_state,
            ..
        } => {
            let connector = if *is_last { " └ " } else { " ├ " };
            let mut spans = vec![Span::styled(connector.to_string(), theme::dim_style())];

            let label = format!("{}:{}", window_index, window_name);
            spans.push(Span::styled(label, base_style));

            // Show agent status inline for single-pane windows
            if let Some(agent) = agent_state {
                append_agent_info(&mut spans, agent);
            }

            Line::from(spans)
        }
        TreeItem::Pane {
            pane,
            is_last_window,
            is_last_pane,
            ..
        } => {
            let prefix = if *is_last_window { "   " } else { " │ " };
            let connector = if *is_last_pane { "└ " } else { "├ " };

            let mut spans = vec![
                Span::styled(prefix.to_string(), theme::dim_style()),
                Span::styled(connector.to_string(), theme::dim_style()),
            ];

            let label = pane.pane_current_command.to_string();
            spans.push(Span::styled(label, base_style));

            if let Some(ref agent) = pane.agent_state {
                append_agent_info(&mut spans, agent);
            }

            Line::from(spans)
        }
    }
}

fn append_agent_info(spans: &mut Vec<Span<'static>>, agent: &AgentState) {
    spans.push(Span::raw(" "));
    spans.push(Span::styled(
        theme::status_icon(&agent.state).to_string(),
        theme::status_style(&agent.state),
    ));
    if let Some(cost) = agent.cost_usd {
        spans.push(Span::styled(
            format!(" ${:.2}", cost),
            theme::dim_style(),
        ));
    }
    if let Some(pct) = agent.context_pct {
        spans.push(Span::styled(format!(" {}%", pct), theme::dim_style()));
    }
}
