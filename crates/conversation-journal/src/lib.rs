//! # Conversation Journal
//!
//! Append-only storage for conversation events.
//! Phase 1 will implement `MemoryConversationJournal` and `JsonlConversationJournal`.

pub mod jsonl;
pub mod memory;

pub use jsonl::JsonlConversationJournal;
pub use memory::MemoryConversationJournal;

use async_trait::async_trait;
use conversation_core::{ConversationEventEnvelope, ConversationId};

/// Trait for append-only conversation event storage.
#[async_trait]
pub trait ConversationJournal: Send + Sync {
    /// Append a single event to the journal.
    async fn append(&self, event: ConversationEventEnvelope) -> anyhow::Result<()>;

    /// Load all events for a given conversation, in append order.
    async fn load(
        &self,
        conversation_id: &ConversationId,
    ) -> anyhow::Result<Vec<ConversationEventEnvelope>>;
}
