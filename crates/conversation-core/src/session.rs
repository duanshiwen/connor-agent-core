//! Conversation session types.

use crate::ids::{ConversationId, ParticipantId};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// The kind of conversation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConversationKind {
    /// A one-on-one conversation between two participants.
    Direct,
    /// A group conversation with multiple participants.
    Group,
    /// A conversation between a user and an AI agent for a specific task.
    AgentTask,
    /// A mixed conversation that may involve humans, agents, and integrations.
    Mixed,
}

/// The lifecycle status of a conversation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConversationStatus {
    /// The conversation is active and accepting new messages.
    Active,
    /// The conversation has been archived.
    Archived,
}

impl Default for ConversationStatus {
    fn default() -> Self {
        Self::Active
    }
}

/// A conversation session — a long-lived container for messages and participants.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ConversationSession {
    pub id: ConversationId,
    pub kind: ConversationKind,
    pub title: Option<String>,
    pub participants: Vec<ParticipantId>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub status: ConversationStatus,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_session() -> ConversationSession {
        let now = chrono::Utc::now();
        ConversationSession {
            id: ConversationId::from("conv-001"),
            kind: ConversationKind::Direct,
            title: None,
            participants: vec![
                ParticipantId::from("user-001"),
                ParticipantId::from("assistant-001"),
            ],
            created_at: now,
            updated_at: now,
            status: ConversationStatus::Active,
        }
    }

    #[test]
    fn conversation_kind_serde_roundtrip() {
        let kinds = vec![
            ConversationKind::Direct,
            ConversationKind::Group,
            ConversationKind::AgentTask,
            ConversationKind::Mixed,
        ];
        for kind in kinds {
            let json = serde_json::to_string(&kind).unwrap();
            let decoded: ConversationKind = serde_json::from_str(&json).unwrap();
            assert_eq!(kind, decoded);
        }
    }

    #[test]
    fn conversation_kind_serializes_as_snake_case() {
        assert_eq!(
            serde_json::to_string(&ConversationKind::AgentTask).unwrap(),
            "\"agent_task\""
        );
    }

    #[test]
    fn conversation_status_serde_roundtrip() {
        let json = serde_json::to_string(&ConversationStatus::Archived).unwrap();
        assert_eq!(json, "\"archived\"");
        let decoded: ConversationStatus = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded, ConversationStatus::Archived);
    }

    #[test]
    fn default_status_is_active() {
        assert_eq!(ConversationStatus::default(), ConversationStatus::Active);
    }

    #[test]
    fn session_serde_roundtrip() {
        let session = sample_session();
        let json = serde_json::to_string_pretty(&session).unwrap();
        let decoded: ConversationSession = serde_json::from_str(&json).unwrap();
        assert_eq!(session, decoded);
    }

    #[test]
    fn session_with_title() {
        let mut session = sample_session();
        session.title = Some("Design Discussion".to_string());
        let json = serde_json::to_string(&session).unwrap();
        let decoded: ConversationSession = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded.title, Some("Design Discussion".to_string()));
    }

    #[test]
    fn group_session() {
        let mut session = sample_session();
        session.kind = ConversationKind::Group;
        session.participants = vec![
            ParticipantId::from("u1"),
            ParticipantId::from("u2"),
            ParticipantId::from("assistant-1"),
        ];
        let json = serde_json::to_string_pretty(&session).unwrap();
        let decoded: ConversationSession = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded.kind, ConversationKind::Group);
        assert_eq!(decoded.participants.len(), 3);
    }
}
