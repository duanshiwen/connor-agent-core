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
pub mod snapshot;
pub mod state;

pub use commands::{
    AppendMessageCommand, ApproveActionCommand, AttachSurfaceCommand, CancelAgentRunCommand,
    CaptureAssetCommand, CloseSurfaceCommand, CompleteActionCommand, CompleteAgentRunCommand,
    CreateAssistantSuggestionCommand, CreateConversationCommand, DenyActionCommand,
    EditMessageCommand, FailActionCommand, FailAgentRunCommand, LinkArtifactCommand,
    LinkEntityCommand, ObserveAssetCommand, ObserveEntityStateCommand, ProcessAssetCommand,
    QueryEntityCommand, RequestActionCommand, RequestAgentRunCommand, RequireActionApprovalCommand,
    StartActionCommand, StartAgentRunCommand, TimeoutAgentRunCommand, UnlinkArtifactCommand,
    UnlinkEntityCommand, UpdateSurfaceCommand,
};
pub use kernel::{Clock, ConversationKernel, IdGenerator, UtcClock, UuidGenerator};
pub use policy::{AgentRunReason, ConversationPolicy, RuleBasedPolicy};
pub use projector::ConversationProjector;
pub use slice_builder::ConversationSliceBuilder;
pub use snapshot::{CONVERSATION_SNAPSHOT_VERSION, ConversationProjectionSnapshot};
pub use state::ConversationState;
