//! Event projector — replays events into `ConversationState`.

use crate::state::ConversationState;
use anyhow::Result;
use conversation_core::{ConversationEvent, ConversationEventEnvelope};

/// Replays a sequence of events into a `ConversationState`.
///
/// This is a pure function: same events → same state, every time.
pub struct ConversationProjector;

impl ConversationProjector {
    /// Project a slice of event envelopes into conversation state.
    pub fn project(events: &[ConversationEventEnvelope]) -> Result<ConversationState> {
        let mut state = ConversationState::default();

        for envelope in events {
            Self::apply_event(&mut state, &envelope.event)?;
        }

        Ok(state)
    }

    fn apply_event(state: &mut ConversationState, event: &ConversationEvent) -> Result<()> {
        match event {
            ConversationEvent::ConversationCreated { session } => {
                state.session = Some(session.clone());
            }

            ConversationEvent::ParticipantAdded { participant } => {
                state
                    .participants
                    .insert(participant.id.clone(), participant.clone());
            }

            ConversationEvent::ParticipantRemoved { participant_id } => {
                state.participants.remove(participant_id);
            }

            ConversationEvent::MessageAppended { message } => {
                if let Some(thread_id) = &message.thread_id {
                    state
                        .threads
                        .entry(thread_id.clone())
                        .or_default()
                        .push(message.id.clone());
                }
                state
                    .messages_by_id
                    .insert(message.id.clone(), message.clone());
                state.messages.push(message.clone());
            }

            ConversationEvent::MessageEdited {
                message_id,
                new_content,
            } => {
                if let Some(msg) = state.messages_by_id.get_mut(message_id) {
                    msg.content = new_content.clone();
                    msg.edited_at = Some(chrono::Utc::now());
                    // Also update in the ordered messages list.
                    if let Some(ordered) = state.messages.iter_mut().find(|m| m.id == *message_id) {
                        ordered.content = new_content.clone();
                        ordered.edited_at = Some(chrono::Utc::now());
                    }
                }
            }

            ConversationEvent::MessageTombstoned { message_id, .. } => {
                // Remove from ordered list but keep in by_id as a tombstone marker.
                state.messages.retain(|m| m.id != *message_id);
            }

            ConversationEvent::AssistantSuggestionCreated {
                suggestion_message, ..
            } => {
                // Suggestions are messages — they go into the timeline.
                if let Some(thread_id) = &suggestion_message.thread_id {
                    state
                        .threads
                        .entry(thread_id.clone())
                        .or_default()
                        .push(suggestion_message.id.clone());
                }
                state
                    .messages_by_id
                    .insert(suggestion_message.id.clone(), suggestion_message.clone());
                state.messages.push(suggestion_message.clone());
            }

            ConversationEvent::ThreadStarted {
                thread_id,
                root_message_id,
            } => {
                state
                    .threads
                    .entry(thread_id.clone())
                    .or_default()
                    .push(root_message_id.clone());
            }

            // Other events don't affect the projected state.
            _ => {}
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use conversation_core::*;

    fn now() -> chrono::DateTime<chrono::Utc> {
        chrono::Utc::now()
    }

    fn envelope(event: ConversationEvent) -> ConversationEventEnvelope {
        ConversationEventEnvelope {
            event_id: EventId::from(format!("evt-{}", uuid::Uuid::new_v4())),
            conversation_id: ConversationId::from("conv-1"),
            occurred_at: now(),
            actor_id: None,
            event,
        }
    }

    fn conversation_created_event() -> ConversationEvent {
        ConversationEvent::ConversationCreated {
            session: ConversationSession {
                id: ConversationId::from("conv-1"),
                kind: ConversationKind::Direct,
                title: None,
                participants: vec![],
                created_at: now(),
                updated_at: now(),
                status: ConversationStatus::Active,
            },
        }
    }

    fn participant_added_event(id: &str, name: &str) -> ConversationEvent {
        ConversationEvent::ParticipantAdded {
            participant: Participant {
                id: ParticipantId::from(id),
                kind: ParticipantKind::Human,
                display_name: name.to_string(),
            },
        }
    }

    fn message_appended_event(
        msg_id: &str,
        text: &str,
        reply_to: Option<&str>,
        thread_id: Option<&str>,
    ) -> ConversationEvent {
        ConversationEvent::MessageAppended {
            message: Message {
                id: MessageId::from(msg_id),
                conversation_id: ConversationId::from("conv-1"),
                sender_id: ParticipantId::from("user-1"),
                content: MessageContent::Text {
                    text: text.to_string(),
                },
                reply_to: reply_to.map(MessageId::from),
                thread_id: thread_id.map(ThreadId::from),
                visibility: Visibility::Conversation,
                created_at: now(),
                edited_at: None,
            },
        }
    }

    #[test]
    fn project_conversation_created() {
        let events = vec![envelope(conversation_created_event())];
        let state = ConversationProjector::project(&events).unwrap();

        assert!(state.session.is_some());
        let session = state.session.unwrap();
        assert_eq!(session.id, ConversationId::from("conv-1"));
        assert_eq!(session.kind, ConversationKind::Direct);
        assert_eq!(session.status, ConversationStatus::Active);
    }

    #[test]
    fn project_empty_events() {
        let events: Vec<ConversationEventEnvelope> = vec![];
        let state = ConversationProjector::project(&events).unwrap();
        assert!(state.session.is_none());
        assert!(state.messages.is_empty());
        assert!(state.participants.is_empty());
    }

    #[test]
    fn project_participants() {
        let events = vec![
            envelope(conversation_created_event()),
            envelope(participant_added_event("u1", "诗闻")),
            envelope(participant_added_event("a1", "小助理")),
        ];
        let state = ConversationProjector::project(&events).unwrap();

        assert_eq!(state.participants.len(), 2);
        assert!(state.participants.contains_key(&ParticipantId::from("u1")));
        assert!(state.participants.contains_key(&ParticipantId::from("a1")));
    }

    #[test]
    fn project_participant_removed() {
        let events = vec![
            envelope(conversation_created_event()),
            envelope(participant_added_event("u1", "诗闻")),
            envelope(participant_added_event("u2", "Other")),
            envelope(ConversationEvent::ParticipantRemoved {
                participant_id: ParticipantId::from("u2"),
            }),
        ];
        let state = ConversationProjector::project(&events).unwrap();

        assert_eq!(state.participants.len(), 1);
        assert!(state.participants.contains_key(&ParticipantId::from("u1")));
        assert!(!state.participants.contains_key(&ParticipantId::from("u2")));
    }

    #[test]
    fn project_messages_ordered() {
        let events = vec![
            envelope(conversation_created_event()),
            envelope(message_appended_event("msg-1", "first", None, None)),
            envelope(message_appended_event("msg-2", "second", None, None)),
            envelope(message_appended_event("msg-3", "third", None, None)),
        ];
        let state = ConversationProjector::project(&events).unwrap();

        assert_eq!(state.messages.len(), 3);
        assert_eq!(state.messages[0].id, MessageId::from("msg-1"));
        assert_eq!(state.messages[1].id, MessageId::from("msg-2"));
        assert_eq!(state.messages[2].id, MessageId::from("msg-3"));

        // Also check messages_by_id
        assert!(state.messages_by_id.contains_key(&MessageId::from("msg-1")));
        assert!(state.messages_by_id.contains_key(&MessageId::from("msg-3")));
    }

    #[test]
    fn project_thread_indexing() {
        let events = vec![
            envelope(conversation_created_event()),
            envelope(message_appended_event(
                "msg-1",
                "root",
                None,
                Some("thread-1"),
            )),
            envelope(message_appended_event(
                "msg-2",
                "reply1",
                Some("msg-1"),
                Some("thread-1"),
            )),
            envelope(message_appended_event(
                "msg-3",
                "reply2",
                Some("msg-1"),
                Some("thread-1"),
            )),
            envelope(message_appended_event("msg-4", "other", None, None)),
        ];
        let state = ConversationProjector::project(&events).unwrap();

        assert_eq!(state.messages.len(), 4);
        assert_eq!(state.threads.len(), 1);

        let thread_msgs = &state.threads[&ThreadId::from("thread-1")];
        assert_eq!(thread_msgs.len(), 3);
        assert_eq!(thread_msgs[0], MessageId::from("msg-1"));
        assert_eq!(thread_msgs[1], MessageId::from("msg-2"));
        assert_eq!(thread_msgs[2], MessageId::from("msg-3"));
    }

    #[test]
    fn project_message_edited() {
        let events = vec![
            envelope(conversation_created_event()),
            envelope(message_appended_event("msg-1", "original", None, None)),
            envelope(ConversationEvent::MessageEdited {
                message_id: MessageId::from("msg-1"),
                new_content: MessageContent::Text {
                    text: "edited".to_string(),
                },
            }),
        ];
        let state = ConversationProjector::project(&events).unwrap();

        let msg = state.messages_by_id.get(&MessageId::from("msg-1")).unwrap();
        match &msg.content {
            MessageContent::Text { text } => assert_eq!(text, "edited"),
            _ => panic!("wrong content type"),
        }
        assert!(msg.edited_at.is_some());
    }

    #[test]
    fn project_message_tombstoned() {
        let events = vec![
            envelope(conversation_created_event()),
            envelope(message_appended_event("msg-1", "keep", None, None)),
            envelope(message_appended_event("msg-2", "delete me", None, None)),
            envelope(message_appended_event("msg-3", "also keep", None, None)),
            envelope(ConversationEvent::MessageTombstoned {
                message_id: MessageId::from("msg-2"),
                reason: "spam".to_string(),
            }),
        ];
        let state = ConversationProjector::project(&events).unwrap();

        // Tombstoned message should be removed from ordered list.
        assert_eq!(state.messages.len(), 2);
        assert_eq!(state.messages[0].id, MessageId::from("msg-1"));
        assert_eq!(state.messages[1].id, MessageId::from("msg-3"));
    }

    #[test]
    fn projection_is_deterministic() {
        let events = vec![
            envelope(conversation_created_event()),
            envelope(participant_added_event("u1", "诗闻")),
            envelope(message_appended_event("msg-1", "hello", None, None)),
            envelope(message_appended_event("msg-2", "world", None, None)),
        ];

        let state1 = ConversationProjector::project(&events).unwrap();
        let state2 = ConversationProjector::project(&events).unwrap();

        assert_eq!(state1.session, state2.session);
        assert_eq!(state1.messages.len(), state2.messages.len());
        assert_eq!(state1.participants.len(), state2.participants.len());
    }
}
