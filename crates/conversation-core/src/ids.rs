//! Domain identifiers.
//!
//! Each ID is a newtype wrapper around `String` to prevent accidental mixing
//! of unrelated identifiers at the type level.

use serde::{Deserialize, Serialize};
use std::fmt;

macro_rules! define_id {
    ($(#[$meta:meta])* $name:ident) => {
        $(#[$meta])*
        #[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
        pub struct $name(pub String);

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(&self.0)
            }
        }

        impl From<String> for $name {
            fn from(s: String) -> Self {
                Self(s)
            }
        }

        impl From<&str> for $name {
            fn from(s: &str) -> Self {
                Self(s.to_string())
            }
        }
    };
}

define_id!(
    /// Unique identifier for a conversation session.
    ConversationId
);

define_id!(
    /// Unique identifier for an event in the journal.
    EventId
);

define_id!(
    /// Unique identifier for a message within a conversation.
    MessageId
);

define_id!(
    /// Unique identifier for a conversation participant.
    ParticipantId
);

define_id!(
    /// Unique identifier for a message thread.
    ThreadId
);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn id_display_roundtrip() {
        let id = ConversationId::from("conv-001");
        assert_eq!(id.to_string(), "conv-001");
    }

    #[test]
    fn id_from_str_and_string() {
        let a = MessageId::from("msg-1");
        let b = MessageId::from("msg-1".to_string());
        assert_eq!(a, b);
    }

    #[test]
    fn id_serde_roundtrip() {
        let id = EventId::from("evt-abc");
        let json = serde_json::to_string(&id).unwrap();
        let decoded: EventId = serde_json::from_str(&json).unwrap();
        assert_eq!(id, decoded);
    }

    #[test]
    fn id_equality_is_value_based() {
        let a = ParticipantId::from("user-1");
        let b = ParticipantId::from("user-1");
        assert_eq!(a, b);
    }

    #[test]
    fn different_ids_are_not_equal() {
        let a = ConversationId::from("conv-1");
        let b = ConversationId::from("conv-2");
        assert_ne!(a, b);
    }
}
