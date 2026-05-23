//! Message visibility rules.
//!
//! Visibility controls who can see a message in a conversation.
//! This is critical for private assistant suggestions and agent-only context.

use crate::ids::ParticipantId;
use serde::{Deserialize, Serialize};

/// Controls who can see a message.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Visibility {
    /// Visible to all participants in the conversation.
    Conversation,

    /// Visible only to a specific user (e.g. assistant suggestion for one user).
    PrivateToUser { user_id: ParticipantId },

    /// Visible only to AI agents, not to human participants.
    AgentOnly,

    /// Visible only to a specific set of participants.
    Participants { participant_ids: Vec<ParticipantId> },
}

impl Default for Visibility {
    fn default() -> Self {
        Self::Conversation
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn conversation_visibility_serde_roundtrip() {
        let vis = Visibility::Conversation;
        let json = serde_json::to_string(&vis).unwrap();
        let decoded: Visibility = serde_json::from_str(&json).unwrap();
        assert_eq!(vis, decoded);
    }

    #[test]
    fn private_to_user_serde_roundtrip() {
        let vis = Visibility::PrivateToUser {
            user_id: ParticipantId::from("user-001"),
        };
        let json = serde_json::to_string_pretty(&vis).unwrap();
        let decoded: Visibility = serde_json::from_str(&json).unwrap();
        assert_eq!(vis, decoded);
    }

    #[test]
    fn agent_only_serde_roundtrip() {
        let vis = Visibility::AgentOnly;
        let json = serde_json::to_string(&vis).unwrap();
        let decoded: Visibility = serde_json::from_str(&json).unwrap();
        assert_eq!(vis, decoded);
    }

    #[test]
    fn participants_visibility_serde_roundtrip() {
        let vis = Visibility::Participants {
            participant_ids: vec![
                ParticipantId::from("user-001"),
                ParticipantId::from("user-002"),
            ],
        };
        let json = serde_json::to_string_pretty(&vis).unwrap();
        let decoded: Visibility = serde_json::from_str(&json).unwrap();
        assert_eq!(vis, decoded);
    }

    #[test]
    fn visibility_serializes_with_kind_tag() {
        let vis = Visibility::AgentOnly;
        let json = serde_json::to_string(&vis).unwrap();
        // Should contain the "kind" field from the tagged representation
        assert!(json.contains("agent_only"));
    }

    #[test]
    fn default_visibility_is_conversation() {
        let vis = Visibility::default();
        assert_eq!(vis, Visibility::Conversation);
    }
}
