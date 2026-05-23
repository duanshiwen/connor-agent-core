//! Conversation slice builder.
//!
//! Constructs bounded subsets of conversation messages for context construction.
//! Supports multiple strategies: recent window, current thread, around trigger.

use crate::state::ConversationState;
use anyhow::Result;
use conversation_core::*;

/// Configuration for the slice builder.
pub struct ConversationSliceBuilder {
    /// Maximum number of messages in a recent-window slice.
    pub max_recent_messages: usize,
}

impl ConversationSliceBuilder {
    pub fn new(max_recent_messages: usize) -> Self {
        Self {
            max_recent_messages,
        }
    }

    /// Build a slice of the most recent messages up to `max_recent_messages`.
    pub fn build_recent_window(
        &self,
        state: &ConversationState,
        trigger_message_id: &MessageId,
    ) -> Result<ConversationSlice> {
        let session = state
            .session
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("no conversation session in state"))?;

        let mut messages: Vec<Message> = state
            .messages
            .iter()
            .filter(|m| {
                m.visibility == Visibility::Conversation || m.visibility == Visibility::AgentOnly
            })
            .cloned()
            .collect();

        if messages.len() > self.max_recent_messages {
            messages = messages[messages.len() - self.max_recent_messages..].to_vec();
        }

        Ok(ConversationSlice {
            id: uuid::Uuid::new_v4().to_string(),
            conversation_id: session.id.clone(),
            trigger_message_id: trigger_message_id.clone(),
            messages,
            reason: SliceBuildReason::RecentWindow,
        })
    }

    /// Build a slice containing all messages in the same thread as the trigger message.
    pub fn build_current_thread(
        &self,
        state: &ConversationState,
        trigger_message_id: &MessageId,
    ) -> Result<ConversationSlice> {
        let session = state
            .session
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("no conversation session in state"))?;

        // Find the thread ID of the trigger message.
        let trigger_msg = state
            .messages_by_id
            .get(trigger_message_id)
            .ok_or_else(|| anyhow::anyhow!("trigger message not found"))?;

        let thread_id = trigger_msg
            .thread_id
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("trigger message is not in a thread"))?;

        // Get all message IDs in this thread.
        let thread_msg_ids = state
            .threads
            .get(thread_id)
            .ok_or_else(|| anyhow::anyhow!("thread not found"))?;

        // Build ordered list of thread messages.
        let messages: Vec<Message> = thread_msg_ids
            .iter()
            .filter_map(|id| state.messages_by_id.get(id))
            .cloned()
            .collect();

        Ok(ConversationSlice {
            id: uuid::Uuid::new_v4().to_string(),
            conversation_id: session.id.clone(),
            trigger_message_id: trigger_message_id.clone(),
            messages,
            reason: SliceBuildReason::CurrentThread,
        })
    }

    /// Build a slice with messages around the trigger (before + after).
    pub fn build_around_trigger(
        &self,
        state: &ConversationState,
        trigger_message_id: &MessageId,
        context_before: usize,
        context_after: usize,
    ) -> Result<ConversationSlice> {
        let session = state
            .session
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("no conversation session in state"))?;

        let trigger_idx = state
            .messages
            .iter()
            .position(|m| m.id == *trigger_message_id)
            .ok_or_else(|| anyhow::anyhow!("trigger message not found in messages list"))?;

        let start = trigger_idx.saturating_sub(context_before);
        let end = (trigger_idx + context_after + 1).min(state.messages.len());

        let messages = state.messages[start..end].to_vec();

        Ok(ConversationSlice {
            id: uuid::Uuid::new_v4().to_string(),
            conversation_id: session.id.clone(),
            trigger_message_id: trigger_message_id.clone(),
            messages,
            reason: SliceBuildReason::AroundTrigger,
        })
    }

    /// Build a slice filtered for a specific user (excludes PrivateToUser for other users).
    pub fn build_for_user(
        &self,
        state: &ConversationState,
        trigger_message_id: &MessageId,
        user_id: &ParticipantId,
    ) -> Result<ConversationSlice> {
        let session = state
            .session
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("no conversation session in state"))?;

        let mut messages: Vec<Message> = state
            .messages
            .iter()
            .filter(|m| match &m.visibility {
                Visibility::Conversation => true,
                Visibility::AgentOnly => true,
                Visibility::PrivateToUser { user_id: uid } => uid == user_id,
                Visibility::Participants { participant_ids } => participant_ids.contains(user_id),
            })
            .cloned()
            .collect();

        if messages.len() > self.max_recent_messages {
            messages = messages[messages.len() - self.max_recent_messages..].to_vec();
        }

        Ok(ConversationSlice {
            id: uuid::Uuid::new_v4().to_string(),
            conversation_id: session.id.clone(),
            trigger_message_id: trigger_message_id.clone(),
            messages,
            reason: SliceBuildReason::RecentWindow,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::projector::ConversationProjector;
    use chrono::Utc;

    fn now() -> chrono::DateTime<Utc> {
        Utc::now()
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

    fn make_conversation() -> ConversationEvent {
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

    fn make_message(
        msg_id: &str,
        text: &str,
        thread_id: Option<&str>,
        visibility: Visibility,
    ) -> ConversationEvent {
        ConversationEvent::MessageAppended {
            message: Message {
                id: MessageId::from(msg_id),
                conversation_id: ConversationId::from("conv-1"),
                sender_id: ParticipantId::from("u1"),
                content: MessageContent::Text {
                    text: text.to_string(),
                },
                reply_to: None,
                thread_id: thread_id.map(ThreadId::from),
                visibility,
                created_at: now(),
                edited_at: None,
            },
        }
    }

    fn make_state_with_messages(n: usize) -> (ConversationState, Vec<ConversationEventEnvelope>) {
        let mut events = vec![envelope(make_conversation())];
        for i in 1..=n {
            events.push(envelope(make_message(
                &format!("msg-{}", i),
                &format!("message {}", i),
                None,
                Visibility::Conversation,
            )));
        }
        let state = ConversationProjector::project(&events).unwrap();
        (state, events)
    }

    // --- Recent window tests ---

    #[test]
    fn recent_window_under_limit_returns_all() {
        let (state, _) = make_state_with_messages(3);
        let builder = ConversationSliceBuilder::new(5);
        let trigger = MessageId::from("msg-3");

        let slice = builder.build_recent_window(&state, &trigger).unwrap();
        assert_eq!(slice.messages.len(), 3);
        assert_eq!(slice.reason, SliceBuildReason::RecentWindow);
    }

    #[test]
    fn recent_window_truncates_to_max() {
        let (state, _) = make_state_with_messages(20);
        let builder = ConversationSliceBuilder::new(5);
        let trigger = MessageId::from("msg-20");

        let slice = builder.build_recent_window(&state, &trigger).unwrap();
        assert_eq!(slice.messages.len(), 5);
        assert_eq!(slice.messages[0].id, MessageId::from("msg-16"));
        assert_eq!(slice.messages[4].id, MessageId::from("msg-20"));
    }

    #[test]
    fn recent_window_exact_limit() {
        let (state, _) = make_state_with_messages(5);
        let builder = ConversationSliceBuilder::new(5);
        let trigger = MessageId::from("msg-5");

        let slice = builder.build_recent_window(&state, &trigger).unwrap();
        assert_eq!(slice.messages.len(), 5);
    }

    // --- Thread slice tests ---

    #[test]
    fn thread_slice_only_includes_thread_messages() {
        let events = vec![
            envelope(make_conversation()),
            envelope(make_message(
                "msg-1",
                "thread root",
                Some("t1"),
                Visibility::Conversation,
            )),
            envelope(make_message(
                "msg-2",
                "thread reply 1",
                Some("t1"),
                Visibility::Conversation,
            )),
            envelope(make_message(
                "msg-3",
                "thread reply 2",
                Some("t1"),
                Visibility::Conversation,
            )),
            envelope(make_message(
                "msg-4",
                "random message",
                None,
                Visibility::Conversation,
            )),
            envelope(make_message(
                "msg-5",
                "another random",
                None,
                Visibility::Conversation,
            )),
        ];
        let state = ConversationProjector::project(&events).unwrap();
        let builder = ConversationSliceBuilder::new(100);

        let slice = builder
            .build_current_thread(&state, &MessageId::from("msg-2"))
            .unwrap();
        assert_eq!(slice.messages.len(), 3);
        assert!(
            slice
                .messages
                .iter()
                .all(|m| m.thread_id == Some(ThreadId::from("t1")))
        );
        assert_eq!(slice.reason, SliceBuildReason::CurrentThread);
    }

    #[test]
    fn thread_slice_fails_for_non_thread_message() {
        let events = vec![
            envelope(make_conversation()),
            envelope(make_message(
                "msg-1",
                "no thread",
                None,
                Visibility::Conversation,
            )),
        ];
        let state = ConversationProjector::project(&events).unwrap();
        let builder = ConversationSliceBuilder::new(100);

        let result = builder.build_current_thread(&state, &MessageId::from("msg-1"));
        assert!(result.is_err());
    }

    // --- Around trigger tests ---

    #[test]
    fn around_trigger_middle() {
        let (state, _) = make_state_with_messages(10);
        let builder = ConversationSliceBuilder::new(100);

        let slice = builder
            .build_around_trigger(&state, &MessageId::from("msg-5"), 2, 2)
            .unwrap();

        assert_eq!(slice.messages.len(), 5); // msg-3,4,5,6,7
        assert_eq!(slice.messages[0].id, MessageId::from("msg-3"));
        assert_eq!(slice.messages[4].id, MessageId::from("msg-7"));
        assert_eq!(slice.reason, SliceBuildReason::AroundTrigger);
    }

    #[test]
    fn around_trigger_at_start() {
        let (state, _) = make_state_with_messages(10);
        let builder = ConversationSliceBuilder::new(100);

        let slice = builder
            .build_around_trigger(&state, &MessageId::from("msg-1"), 2, 2)
            .unwrap();

        assert_eq!(slice.messages.len(), 3); // msg-1,2,3
        assert_eq!(slice.messages[0].id, MessageId::from("msg-1"));
    }

    #[test]
    fn around_trigger_at_end() {
        let (state, _) = make_state_with_messages(10);
        let builder = ConversationSliceBuilder::new(100);

        let slice = builder
            .build_around_trigger(&state, &MessageId::from("msg-10"), 2, 2)
            .unwrap();

        assert_eq!(slice.messages.len(), 3); // msg-8,9,10
        assert_eq!(slice.messages[2].id, MessageId::from("msg-10"));
    }

    // --- Visibility filter tests ---

    #[test]
    fn visibility_filter_for_user_includes_private() {
        let events = vec![
            envelope(make_conversation()),
            envelope(make_message(
                "msg-1",
                "public",
                None,
                Visibility::Conversation,
            )),
            envelope(make_message(
                "msg-2",
                "private to u1",
                None,
                Visibility::PrivateToUser {
                    user_id: ParticipantId::from("u1"),
                },
            )),
            envelope(make_message(
                "msg-3",
                "public again",
                None,
                Visibility::Conversation,
            )),
        ];
        let state = ConversationProjector::project(&events).unwrap();
        let builder = ConversationSliceBuilder::new(100);

        // u1 can see all 3
        let slice_u1 = builder
            .build_for_user(
                &state,
                &MessageId::from("msg-3"),
                &ParticipantId::from("u1"),
            )
            .unwrap();
        assert_eq!(slice_u1.messages.len(), 3);

        // u2 can't see private message
        let slice_u2 = builder
            .build_for_user(
                &state,
                &MessageId::from("msg-3"),
                &ParticipantId::from("u2"),
            )
            .unwrap();
        assert_eq!(slice_u2.messages.len(), 2);
    }

    #[test]
    fn visibility_filter_participants_list() {
        let events = vec![
            envelope(make_conversation()),
            envelope(make_message(
                "msg-1",
                "public",
                None,
                Visibility::Conversation,
            )),
            envelope(make_message(
                "msg-2",
                "only u1 and u2",
                None,
                Visibility::Participants {
                    participant_ids: vec![ParticipantId::from("u1"), ParticipantId::from("u2")],
                },
            )),
        ];
        let state = ConversationProjector::project(&events).unwrap();
        let builder = ConversationSliceBuilder::new(100);

        let slice_u1 = builder
            .build_for_user(
                &state,
                &MessageId::from("msg-2"),
                &ParticipantId::from("u1"),
            )
            .unwrap();
        assert_eq!(slice_u1.messages.len(), 2);

        let slice_u3 = builder
            .build_for_user(
                &state,
                &MessageId::from("msg-2"),
                &ParticipantId::from("u3"),
            )
            .unwrap();
        assert_eq!(slice_u3.messages.len(), 1);
    }

    #[test]
    fn visibility_filter_respects_max_recent() {
        let mut events = vec![envelope(make_conversation())];
        for i in 1..=20 {
            let vis = if i == 5 {
                Visibility::PrivateToUser {
                    user_id: ParticipantId::from("u1"),
                }
            } else {
                Visibility::Conversation
            };
            events.push(envelope(make_message(
                &format!("msg-{}", i),
                &format!("msg {}", i),
                None,
                vis,
            )));
        }
        let state = ConversationProjector::project(&events).unwrap();
        let builder = ConversationSliceBuilder::new(5);

        // u1 sees 5 private + 19 public = 20 total, truncated to last 5
        let slice = builder
            .build_for_user(
                &state,
                &MessageId::from("msg-20"),
                &ParticipantId::from("u1"),
            )
            .unwrap();
        assert_eq!(slice.messages.len(), 5);
    }
}
