use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentStatus {
    Started,
    Running,
    Waiting,
    Permission,
    Ended,
}

impl std::fmt::Display for AgentStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AgentStatus::Started => write!(f, "started"),
            AgentStatus::Running => write!(f, "running"),
            AgentStatus::Waiting => write!(f, "waiting"),
            AgentStatus::Permission => write!(f, "permission"),
            AgentStatus::Ended => write!(f, "ended"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentState {
    pub tmux_pane: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    pub state: AgentStatus,
    pub updated_at: u64,
}

impl AgentState {
    pub fn new(tmux_pane: String, state: AgentStatus) -> Self {
        Self {
            tmux_pane,
            session_id: None,
            state,
            updated_at: now(),
        }
    }
}

fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_agent_status_serialize() {
        assert_eq!(
            serde_json::to_string(&AgentStatus::Running).unwrap(),
            "\"running\""
        );
        assert_eq!(
            serde_json::to_string(&AgentStatus::Waiting).unwrap(),
            "\"waiting\""
        );
        assert_eq!(
            serde_json::to_string(&AgentStatus::Permission).unwrap(),
            "\"permission\""
        );
    }

    #[test]
    fn test_agent_status_deserialize() {
        assert_eq!(
            serde_json::from_str::<AgentStatus>("\"running\"").unwrap(),
            AgentStatus::Running
        );
        assert_eq!(
            serde_json::from_str::<AgentStatus>("\"started\"").unwrap(),
            AgentStatus::Started
        );
    }

    #[test]
    fn test_agent_status_display() {
        assert_eq!(AgentStatus::Running.to_string(), "running");
        assert_eq!(AgentStatus::Waiting.to_string(), "waiting");
        assert_eq!(AgentStatus::Permission.to_string(), "permission");
        assert_eq!(AgentStatus::Started.to_string(), "started");
        assert_eq!(AgentStatus::Ended.to_string(), "ended");
    }

    #[test]
    fn test_agent_state_new() {
        let state = AgentState::new("%5".to_string(), AgentStatus::Running);
        assert_eq!(state.tmux_pane, "%5");
        assert_eq!(state.state, AgentStatus::Running);
        assert!(state.session_id.is_none());
        assert!(state.updated_at > 0);
    }

    #[test]
    fn test_agent_state_roundtrip_json() {
        let state = AgentState {
            tmux_pane: "%5".to_string(),
            session_id: Some("abc123".to_string()),
            state: AgentStatus::Running,
            updated_at: 1234567890,
        };

        let json = serde_json::to_string(&state).unwrap();
        let parsed: AgentState = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed.tmux_pane, "%5");
        assert_eq!(parsed.session_id, Some("abc123".to_string()));
        assert_eq!(parsed.state, AgentStatus::Running);
        assert_eq!(parsed.updated_at, 1234567890);
    }

    #[test]
    fn test_agent_state_optional_fields_omitted() {
        let state = AgentState::new("%0".to_string(), AgentStatus::Waiting);
        let json = serde_json::to_string(&state).unwrap();

        // Optional None fields should be omitted
        assert!(!json.contains("session_id"));
    }

    #[test]
    fn test_agent_state_deserialize_minimal() {
        let json = r#"{"tmux_pane":"%0","state":"waiting","updated_at":100}"#;
        let state: AgentState = serde_json::from_str(json).unwrap();
        assert_eq!(state.tmux_pane, "%0");
        assert_eq!(state.state, AgentStatus::Waiting);
        assert!(state.session_id.is_none());
    }
}
