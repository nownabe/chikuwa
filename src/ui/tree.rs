use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;
use unicode_width::UnicodeWidthStr;

use crate::agent::state::{AgentState, AgentStatus};
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
        worktree_name: Option<String>,
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
        /// Pane title (e.g. nvim sets this to the filename).
        pane_title: Option<String>,
        /// Whether this window has more than one pane.
        has_multiple_panes: bool,
        /// Session-level git toplevel, for deciding relative vs absolute path.
        session_toplevel: Option<String>,
    },
    Pane {
        session_name: String,
        window_index: u32,
        pane: TmuxPane,
        /// Session-level git toplevel, for deciding relative vs absolute path.
        session_toplevel: Option<String>,
    },
}

impl TreeItem {
    /// Whether the cursor can land on this item.
    pub fn is_selectable(&self) -> bool {
        match self {
            TreeItem::Session { .. } => false,
            TreeItem::Window {
                has_multiple_panes, ..
            } => !*has_multiple_panes,
            TreeItem::Pane { .. } => true,
        }
    }

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

/// Shorten a path by abbreviating intermediate components to their first char.
/// e.g. "/home/user/src/github.com/nownabe/chikuwa" → "~/s/g/n/chikuwa"
fn shorten_path(path: &str) -> String {
    let home = std::env::var("HOME").unwrap_or_default();
    let (prefix, rest) = if !home.is_empty() && path.starts_with(&home) {
        ("~", &path[home.len()..])
    } else {
        ("", path)
    };

    let components: Vec<&str> = rest.split('/').filter(|s| !s.is_empty()).collect();
    if components.is_empty() {
        return if prefix.is_empty() {
            "/".to_string()
        } else {
            prefix.to_string()
        };
    }

    let mut parts = Vec::with_capacity(components.len());
    for (i, comp) in components.iter().enumerate() {
        if i == components.len() - 1 {
            parts.push(comp.to_string());
        } else {
            // First character (handles multi-byte chars)
            parts.push(comp.chars().next().unwrap().to_string());
        }
    }

    format!("{}/{}", prefix, parts.join("/"))
}

/// Compute relative path from toplevel, with progressive abbreviation for long paths.
/// Always includes the repo directory name as prefix (e.g. "chikuwa/src/ui").
fn relative_path(path: &str, toplevel: Option<&str>) -> String {
    let Some(toplevel) = toplevel else {
        return shorten_path(path);
    };

    // Extract repo dir name from toplevel (e.g. "/home/user/project" → "project")
    let repo_dir = std::path::Path::new(toplevel)
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();

    let full = if let Some(rest) = path.strip_prefix(toplevel) {
        let rest = rest.strip_prefix('/').unwrap_or(rest);
        if rest.is_empty() {
            format!("{}/", repo_dir)
        } else {
            format!("{}/{}", repo_dir, rest)
        }
    } else {
        return shorten_path(path);
    };

    shorten_relative_path(&full, 30)
}

/// Abbreviate intermediate directory components progressively from left
/// until the path fits within max_len. The first and last components are
/// never abbreviated.
pub(crate) fn shorten_relative_path(path: &str, max_len: usize) -> String {
    if path.len() <= max_len {
        return path.to_string();
    }

    let parts: Vec<&str> = path.split('/').collect();
    if parts.len() <= 2 {
        return path.to_string();
    }

    // Start abbreviating from index 1 (skip first component = repo dir)
    // and stop before last component (filename or trailing dir)
    let mut abbreviated: Vec<String> = parts.iter().map(|s| s.to_string()).collect();
    for i in 1..abbreviated.len() - 1 {
        if let Some(c) = abbreviated[i].chars().next() {
            abbreviated[i] = c.to_string();
        }
        let result = abbreviated.join("/");
        if result.len() <= max_len {
            return result;
        }
    }

    abbreviated.join("/")
}

/// Compute a display label: relative path for shells, pane_title for nvim, command name otherwise.
fn display_label(command: &str, path: &str, pane_title: &str, toplevel: Option<&str>) -> String {
    if let Some(activity) = extract_claude_activity(pane_title) {
        return activity;
    }
    if is_shell(command) {
        let p = relative_path(path, toplevel);
        if p.ends_with('/') { p } else { format!("{}/", p) }
    } else if command == "nvim" && !pane_title.is_empty() {
        pane_title.to_string()
    } else {
        command.to_string()
    }
}

/// Return the git info and prefix for an item, if it's a Claude Code pane with git info.
fn item_git_info<'a>(item: &'a TreeItem) -> Option<(&'a GitInfo, &'static str)> {
    match item {
        TreeItem::Window {
            git_info: Some(gi),
            pane_title,
            ..
        } if is_claude_code_title(pane_title.as_deref().unwrap_or(""))
            && (gi.branch.is_some() || gi.pr.is_some()) =>
        {
            Some((gi, "  "))
        }
        TreeItem::Pane { pane, .. }
            if is_claude_code_title(&pane.pane_title)
                && pane
                    .git_info
                    .as_ref()
                    .map_or(false, |gi| gi.branch.is_some() || gi.pr.is_some()) =>
        {
            Some((pane.git_info.as_ref().unwrap(), "    "))
        }
        _ => None,
    }
}

