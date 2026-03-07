use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;

use crate::agent::state::AgentState;
use crate::git::GitInfo;
use crate::tmux::types::{TmuxPane, TmuxSession};
use crate::ui::theme;

/// A flattened tree item for rendering and selection.
#[derive(Debug, Clone)]
pub enum TreeItem {
    Session {
        name: String,
        attached: bool,
        collapsed: bool,
        repo_name: Option<String>,
    },
    Window {
        session_name: String,
        window_index: u32,
        window_name: String,
        /// Agent state if this window has exactly one pane with an agent.
        agent_state: Option<AgentState>,
        /// Git info for single-pane windows.
        git_info: Option<GitInfo>,
        /// Current path of the (single) pane, for display_label.
        pane_current_path: Option<String>,
        /// Current command of the (single) pane, for display_label.
        pane_current_command: Option<String>,
    },
    Pane {
        session_name: String,
        window_index: u32,
        pane: TmuxPane,
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

/// Returns true if the command is a shell (zsh, bash, fish, etc.)
fn is_shell(command: &str) -> bool {
    matches!(
        command,
        "zsh" | "bash" | "fish" | "sh" | "dash" | "ksh" | "csh" | "tcsh"
    )
}

/// Compute a display label: directory basename for shells, command name otherwise.
fn display_label(command: &str, path: &str) -> String {
    if is_shell(command) {
        std::path::Path::new(path)
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| command.to_string())
    } else {
        command.to_string()
    }
}

/// Whether this item has displayable git info (branch or PR).
fn item_has_git_info(item: &TreeItem) -> bool {
    match item {
        TreeItem::Window {
            git_info: Some(gi), ..
        } => gi.branch.is_some() || gi.pr.is_some(),
        TreeItem::Pane { pane, .. } => pane
            .git_info
            .as_ref()
            .map_or(false, |gi| gi.branch.is_some() || gi.pr.is_some()),
        _ => false,
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
            repo_name: session.repo_name.clone(),
        });

        if collapsed {
            continue;
        }

        for window in session.windows.iter() {

            // For single-pane windows, embed pane details in the Window item
            let (agent_state, git_info, pane_current_path, pane_current_command) =
                if window.panes.len() == 1 {
                    let pane = &window.panes[0];
                    (
                        pane.agent_state.clone(),
                        pane.git_info.clone(),
                        Some(pane.pane_current_path.clone()),
                        Some(pane.pane_current_command.clone()),
                    )
                } else {
                    // For multi-pane windows, use active pane's path for the label
                    let active_pane = window.panes.iter().find(|p| p.pane_active);
                    let path = active_pane.map(|p| p.pane_current_path.clone());
                    let cmd = active_pane.map(|p| p.pane_current_command.clone());
                    (None, None, path, cmd)
                };

            items.push(TreeItem::Window {
                session_name: session.session_name.clone(),
                window_index: window.window_index,
                window_name: window.window_name.clone(),
                agent_state,
                git_info,
                pane_current_path,
                pane_current_command,
            });

            // Only show individual panes if there's more than one
            if window.panes.len() > 1 {
                for pane in window.panes.iter() {
                    items.push(TreeItem::Pane {
                        session_name: session.session_name.clone(),
                        window_index: window.window_index,
                        pane: pane.clone(),
                    });
                }
            }
        }
    }

    items
}

/// Compute the visual row index for a given item index.
/// Visual rows include session borders and git sub-lines.
pub fn item_to_visual_row(items: &[TreeItem], target: usize) -> usize {
    let mut visual = 0;
    let mut i = 0;

    while i < items.len() {
        if i == target {
            return visual;
        }

        match &items[i] {
            TreeItem::Session {
                collapsed: true, ..
            } => {
                visual += 1;
                i += 1;
            }
            TreeItem::Session {
                collapsed: false, ..
            } => {
                visual += 1; // top border
                i += 1;

                while i < items.len() && !matches!(&items[i], TreeItem::Session { .. }) {
                    if i == target {
                        return visual;
                    }
                    visual += 1;
                    if item_has_git_info(&items[i]) {
                        visual += 1; // git sub-line
                    }
                    i += 1;
                }

                visual += 1; // bottom border
            }
            _ => {
                visual += 1;
                if item_has_git_info(&items[i]) {
                    visual += 1;
                }
                i += 1;
            }
        }
    }

    visual
}

