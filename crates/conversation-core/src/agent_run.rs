//! Agent run lifecycle state.

use crate::ids::MessageId;
use serde::{Deserialize, Serialize};

/// Lifecycle status of an agent run requested from a conversation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentRunStatus {
    Requested,
    Started,
    Completed,
    Failed,
    Cancelled,
    TimedOut,
}

/// Projected state for a single agent run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentRunState {
    pub run_id: String,
    pub trigger_message_id: MessageId,
    pub context_slice_id: String,
    pub status: AgentRunStatus,
    pub output_message_id: Option<MessageId>,
    pub error_code: Option<String>,
    pub error_message: Option<String>,
    pub cancel_reason: Option<String>,
}

impl AgentRunState {
    pub fn requested(
        run_id: String,
        trigger_message_id: MessageId,
        context_slice_id: String,
    ) -> Self {
        Self {
            run_id,
            trigger_message_id,
            context_slice_id,
            status: AgentRunStatus::Requested,
            output_message_id: None,
            error_code: None,
            error_message: None,
            cancel_reason: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn agent_run_status_serde_roundtrip() {
        let statuses = vec![
            AgentRunStatus::Requested,
            AgentRunStatus::Started,
            AgentRunStatus::Completed,
            AgentRunStatus::Failed,
            AgentRunStatus::Cancelled,
            AgentRunStatus::TimedOut,
        ];

        for status in statuses {
            let json = serde_json::to_string(&status).unwrap();
            let decoded: AgentRunStatus = serde_json::from_str(&json).unwrap();
            assert_eq!(decoded, status);
        }
    }

    #[test]
    fn agent_run_state_serde_roundtrip() {
        let state = AgentRunState::requested(
            "run-1".to_string(),
            MessageId::from("msg-1"),
            "slice-1".to_string(),
        );

        let json = serde_json::to_string_pretty(&state).unwrap();
        let decoded: AgentRunState = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded, state);
    }
}
