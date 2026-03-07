use std::time::Duration;

use anyhow::Result;
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyModifiers};
use tokio::sync::mpsc;

/// Application events.
#[derive(Debug)]
pub enum AppEvent {
    Key(KeyEvent),
    Tick,
}

/// Spawn an event loop that sends key events and periodic ticks.
pub async fn event_loop(tx: mpsc::Sender<AppEvent>, tick_rate: Duration) -> Result<()> {
    loop {
        if event::poll(tick_rate)? {
            if let Event::Key(key) = event::read()? {
                if tx.send(AppEvent::Key(key)).await.is_err() {
                    return Ok(());
                }
            }
        } else if tx.send(AppEvent::Tick).await.is_err() {
            return Ok(());
        }
    }
}

/// Process a key event and return an action.
pub fn handle_key(key: KeyEvent) -> Action {
    match key.code {
        KeyCode::Char('q') => Action::Quit,
        KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => Action::Quit,
        KeyCode::Char('j') | KeyCode::Down => Action::Down,
        KeyCode::Char('k') | KeyCode::Up => Action::Up,
        KeyCode::Enter | KeyCode::Char(' ') => Action::Select,
        KeyCode::Char('g') => Action::Top,
        KeyCode::Char('G') => Action::Bottom,
        _ => Action::None,
    }
}

#[derive(Debug, PartialEq)]
pub enum Action {
    Quit,
    Up,
    Down,
    Select,
    Top,
    Bottom,
    None,
}