/// Total number of visual rows.
pub fn total_visual_rows(items: &[TreeItem]) -> usize {
    let mut visual = 0;
    let mut i = 0;

    while i < items.len() {
        match &items[i] {
            TreeItem::Session {
                collapsed: true, ..
            } => {
                visual += 1;
                i += 1;
            }
            TreeItem::Session {
                collapsed: false, ..
            } => {
                visual += 1; // top border
                i += 1;
                while i < items.len() && !matches!(&items[i], TreeItem::Session { .. }) {
                    visual += 1;
                    if item_has_git_info(&items[i]) {
                        visual += 1;
                    }
                    i += 1;
                }
                visual += 1; // bottom border
            }
            _ => {
                visual += 1;
                if item_has_git_info(&items[i]) {
                    visual += 1;
                }
                i += 1;
            }
        }
    }

    visual
}

/// Render the tree view with per-session borders.
pub fn render(
    f: &mut Frame,
    area: Rect,
    items: &[TreeItem],
    selected: usize,
    scroll_offset: usize,
) {
    let visual_lines = build_visual_lines(items, area.width, selected);

    let visible_height = area.height as usize;
    let visible_lines: Vec<Line> = visual_lines
        .into_iter()
        .skip(scroll_offset)
        .take(visible_height)
        .collect();

    let paragraph = Paragraph::new(visible_lines);
    f.render_widget(paragraph, area);
}

fn build_visual_lines(items: &[TreeItem], width: u16, selected: usize) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    let mut i = 0;

    while i < items.len() {
        match &items[i] {
            TreeItem::Session {
                collapsed: true,
                name,
                attached,
                repo_name,
            } => {
                lines.push(render_collapsed_session(
                    name,
                    *attached,
                    repo_name.as_deref(),
                    i == selected,
                ));
                i += 1;
            }
            TreeItem::Session {
                collapsed: false,
                name,
                attached,
                repo_name,
            } => {
                let is_selected = i == selected;
                let name = name.clone();
                let attached = *attached;
                let repo_name = repo_name.clone();
                i += 1;

                // Collect content items until next Session or end
                let content_start = i;
                while i < items.len() && !matches!(&items[i], TreeItem::Session { .. }) {
                    i += 1;
                }
                let content_end = i;

                // Top border
                lines.push(render_session_top_border(
                    &name,
                    attached,
                    repo_name.as_deref(),
                    width,
                    is_selected,
                ));

                // Content items with git sub-lines
                for j in content_start..content_end {
                    let is_sel = j == selected;
                    lines.push(render_bordered_item(&items[j], width, is_sel));
                    if let Some(git_line) =
                        render_bordered_git_sub_line(&items[j], width, is_sel)
                    {
                        lines.push(git_line);
                    }
                }

                // Bottom border
                lines.push(render_session_bottom_border(width));
            }
            _ => {
                // Orphan item (shouldn't happen)
                i += 1;
            }
        }
    }

    lines
}

fn render_collapsed_session(
    name: &str,
    attached: bool,
    repo_name: Option<&str>,
    selected: bool,
) -> Line<'static> {
    let style = if attached {
        Style::default()
            .fg(theme::COLOR_WHITE)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::DarkGray)
    };

    let mut spans = vec![Span::styled(
        format!(
            "{} {} {}",
            theme::ICON_CARET_RIGHT,
            theme::ICON_FOLDER,
            name,
        ),
        style,
    )];

    if let Some(repo) = repo_name {
        spans.push(Span::styled(format!(" \u{2500}\u{2500} {}", repo), theme::dim_style()));
    }

    let mut line = Line::from(spans);
    if selected {
        for span in &mut line.spans {
            span.style = span.style.bg(Color::DarkGray);
        }
    }
    line
}

fn render_session_top_border(
    name: &str,
    attached: bool,
    repo_name: Option<&str>,
    width: u16,
    selected: bool,
) -> Line<'static> {
    let left_text = format!(" {} ", name);
    let right_text = repo_name
        .map(|r| format!(" {} ", r))
        .unwrap_or_default();

    let left_width = left_text.chars().count();
    let right_width = right_text.chars().count();
    let fill_count = (width as usize).saturating_sub(2 + left_width + right_width);
    let fill = "\u{2500}".repeat(fill_count);

    let session_style = if attached {
        Style::default()
            .fg(theme::COLOR_WHITE)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::DarkGray)
    };

    let mut spans = vec![
        Span::styled("\u{250c}", theme::dim_style()),
        Span::styled(left_text, session_style),
        Span::styled(fill, theme::dim_style()),
    ];

    if !right_text.is_empty() {
        spans.push(Span::styled(right_text, theme::dim_style()));
    }

    spans.push(Span::styled("\u{2510}", theme::dim_style()));

    let mut line = Line::from(spans);
    if selected {
        for span in &mut line.spans {
            span.style = span.style.bg(Color::DarkGray);
        }
    }
    line
}

