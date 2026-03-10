use std::time::Duration;

use anyhow::Result;
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyModifiers};
use tokio::sync::mpsc;

use crate::agent::state::AgentState;
use crate::usage::Usage;

/// Application events.
#[derive(Debug)]
pub enum AppEvent {
    Key(KeyEvent),
    Tick,
    AnimationTick,
    AgentStateUpdate(AgentState),
    TmuxChanged,
    UsageUpdate(Usage),
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

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyEventState, KeyModifiers};

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent {
            code,
            modifiers: KeyModifiers::NONE,
            kind: KeyEventKind::Press,
            state: KeyEventState::NONE,
        }
    }

    fn key_with_mod(code: KeyCode, modifiers: KeyModifiers) -> KeyEvent {
        KeyEvent {
            code,
            modifiers,
            kind: KeyEventKind::Press,
            state: KeyEventState::NONE,
        }
    }

    #[test]
    fn test_quit_q() {
        assert_eq!(handle_key(key(KeyCode::Char('q'))), Action::Quit);
    }

    #[test]
    fn test_quit_ctrl_c() {
        assert_eq!(
            handle_key(key_with_mod(KeyCode::Char('c'), KeyModifiers::CONTROL)),
            Action::Quit
        );
    }

    #[test]
    fn test_navigation_jk() {
        assert_eq!(handle_key(key(KeyCode::Char('j'))), Action::Down);
        assert_eq!(handle_key(key(KeyCode::Char('k'))), Action::Up);
    }

    #[test]
    fn test_navigation_arrows() {
        assert_eq!(handle_key(key(KeyCode::Down)), Action::Down);
        assert_eq!(handle_key(key(KeyCode::Up)), Action::Up);
    }

    #[test]
    fn test_select_enter() {
        assert_eq!(handle_key(key(KeyCode::Enter)), Action::Select);
    }

    #[test]
    fn test_select_space() {
        assert_eq!(handle_key(key(KeyCode::Char(' '))), Action::Select);
    }

    #[test]
    fn test_top_bottom() {
        assert_eq!(handle_key(key(KeyCode::Char('g'))), Action::Top);
        assert_eq!(handle_key(key(KeyCode::Char('G'))), Action::Bottom);
    }

    #[test]
    fn test_unknown_key() {
        assert_eq!(handle_key(key(KeyCode::Char('x'))), Action::None);
        assert_eq!(handle_key(key(KeyCode::F(1))), Action::None);
    }
}
