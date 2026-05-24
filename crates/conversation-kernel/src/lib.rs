//! # Conversation Kernel
//!
//! The Conversation Kernel provides commands for managing conversations,
//! a projector for replaying event streams into state, and a slice builder
//! for constructing context windows.

pub mod commands;
pub mod kernel;
pub mod policy;
pub mod projector;
pub mod slice_builder;
pub mod state;

pub use commands::{
    AppendMessageCommand, CancelAgentRunCommand, CompleteAgentRunCommand,
    CreateAssistantSuggestionCommand, CreateConversationCommand, EditMessageCommand,
    FailAgentRunCommand, RequestAgentRunCommand, StartAgentRunCommand, TimeoutAgentRunCommand,
};
pub use kernel::{Clock, ConversationKernel, IdGenerator, UtcClock, UuidGenerator};
pub use policy::{AgentRunReason, ConversationPolicy, RuleBasedPolicy};
pub use projector::ConversationProjector;
pub use slice_builder::ConversationSliceBuilder;
pub use state::ConversationState;
