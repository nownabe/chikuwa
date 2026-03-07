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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::state::AgentStatus;
    use crate::tmux::types::{TmuxPane, TmuxSession, TmuxWindow};
    use std::collections::HashSet;

    fn make_pane(pane_id: &str, command: &str, agent: Option<AgentState>) -> TmuxPane {
        TmuxPane {
            pane_id: pane_id.to_string(),
            pane_index: 0,
            pane_current_command: command.to_string(),
            pane_current_path: "/home/user".to_string(),
            pane_active: true,
            agent_state: agent,
        }
    }

    fn make_sessions() -> Vec<TmuxSession> {
        vec![
            TmuxSession {
                session_name: "main".to_string(),
                session_attached: true,
                windows: vec![
                    TmuxWindow {
                        window_index: 0,
                        window_name: "claude".to_string(),
                        window_active: true,
                        panes: vec![make_pane("%0", "node", Some(AgentState {
                            tmux_pane: "%0".to_string(),
                            session_id: None,
                            state: AgentStatus::Running,
                            updated_at: 100,
                        }))],
                    },
                    TmuxWindow {
                        window_index: 1,
                        window_name: "zsh".to_string(),
                        window_active: false,
                        panes: vec![make_pane("%1", "zsh", None)],
                    },
                ],
            },
            TmuxSession {
                session_name: "dev".to_string(),
                session_attached: false,
                windows: vec![TmuxWindow {
                    window_index: 0,
                    window_name: "work".to_string(),
                    window_active: true,
                    panes: vec![
                        make_pane("%2", "zsh", None),
                        make_pane("%3", "vim", None),
                    ],
                }],
            },
        ]
    }

    #[test]
    fn test_flatten_basic_structure() {
        let sessions = make_sessions();
        let items = flatten(&sessions, &HashSet::new());

        // main session + 2 windows + dev session + 1 window + 2 panes
        assert_eq!(items.len(), 7);

        // First item is session "main"
        assert!(matches!(&items[0], TreeItem::Session { name, attached: true, collapsed: false } if name == "main"));
        // Second is window 0:claude
        assert!(matches!(&items[1], TreeItem::Window { window_name, .. } if window_name == "claude"));
        // Third is window 1:zsh
        assert!(matches!(&items[2], TreeItem::Window { window_name, .. } if window_name == "zsh"));
        // Fourth is session "dev"
        assert!(matches!(&items[3], TreeItem::Session { name, attached: false, .. } if name == "dev"));
        // Fifth is window 0:work
        assert!(matches!(&items[4], TreeItem::Window { window_name, .. } if window_name == "work"));
        // Sixth and seventh are panes (multi-pane window)
        assert!(matches!(&items[5], TreeItem::Pane { .. }));
        assert!(matches!(&items[6], TreeItem::Pane { .. }));
    }

    #[test]
    fn test_flatten_collapsed_session() {
        let sessions = make_sessions();
        let mut collapsed = HashSet::new();
        collapsed.insert("main".to_string());

        let items = flatten(&sessions, &collapsed);

        // main (collapsed) + dev + 1 window + 2 panes = 5
        assert_eq!(items.len(), 5);
        assert!(matches!(&items[0], TreeItem::Session { name, collapsed: true, .. } if name == "main"));
        assert!(matches!(&items[1], TreeItem::Session { name, .. } if name == "dev"));
    }

    #[test]
    fn test_flatten_single_pane_window_embeds_agent_state() {
        let sessions = make_sessions();
        let items = flatten(&sessions, &HashSet::new());

        // Window "claude" has 1 pane with agent, so agent_state should be embedded
        if let TreeItem::Window { agent_state, .. } = &items[1] {
            assert!(agent_state.is_some());
            assert_eq!(agent_state.as_ref().unwrap().state, AgentStatus::Running);
        } else {
            panic!("Expected Window item");
        }
    }

    #[test]
    fn test_flatten_multi_pane_window_no_agent_on_window() {
        let sessions = make_sessions();
        let items = flatten(&sessions, &HashSet::new());

        // Window "work" has 2 panes, agent_state should be None
        if let TreeItem::Window { agent_state, .. } = &items[4] {
            assert!(agent_state.is_none());
        } else {
            panic!("Expected Window item");
        }
    }

    #[test]
    fn test_tmux_target_session() {
        let item = TreeItem::Session {
            name: "main".to_string(),
            attached: true,
            collapsed: false,
        };
        assert_eq!(item.tmux_target(), "main");
    }

    #[test]
    fn test_tmux_target_window() {
        let item = TreeItem::Window {
            session_name: "main".to_string(),
            window_index: 2,
            window_name: "zsh".to_string(),
            is_last: false,
            agent_state: None,
        };
        assert_eq!(item.tmux_target(), "main:2");
    }

    #[test]
    fn test_tmux_target_pane() {
        let item = TreeItem::Pane {
            session_name: "dev".to_string(),
            window_index: 1,
            pane: make_pane("%5", "zsh", None),
            is_last_window: false,
            is_last_pane: false,
        };
        assert_eq!(item.tmux_target(), "dev:1.0");
    }

    #[test]
    fn test_flatten_empty() {
        let items = flatten(&[], &HashSet::new());
        assert!(items.is_empty());
    }

    #[test]
    fn test_flatten_is_last_flags() {
        let sessions = make_sessions();
        let items = flatten(&sessions, &HashSet::new());

        // Window 0:claude is not last
        if let TreeItem::Window { is_last, .. } = &items[1] {
            assert!(!is_last);
        }
        // Window 1:zsh is last
        if let TreeItem::Window { is_last, .. } = &items[2] {
            assert!(is_last);
        }
    }
}

fn append_agent_info(spans: &mut Vec<Span<'static>>, agent: &AgentState) {
    spans.push(Span::raw(" "));
    spans.push(Span::styled(
        theme::status_icon(&agent.state).to_string(),
        theme::status_style(&agent.state),
    ));
}
