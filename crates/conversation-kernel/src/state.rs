//! Projected conversation state.
//!
//! `ConversationState` is a read model derived from replaying events.
//! It is never mutated directly — always rebuilt from the event journal.

use conversation_core::*;
use std::collections::HashMap;

/// The projected state of a conversation, rebuilt from its event journal.
#[derive(Debug, Clone, Default)]
pub struct ConversationState {
    /// The conversation session (None until `ConversationCreated` is seen).
    pub session: Option<ConversationSession>,

    /// All participants, keyed by ID.
    pub participants: HashMap<ParticipantId, Participant>,

    /// Messages in append order.
    pub messages: Vec<Message>,

    /// Messages indexed by ID for fast lookup.
    pub messages_by_id: HashMap<MessageId, Message>,

    /// Thread index: thread_id → message IDs in that thread.
    pub threads: HashMap<ThreadId, Vec<MessageId>>,
}