/// Count the number of visual rows the git sub-lines occupy for an item.
fn git_info_visual_rows(item: &TreeItem, width: u16) -> usize {
    let (gi, prefix) = match item_git_info(item) {
        Some(v) => v,
        None => return 0,
    };
    let content_width = (width as usize).saturating_sub(4);
    let prefix_width = prefix.width();
    if let Some(ref pr) = gi.pr {
        let header = format!("{} #{} ", theme::ICON_PR, pr.number);
        let header_width = header.width() + prefix_width;
        let title_width = pr.title.width();
        let first_line_avail = content_width.saturating_sub(header_width);
        if title_width <= first_line_avail {
            1
        } else {
            let wrap_avail = content_width.saturating_sub(prefix_width);
            if wrap_avail == 0 {
                return 1;
            }
            // First line fits header + part of title
            let remaining = title_width.saturating_sub(first_line_avail);
            1 + (remaining + wrap_avail - 1) / wrap_avail
        }
    } else {
        1 // branch line is always 1 row
    }
}

/// Check if a pane title looks like it was set by Claude Code.
/// Claude Code titles start with a non-alphanumeric icon character (e.g. "✳", "⠐").
fn is_claude_code_title(pane_title: &str) -> bool {
    pane_title
        .chars()
        .next()
        .map_or(false, |c| !c.is_alphanumeric() && !c.is_ascii())
}

/// Extract Claude activity text from a pane title, stripping leading icon characters.
/// Returns None if the title doesn't look like a Claude Code title, or is the default "Claude Code".
fn extract_claude_activity(pane_title: &str) -> Option<String> {
    if !is_claude_code_title(pane_title) {
        return None;
    }
    // Strip the leading spinner/icon character and whitespace.
    // Pane titles look like "✳ Some task" or "⠐ Claude Code".
    let activity = pane_title
        .strip_prefix(|c: char| !c.is_alphanumeric())
        .unwrap_or(pane_title)
        .trim();
    if activity.is_empty() {
        return None;
    }
    Some(activity.to_string())
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
            worktree_name: session.worktree_name.clone(),
        });

        if collapsed {
            continue;
        }

        for window in session.windows.iter() {

            // For single-pane windows, embed pane details in the Window item
            let (agent_state, git_info, pane_current_path, pane_current_command, pane_title) =
                if window.panes.len() == 1 {
                    let pane = &window.panes[0];
                    (
                        pane.agent_state.clone(),
                        pane.git_info.clone(),
                        Some(pane.pane_current_path.clone()),
                        Some(pane.pane_current_command.clone()),
                        Some(pane.pane_title.clone()),
                    )
                } else {
                    (None, None, None, None, None)
                };

            items.push(TreeItem::Window {
                session_name: session.session_name.clone(),
                window_index: window.window_index,
                window_name: window.window_name.clone(),
                agent_state,
                git_info,
                pane_current_path,
                pane_current_command,
                pane_title,
                has_multiple_panes: window.panes.len() > 1,
                session_toplevel: session.toplevel.clone(),
            });

            // Only show individual panes if there's more than one
            if window.panes.len() > 1 {
                for pane in window.panes.iter() {
                    items.push(TreeItem::Pane {
                        session_name: session.session_name.clone(),
                        window_index: window.window_index,
                        pane: pane.clone(),
                        session_toplevel: session.toplevel.clone(),
                    });
                }
            }
        }
    }

    items
}

/// Compute the visual row index for a given item index.
/// Visual rows include session borders and sub-lines.
pub fn item_to_visual_row(items: &[TreeItem], target: usize, width: u16) -> usize {
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
                    if item_has_agent_status(&items[i]) {
                        visual += 1;
                    }
                    visual += git_info_visual_rows(&items[i], width);
                    i += 1;
                }

                visual += 1; // bottom border
            }
            _ => {
                visual += 1;
                if item_has_agent_status(&items[i]) {
                    visual += 1;
                }
                visual += git_info_visual_rows(&items[i], width);
                i += 1;
            }
        }
    }

    visual
}