fn render_session_bottom_border(width: u16) -> Line<'static> {
    let fill_count = (width as usize).saturating_sub(2);
    let fill = "\u{2500}".repeat(fill_count);
    Line::from(vec![
        Span::styled("\u{2514}", theme::dim_style()),
        Span::styled(fill, theme::dim_style()),
        Span::styled("\u{2518}", theme::dim_style()),
    ])
}

fn render_bordered_item(item: &TreeItem, width: u16, selected: bool) -> Line<'static> {
    let content_width = (width as usize).saturating_sub(4); // "│ " + content + " │"
    let mut content_spans = render_content_spans(item);
    truncate_spans(&mut content_spans, content_width);

    let content_display_width: usize =
        content_spans.iter().map(|s| s.content.chars().count()).sum();
    let padding_len = content_width.saturating_sub(content_display_width);

    if selected {
        for span in &mut content_spans {
            span.style = span.style.bg(Color::DarkGray);
        }
    }

    let mut spans = vec![Span::styled("\u{2502} ", theme::dim_style())];
    spans.extend(content_spans);
    if padding_len > 0 {
        let pad_style = if selected {
            Style::default().bg(Color::DarkGray)
        } else {
            Style::default()
        };
        spans.push(Span::styled(" ".repeat(padding_len), pad_style));
    }
    spans.push(Span::styled(" \u{2502}", theme::dim_style()));

    Line::from(spans)
}

/// Render a git info sub-line for an item, if it has displayable git info.
fn render_bordered_git_sub_line(
    item: &TreeItem,
    width: u16,
    selected: bool,
) -> Option<Line<'static>> {
    let (gi, prefix) = match item {
        TreeItem::Window {
            git_info: Some(gi),
            ..
        } if gi.branch.is_some() || gi.pr.is_some() => (gi, " "),
        TreeItem::Pane { pane, .. }
            if pane
                .git_info
                .as_ref()
                .map_or(false, |gi| gi.branch.is_some() || gi.pr.is_some()) =>
        {
            (pane.git_info.as_ref().unwrap(), "   ")
        }
        _ => return None,
    };

    let git_spans = git_display_spans(gi);
    if git_spans.is_empty() {
        return None;
    }

    let content_width = (width as usize).saturating_sub(4);
    let mut inner_spans = vec![Span::styled(prefix.to_string(), theme::dim_style())];
    inner_spans.extend(git_spans);
    truncate_spans(&mut inner_spans, content_width);

    let inner_width: usize = inner_spans.iter().map(|s| s.content.chars().count()).sum();
    let padding_len = content_width.saturating_sub(inner_width);

    if selected {
        for span in &mut inner_spans {
            span.style = span.style.bg(Color::DarkGray);
        }
    }

    let mut spans = vec![Span::styled("\u{2502} ", theme::dim_style())];
    spans.extend(inner_spans);
    if padding_len > 0 {
        let pad_style = if selected {
            Style::default().bg(Color::DarkGray)
        } else {
            Style::default()
        };
        spans.push(Span::styled(" ".repeat(padding_len), pad_style));
    }
    spans.push(Span::styled(" \u{2502}", theme::dim_style()));

    Some(Line::from(spans))
}

/// Build git display spans: PR takes priority over branch.
fn git_display_spans(gi: &GitInfo) -> Vec<Span<'static>> {
    if let Some(ref pr) = gi.pr {
        vec![Span::styled(
            format!("{} #{} {}", theme::ICON_PR, pr.number, pr.title),
            theme::pr_style(),
        )]
    } else if let Some(ref branch) = gi.branch {
        vec![Span::styled(
            format!("{} {}", theme::ICON_GIT_BRANCH, branch),
            theme::branch_style(),
        )]
    } else {
        vec![]
    }
}

