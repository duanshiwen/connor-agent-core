//! Conversation participants.

use crate::ids::ParticipantId;
use serde::{Deserialize, Serialize};

/// The kind of participant in a conversation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ParticipantKind {
    /// A human user.
    Human,
    /// An AI agent (e.g. assistant, small helper).
    Agent,
    /// A system actor (automated notifications, bots).
    System,
    /// An external integration (webhook, API consumer).
    Integration,
}

/// A participant in a conversation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Participant {
    pub id: ParticipantId,
    pub kind: ParticipantKind,
    pub display_name: String,
}

impl ParticipantKind {
    /// Returns `true` for participants that can appear as foreground actors
    /// in a conversation (send messages, initiate actions).
    ///
    /// Only `Human` and `Agent` are foreground participants.
    /// `System` and `Integration` are background-only — they can emit
    /// system notices but cannot send regular messages.
    pub fn is_foreground(&self) -> bool {
        matches!(self, ParticipantKind::Human | ParticipantKind::Agent)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_human() -> Participant {
        Participant {
            id: ParticipantId::from("user-001"),
            kind: ParticipantKind::Human,
            display_name: "Test User".to_string(),
        }
    }

    fn sample_agent() -> Participant {
        Participant {
            id: ParticipantId::from("assistant-001"),
            kind: ParticipantKind::Agent,
            display_name: "小助理".to_string(),
        }
    }

    #[test]
    fn participant_kind_serde_roundtrip() {
        let kinds = vec![
            ParticipantKind::Human,
            ParticipantKind::Agent,
            ParticipantKind::System,
            ParticipantKind::Integration,
        ];

        for kind in kinds {
            let json = serde_json::to_string(&kind).unwrap();
            let decoded: ParticipantKind = serde_json::from_str(&json).unwrap();
            assert_eq!(kind, decoded);
        }
    }

    #[test]
    fn participant_kind_serializes_as_snake_case() {
        let json = serde_json::to_string(&ParticipantKind::Agent).unwrap();
        assert_eq!(json, "\"agent\"");
    }

    #[test]
    fn participant_serde_roundtrip() {
        let p = sample_human();
        let json = serde_json::to_string_pretty(&p).unwrap();
        let decoded: Participant = serde_json::from_str(&json).unwrap();
        assert_eq!(p, decoded);
    }

    #[test]
    fn participant_display_name_preserved() {
        let p = sample_agent();
        let json = serde_json::to_string(&p).unwrap();
        assert!(json.contains("小助理"));
    }

    #[test]
    fn foreground_participant_classification() {
        assert!(ParticipantKind::Human.is_foreground());
        assert!(ParticipantKind::Agent.is_foreground());
        assert!(!ParticipantKind::System.is_foreground());
        assert!(!ParticipantKind::Integration.is_foreground());
    }
}