/// Total number of visual rows.
pub fn total_visual_rows(items: &[TreeItem], width: u16) -> usize {
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
                    if item_has_agent_status(&items[i]) {
                        visual += 1;
                    }
                    visual += git_info_visual_rows(&items[i], width);
                    i += 1;
                }
                visual += 1; // bottom border
            }
            _ => {
                visual += 1;
                if item_has_agent_status(&items[i]) {
                    visual += 1;
                }
                visual += git_info_visual_rows(&items[i], width);
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
    anim_frame: usize,
) {
    let visual_lines = build_visual_lines(items, area.width, selected, anim_frame);

    let visible_height = area.height as usize;
    let visible_lines: Vec<Line> = visual_lines
        .into_iter()
        .skip(scroll_offset)
        .take(visible_height)
        .collect();

    let paragraph = Paragraph::new(visible_lines);
    f.render_widget(paragraph, area);
}

fn build_visual_lines(items: &[TreeItem], width: u16, selected: usize, anim_frame: usize) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    let mut i = 0;

    while i < items.len() {
        match &items[i] {
            TreeItem::Session {
                collapsed: true,
                name,
                attached,
                repo_name,
                worktree_name,
            } => {
                lines.push(render_collapsed_session(
                    name,
                    *attached,
                    repo_name.as_deref(),
                    worktree_name.as_deref(),
                    i == selected,
                ));
                i += 1;
            }
            TreeItem::Session {
                collapsed: false,
                name,
                attached,
                repo_name,
                worktree_name,
            } => {
                let is_selected = i == selected;
                let name = name.clone();
                let attached = *attached;
                let repo_name = repo_name.clone();
                let worktree_name = worktree_name.clone();
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
                    worktree_name.as_deref(),
                    width,
                    is_selected,
                ));

                // Content items with agent status and git sub-lines
                for j in content_start..content_end {
                    let is_sel = j == selected;
                    lines.push(render_bordered_item(&items[j], width, is_sel, attached, anim_frame));
                    if let Some(status_line) =
                        render_bordered_agent_status_sub_line(&items[j], width, is_sel, attached, anim_frame)
                    {
                        lines.push(status_line);
                    }
                    lines.extend(render_bordered_git_sub_lines(&items[j], width, is_sel, attached));
                }

                // Bottom border
                lines.push(render_session_bottom_border(width, attached));
            }
            _ => {
                // Orphan item (shouldn't happen)
                i += 1;
            }
        }
    }

    lines
}

/// Style for session name text.
fn session_name_style(attached: bool) -> Style {
    if attached {
        Style::default()
            .fg(theme::COLOR_WHITE)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(theme::COLOR_PURPLE)
    }
}

/// Style for session borders and repo name.
fn session_border_style(attached: bool) -> Style {
    if attached {
        theme::branch_style()
    } else {
        Style::default().fg(theme::COLOR_PURPLE)
    }
}

fn render_collapsed_session(
    name: &str,
    attached: bool,
    repo_name: Option<&str>,
    worktree_name: Option<&str>,
    selected: bool,
) -> Line<'static> {
    let name_style = session_name_style(attached);

    let mut spans = vec![Span::styled(
        format!(
            "{} {} {}",
            theme::ICON_CARET_RIGHT,
            theme::ICON_SESSION,
            name,
        ),
        name_style,
    )];

    if let Some(repo) = repo_name {
        let repo_short = repo.rsplit('/').next().unwrap_or(repo);
        let wt = worktree_name
            .map(|w| format!(" ({})", w))
            .unwrap_or_default();
        spans.push(Span::styled(
            format!(" \u{2500}\u{2500} {} {}{}", theme::ICON_GITHUB, repo_short, wt),
            name_style,
        ));
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
    worktree_name: Option<&str>,
    width: u16,
    selected: bool,
) -> Line<'static> {
    let left_text = format!(" {} {} ", theme::ICON_SESSION, name);
    let right_text = repo_name
        .map(|r| {
            let repo_short = r.rsplit('/').next().unwrap_or(r);
            let wt = worktree_name
                .map(|w| format!(" ({})", w))
                .unwrap_or_default();
            format!(" {} {}{} ", theme::ICON_GITHUB, repo_short, wt)
        })
        .unwrap_or_default();

    let left_width = left_text.width();
    let right_width = right_text.width();
    let fill_count = (width as usize).saturating_sub(2 + left_width + right_width);
    let fill = "\u{2500}".repeat(fill_count);

    let name_style = session_name_style(attached);
    let border_style = session_border_style(attached);

    let mut spans = vec![
        Span::styled("\u{250c}", border_style),
        Span::styled(left_text, name_style),
        Span::styled(fill, border_style),
    ];

    if !right_text.is_empty() {
        spans.push(Span::styled(right_text, name_style));
    }

    spans.push(Span::styled("\u{2510}", border_style));

    let mut line = Line::from(spans);
    if selected {
        for span in &mut line.spans {
            span.style = span.style.bg(Color::DarkGray);
        }
    }
    line
}

