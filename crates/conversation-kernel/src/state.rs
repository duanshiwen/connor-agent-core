//! Projected conversation state.
//!
//! `ConversationState` is a read model derived from replaying events.
//! It is never mutated directly — always rebuilt from the event journal.

use action_core::ActionId;
use artifact_core::{ArtifactDescriptor, ArtifactId};
use conversation_core::*;
use entity_core::{EntityDescriptor, EntityId};
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

    /// Linked entities keyed by entity ID.
    pub linked_entities: HashMap<EntityId, EntityDescriptor>,

    /// Linked artifacts keyed by artifact ID.
    pub linked_artifacts: HashMap<ArtifactId, ArtifactDescriptor>,

    /// Actions keyed by action ID.
    pub actions: HashMap<ActionId, ConversationActionState>,

    /// Agent runs keyed by run ID.
    pub agent_runs: HashMap<String, AgentRunState>,

    /// Completed agent runs, keyed by run ID, pointing to their output message.
    ///
    /// Kept as a convenience index for existing runtime code; `agent_runs` is
    /// the canonical lifecycle projection.
    pub completed_agent_runs: HashMap<String, MessageId>,
}