/// Truncate spans to fit within max_width characters.
fn truncate_spans(spans: &mut Vec<Span<'static>>, max_width: usize) {
    let mut total = 0;
    let mut truncate_at = spans.len();
    for (i, span) in spans.iter().enumerate() {
        let span_width = span.content.chars().count();
        if total + span_width > max_width {
            truncate_at = i;
            break;
        }
        total += span_width;
    }
    if truncate_at < spans.len() {
        let remaining = max_width.saturating_sub(total);
        if remaining > 0 {
            let truncated: String = spans[truncate_at].content.chars().take(remaining).collect();
            let style = spans[truncate_at].style;
            spans[truncate_at] = Span::styled(truncated, style);
            spans.truncate(truncate_at + 1);
        } else {
            spans.truncate(truncate_at);
        }
    }
}

fn render_content_spans(item: &TreeItem) -> Vec<Span<'static>> {
    match item {
        TreeItem::Window {
            window_name,
            agent_state,
            pane_current_path,
            pane_current_command,
            ..
        } => {
            let mut spans = Vec::new();

            let label =
                if let (Some(cmd), Some(path)) = (pane_current_command, pane_current_path) {
                    display_label(cmd, path)
                } else {
                    window_name.clone()
                };
            spans.push(Span::raw(label));

            if let Some(agent) = agent_state {
                append_agent_info(&mut spans, agent);
            }

            spans
        }
        TreeItem::Pane { pane, .. } => {
            let mut spans = vec![Span::styled("  ".to_string(), theme::dim_style())];

            let label = display_label(&pane.pane_current_command, &pane.pane_current_path);
            spans.push(Span::raw(label));

            if let Some(ref agent) = pane.agent_state {
                append_agent_info(&mut spans, agent);
            }

            spans
        }
        _ => vec![],
    }
}

