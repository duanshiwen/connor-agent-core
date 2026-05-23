//! Conversation events — the append-only journal entries.
//!
//! Every state change in a conversation is represented as a `ConversationEvent`.
//! Events are wrapped in `ConversationEventEnvelope` with metadata (ID, timestamp, actor).

use crate::ids::{ConversationId, EventId, MessageId, ParticipantId};
use crate::message::{Message, MessageContent};
use crate::participant::Participant;
use crate::session::ConversationSession;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Metadata wrapping a conversation event.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ConversationEventEnvelope {
    pub event_id: EventId,
    pub conversation_id: ConversationId,
    pub occurred_at: DateTime<Utc>,
    pub actor_id: Option<ParticipantId>,
    pub event: ConversationEvent,
}

/// The trigger that caused an assistant suggestion.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SuggestionTrigger {
    /// The user explicitly mentioned the assistant.
    Mention,
    /// The assistant detected a complex concept.
    ComplexConcept,
    /// The assistant detected a reference to an older conversation.
    HistoricalReference,
    /// The assistant proactively suggested based on context.
    Proactive,
}

/// All possible conversation events.
///
/// This is the core vocabulary of the Conversation Kernel.
/// New event types can be added without breaking existing journals
/// because unknown tags are handled gracefully during deserialization.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ConversationEvent {
    /// A new conversation was created.
    ConversationCreated { session: ConversationSession },

    /// A participant was added to the conversation.
    ParticipantAdded { participant: Participant },

    /// A participant was removed from the conversation.
    ParticipantRemoved { participant_id: ParticipantId },

    /// A message was appended to the conversation.
    MessageAppended { message: Message },

    /// A message's content was edited.
    MessageEdited {
        message_id: MessageId,
        new_content: MessageContent,
    },

    /// A message was tombstoned (soft-deleted).
    MessageTombstoned {
        message_id: MessageId,
        reason: String,
    },

    /// A new thread was started.
    ThreadStarted {
        thread_id: crate::ids::ThreadId,
        root_message_id: MessageId,
    },

    /// The assistant created a private suggestion for a user.
    AssistantSuggestionCreated {
        suggestion_message: Message,
        trigger: SuggestionTrigger,
    },

    /// A local triage result was attached to a message.
    LocalTriageAttached {
        message_id: MessageId,
        triage_ref: String,
    },

    /// A conversation slice was built for context construction.
    ContextSliceBuilt {
        slice_id: String,
        trigger_message_id: MessageId,
        message_ids: Vec<MessageId>,
    },

    /// An agent run was requested (does NOT directly invoke a model).
    AgentRunRequested {
        run_id: String,
        trigger_message_id: MessageId,
        context_slice_id: String,
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ids::ThreadId;
    use crate::message::SuggestedAction;
    use crate::session::{ConversationKind, ConversationStatus};
    use crate::visibility::Visibility;

    fn now() -> DateTime<Utc> {
        Utc::now()
    }

    fn make_conversation_created() -> ConversationEvent {
        ConversationEvent::ConversationCreated {
            session: ConversationSession {
                id: ConversationId::from("conv-001"),
                kind: ConversationKind::Direct,
                title: None,
                participants: vec![ParticipantId::from("u1"), ParticipantId::from("a1")],
                created_at: now(),
                updated_at: now(),
                status: ConversationStatus::Active,
            },
        }
    }

    fn make_participant_added() -> ConversationEvent {
        ConversationEvent::ParticipantAdded {
            participant: Participant {
                id: ParticipantId::from("u1"),
                kind: crate::participant::ParticipantKind::Human,
                display_name: "诗闻".to_string(),
            },
        }
    }

    fn make_message_appended() -> ConversationEvent {
        ConversationEvent::MessageAppended {
            message: Message {
                id: MessageId::from("msg-001"),
                conversation_id: ConversationId::from("conv-001"),
                sender_id: ParticipantId::from("u1"),
                content: MessageContent::Text {
                    text: "Hello".to_string(),
                },
                reply_to: None,
                thread_id: None,
                visibility: Visibility::Conversation,
                created_at: now(),
                edited_at: None,
            },
        }
    }

    fn make_thread_started() -> ConversationEvent {
        ConversationEvent::ThreadStarted {
            thread_id: ThreadId::from("thread-001"),
            root_message_id: MessageId::from("msg-001"),
        }
    }

    fn make_assistant_suggestion() -> ConversationEvent {
        ConversationEvent::AssistantSuggestionCreated {
            suggestion_message: Message {
                id: MessageId::from("msg-sug-001"),
                conversation_id: ConversationId::from("conv-001"),
                sender_id: ParticipantId::from("assistant-001"),
                content: MessageContent::AgentSuggestion {
                    text: "This might be sarcasm".to_string(),
                    actions: vec![SuggestedAction {
                        id: "ack".into(),
                        label: "Got it".into(),
                        action_type: "dismiss".into(),
                    }],
                },
                reply_to: None,
                thread_id: None,
                visibility: Visibility::PrivateToUser {
                    user_id: ParticipantId::from("u1"),
                },
                created_at: now(),
                edited_at: None,
            },
            trigger: SuggestionTrigger::Mention,
        }
    }

    fn make_context_slice_built() -> ConversationEvent {
        ConversationEvent::ContextSliceBuilt {
            slice_id: "slice-001".to_string(),
            trigger_message_id: MessageId::from("msg-003"),
            message_ids: vec![
                MessageId::from("msg-001"),
                MessageId::from("msg-002"),
                MessageId::from("msg-003"),
            ],
        }
    }

    fn make_agent_run_requested() -> ConversationEvent {
        ConversationEvent::AgentRunRequested {
            run_id: "run-001".to_string(),
            trigger_message_id: MessageId::from("msg-003"),
            context_slice_id: "slice-001".to_string(),
        }
    }

    fn wrap_event(event: ConversationEvent) -> ConversationEventEnvelope {
        ConversationEventEnvelope {
            event_id: EventId::from(format!("evt-{}", uuid::Uuid::new_v4())),
            conversation_id: ConversationId::from("conv-001"),
            occurred_at: now(),
            actor_id: Some(ParticipantId::from("u1")),
            event,
        }
    }

    // --- Serde roundtrip tests for each event variant ---

    #[test]
    fn conversation_created_roundtrip() {
        let envelope = wrap_event(make_conversation_created());
        let json = serde_json::to_string_pretty(&envelope).unwrap();
        let decoded: ConversationEventEnvelope = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded.event, envelope.event);
    }

    #[test]
    fn participant_added_roundtrip() {
        let envelope = wrap_event(make_participant_added());
        let json = serde_json::to_string_pretty(&envelope).unwrap();
        let decoded: ConversationEventEnvelope = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded.event, envelope.event);
    }

    #[test]
    fn participant_removed_roundtrip() {
        let envelope = wrap_event(ConversationEvent::ParticipantRemoved {
            participant_id: ParticipantId::from("u1"),
        });
        let json = serde_json::to_string(&envelope).unwrap();
        let decoded: ConversationEventEnvelope = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded.event, envelope.event);
    }

    #[test]
    fn message_appended_roundtrip() {
        let envelope = wrap_event(make_message_appended());
        let json = serde_json::to_string_pretty(&envelope).unwrap();
        let decoded: ConversationEventEnvelope = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded.event, envelope.event);
    }

    #[test]
    fn message_edited_roundtrip() {
        let envelope = wrap_event(ConversationEvent::MessageEdited {
            message_id: MessageId::from("msg-001"),
            new_content: MessageContent::Text {
                text: "Edited text".to_string(),
            },
        });
        let json = serde_json::to_string(&envelope).unwrap();
        let decoded: ConversationEventEnvelope = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded.event, envelope.event);
    }

    #[test]
    fn message_tombstoned_roundtrip() {
        let envelope = wrap_event(ConversationEvent::MessageTombstoned {
            message_id: MessageId::from("msg-001"),
            reason: "spam".to_string(),
        });
        let json = serde_json::to_string(&envelope).unwrap();
        let decoded: ConversationEventEnvelope = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded.event, envelope.event);
    }

    #[test]
    fn thread_started_roundtrip() {
        let envelope = wrap_event(make_thread_started());
        let json = serde_json::to_string(&envelope).unwrap();
        let decoded: ConversationEventEnvelope = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded.event, envelope.event);
    }

    #[test]
    fn assistant_suggestion_roundtrip() {
        let envelope = wrap_event(make_assistant_suggestion());
        let json = serde_json::to_string_pretty(&envelope).unwrap();
        let decoded: ConversationEventEnvelope = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded.event, envelope.event);
    }

    #[test]
    fn local_triage_attached_roundtrip() {
        let envelope = wrap_event(ConversationEvent::LocalTriageAttached {
            message_id: MessageId::from("msg-001"),
            triage_ref: "triage-001".to_string(),
        });
        let json = serde_json::to_string(&envelope).unwrap();
        let decoded: ConversationEventEnvelope = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded.event, envelope.event);
    }

    #[test]
    fn context_slice_built_roundtrip() {
        let envelope = wrap_event(make_context_slice_built());
        let json = serde_json::to_string(&envelope).unwrap();
        let decoded: ConversationEventEnvelope = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded.event, envelope.event);
    }

    #[test]
    fn agent_run_requested_roundtrip() {
        let envelope = wrap_event(make_agent_run_requested());
        let json = serde_json::to_string(&envelope).unwrap();
        let decoded: ConversationEventEnvelope = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded.event, envelope.event);
    }

    // --- Event type tag tests ---

    #[test]
    fn event_type_tag_is_snake_case() {
        let json = serde_json::to_string(&make_message_appended()).unwrap();
        assert!(json.contains("\"type\":\"message_appended\""));
    }

    #[test]
    fn conversation_created_has_type_tag() {
        let json = serde_json::to_string(&make_conversation_created()).unwrap();
        assert!(json.contains("\"type\":\"conversation_created\""));
    }

    // --- Integration: full event sequence ---

    #[test]
    fn full_conversation_event_sequence_roundtrip() {
        let events = vec![
            wrap_event(make_conversation_created()),
            wrap_event(make_participant_added()),
            wrap_event(make_message_appended()),
            wrap_event(make_thread_started()),
            wrap_event(make_assistant_suggestion()),
            wrap_event(make_context_slice_built()),
            wrap_event(make_agent_run_requested()),
        ];

        // Serialize all events to JSONL
        let jsonl: String = events
            .iter()
            .map(|e| serde_json::to_string(e).unwrap())
            .collect::<Vec<_>>()
            .join("\n");

        // Deserialize back
        let decoded: Vec<ConversationEventEnvelope> = jsonl
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();

        assert_eq!(events.len(), decoded.len());
        for (original, restored) in events.iter().zip(decoded.iter()) {
            assert_eq!(original.event, restored.event);
            assert_eq!(original.conversation_id, restored.conversation_id);
        }
    }
}
