//! Conversation slices — a window into the message timeline.

use crate::ids::{ConversationId, MessageId};
use crate::message::Message;
use serde::{Deserialize, Serialize};

/// Why a slice was built.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SliceBuildReason {
    /// A simple window of the most recent messages.
    RecentWindow,
    /// Messages within the current thread.
    CurrentThread,
    /// Messages around the trigger message (before + after).
    AroundTrigger,
    /// The user accepted an assistant suggestion.
    AssistantSuggestionAccepted,
}

/// A conversation slice — a bounded subset of messages for context construction.
///
/// The Conversation Kernel produces slices; it does not build full `ContextPacket`s.
/// Slices are consumed by the Context Kernel which enriches them with browser context,
/// memory, knowledge, and permission information.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ConversationSlice {
    /// Unique identifier for this slice.
    pub id: String,
    /// The conversation this slice belongs to.
    pub conversation_id: ConversationId,
    /// The message that triggered this slice construction.
    pub trigger_message_id: MessageId,
    /// The messages included in this slice.
    pub messages: Vec<Message>,
    /// Why this slice was built.
    pub reason: SliceBuildReason,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ids::ParticipantId;
    use crate::message::MessageContent;
    use crate::visibility::Visibility;

    fn sample_slice() -> ConversationSlice {
        let now = chrono::Utc::now();
        ConversationSlice {
            id: "slice-001".to_string(),
            conversation_id: ConversationId::from("conv-001"),
            trigger_message_id: MessageId::from("msg-003"),
            messages: vec![
                Message {
                    id: MessageId::from("msg-001"),
                    conversation_id: ConversationId::from("conv-001"),
                    sender_id: ParticipantId::from("user-001"),
                    content: MessageContent::Text {
                        text: "hello".into(),
                    },
                    reply_to: None,
                    thread_id: None,
                    visibility: Visibility::Conversation,
                    created_at: now,
                    edited_at: None,
                },
                Message {
                    id: MessageId::from("msg-002"),
                    conversation_id: ConversationId::from("conv-001"),
                    sender_id: ParticipantId::from("user-002"),
                    content: MessageContent::Text {
                        text: "world".into(),
                    },
                    reply_to: None,
                    thread_id: None,
                    visibility: Visibility::Conversation,
                    created_at: now,
                    edited_at: None,
                },
                Message {
                    id: MessageId::from("msg-003"),
                    conversation_id: ConversationId::from("conv-001"),
                    sender_id: ParticipantId::from("user-001"),
                    content: MessageContent::Text {
                        text: "trigger".into(),
                    },
                    reply_to: None,
                    thread_id: None,
                    visibility: Visibility::Conversation,
                    created_at: now,
                    edited_at: None,
                },
            ],
            reason: SliceBuildReason::RecentWindow,
        }
    }

    #[test]
    fn slice_build_reason_serde_roundtrip() {
        let reasons = vec![
            SliceBuildReason::RecentWindow,
            SliceBuildReason::CurrentThread,
            SliceBuildReason::AroundTrigger,
            SliceBuildReason::AssistantSuggestionAccepted,
        ];
        for reason in reasons {
            let json = serde_json::to_string(&reason).unwrap();
            let decoded: SliceBuildReason = serde_json::from_str(&json).unwrap();
            assert_eq!(reason, decoded);
        }
    }

    #[test]
    fn slice_serde_roundtrip() {
        let slice = sample_slice();
        let json = serde_json::to_string_pretty(&slice).unwrap();
        let decoded: ConversationSlice = serde_json::from_str(&json).unwrap();
        assert_eq!(slice, decoded);
    }

    #[test]
    fn slice_preserves_message_order() {
        let slice = sample_slice();
        assert_eq!(slice.messages.len(), 3);
        assert_eq!(slice.messages[0].id.0, "msg-001");
        assert_eq!(slice.messages[2].id.0, "msg-003");
    }
}
