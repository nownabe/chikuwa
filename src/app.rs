use std::collections::{HashMap, HashSet};
use std::io;
use std::path::PathBuf;
use std::time::Duration;

use anyhow::Result;
use crossterm::event::{DisableMouseCapture, EnableMouseCapture};
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::Terminal;
use tokio::sync::mpsc;

use crate::agent::state::AgentState;
use crate::event::{self, Action, AppEvent};
use crate::git::GitInfoCache;
use crate::ipc;
use crate::tmux::{client as tmux_client, types::TmuxSession};
use crate::ui::{status_bar, tree};

/// Extract the filename and optional directory from an nvim pane_title.
/// Nvim titles are typically formatted as "filename (dir) - Nvim".
/// Plugin UIs like NeoTree produce titles like "neo-tree filesystem [1] - Nvim".
/// Returns Some((filename, Option<dir>)) for valid file titles, None for plugin UIs.
fn extract_nvim_file_info(title: &str) -> Option<(&str, Option<&str>)> {
    // Nvim standard format: "filename (dir) - Nvim" or "filename - Nvim"
    if let Some(rest) = title.strip_suffix(" - Nvim") {
        // Try to extract "filename (dir)"
        if let Some(paren_start) = rest.find(" (") {
            let name = &rest[..paren_start];
            if !name.is_empty() && !name.contains(' ') {
                let dir = &rest[paren_start + 2..];
                let dir = dir.strip_suffix(')').unwrap_or(dir);
                return Some((name, Some(dir)));
            }
            return None;
        }
        // "filename - Nvim" without directory
        if !rest.is_empty() && !rest.contains(' ') {
            return Some((rest, None));
        }
        return None;
    }
    // Bare filename without " - Nvim" suffix
    if !title.is_empty() && !title.contains(' ') && !title.starts_with("term://") {
        return Some((title, None));
    }
    None
}

/// Compute relative path from git toplevel, abbreviating directories
/// progressively from left if the result exceeds max_len.
fn relative_nvim_path(filename: &str, dir: Option<&str>, toplevel: Option<&str>) -> String {
    let Some(dir) = dir else {
        return filename.to_string();
    };
    let Some(toplevel) = toplevel else {
        return filename.to_string();
    };

    // Expand ~ in dir
    let home = std::env::var("HOME").unwrap_or_default();
    let expanded_dir = if dir.starts_with("~/") {
        format!("{}{}", home, &dir[1..])
    } else if dir == "~" {
        home.clone()
    } else {
        dir.to_string()
    };

    // Extract repo dir name from toplevel
    let repo_dir = std::path::Path::new(toplevel)
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();

    // Compute relative path from toplevel
    let full_path = format!("{}/{}", expanded_dir, filename);
    let full = if let Some(rest) = full_path.strip_prefix(toplevel) {
        let rest = rest.strip_prefix('/').unwrap_or(rest);
        if rest.is_empty() {
            return filename.to_string();
        }
        format!("{}/{}", repo_dir, rest)
    } else {
        return filename.to_string();
    };

    tree::shorten_relative_path(&full, 30)
}

pub struct App {
    sessions: Vec<TmuxSession>,
    tree_items: Vec<tree::TreeItem>,
    selected: usize,
    scroll_offset: usize,
    collapsed: HashSet<String>,
    should_quit: bool,
    agent_states: HashMap<String, AgentState>,
    git_cache: GitInfoCache,
    anim_frame: usize,
    /// Cache of last valid nvim file title per pane_id.
    nvim_title_cache: HashMap<String, String>,
    /// Last known terminal width for visual row calculations.
    last_width: u16,
}

impl App {
    pub fn new() -> Self {
        Self {
            sessions: Vec::new(),
            tree_items: Vec::new(),
            selected: 0,
            scroll_offset: 0,
            collapsed: HashSet::new(),
            should_quit: false,
            agent_states: HashMap::new(),
            git_cache: GitInfoCache::new(),
            anim_frame: 0,
            nvim_title_cache: HashMap::new(),
            last_width: 80,
        }
    }

    /// Refresh tmux data and rebuild the tree.
    async fn refresh(&mut self) -> Result<()> {
        match tmux_client::fetch_tree(&self.agent_states).await {
            Ok(sessions) => {
                self.sessions = sessions;
                self.merge_git_info().await;
                self.fixup_nvim_titles();
                self.rebuild_tree();
            }
            Err(_) => {
                // tmux not running or error - show empty state
                self.sessions.clear();
                self.tree_items.clear();
            }
        }
        Ok(())
    }

