use std::io::Read;

use anyhow::{Context, Result};
use serde::Deserialize;

use crate::agent::state::{AgentState, AgentStatus, ToolInfo};
use crate::ipc;

/// Input JSON from Claude Code hooks (stdin).
#[derive(Debug, Deserialize)]
struct HookInput {
    hook_event_name: String,
    #[serde(default)]
    session_id: Option<String>,
    #[serde(default)]
    tool_name: Option<String>,
    #[serde(default)]
    tool_input: Option<serde_json::Value>,
}

/// Extract a short detail string from tool_input based on the tool name.
/// For tools with file paths, formats as `file_path:line_number` (nvim-compatible) when a line number is available.
fn extract_tool_detail(tool_name: &str, input: &serde_json::Value) -> Option<String> {
    let s = match tool_name {
        "Bash" => input.get("command")?.as_str()?,
        "Read" => {
            let path = input.get("file_path")?.as_str()?;
            if let Some(offset) = input.get("offset").and_then(|v| v.as_u64()) {
                return Some(format!("{path}:{offset}"));
            }
            path
        }
        "Write" | "Edit" => input.get("file_path")?.as_str()?,
        "NotebookEdit" => input.get("notebook_path")?.as_str()?,
        "Grep" => input.get("pattern")?.as_str()?,
        "Glob" => input.get("pattern")?.as_str()?,
        "WebFetch" => input.get("url")?.as_str()?,
        "WebSearch" => input.get("query")?.as_str()?,
        "Task" => {
            if let Some(desc) = input.get("description").and_then(|v| v.as_str()) {
                return Some(desc.to_string());
            }
            return None;
        }
        _ => return None,
    };
    Some(s.to_string())
}

/// Run the hook subcommand: read stdin JSON, determine event from hook_event_name, send state via IPC.
pub async fn run() -> Result<()> {
    let pane_id = std::env::var("TMUX_PANE")
        .context("TMUX_PANE environment variable not set (not running inside tmux?)")?;

    let mut stdin_buf = String::new();
    std::io::stdin()
        .read_to_string(&mut stdin_buf)
        .context("Failed to read stdin")?;

    let input: HookInput = serde_json::from_str(stdin_buf.trim())
        .context("Failed to parse hook input JSON from stdin")?;

    let status = match input.hook_event_name.as_str() {
        "SessionStart" => AgentStatus::Started,
        "UserPromptSubmit" | "PreToolUse" | "PostToolUse" | "PostToolUseFailure"
        | "SubagentStart" | "SubagentStop" => AgentStatus::Running,
        "Stop" => AgentStatus::Waiting,
        "PermissionRequest" => AgentStatus::Permission,
        "Notification" => {
            if stdin_buf.contains("permission_prompt") {
                AgentStatus::Permission
            } else {
                return Ok(());
            }
        }
        "SessionEnd" => AgentStatus::Ended,
        _ => return Ok(()),
    };

    let mut state = AgentState::new(pane_id, status);
    state.session_id = input.session_id;
    state.hook_event_name = Some(input.hook_event_name);
    if let Some(ref name) = input.tool_name {
        let detail = input
            .tool_input
            .as_ref()
            .and_then(|inp| extract_tool_detail(name, inp));
        state.tools = vec![ToolInfo {
            name: name.clone(),
            detail,
        }];
    }
    state.tool_name = input.tool_name;
    ipc::send_state(&state).await?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hook_input_deserialize() {
        let json = r#"{"hook_event_name":"SessionStart","session_id":"abc123"}"#;
        let input: HookInput = serde_json::from_str(json).unwrap();
        assert_eq!(input.hook_event_name, "SessionStart");
        assert_eq!(input.session_id, Some("abc123".to_string()));
    }

    #[test]
    fn test_hook_input_deserialize_without_session_id() {
        let json = r#"{"hook_event_name":"SessionEnd"}"#;
        let input: HookInput = serde_json::from_str(json).unwrap();
        assert_eq!(input.hook_event_name, "SessionEnd");
        assert!(input.session_id.is_none());
    }

    #[test]
    fn test_hook_input_deserialize_with_extra_fields() {
        let json = r#"{"hook_event_name":"Notification","session_id":"s1","message":"permission_prompt foo"}"#;
        let input: HookInput = serde_json::from_str(json).unwrap();
        assert_eq!(input.hook_event_name, "Notification");
        assert_eq!(input.session_id, Some("s1".to_string()));
    }

    #[test]
    fn test_extract_tool_detail_read_with_offset() {
        let input = serde_json::json!({"file_path": "/src/main.rs", "offset": 42});
        assert_eq!(
            extract_tool_detail("Read", &input),
            Some("/src/main.rs:42".to_string())
        );
    }

    #[test]
    fn test_extract_tool_detail_read_without_offset() {
        let input = serde_json::json!({"file_path": "/src/main.rs"});
        assert_eq!(
            extract_tool_detail("Read", &input),
            Some("/src/main.rs".to_string())
        );
    }

    #[test]
    fn test_extract_tool_detail_edit() {
        let input = serde_json::json!({"file_path": "/src/lib.rs", "old_string": "foo", "new_string": "bar"});
        assert_eq!(
            extract_tool_detail("Edit", &input),
            Some("/src/lib.rs".to_string())
        );
    }

    #[test]
    fn test_extract_tool_detail_write() {
        let input = serde_json::json!({"file_path": "/src/new.rs", "content": "fn main() {}"});
        assert_eq!(
            extract_tool_detail("Write", &input),
            Some("/src/new.rs".to_string())
        );
    }

    #[test]
    fn test_extract_tool_detail_notebook_edit() {
        let input =
            serde_json::json!({"notebook_path": "/notebooks/test.ipynb", "new_source": "x = 1"});
        assert_eq!(
            extract_tool_detail("NotebookEdit", &input),
            Some("/notebooks/test.ipynb".to_string())
        );
    }

    #[test]
    fn test_extract_tool_detail_bash() {
        let input = serde_json::json!({"command": "ls -la"});
        assert_eq!(
            extract_tool_detail("Bash", &input),
            Some("ls -la".to_string())
        );
    }

    #[test]
    fn test_extract_tool_detail_unknown_tool() {
        let input = serde_json::json!({"foo": "bar"});
        assert_eq!(extract_tool_detail("UnknownTool", &input), None);
    }
}