fn render_session_bottom_border(width: u16, attached: bool) -> Line<'static> {
    let style = session_border_style(attached);
    let fill_count = (width as usize).saturating_sub(2);
    let fill = "\u{2500}".repeat(fill_count);
    Line::from(vec![
        Span::styled("\u{2514}", style),
        Span::styled(fill, style),
        Span::styled("\u{2518}", style),
    ])
}

fn render_bordered_item(
    item: &TreeItem,
    width: u16,
    selected: bool,
    session_attached: bool,
    anim_frame: usize,
) -> Line<'static> {
    let border_style = session_border_style(session_attached);
    let content_width = (width as usize).saturating_sub(4); // "│ " + content + " │"
    let mut content_spans = render_content_spans(item, session_attached, anim_frame);
    truncate_spans(&mut content_spans, content_width);

    let content_display_width: usize =
        content_spans.iter().map(|s| s.content.width()).sum();
    let padding_len = content_width.saturating_sub(content_display_width);

    if selected {
        for span in &mut content_spans {
            span.style = span.style.bg(Color::DarkGray);
        }
    }

    let mut spans = vec![Span::styled("\u{2502} ", border_style)];
    spans.extend(content_spans);
    if padding_len > 0 {
        let pad_style = if selected {
            Style::default().bg(Color::DarkGray)
        } else {
            Style::default()
        };
        spans.push(Span::styled(" ".repeat(padding_len), pad_style));
    }
    spans.push(Span::styled(" \u{2502}", border_style));

    Line::from(spans)
}

/// Check if a tree item has an agent status to display.
fn item_has_agent_status(item: &TreeItem) -> bool {
    match item {
        TreeItem::Window { agent_state, .. } => agent_state.is_some(),
        TreeItem::Pane { pane, .. } => pane.agent_state.is_some(),
        _ => false,
    }
}

/// Render an agent status sub-line (e.g. "· running") for an item.
fn render_bordered_agent_status_sub_line(
    item: &TreeItem,
    width: u16,
    selected: bool,
    session_attached: bool,
    anim_frame: usize,
) -> Option<Line<'static>> {
    let (agent, prefix) = match item {
        TreeItem::Window { agent_state: Some(agent), .. } => (agent, "  "),
        TreeItem::Pane { pane, .. } => (pane.agent_state.as_ref()?, "    "),
        _ => return None,
    };

    let status_label = match agent.state {
        AgentStatus::Started => "starting",
        AgentStatus::Running => "running",
        AgentStatus::Waiting => "waiting",
        AgentStatus::Permission => "needs input",
        AgentStatus::Ended => "ended",
    };

    let content_width = (width as usize).saturating_sub(4);
    let mut inner_spans = vec![
        Span::styled(
            prefix.to_string(),
            Style::default().fg(theme::COLOR_PURPLE),
        ),
        Span::styled(
            theme::status_icon(&agent.state, anim_frame).to_string(),
            theme::status_style(&agent.state, session_attached),
        ),
        Span::styled(
            format!(" {}", status_label),
            Style::default().fg(Color::Rgb(0x7a, 0x7a, 0x7a)),
        ),
    ];
    truncate_spans(&mut inner_spans, content_width);

    let inner_width: usize = inner_spans.iter().map(|s| s.content.width()).sum();
    let padding_len = content_width.saturating_sub(inner_width);

    if selected {
        for span in &mut inner_spans {
            span.style = span.style.bg(Color::DarkGray);
        }
    }

    let border_style = session_border_style(session_attached);
    let mut spans = vec![Span::styled("\u{2502} ", border_style)];
    spans.extend(inner_spans);
    if padding_len > 0 {
        let pad_style = if selected {
            Style::default().bg(Color::DarkGray)
        } else {
            Style::default()
        };
        spans.push(Span::styled(" ".repeat(padding_len), pad_style));
    }
    spans.push(Span::styled(" \u{2502}", border_style));

    Some(Line::from(spans))
}

