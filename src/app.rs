use std::collections::HashSet;
use std::io;
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

use crate::event::{self, Action, AppEvent};
use crate::tmux::{client as tmux_client, types::TmuxSession};
use crate::ui::{status_bar, tree};

pub struct App {
    sessions: Vec<TmuxSession>,
    tree_items: Vec<tree::TreeItem>,
    selected: usize,
    scroll_offset: usize,
    collapsed: HashSet<String>,
    should_quit: bool,
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
        }
    }

    /// Refresh tmux data and rebuild the tree.
    async fn refresh(&mut self) -> Result<()> {
        match tmux_client::fetch_tree().await {
            Ok(sessions) => {
                self.sessions = sessions;
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

    fn rebuild_tree(&mut self) {
        self.tree_items = tree::flatten(&self.sessions, &self.collapsed);
        // Clamp selected index
        if !self.tree_items.is_empty() && self.selected >= self.tree_items.len() {
            self.selected = self.tree_items.len() - 1;
        }
    }

    fn move_up(&mut self) {
        if self.selected > 0 {
            self.selected -= 1;
            self.ensure_visible();
        }
    }

    fn move_down(&mut self) {
        if !self.tree_items.is_empty() && self.selected < self.tree_items.len() - 1 {
            self.selected += 1;
            self.ensure_visible();
        }
    }

    fn move_top(&mut self) {
        self.selected = 0;
        self.scroll_offset = 0;
    }

    fn move_bottom(&mut self) {
        if !self.tree_items.is_empty() {
            self.selected = self.tree_items.len() - 1;
            // scroll_offset will be adjusted in ensure_visible during render
        }
    }

    fn ensure_visible(&mut self) {
        if self.selected < self.scroll_offset {
            self.scroll_offset = self.selected;
        }
        // The visible height will be applied during rendering
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

    // Restore terminal
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
    tokio::task::spawn_blocking(move || {
        let rt = tokio::runtime::Handle::current();
        rt.block_on(event::event_loop(tx, tick_rate))
    });

    loop {
        // Draw
        terminal.draw(|f| {
            let size = f.area();

            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Min(3), Constraint::Length(2)])
                .split(size);

            // Adjust scroll for visible area
            let visible_height = chunks[0].height.saturating_sub(2) as usize; // account for border
            if app.selected >= app.scroll_offset + visible_height {
                app.scroll_offset = app.selected.saturating_sub(visible_height - 1);
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
            }
        }
    }
}
