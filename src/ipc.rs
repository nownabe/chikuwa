use std::path::PathBuf;

use anyhow::{Context, Result};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::mpsc;

use crate::agent::state::AgentState;
use crate::event::AppEvent;

/// Returns the Unix domain socket path.
pub fn socket_path() -> PathBuf {
    if let Ok(runtime_dir) = std::env::var("XDG_RUNTIME_DIR") {
        PathBuf::from(runtime_dir).join("chikuwa.sock")
    } else {
        PathBuf::from("/tmp/chikuwa.sock")
    }
}

/// Client side: connect to the socket, send a JSON line, and disconnect.
/// Fails silently if the TUI is not running.
pub async fn send_state(state: &AgentState) -> Result<()> {
    let path = socket_path();

    let mut stream = match UnixStream::connect(&path).await {
        Ok(s) => s,
        Err(_) => return Ok(()), // TUI not running, fail silently
    };

    let mut json = serde_json::to_string(state).context("Failed to serialize agent state")?;
    json.push('\n');

    stream
        .write_all(json.as_bytes())
        .await
        .context("Failed to write to socket")?;
    stream.shutdown().await.ok();

    Ok(())
}

/// Server side: listen on the socket and send events through the channel.
pub async fn start_listener(tx: mpsc::Sender<AppEvent>) -> Result<()> {
    let path = socket_path();

    // Remove stale socket file if it exists
    if path.exists() {
        std::fs::remove_file(&path).ok();
    }

    let listener = UnixListener::bind(&path).context("Failed to bind Unix socket")?;

    loop {
        let (stream, _) = match listener.accept().await {
            Ok(conn) => conn,
            Err(_) => continue,
        };

        let tx = tx.clone();
        tokio::spawn(async move {
            let reader = BufReader::new(stream);
            let mut lines = reader.lines();

            while let Ok(Some(line)) = lines.next_line().await {
                let line = line.trim().to_string();
                if line.is_empty() {
                    continue;
                }

                if let Ok(state) = serde_json::from_str::<AgentState>(&line) {
                    let _ = tx.send(AppEvent::AgentStateUpdate(state)).await;
                }
            }
        });
    }
}

/// Remove the socket file on shutdown.
pub fn cleanup_socket() {
    let path = socket_path();
    if path.exists() {
        std::fs::remove_file(&path).ok();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_socket_path_with_xdg() {
        std::env::set_var("XDG_RUNTIME_DIR", "/run/user/1000");
        let path = socket_path();
        assert_eq!(path, PathBuf::from("/run/user/1000/chikuwa.sock"));
        std::env::remove_var("XDG_RUNTIME_DIR");
    }

    #[test]
    fn test_socket_path_fallback() {
        std::env::remove_var("XDG_RUNTIME_DIR");
        let path = socket_path();
        assert_eq!(path, PathBuf::from("/tmp/chikuwa.sock"));
    }
}