/// Render git info sub-lines for an item. Returns multiple lines for long PR titles.
fn render_bordered_git_sub_lines(
    item: &TreeItem,
    width: u16,
    selected: bool,
    session_attached: bool,
) -> Vec<Line<'static>> {
    let (gi, prefix) = match item_git_info(item) {
        Some(v) => v,
        None => return vec![],
    };

    let content_width = (width as usize).saturating_sub(4);
    let git_style = Style::default().fg(Color::Rgb(0x7a, 0x7a, 0x7a));
    let border_style = session_border_style(session_attached);
    let prefix_style = Style::default().fg(theme::COLOR_PURPLE);

    // For branch (no PR), single line with truncation
    if gi.pr.is_none() {
        if let Some(ref branch) = gi.branch {
            let text = format!("{} {}", theme::ICON_GIT_BRANCH, branch);
            let mut inner_spans = vec![
                Span::styled(prefix.to_string(), prefix_style),
                Span::styled(text, git_style),
            ];
            truncate_spans(&mut inner_spans, content_width);
            return vec![wrap_bordered_line(inner_spans, content_width, selected, border_style)];
        }
        return vec![];
    }

    // PR: may need multiple lines for long titles
    let pr = gi.pr.as_ref().unwrap();
    let header = format!("{} #{} ", theme::ICON_PR, pr.number);
    let header_width = header.width();
    let prefix_width = prefix.width();
    let first_line_avail = content_width.saturating_sub(prefix_width + header_width);

    // Split title into lines by display width
    let title_lines = wrap_text(&pr.title, first_line_avail, content_width.saturating_sub(prefix_width));

    let mut lines = Vec::new();
    for (i, title_chunk) in title_lines.iter().enumerate() {
        let inner_spans = if i == 0 {
            vec![
                Span::styled(prefix.to_string(), prefix_style),
                Span::styled(header.clone(), git_style),
                Span::styled(title_chunk.clone(), git_style),
            ]
        } else {
            vec![
                Span::styled(prefix.to_string(), prefix_style),
                Span::styled(title_chunk.clone(), git_style),
            ]
        };
        lines.push(wrap_bordered_line(inner_spans, content_width, selected, border_style));
    }

    lines
}

/// Wrap text into lines by display width. First line has `first_width`, subsequent lines have `rest_width`.
fn wrap_text(text: &str, first_width: usize, rest_width: usize) -> Vec<String> {
    if first_width == 0 && rest_width == 0 {
        return vec![text.to_string()];
    }
    let mut lines = Vec::new();
    let mut current = String::new();
    let mut current_width = 0;
    let mut max_width = first_width;

    for c in text.chars() {
        let cw = UnicodeWidthStr::width(c.to_string().as_str());
        if current_width + cw > max_width && !current.is_empty() {
            lines.push(current);
            current = String::new();
            current_width = 0;
            max_width = rest_width;
        }
        current.push(c);
        current_width += cw;
    }
    if !current.is_empty() {
        lines.push(current);
    }
    if lines.is_empty() {
        lines.push(String::new());
    }
    lines
}

/// Build a single bordered line from inner spans with padding.
fn wrap_bordered_line(
    mut inner_spans: Vec<Span<'static>>,
    content_width: usize,
    selected: bool,
    border_style: Style,
) -> Line<'static> {
    let inner_width: usize = inner_spans.iter().map(|s| s.content.width()).sum();
    let padding_len = content_width.saturating_sub(inner_width);

    if selected {
        for span in &mut inner_spans {
            span.style = span.style.bg(Color::DarkGray);
        }
    }

    let mut spans = vec![Span::styled("\u{2502} ", border_style)];
    spans.extend(inner_spans);
    if padding_len > 0 {
        let pad_style = if selected {
            Style::default().bg(Color::DarkGray)
        } else {
            Style::default()
        };
        spans.push(Span::styled(" ".repeat(padding_len), pad_style));
    }
    spans.push(Span::styled(" \u{2502}", border_style));

    Line::from(spans)
}

/// Truncate spans to fit within max_width display columns.
fn truncate_spans(spans: &mut Vec<Span<'static>>, max_width: usize) {
    let mut total = 0;
    let mut truncate_at = spans.len();
    for (i, span) in spans.iter().enumerate() {
        let span_width = span.content.width();
        if total + span_width > max_width {
            truncate_at = i;
            break;
        }
        total += span_width;
    }
    if truncate_at < spans.len() {
        let remaining = max_width.saturating_sub(total);
        if remaining > 0 {
            // Truncate by display width, not char count
            let mut w = 0;
            let truncated: String = spans[truncate_at]
                .content
                .chars()
                .take_while(|c| {
                    w += UnicodeWidthStr::width(c.to_string().as_str());
                    w <= remaining
                })
                .collect();
            let style = spans[truncate_at].style;
            spans[truncate_at] = Span::styled(truncated, style);
            spans.truncate(truncate_at + 1);
        } else {
            spans.truncate(truncate_at);
        }
    }
}