fn append_agent_info(spans: &mut Vec<Span<'static>>, agent: &AgentState) {
    spans.push(Span::raw(" "));
    spans.push(Span::styled(
        theme::status_icon(&agent.state).to_string(),
        theme::status_style(&agent.state),
    ));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::state::AgentStatus;
    use crate::git::PrInfo;
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
            git_info: None,
        }
    }

    fn make_pane_with_git(pane_id: &str, command: &str, git_info: GitInfo) -> TmuxPane {
        TmuxPane {
            pane_id: pane_id.to_string(),
            pane_index: 0,
            pane_current_command: command.to_string(),
            pane_current_path: "/home/user".to_string(),
            pane_active: true,
            agent_state: None,
            git_info: Some(git_info),
        }
    }

    fn make_sessions() -> Vec<TmuxSession> {
        vec![
            TmuxSession {
                session_name: "main".to_string(),
                session_attached: true,
                repo_name: Some("nownabe/chikuwa".to_string()),
                windows: vec![
                    TmuxWindow {
                        window_index: 0,
                        window_name: "claude".to_string(),
                        window_active: true,
                        panes: vec![make_pane(
                            "%0",
                            "node",
                            Some(AgentState {
                                tmux_pane: "%0".to_string(),
                                session_id: None,
                                state: AgentStatus::Running,
                                updated_at: 100,
                            }),
                        )],
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
                repo_name: None,
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

        assert_eq!(items.len(), 7);

        assert!(
            matches!(&items[0], TreeItem::Session { name, attached: true, collapsed: false, .. } if name == "main")
        );
        assert!(
            matches!(&items[1], TreeItem::Window { window_name, .. } if window_name == "claude")
        );
        assert!(
            matches!(&items[2], TreeItem::Window { window_name, .. } if window_name == "zsh")
        );
        assert!(
            matches!(&items[3], TreeItem::Session { name, attached: false, .. } if name == "dev")
        );
        assert!(
            matches!(&items[4], TreeItem::Window { window_name, .. } if window_name == "work")
        );
        assert!(matches!(&items[5], TreeItem::Pane { .. }));
        assert!(matches!(&items[6], TreeItem::Pane { .. }));
    }

    #[test]
    fn test_flatten_collapsed_session() {
        let sessions = make_sessions();
        let mut collapsed = HashSet::new();
        collapsed.insert("main".to_string());

        let items = flatten(&sessions, &collapsed);

        assert_eq!(items.len(), 5);
        assert!(
            matches!(&items[0], TreeItem::Session { name, collapsed: true, .. } if name == "main")
        );
        assert!(matches!(&items[1], TreeItem::Session { name, .. } if name == "dev"));
    }

    #[test]
    fn test_flatten_single_pane_window_embeds_agent_state() {
        let sessions = make_sessions();
        let items = flatten(&sessions, &HashSet::new());

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
            repo_name: None,
        };
        assert_eq!(item.tmux_target(), "main");
    }

    #[test]
    fn test_tmux_target_window() {
        let item = TreeItem::Window {
            session_name: "main".to_string(),
            window_index: 2,
            window_name: "zsh".to_string(),
            agent_state: None,
            git_info: None,
            pane_current_path: None,
            pane_current_command: None,
        };
        assert_eq!(item.tmux_target(), "main:2");
    }

    #[test]
    fn test_tmux_target_pane() {
        let item = TreeItem::Pane {
            session_name: "dev".to_string(),
            window_index: 1,
            pane: make_pane("%5", "zsh", None),
        };
        assert_eq!(item.tmux_target(), "dev:1.0");
    }

    #[test]
    fn test_flatten_empty() {
        let items = flatten(&[], &HashSet::new());
        assert!(items.is_empty());
    }

    #[test]
    fn test_is_shell() {
        assert!(is_shell("zsh"));
        assert!(is_shell("bash"));
        assert!(is_shell("fish"));
        assert!(is_shell("sh"));
        assert!(is_shell("dash"));
        assert!(is_shell("ksh"));
        assert!(is_shell("csh"));
        assert!(is_shell("tcsh"));
        assert!(!is_shell("vim"));
        assert!(!is_shell("node"));
        assert!(!is_shell("python"));
    }

    #[test]
    fn test_display_label_shell() {
        assert_eq!(display_label("zsh", "/home/user/projects/myapp"), "myapp");
        assert_eq!(display_label("bash", "/tmp"), "tmp");
        assert_eq!(display_label("fish", "/"), "fish");
    }

    #[test]
    fn test_display_label_non_shell() {
        assert_eq!(display_label("vim", "/home/user"), "vim");
        assert_eq!(display_label("node", "/home/user/project"), "node");
    }

    #[test]
    fn test_flatten_single_pane_window_has_path_and_command() {
        let sessions = make_sessions();
        let items = flatten(&sessions, &HashSet::new());

        if let TreeItem::Window {
            pane_current_path,
            pane_current_command,
            ..
        } = &items[2]
        {
            assert_eq!(pane_current_path.as_deref(), Some("/home/user"));
            assert_eq!(pane_current_command.as_deref(), Some("zsh"));
        } else {
            panic!("Expected Window item");
        }
    }

    #[test]
    fn test_flatten_session_has_repo_name() {
        let sessions = make_sessions();
        let items = flatten(&sessions, &HashSet::new());

        if let TreeItem::Session { repo_name, .. } = &items[0] {
            assert_eq!(repo_name.as_deref(), Some("nownabe/chikuwa"));
        } else {
            panic!("Expected Session item");
        }

        if let TreeItem::Session { repo_name, .. } = &items[3] {
            assert!(repo_name.is_none());
        } else {
            panic!("Expected Session item");
        }
    }

    #[test]
    fn test_item_to_visual_row_collapsed() {
        let items = vec![
            TreeItem::Session {
                name: "a".to_string(),
                attached: true,
                collapsed: true,
                repo_name: None,
            },
            TreeItem::Session {
                name: "b".to_string(),
                attached: false,
                collapsed: true,
                repo_name: None,
            },
        ];

        assert_eq!(item_to_visual_row(&items, 0), 0);
        assert_eq!(item_to_visual_row(&items, 1), 1);
        assert_eq!(total_visual_rows(&items), 2);
    }

    #[test]
    fn test_item_to_visual_row_expanded_no_git() {
        let sessions = vec![TmuxSession {
            session_name: "main".to_string(),
            session_attached: true,
            repo_name: None,
            windows: vec![
                TmuxWindow {
                    window_index: 0,
                    window_name: "a".to_string(),
                    window_active: true,
                    panes: vec![make_pane("%0", "zsh", None)],
                },
                TmuxWindow {
                    window_index: 1,
                    window_name: "b".to_string(),
                    window_active: false,
                    panes: vec![make_pane("%1", "zsh", None)],
                },
            ],
        }];
        let items = flatten(&sessions, &HashSet::new());

        // No git info: top_border(0), window_a(1), window_b(2), bottom_border(3)
        assert_eq!(item_to_visual_row(&items, 0), 0);
        assert_eq!(item_to_visual_row(&items, 1), 1);
        assert_eq!(item_to_visual_row(&items, 2), 2);
        assert_eq!(total_visual_rows(&items), 4);
    }

    #[test]
    fn test_item_to_visual_row_with_git() {
        let sessions = vec![TmuxSession {
            session_name: "main".to_string(),
            session_attached: true,
            repo_name: None,
            windows: vec![
                TmuxWindow {
                    window_index: 0,
                    window_name: "claude".to_string(),
                    window_active: true,
                    panes: vec![make_pane_with_git(
                        "%0",
                        "node",
                        GitInfo {
                            branch: Some("main".to_string()),
                            pr: None,
                            repo_name: None,
                        },
                    )],
                },
                TmuxWindow {
                    window_index: 1,
                    window_name: "zsh".to_string(),
                    window_active: false,
                    panes: vec![make_pane("%1", "zsh", None)],
                },
            ],
        }];
        let items = flatten(&sessions, &HashSet::new());

        // top_border(0), window_claude(1), git_sub(2), window_zsh(3), bottom_border(4)
        assert_eq!(item_to_visual_row(&items, 0), 0); // Session
        assert_eq!(item_to_visual_row(&items, 1), 1); // Window claude
        assert_eq!(item_to_visual_row(&items, 2), 3); // Window zsh (after git sub-line)
        assert_eq!(total_visual_rows(&items), 5);
    }

    #[test]
    fn test_item_to_visual_row_mixed() {
        let items = vec![
            TreeItem::Session {
                name: "collapsed".to_string(),
                attached: false,
                collapsed: true,
                repo_name: None,
            },
            TreeItem::Session {
                name: "expanded".to_string(),
                attached: true,
                collapsed: false,
                repo_name: None,
            },
            TreeItem::Window {
                session_name: "expanded".to_string(),
                window_index: 0,
                window_name: "w".to_string(),
                agent_state: None,
                git_info: None,
                pane_current_path: None,
                pane_current_command: None,
            },
        ];

        assert_eq!(item_to_visual_row(&items, 0), 0);
        assert_eq!(item_to_visual_row(&items, 1), 1);
        assert_eq!(item_to_visual_row(&items, 2), 2);
        assert_eq!(total_visual_rows(&items), 4);
    }

    #[test]
    fn test_git_display_spans_pr_priority() {
        let gi = GitInfo {
            branch: Some("feature/x".to_string()),
            pr: Some(PrInfo {
                number: 42,
                title: "Fix bug".to_string(),
            }),
            repo_name: None,
        };
        let spans = git_display_spans(&gi);
        assert_eq!(spans.len(), 1);
        assert!(spans[0].content.contains("#42"));
        assert!(!spans[0].content.contains("feature/x"));
    }

    #[test]
    fn test_git_display_spans_branch_only() {
        let gi = GitInfo {
            branch: Some("main".to_string()),
            pr: None,
            repo_name: None,
        };
        let spans = git_display_spans(&gi);
        assert_eq!(spans.len(), 1);
        assert!(spans[0].content.contains("main"));
    }

    #[test]
    fn test_git_display_spans_empty() {
        let gi = GitInfo {
            branch: None,
            pr: None,
            repo_name: None,
        };
        let spans = git_display_spans(&gi);
        assert!(spans.is_empty());
    }

    #[test]
    fn test_item_has_git_info() {
        let with_branch = TreeItem::Window {
            session_name: "s".to_string(),
            window_index: 0,
            window_name: "w".to_string(),
            agent_state: None,
            git_info: Some(GitInfo {
                branch: Some("main".to_string()),
                pr: None,
                repo_name: None,
            }),
            pane_current_path: None,
            pane_current_command: None,
        };
        assert!(item_has_git_info(&with_branch));

        let without = TreeItem::Window {
            session_name: "s".to_string(),
            window_index: 0,
            window_name: "w".to_string(),
            agent_state: None,
            git_info: None,
            pane_current_path: None,
            pane_current_command: None,
        };
        assert!(!item_has_git_info(&without));

        let session = TreeItem::Session {
            name: "s".to_string(),
            attached: true,
            collapsed: false,
            repo_name: None,
        };
        assert!(!item_has_git_info(&session));
    }
}
