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

pub struct App {
    sessions: Vec<TmuxSession>,
    tree_items: Vec<tree::TreeItem>,
    selected: usize,
    scroll_offset: usize,
    collapsed: HashSet<String>,
    should_quit: bool,
    agent_states: HashMap<String, AgentState>,
    git_cache: GitInfoCache,
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
        }
    }

    /// Refresh tmux data and rebuild the tree.
    async fn refresh(&mut self) -> Result<()> {
        match tmux_client::fetch_tree(&self.agent_states).await {
            Ok(sessions) => {
                self.sessions = sessions;
                self.merge_git_info().await;
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
            // Derive repo_name from the first pane that has one
            session.repo_name = session
                .windows
                .iter()
                .flat_map(|w| w.panes.iter())
                .find_map(|p| p.git_info.as_ref().and_then(|gi| gi.repo_name.clone()));
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
        let total_visual = tree::total_visual_rows(&self.tree_items);
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
        let visual = tree::item_to_visual_row(&self.tree_items, self.selected);
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
            let selected_visual =
                tree::item_to_visual_row(&app.tree_items, app.selected);
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