    /// Fetch git info for all unique pane paths and merge into panes.
    async fn merge_git_info(&mut self) {
        // Collect unique paths
        let mut active_paths = HashSet::new();
        for session in &self.sessions {
            for window in &session.windows {
                for pane in &window.panes {
                    active_paths.insert(PathBuf::from(&pane.pane_current_path));
                }
            }
        }

        // GC stale cache entries
        self.git_cache.retain_paths(&active_paths);

        // Fetch git info for each unique path
        let mut path_info: HashMap<String, crate::git::GitInfo> = HashMap::new();
        for path in &active_paths {
            if let Some(path_str) = path.to_str() {
                if let Some(info) = self.git_cache.get(path_str).await {
                    path_info.insert(path_str.to_string(), info);
                }
            }
        }

        // Merge into panes and derive session repo_name
        for session in &mut self.sessions {
            for window in &mut session.windows {
                for pane in &mut window.panes {
                    pane.git_info = path_info.get(&pane.pane_current_path).cloned();
                }
            }
            // Derive repo_name and toplevel from the first pane that has them
            session.repo_name = session
                .windows
                .iter()
                .flat_map(|w| w.panes.iter())
                .find_map(|p| p.git_info.as_ref().and_then(|gi| gi.repo_name.clone()));
            session.toplevel = session
                .windows
                .iter()
                .flat_map(|w| w.panes.iter())
                .find_map(|p| p.git_info.as_ref().and_then(|gi| gi.toplevel.clone()));
            session.worktree_name = session
                .windows
                .iter()
                .flat_map(|w| w.panes.iter())
                .find_map(|p| p.git_info.as_ref().and_then(|gi| gi.worktree_name.clone()));
        }
    }

    /// For nvim panes, extract the filename from the title and compute
    /// relative path from git toplevel. Plugin UI titles are replaced with
    /// the last known path from cache.
    fn fixup_nvim_titles(&mut self) {
        for session in &mut self.sessions {
            let session_toplevel = session.toplevel.clone();
            for window in &mut session.windows {
                for pane in &mut window.panes {
                    if pane.pane_current_command != "nvim" {
                        continue;
                    }
                    if let Some((filename, dir)) = extract_nvim_file_info(&pane.pane_title) {
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
                        let label = relative_nvim_path(filename, dir, toplevel);
                        self.nvim_title_cache
                            .insert(pane.pane_id.clone(), label.clone());
                        pane.pane_title = label;
                    } else if let Some(cached) = self.nvim_title_cache.get(&pane.pane_id) {
                        pane.pane_title = cached.clone();
                    }
                }
            }
        }
    }

    fn rebuild_tree(&mut self) {
        self.tree_items = tree::flatten(&self.sessions, &self.collapsed);
        // Clamp selected index
        if !self.tree_items.is_empty() && self.selected >= self.tree_items.len() {
            self.selected = self.tree_items.len() - 1;
        }
        // Ensure selected is not a Session item
        self.snap_to_selectable();
        // Clamp scroll offset to valid visual row range
        let total_visual = tree::total_visual_rows(&self.tree_items, self.last_width);
        if total_visual > 0 && self.scroll_offset >= total_visual {
            self.scroll_offset = total_visual - 1;
        }
    }

    /// Merge agent states into the existing session tree without re-fetching tmux.
    fn merge_agent_states(&mut self) {
        for session in &mut self.sessions {
            for window in &mut session.windows {
                for pane in &mut window.panes {
                    pane.agent_state = self.agent_states.get(&pane.pane_id).cloned();
                }
            }
        }
        self.rebuild_tree();
    }

    fn move_up(&mut self) {
        let mut idx = self.selected;
        while idx > 0 {
            idx -= 1;
            if self.tree_items[idx].is_selectable() {
                self.selected = idx;
                self.ensure_visible();
                return;
            }
        }
    }

    fn move_down(&mut self) {
        let mut idx = self.selected;
        while idx < self.tree_items.len().saturating_sub(1) {
            idx += 1;
            if self.tree_items[idx].is_selectable() {
                self.selected = idx;
                self.ensure_visible();
                return;
            }
        }
    }

    fn move_top(&mut self) {
        if let Some(idx) = self
            .tree_items
            .iter()
            .position(|item| item.is_selectable())
        {
            self.selected = idx;
        }
        self.scroll_offset = 0;
    }

    fn move_bottom(&mut self) {
        if let Some(idx) = self
            .tree_items
            .iter()
            .rposition(|item| item.is_selectable())
        {
            self.selected = idx;
        }
    }

