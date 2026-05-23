//! In-memory journal implementation for testing.

use async_trait::async_trait;
use conversation_core::{ConversationEventEnvelope, ConversationId};
use std::collections::HashMap;
use std::sync::Mutex;

use crate::ConversationJournal;

/// An in-memory journal that stores events in a `HashMap`.
///
/// **Not suitable for production** — intended for unit and integration tests.
pub struct MemoryConversationJournal {
    events: Mutex<HashMap<ConversationId, Vec<ConversationEventEnvelope>>>,
}

impl MemoryConversationJournal {
    pub fn new() -> Self {
        Self {
            events: Mutex::new(HashMap::new()),
        }
    }
}

impl Default for MemoryConversationJournal {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl ConversationJournal for MemoryConversationJournal {
    async fn append(&self, event: ConversationEventEnvelope) -> anyhow::Result<()> {
        let mut events = self.events.lock().unwrap();
        events
            .entry(event.conversation_id.clone())
            .or_default()
            .push(event);
        Ok(())
    }

    async fn load(
        &self,
        conversation_id: &ConversationId,
    ) -> anyhow::Result<Vec<ConversationEventEnvelope>> {
        let events = self.events.lock().unwrap();
        Ok(events.get(conversation_id).cloned().unwrap_or_default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use conversation_core::*;

    fn make_event(conversation_id: &str, event_id: &str) -> ConversationEventEnvelope {
        ConversationEventEnvelope {
            event_id: EventId::from(event_id),
            conversation_id: ConversationId::from(conversation_id),
            occurred_at: chrono::Utc::now(),
            actor_id: None,
            event: ConversationEvent::ConversationCreated {
                session: ConversationSession {
                    id: ConversationId::from(conversation_id),
                    kind: ConversationKind::Direct,
                    title: None,
                    participants: vec![],
                    created_at: chrono::Utc::now(),
                    updated_at: chrono::Utc::now(),
                    status: ConversationStatus::Active,
                },
            },
        }
    }

    #[tokio::test]
    async fn append_single_event() {
        let journal = MemoryConversationJournal::new();
        let event = make_event("conv-1", "evt-1");

        journal.append(event.clone()).await.unwrap();

        let loaded = journal.load(&ConversationId::from("conv-1")).await.unwrap();
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].event_id, EventId::from("evt-1"));
    }

    #[tokio::test]
    async fn append_multiple_events_to_same_conversation() {
        let journal = MemoryConversationJournal::new();

        journal.append(make_event("conv-1", "evt-1")).await.unwrap();
        journal.append(make_event("conv-1", "evt-2")).await.unwrap();
        journal.append(make_event("conv-1", "evt-3")).await.unwrap();

        let loaded = journal.load(&ConversationId::from("conv-1")).await.unwrap();
        assert_eq!(loaded.len(), 3);
        assert_eq!(loaded[0].event_id, EventId::from("evt-1"));
        assert_eq!(loaded[2].event_id, EventId::from("evt-3"));
    }

    #[tokio::test]
    async fn events_from_different_conversations_are_isolated() {
        let journal = MemoryConversationJournal::new();

        journal.append(make_event("conv-1", "evt-1")).await.unwrap();
        journal.append(make_event("conv-2", "evt-2")).await.unwrap();

        let conv1 = journal.load(&ConversationId::from("conv-1")).await.unwrap();
        let conv2 = journal.load(&ConversationId::from("conv-2")).await.unwrap();

        assert_eq!(conv1.len(), 1);
        assert_eq!(conv2.len(), 1);
        assert_eq!(conv1[0].event_id, EventId::from("evt-1"));
        assert_eq!(conv2[0].event_id, EventId::from("evt-2"));
    }

    #[tokio::test]
    async fn load_nonexistent_returns_empty() {
        let journal = MemoryConversationJournal::new();
        let loaded = journal
            .load(&ConversationId::from("nonexistent"))
            .await
            .unwrap();
        assert!(loaded.is_empty());
    }
}
