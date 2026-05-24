//! # Conversation Core
//!
//! Core types for the Conversation Kernel.
//!
//! This crate defines the domain types used across the conversation subsystem:
//! IDs, events, messages, participants, visibility rules, sessions, and slices.
//! All types are serializable and designed for event-sourced append-only workflows.

pub mod action_lifecycle;
pub mod agent_run;
pub mod error;
pub mod event;
pub mod ids;
pub mod message;
pub mod participant;
pub mod session;
pub mod slice;
pub mod visibility;

// Re-export commonly used types for convenience.
pub use action_lifecycle::{ConversationActionState, ConversationActionStatus};
pub use agent_run::{AgentRunState, AgentRunStatus};
pub use error::CoreError;
pub use event::{
    CURRENT_SCHEMA_VERSION, ConversationEvent, ConversationEventEnvelope, SuggestionTrigger,
};
pub use ids::{ConversationId, EventId, MessageId, ParticipantId, ThreadId};
pub use message::{Message, MessageContent, SuggestedAction};
pub use participant::{Participant, ParticipantKind};
pub use session::{ConversationKind, ConversationSession, ConversationStatus};
pub use slice::{ConversationSlice, SliceBuildReason};
pub use visibility::Visibility;