    /// Snap selected to a selectable item if it currently points to a non-selectable one.
    fn snap_to_selectable(&mut self) {
        if self.tree_items.is_empty() {
            return;
        }
        if self.tree_items[self.selected].is_selectable() {
            return;
        }
        // Try forward first, then backward
        if let Some(offset) = self.tree_items[self.selected..]
            .iter()
            .position(|item| item.is_selectable())
        {
            self.selected += offset;
        } else if let Some(idx) = self
            .tree_items
            .iter()
            .position(|item| item.is_selectable())
        {
            self.selected = idx;
        }
    }

    fn ensure_visible(&mut self) {
        let visual = tree::item_to_visual_row(&self.tree_items, self.selected, self.last_width);
        if visual < self.scroll_offset {
            self.scroll_offset = visual;
        }
        // Upper bound adjusted during rendering
    }

    async fn handle_select(&mut self) -> Result<()> {
        if self.tree_items.is_empty() {
            return Ok(());
        }

        let item = &self.tree_items[self.selected];

        // Toggle collapse for sessions
        if let tree::TreeItem::Session { name, .. } = item {
            let name = name.clone();
            if self.collapsed.contains(&name) {
                self.collapsed.remove(&name);
            } else {
                self.collapsed.insert(name);
            }
            self.rebuild_tree();
            return Ok(());
        }

        // Switch tmux for windows/panes
        let target = item.tmux_target();
        if let Ok(Some(client)) = tmux_client::detect_client().await {
            let _ = tmux_client::switch_to(&client, &target).await;
            self.refresh().await?;
        }

        Ok(())
    }
}

/// Run the TUI application.
pub async fn run() -> Result<()> {
    // Setup terminal
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let result = run_app(&mut terminal).await;

    // Cleanup
    ipc::cleanup_socket();
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;

    result
}

