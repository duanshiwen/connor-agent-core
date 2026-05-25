//! Conversation action lifecycle state.
//!
//! These types describe how action requests are represented in the conversation
//! event stream and projected into read state. Execution, policy evaluation, and
//! audit writing live outside `conversation-core`.

use crate::ParticipantId;
use action_core::{ActionId, ActionKind, ActionRequest, ActionResult};
use serde::{Deserialize, Serialize};

/// Conversation-level lifecycle status for an action invocation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConversationActionStatus {
    /// Action was requested and is awaiting policy/execution handling.
    Requested,
    /// Action requires approval before it can start.
    ApprovalRequired,
    /// Action was approved by a participant.
    Approved,
    /// Action was denied and must not execute.
    Denied,
    /// Action execution started.
    Started,
    /// Action execution completed successfully.
    Completed,
    /// Action execution failed.
    Failed,
}

impl ConversationActionStatus {
    /// Returns true if this status is terminal for PR 15A lifecycle purposes.
    pub fn is_terminal(&self) -> bool {
        matches!(
            self,
            ConversationActionStatus::Denied
                | ConversationActionStatus::Completed
                | ConversationActionStatus::Failed
        )
    }
}

/// Projected state for a single action invocation in a conversation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ConversationActionState {
    pub action_id: ActionId,
    pub action_kind: ActionKind,
    pub status: ConversationActionStatus,
    pub request: Option<ActionRequest>,
    pub approved_by: Option<ParticipantId>,
    pub denial_reason: Option<String>,
    pub approval_required_reason: Option<String>,
    pub result: Option<ActionResult>,
    pub error_message: Option<String>,
}

impl ConversationActionState {
    /// Build initial projected state from an action request.
    pub fn requested(request: ActionRequest) -> Self {
        Self {
            action_id: request.action_id.clone(),
            action_kind: request.action_kind.clone(),
            status: ConversationActionStatus::Requested,
            request: Some(request),
            approved_by: None,
            denial_reason: None,
            approval_required_reason: None,
            result: None,
            error_message: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use action_core::{ActionResultPayload, ActionStatus};
    use chrono::Utc;

    fn sample_request() -> ActionRequest {
        ActionRequest {
            action_id: ActionId::from("action-001"),
            action_kind: ActionKind::from("knowledge.search"),
            input: serde_json::json!({"query": "agent os"}),
            requested_by: "user-1".to_string(),
            conversation_id: Some("conv-1".to_string()),
            message_id: Some("msg-1".to_string()),
            requested_at: Utc::now(),
        }
    }

    #[test]
    fn conversation_action_status_serde_roundtrip() {
        let statuses = vec![
            ConversationActionStatus::Requested,
            ConversationActionStatus::ApprovalRequired,
            ConversationActionStatus::Approved,
            ConversationActionStatus::Denied,
            ConversationActionStatus::Started,
            ConversationActionStatus::Completed,
            ConversationActionStatus::Failed,
        ];

        for status in statuses {
            let json = serde_json::to_string(&status).unwrap();
            let decoded: ConversationActionStatus = serde_json::from_str(&json).unwrap();
            assert_eq!(decoded, status);
        }
    }

    #[test]
    fn conversation_action_terminal_helper() {
        assert!(!ConversationActionStatus::Requested.is_terminal());
        assert!(!ConversationActionStatus::ApprovalRequired.is_terminal());
        assert!(!ConversationActionStatus::Approved.is_terminal());
        assert!(!ConversationActionStatus::Started.is_terminal());
        assert!(ConversationActionStatus::Denied.is_terminal());
        assert!(ConversationActionStatus::Completed.is_terminal());
        assert!(ConversationActionStatus::Failed.is_terminal());
    }

    #[test]
    fn conversation_action_state_serde_roundtrip() {
        let request = sample_request();
        let mut state = ConversationActionState::requested(request);
        state.status = ConversationActionStatus::Completed;
        state.result = Some(ActionResult {
            status: ActionStatus::Completed,
            payload: ActionResultPayload::Text("done".to_string()),
            summary: "Completed search".to_string(),
            completed_at: Utc::now(),
        });

        let json = serde_json::to_string_pretty(&state).unwrap();
        let decoded: ConversationActionState = serde_json::from_str(&json).unwrap();

        assert_eq!(decoded.action_id, state.action_id);
        assert_eq!(decoded.action_kind, state.action_kind);
        assert_eq!(decoded.status, state.status);
        assert!(decoded.request.is_some());
        assert!(decoded.result.is_some());
    }
}