/// Choose the icon for a Window or Pane item.
/// Priority: claude > neovim > shell > multi-pane window. Fallback: terminal.
fn item_icon(
    agent_state: Option<&AgentState>,
    pane_title: &str,
    command: Option<&str>,
    has_multiple_panes: bool,
) -> &'static str {
    if agent_state.is_some() || is_claude_code_title(pane_title) {
        return theme::ICON_CLAUDE;
    }
    if let Some(cmd) = command {
        if cmd == "nvim" {
            return theme::ICON_NEOVIM;
        }
        if is_shell(cmd) {
            return theme::ICON_TERMINAL;
        }
    }
    if has_multiple_panes {
        return theme::ICON_WINDOW;
    }
    theme::ICON_TERMINAL
}

fn render_content_spans(item: &TreeItem, session_attached: bool, _anim_frame: usize) -> Vec<Span<'static>> {
    let icon_style = session_border_style(session_attached);
    match item {
        TreeItem::Window {
            window_name,
            agent_state,
            git_info,
            pane_current_path,
            pane_current_command,
            pane_title,
            has_multiple_panes,
            session_toplevel,
            ..
        } => {
            let mut spans = Vec::new();

            let icon = item_icon(
                agent_state.as_ref(),
                pane_title.as_deref().unwrap_or(""),
                pane_current_command.as_deref(),
                *has_multiple_panes,
            );
            spans.push(Span::styled(format!("{} ", icon), icon_style));

            if !*has_multiple_panes {
                let pane_toplevel = git_info
                    .as_ref()
                    .and_then(|gi| gi.toplevel.as_deref());
                // Only use relative path when pane's repo matches session's repo
                let toplevel = if pane_toplevel == session_toplevel.as_deref() {
                    pane_toplevel
                } else {
                    None
                };
                let label =
                    if let (Some(cmd), Some(path)) = (pane_current_command, pane_current_path) {
                        display_label(cmd, path, pane_title.as_deref().unwrap_or(""), toplevel)
                    } else {
                        window_name.clone()
                    };
                let needs_attention = matches!(
                    agent_state.as_ref().map(|a| &a.state),
                    Some(AgentStatus::Permission | AgentStatus::Waiting)
                );
                if needs_attention {
                    spans.push(Span::styled(
                        label,
                        Style::default()
                            .fg(theme::COLOR_WHITE)
                            .add_modifier(Modifier::BOLD),
                    ));
                } else {
                    spans.push(Span::raw(label));
                }
            }

            spans
        }
        TreeItem::Pane { pane, session_toplevel, .. } => {
            let icon = item_icon(
                pane.agent_state.as_ref(),
                &pane.pane_title,
                Some(&pane.pane_current_command),
                false,
            );
            let mut spans = vec![
                Span::styled("  ".to_string(), icon_style),
                Span::styled(format!("{} ", icon), icon_style),
            ];

            let pane_toplevel = pane
                .git_info
                .as_ref()
                .and_then(|gi| gi.toplevel.as_deref());
            // Only use relative path when pane's repo matches session's repo
            let toplevel = if pane_toplevel == session_toplevel.as_deref() {
                pane_toplevel
            } else {
                None
            };
            let label = display_label(&pane.pane_current_command, &pane.pane_current_path, &pane.pane_title, toplevel);
            let needs_attention = matches!(
                pane.agent_state.as_ref().map(|a| &a.state),
                Some(AgentStatus::Permission | AgentStatus::Waiting)
            );
            if needs_attention {
                spans.push(Span::styled(
                    label,
                    Style::default()
                        .fg(theme::COLOR_WHITE)
                        .add_modifier(Modifier::BOLD),
                ));
            } else {
                spans.push(Span::raw(label));
            }

            spans
        }
        _ => vec![],
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
            pane_title: String::new(),
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
            pane_title: String::new(),
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
                toplevel: None,
                worktree_name: None,
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
                toplevel: None,
                worktree_name: None,
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
            worktree_name: None,
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
            pane_title: None,
            has_multiple_panes: false,
            session_toplevel: None,
        };
        assert_eq!(item.tmux_target(), "main:2");
    }

    #[test]
    fn test_tmux_target_pane() {
        let item = TreeItem::Pane {
            session_name: "dev".to_string(),
            window_index: 1,
            pane: make_pane("%5", "zsh", None),
            session_toplevel: None,
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
    fn test_shorten_relative_path() {
        assert_eq!(shorten_relative_path("repo/ui/theme.rs", 30), "repo/ui/theme.rs");
        // First component (repo dir) is never abbreviated
        assert_eq!(
            shorten_relative_path("repo/deeply/nested/dir/file.rs", 20),
            "repo/d/n/dir/file.rs"
        );
        assert_eq!(shorten_relative_path("file.rs", 30), "file.rs");
        assert_eq!(shorten_relative_path("repo/file.rs", 5), "repo/file.rs");
    }

    #[test]
    fn test_shorten_path() {
        std::env::set_var("HOME", "/home/user");
        assert_eq!(shorten_path("/home/user/src/github.com/nownabe/chikuwa"), "~/s/g/n/chikuwa");
        assert_eq!(shorten_path("/home/user/projects"), "~/projects");
        assert_eq!(shorten_path("/home/user"), "~");
        assert_eq!(shorten_path("/tmp/foo/bar"), "/t/f/bar");
        assert_eq!(shorten_path("/"), "/");
    }

    #[test]
    fn test_display_label_shell_with_toplevel() {
        assert_eq!(
            display_label("zsh", "/home/user/project/src", "", Some("/home/user/project")),
            "project/src/"
        );
        assert_eq!(
            display_label("zsh", "/home/user/project", "", Some("/home/user/project")),
            "project/"
        );
    }

    #[test]
    fn test_display_label_shell_without_toplevel() {
        std::env::set_var("HOME", "/home/user");
        assert_eq!(display_label("zsh", "/home/user/projects/myapp", "", None), "~/p/myapp/");
        assert_eq!(display_label("bash", "/tmp", "", None), "/tmp/");
    }

    #[test]
    fn test_display_label_nvim() {
        assert_eq!(display_label("nvim", "/home/user", "app.rs", None), "app.rs");
        assert_eq!(display_label("nvim", "/home/user", "", None), "nvim");
    }

    #[test]
    fn test_display_label_non_shell() {
        assert_eq!(display_label("vim", "/home/user", "", None), "vim");
        assert_eq!(display_label("node", "/home/user/project", "", None), "node");
    }

    #[test]
    fn test_relative_path() {
        assert_eq!(relative_path("/home/user/project/src/ui", Some("/home/user/project")), "project/src/ui");
        assert_eq!(relative_path("/home/user/project", Some("/home/user/project")), "project/");
    }

    #[test]
    fn test_relative_path_no_toplevel() {
        std::env::set_var("HOME", "/home/user");
        assert_eq!(relative_path("/home/user/projects/myapp", None), "~/p/myapp");
    }

    #[test]
    fn test_display_label_shell_mismatched_toplevel() {
        std::env::set_var("HOME", "/home/user");
        // When toplevel is None (mismatched session), falls back to shortened absolute path
        assert_eq!(
            display_label("zsh", "/home/user/src/github.com/nownabe/chikuwa/path/to/dir", "", None),
            "~/s/g/n/c/p/t/dir/"
        );
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
                worktree_name: None,
            },
            TreeItem::Session {
                name: "b".to_string(),
                attached: false,
                collapsed: true,
                repo_name: None,
                worktree_name: None,
            },
        ];

        assert_eq!(item_to_visual_row(&items, 0, 80), 0);
        assert_eq!(item_to_visual_row(&items, 1, 80), 1);
        assert_eq!(total_visual_rows(&items, 80), 2);
    }

    #[test]
    fn test_item_to_visual_row_expanded_no_git() {
        let sessions = vec![TmuxSession {
            session_name: "main".to_string(),
            session_attached: true,
            repo_name: None,
            toplevel: None,
            worktree_name: None,
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
        assert_eq!(item_to_visual_row(&items, 0, 80), 0);
        assert_eq!(item_to_visual_row(&items, 1, 80), 1);
        assert_eq!(item_to_visual_row(&items, 2, 80), 2);
        assert_eq!(total_visual_rows(&items, 80), 4);
    }

    #[test]
    fn test_item_to_visual_row_with_git() {
        let sessions = vec![TmuxSession {
            session_name: "main".to_string(),
            session_attached: true,
            repo_name: None,
            toplevel: None,
            worktree_name: None,
            windows: vec![
                TmuxWindow {
                    window_index: 0,
                    window_name: "claude".to_string(),
                    window_active: true,
                    panes: vec![{
                        let mut p = make_pane_with_git(
                            "%0",
                            "node",
                            GitInfo {
                                branch: Some("main".to_string()),
                                pr: None,
                                repo_name: None,
                                toplevel: None,
                                worktree_name: None,
                            },
                        );
                        p.pane_title = "✳ Claude Code".to_string();
                        p
                    }],
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
        assert_eq!(item_to_visual_row(&items, 0, 80), 0); // Session
        assert_eq!(item_to_visual_row(&items, 1, 80), 1); // Window claude
        assert_eq!(item_to_visual_row(&items, 2, 80), 3); // Window zsh (after git sub-line)
        assert_eq!(total_visual_rows(&items, 80), 5);
    }

    #[test]
    fn test_item_to_visual_row_mixed() {
        let items = vec![
            TreeItem::Session {
                name: "collapsed".to_string(),
                attached: false,
                collapsed: true,
                repo_name: None,
                worktree_name: None,
            },
            TreeItem::Session {
                name: "expanded".to_string(),
                attached: true,
                collapsed: false,
                repo_name: None,
                worktree_name: None,
            },
            TreeItem::Window {
                session_name: "expanded".to_string(),
                window_index: 0,
                window_name: "w".to_string(),
                agent_state: None,
                git_info: None,
                pane_current_path: None,
                pane_current_command: None,
                pane_title: None,
                has_multiple_panes: false,
                session_toplevel: None,
            },
        ];

        assert_eq!(item_to_visual_row(&items, 0, 80), 0);
        assert_eq!(item_to_visual_row(&items, 1, 80), 1);
        assert_eq!(item_to_visual_row(&items, 2, 80), 2);
        assert_eq!(total_visual_rows(&items, 80), 4);
    }

    #[test]
    fn test_wrap_text() {
        // Fits in first line
        assert_eq!(wrap_text("short", 10, 10), vec!["short"]);
        // Wraps to second line
        assert_eq!(wrap_text("hello world!", 5, 10), vec!["hello", " world!"]);
        // Multiple wraps
        assert_eq!(
            wrap_text("abcdefghij", 3, 4),
            vec!["abc", "defg", "hij"]
        );
    }

    #[test]
    fn test_item_git_info() {
        // Claude Code pane with git info → Some
        let claude_with_branch = TreeItem::Window {
            session_name: "s".to_string(),
            window_index: 0,
            window_name: "w".to_string(),
            agent_state: None,
            git_info: Some(GitInfo {
                branch: Some("main".to_string()),
                pr: None,
                repo_name: None,
                toplevel: None,
                worktree_name: None,
            }),
            pane_current_path: None,
            pane_current_command: None,
            pane_title: Some("✳ Claude Code".to_string()),
            has_multiple_panes: false,
            session_toplevel: None,
        };
        assert!(item_git_info(&claude_with_branch).is_some());

        // Non-Claude pane with git info → None
        let non_claude_with_branch = TreeItem::Window {
            session_name: "s".to_string(),
            window_index: 0,
            window_name: "w".to_string(),
            agent_state: None,
            git_info: Some(GitInfo {
                branch: Some("main".to_string()),
                pr: None,
                repo_name: None,
                toplevel: None,
                worktree_name: None,
            }),
            pane_current_path: None,
            pane_current_command: None,
            pane_title: None,
            has_multiple_panes: false,
            session_toplevel: None,
        };
        assert!(item_git_info(&non_claude_with_branch).is_none());

        let session = TreeItem::Session {
            name: "s".to_string(),
            attached: true,
            collapsed: false,
            repo_name: None,
            worktree_name: None,
        };
        assert!(item_git_info(&session).is_none());
    }

    #[test]
    fn test_extract_claude_activity_strips_icon() {
        let result = extract_claude_activity("✳ Rust環境変数設定");
        assert_eq!(result, Some("Rust環境変数設定".to_string()));
    }

    #[test]
    fn test_extract_claude_activity_default_title() {
        assert_eq!(extract_claude_activity("⠐ Claude Code"), Some("Claude Code".to_string()));
        assert_eq!(extract_claude_activity("✳ Claude Code"), Some("Claude Code".to_string()));
    }

    #[test]
    fn test_extract_claude_activity_none_for_non_claude() {
        // Regular pane titles (no leading icon) should return None
        assert_eq!(extract_claude_activity(""), None);
        assert_eq!(extract_claude_activity("zsh"), None);
        assert_eq!(extract_claude_activity("~/src/project"), None);
    }

    #[test]
    fn test_display_label_claude_activity() {
        assert_eq!(
            display_label("node", "/home/user", "✳ Rust環境変数設定", None),
            "Rust環境変数設定"
        );
        assert_eq!(
            display_label("node", "/home/user", "✳ Fixing bug", None),
            "Fixing bug"
        );
    }

    #[test]
    fn test_display_label_claude_default_title() {
        assert_eq!(
            display_label("node", "/home/user", "⠐ Claude Code", None),
            "Claude Code"
        );
        assert_eq!(
            display_label("node", "/home/user", "✳ Claude Code", None),
            "Claude Code"
        );
    }
}
