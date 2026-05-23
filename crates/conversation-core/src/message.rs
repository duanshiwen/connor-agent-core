//! Conversation messages.

use crate::ids::{ConversationId, MessageId, ParticipantId, ThreadId};
use crate::visibility::Visibility;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// The content of a message, discriminated by kind.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum MessageContent {
    /// A plain text message from a human or agent.
    Text { text: String },

    /// A system-generated notice (e.g. "User joined the conversation").
    SystemNotice { text: String },

    /// A suggestion from the assistant, visible privately to the target user.
    AgentSuggestion {
        text: String,
        actions: Vec<SuggestedAction>,
    },
}

/// A suggested action the user can take in response to an agent suggestion.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SuggestedAction {
    pub id: String,
    pub label: String,
    pub action_type: String,
}

/// A message in a conversation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Message {
    pub id: MessageId,
    pub conversation_id: ConversationId,
    pub sender_id: ParticipantId,
    pub content: MessageContent,
    pub reply_to: Option<MessageId>,
    pub thread_id: Option<ThreadId>,
    pub visibility: Visibility,
    pub created_at: DateTime<Utc>,
    pub edited_at: Option<DateTime<Utc>>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_text_message() -> Message {
        Message {
            id: MessageId::from("msg-001"),
            conversation_id: ConversationId::from("conv-001"),
            sender_id: ParticipantId::from("user-001"),
            content: MessageContent::Text {
                text: "Hello, world!".to_string(),
            },
            reply_to: None,
            thread_id: None,
            visibility: Visibility::Conversation,
            created_at: chrono::Utc::now(),
            edited_at: None,
        }
    }

    fn sample_suggestion_message() -> Message {
        Message {
            id: MessageId::from("msg-002"),
            conversation_id: ConversationId::from("conv-001"),
            sender_id: ParticipantId::from("assistant-001"),
            content: MessageContent::AgentSuggestion {
                text: "这句话可能有讽刺意味".to_string(),
                actions: vec![SuggestedAction {
                    id: "ack".to_string(),
                    label: "知道了".to_string(),
                    action_type: "dismiss".to_string(),
                }],
            },
            reply_to: None,
            thread_id: None,
            visibility: Visibility::PrivateToUser {
                user_id: ParticipantId::from("user-001"),
            },
            created_at: chrono::Utc::now(),
            edited_at: None,
        }
    }

    #[test]
    fn text_message_serde_roundtrip() {
        let msg = sample_text_message();
        let json = serde_json::to_string_pretty(&msg).unwrap();
        let decoded: Message = serde_json::from_str(&json).unwrap();
        assert_eq!(msg, decoded);
    }

    #[test]
    fn system_notice_serde_roundtrip() {
        let msg = Message {
            id: MessageId::from("msg-sys"),
            conversation_id: ConversationId::from("conv-001"),
            sender_id: ParticipantId::from("system"),
            content: MessageContent::SystemNotice {
                text: "User joined".to_string(),
            },
            reply_to: None,
            thread_id: None,
            visibility: Visibility::Conversation,
            created_at: chrono::Utc::now(),
            edited_at: None,
        };
        let json = serde_json::to_string(&msg).unwrap();
        let decoded: Message = serde_json::from_str(&json).unwrap();
        assert_eq!(msg, decoded);
    }

    #[test]
    fn agent_suggestion_serde_roundtrip() {
        let msg = sample_suggestion_message();
        let json = serde_json::to_string_pretty(&msg).unwrap();
        let decoded: Message = serde_json::from_str(&json).unwrap();
        assert_eq!(msg, decoded);
    }

    #[test]
    fn message_content_serializes_with_kind_tag() {
        let content = MessageContent::Text {
            text: "hi".to_string(),
        };
        let json = serde_json::to_string(&content).unwrap();
        assert!(json.contains("\"kind\":\"text\""));
    }

    #[test]
    fn message_with_reply_to() {
        let msg = Message {
            id: MessageId::from("msg-003"),
            conversation_id: ConversationId::from("conv-001"),
            sender_id: ParticipantId::from("user-002"),
            content: MessageContent::Text {
                text: "reply!".to_string(),
            },
            reply_to: Some(MessageId::from("msg-001")),
            thread_id: Some(ThreadId::from("thread-001")),
            visibility: Visibility::Conversation,
            created_at: chrono::Utc::now(),
            edited_at: None,
        };
        let json = serde_json::to_string_pretty(&msg).unwrap();
        let decoded: Message = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded.reply_to, Some(MessageId::from("msg-001")));
        assert_eq!(decoded.thread_id, Some(ThreadId::from("thread-001")));
    }

    #[test]
    fn message_with_edit_timestamp() {
        let mut msg = sample_text_message();
        msg.edited_at = Some(chrono::Utc::now());
        let json = serde_json::to_string(&msg).unwrap();
        let decoded: Message = serde_json::from_str(&json).unwrap();
        assert!(decoded.edited_at.is_some());
    }
}