async fn run_app(terminal: &mut Terminal<CrosstermBackend<io::Stdout>>) -> Result<()> {
    let mut app = App::new();

    // Initial data fetch
    app.refresh().await?;

    // Event channel
    let (tx, mut rx) = mpsc::channel(32);
    let tick_rate = Duration::from_secs(2);

    // Spawn event loop in a blocking thread (crossterm events are blocking)
    let event_tx = tx.clone();
    tokio::task::spawn_blocking(move || {
        let rt = tokio::runtime::Handle::current();
        rt.block_on(event::event_loop(event_tx, tick_rate))
    });

    // Start IPC socket listener
    let ipc_tx = tx.clone();
    tokio::spawn(async move {
        if let Err(e) = ipc::start_listener(ipc_tx).await {
            eprintln!("IPC listener error: {}", e);
        }
    });

    // Animation tick (80ms for smooth spinner)
    let anim_tx = tx.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_millis(130));
        loop {
            interval.tick().await;
            if anim_tx.send(AppEvent::AnimationTick).await.is_err() {
                break;
            }
        }
    });

    loop {
        // Draw
        terminal.draw(|f| {
            let size = f.area();

            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Min(3), Constraint::Length(2)])
                .split(size);

            // Adjust scroll for visible area (visual rows, no outer border)
            let visible_height = chunks[0].height as usize;
            app.last_width = chunks[0].width;
            let selected_visual =
                tree::item_to_visual_row(&app.tree_items, app.selected, app.last_width);
            if selected_visual >= app.scroll_offset + visible_height {
                app.scroll_offset = selected_visual.saturating_sub(visible_height - 1);
            }
            if selected_visual < app.scroll_offset {
                app.scroll_offset = selected_visual;
            }

            // Render tree with inline agent status on single-pane windows
            tree::render(
                f,
                chunks[0],
                &app.tree_items,
                app.selected,
                app.scroll_offset,
                app.anim_frame,
            );

            // Render status bar
            status_bar::render(f, chunks[1], &app.sessions);
        })?;

        if app.should_quit {
            return Ok(());
        }

        // Handle events
        if let Some(evt) = rx.recv().await {
            match evt {
                AppEvent::Key(key) => {
                    let action = event::handle_key(key);
                    match action {
                        Action::Quit => app.should_quit = true,
                        Action::Up => app.move_up(),
                        Action::Down => app.move_down(),
                        Action::Select => app.handle_select().await?,
                        Action::Top => app.move_top(),
                        Action::Bottom => app.move_bottom(),
                        Action::None => {}
                    }
                }
                AppEvent::Tick => {
                    app.refresh().await?;
                }
                AppEvent::AnimationTick => {
                    app.anim_frame = app.anim_frame.wrapping_add(1);
                }
                AppEvent::AgentStateUpdate(state) => {
                    use crate::agent::state::AgentStatus;
                    if state.state == AgentStatus::Ended {
                        app.agent_states.remove(&state.tmux_pane);
                    } else if let Some(existing) =
                        app.agent_states.get(&state.tmux_pane)
                    {
                        // Preserve existing session_id if incoming is None
                        let session_id = state
                            .session_id
                            .clone()
                            .or_else(|| existing.session_id.clone());
                        let mut merged = state;
                        merged.session_id = session_id;
                        app.agent_states
                            .insert(merged.tmux_pane.clone(), merged);
                    } else {
                        app.agent_states
                            .insert(state.tmux_pane.clone(), state);
                    }
                    app.merge_agent_states();
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tmux::types::{TmuxPane, TmuxSession, TmuxWindow};

    fn make_nvim_pane(pane_id: &str, title: &str) -> TmuxPane {
        TmuxPane {
            pane_id: pane_id.to_string(),
            pane_index: 0,
            pane_current_command: "nvim".to_string(),
            pane_current_path: "/home/user".to_string(),
            pane_title: title.to_string(),
            pane_active: true,
            agent_state: None,
            git_info: None,
        }
    }

    fn make_session(panes: Vec<TmuxPane>) -> TmuxSession {
        TmuxSession {
            session_name: "test".to_string(),
            session_attached: true,
            windows: vec![TmuxWindow {
                window_index: 0,
                window_name: "nvim".to_string(),
                window_active: true,
                panes,
            }],
            repo_name: None,
            toplevel: None,
            worktree_name: None,
        }
    }

    #[test]
    fn test_extract_nvim_file_info_standard_format() {
        assert_eq!(
            extract_nvim_file_info("theme.rs (~/src/project/src/ui) - Nvim"),
            Some(("theme.rs", Some("~/src/project/src/ui")))
        );
        assert_eq!(
            extract_nvim_file_info("CLAUDE.md (~/src/project/.claude) - Nvim"),
            Some(("CLAUDE.md", Some("~/src/project/.claude")))
        );
    }

    #[test]
    fn test_extract_nvim_file_info_no_dir() {
        assert_eq!(
            extract_nvim_file_info("app.rs - Nvim"),
            Some(("app.rs", None))
        );
    }

    #[test]
    fn test_extract_nvim_file_info_bare() {
        assert_eq!(extract_nvim_file_info("app.rs"), Some(("app.rs", None)));
    }

    #[test]
    fn test_extract_nvim_file_info_invalid() {
        assert_eq!(extract_nvim_file_info(""), None);
        assert_eq!(extract_nvim_file_info("neo-tree filesystem [1]"), None);
        assert_eq!(
            extract_nvim_file_info("neo-tree filesystem [1] - Nvim"),
            None
        );
        assert_eq!(extract_nvim_file_info("[No Name] - Nvim"), None);
        assert_eq!(extract_nvim_file_info("term://something"), None);
    }

    #[test]
    fn test_relative_nvim_path_with_toplevel() {
        std::env::set_var("HOME", "/home/user");
        assert_eq!(
            relative_nvim_path(
                "theme.rs",
                Some("~/src/project/src/ui"),
                Some("/home/user/src/project")
            ),
            "project/src/ui/theme.rs"
        );
    }

    #[test]
    fn test_relative_nvim_path_no_dir() {
        assert_eq!(relative_nvim_path("app.rs", None, Some("/project")), "app.rs");
    }

    #[test]
    fn test_relative_nvim_path_no_toplevel() {
        assert_eq!(
            relative_nvim_path("app.rs", Some("~/project/src"), None),
            "app.rs"
        );
    }

    #[test]
    fn test_relative_nvim_path_abbreviation() {
        std::env::set_var("HOME", "/home/user");
        // A long relative path should be abbreviated
        let result = relative_nvim_path(
            "very_long_filename.rs",
            Some("~/project/src/deeply/nested/directory"),
            Some("/home/user/project"),
        );
        // "src/deeply/nested/directory/very_long_filename.rs" is > 30 chars
        // Should abbreviate to something like "s/d/n/directory/very_long_filename.rs"
        assert!(result.len() <= 30 || !result.contains("deeply"));
        assert!(result.ends_with("very_long_filename.rs"));
    }

    #[test]
    fn test_fixup_computes_relative_path() {
        std::env::set_var("HOME", "/home/user");
        let mut app = App::new();
        let mut pane = make_nvim_pane("%0", "theme.rs (~/project/src/ui) - Nvim");
        pane.git_info = Some(crate::git::GitInfo {
            branch: None,
            pr: None,
            repo_name: None,
            toplevel: Some("/home/user/project".to_string()),
            worktree_name: None,
        });
        let mut session = make_session(vec![pane]);
        session.toplevel = Some("/home/user/project".to_string());
        app.sessions = vec![session];

        app.fixup_nvim_titles();

        assert_eq!(
            app.sessions[0].windows[0].panes[0].pane_title,
            "project/src/ui/theme.rs"
        );
    }

    #[test]
    fn test_fixup_restores_cached_title_for_plugin_ui() {
        std::env::set_var("HOME", "/home/user");
        let mut app = App::new();
        let mut pane = make_nvim_pane("%0", "app.rs (~/project/src) - Nvim");
        pane.git_info = Some(crate::git::GitInfo {
            branch: None,
            pr: None,
            repo_name: None,
            toplevel: Some("/home/user/project".to_string()),
            worktree_name: None,
        });
        let mut session = make_session(vec![pane]);
        session.toplevel = Some("/home/user/project".to_string());
        app.sessions = vec![session];
        app.fixup_nvim_titles();

        // Second refresh: plugin UI title → restored from cache
        app.sessions = vec![make_session(vec![make_nvim_pane(
            "%0",
            "neo-tree filesystem [1]",
        )])];
        app.fixup_nvim_titles();

        assert_eq!(
            app.sessions[0].windows[0].panes[0].pane_title,
            "project/src/app.rs"
        );
    }

    #[test]
    fn test_fixup_no_cache_leaves_invalid_title() {
        let mut app = App::new();
        app.sessions = vec![make_session(vec![make_nvim_pane(
            "%0",
            "neo-tree filesystem [1]",
        )])];
        app.fixup_nvim_titles();

        assert_eq!(
            app.sessions[0].windows[0].panes[0].pane_title,
            "neo-tree filesystem [1]"
        );
    }

    #[test]
    fn test_fixup_skips_non_nvim_panes() {
        let mut app = App::new();
        let mut pane = make_nvim_pane("%0", "some title with spaces");
        pane.pane_current_command = "zsh".to_string();
        app.sessions = vec![make_session(vec![pane])];

        app.fixup_nvim_titles();

        assert!(app.nvim_title_cache.is_empty());
    }

    #[test]
    fn test_fixup_updates_cache_on_file_change() {
        std::env::set_var("HOME", "/home/user");
        let mut app = App::new();
        let mut pane = make_nvim_pane("%0", "app.rs (~/project/src) - Nvim");
        pane.git_info = Some(crate::git::GitInfo {
            branch: None,
            pr: None,
            repo_name: None,
            toplevel: Some("/home/user/project".to_string()),
            worktree_name: None,
        });
        let mut session = make_session(vec![pane]);
        session.toplevel = Some("/home/user/project".to_string());
        app.sessions = vec![session];
        app.fixup_nvim_titles();
        assert_eq!(app.nvim_title_cache.get("%0").unwrap(), "project/src/app.rs");

        let mut pane2 = make_nvim_pane("%0", "main.rs (~/project/src) - Nvim");
        pane2.git_info = Some(crate::git::GitInfo {
            branch: None,
            pr: None,
            repo_name: None,
            toplevel: Some("/home/user/project".to_string()),
            worktree_name: None,
        });
        let mut session2 = make_session(vec![pane2]);
        session2.toplevel = Some("/home/user/project".to_string());
        app.sessions = vec![session2];
        app.fixup_nvim_titles();
        assert_eq!(app.nvim_title_cache.get("%0").unwrap(), "project/src/main.rs");
    }

    #[test]
    fn test_fixup_mismatched_session_toplevel_falls_back_to_filename() {
        std::env::set_var("HOME", "/home/user");
        let mut app = App::new();
        let mut pane = make_nvim_pane("%0", "theme.rs (~/chikuwa/src/ui) - Nvim");
        pane.git_info = Some(crate::git::GitInfo {
            branch: None,
            pr: None,
            repo_name: None,
            toplevel: Some("/home/user/chikuwa".to_string()),
            worktree_name: None,
        });
        // Session belongs to a different repo
        let mut session = make_session(vec![pane]);
        session.toplevel = Some("/home/user/other-project".to_string());
        app.sessions = vec![session];

        app.fixup_nvim_titles();

        // Should fall back to just the filename since repos don't match
        assert_eq!(
            app.sessions[0].windows[0].panes[0].pane_title,
            "theme.rs"
        );
    }
}
