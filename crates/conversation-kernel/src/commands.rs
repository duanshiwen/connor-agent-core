//! Command structs for the Conversation Kernel.
//!
//! Commands are validated before producing events. Each command contains
//! the data needed to produce one or more `ConversationEvent`s.

use action_core::{ActionId, ActionRequest, ActionResult};
use artifact_core::{ArtifactDescriptor, ArtifactId};
use conversation_core::*;
use entity_core::{EntityDescriptor, EntityId, LinkReason};

/// Command to create a new conversation.
#[derive(Debug, Clone)]
pub struct CreateConversationCommand {
    pub kind: ConversationKind,
    pub title: Option<String>,
    pub participants: Vec<Participant>,
    pub actor_id: Option<ParticipantId>,
}

/// Command to append a message to a conversation.
#[derive(Debug, Clone)]
pub struct AppendMessageCommand {
    pub conversation_id: ConversationId,
    pub sender_id: ParticipantId,
    pub content: MessageContent,
    pub reply_to: Option<MessageId>,
    pub thread_id: Option<ThreadId>,
    pub visibility: Visibility,
}

/// Command to edit an existing message.
#[derive(Debug, Clone)]
pub struct EditMessageCommand {
    pub conversation_id: ConversationId,
    pub message_id: MessageId,
    pub new_content: MessageContent,
    pub edited_by: ParticipantId,
}

/// Command to create a private assistant suggestion.
#[derive(Debug, Clone)]
pub struct CreateAssistantSuggestionCommand {
    pub conversation_id: ConversationId,
    pub target_user_id: ParticipantId,
    pub text: String,
    pub actions: Vec<SuggestedAction>,
    pub trigger: SuggestionTrigger,
}

/// Command to link an entity to a conversation.
#[derive(Debug, Clone)]
pub struct LinkEntityCommand {
    pub conversation_id: ConversationId,
    pub entity: EntityDescriptor,
    pub reason: LinkReason,
    pub linked_by: Option<ParticipantId>,
}

/// Command to unlink an entity from a conversation.
#[derive(Debug, Clone)]
pub struct UnlinkEntityCommand {
    pub conversation_id: ConversationId,
    pub entity_id: EntityId,
    pub reason: String,
    pub unlinked_by: Option<ParticipantId>,
}

/// Command to link an artifact to a conversation.
#[derive(Debug, Clone)]
pub struct LinkArtifactCommand {
    pub conversation_id: ConversationId,
    pub artifact: ArtifactDescriptor,
    pub linked_by: Option<ParticipantId>,
}

/// Command to unlink an artifact from a conversation.
#[derive(Debug, Clone)]
pub struct UnlinkArtifactCommand {
    pub conversation_id: ConversationId,
    pub artifact_id: ArtifactId,
    pub reason: String,
    pub unlinked_by: Option<ParticipantId>,
}

/// Command to record metadata about an entity state observation.
#[derive(Debug, Clone)]
pub struct ObserveEntityStateCommand {
    pub conversation_id: ConversationId,
    pub entity_id: EntityId,
    pub state_ref: String,
    pub observed_by: Option<ParticipantId>,
}

/// Command to record metadata about an entity query.
#[derive(Debug, Clone)]
pub struct QueryEntityCommand {
    pub conversation_id: ConversationId,
    pub entity_id: EntityId,
    pub query: String,
    pub result_ref: Option<String>,
    pub queried_by: Option<ParticipantId>,
}

/// Command to request an agent run (does NOT directly invoke a model).
#[derive(Debug, Clone)]
pub struct RequestAgentRunCommand {
    pub conversation_id: ConversationId,
    pub trigger_message_id: MessageId,
    pub requested_by: ParticipantId,
}

/// Command to mark an agent run as started.
#[derive(Debug, Clone)]
pub struct StartAgentRunCommand {
    pub conversation_id: ConversationId,
    pub run_id: String,
    pub started_by: ParticipantId,
}

/// Command to mark an agent run as completed.
#[derive(Debug, Clone)]
pub struct CompleteAgentRunCommand {
    pub conversation_id: ConversationId,
    pub run_id: String,
    pub output_message_id: MessageId,
    pub completed_by: ParticipantId,
}

/// Command to mark an agent run as failed.
#[derive(Debug, Clone)]
pub struct FailAgentRunCommand {
    pub conversation_id: ConversationId,
    pub run_id: String,
    pub error_code: String,
    pub error_message: String,
    pub failed_by: ParticipantId,
}

/// Command to mark an agent run as cancelled.
#[derive(Debug, Clone)]
pub struct CancelAgentRunCommand {
    pub conversation_id: ConversationId,
    pub run_id: String,
    pub reason: String,
    pub cancelled_by: ParticipantId,
}

/// Command to record that an action was requested.
#[derive(Debug, Clone)]
pub struct RequestActionCommand {
    pub conversation_id: ConversationId,
    pub action_request: ActionRequest,
    pub requested_by: Option<ParticipantId>,
}

/// Command to record that an action requires approval.
#[derive(Debug, Clone)]
pub struct RequireActionApprovalCommand {
    pub conversation_id: ConversationId,
    pub action_id: ActionId,
    pub reason: String,
    pub required_by: Option<ParticipantId>,
}

/// Command to record action approval.
#[derive(Debug, Clone)]
pub struct ApproveActionCommand {
    pub conversation_id: ConversationId,
    pub action_id: ActionId,
    pub approved_by: ParticipantId,
}

/// Command to record action denial.
#[derive(Debug, Clone)]
pub struct DenyActionCommand {
    pub conversation_id: ConversationId,
    pub action_id: ActionId,
    pub reason: String,
    pub denied_by: Option<ParticipantId>,
}

/// Command to record action execution start.
#[derive(Debug, Clone)]
pub struct StartActionCommand {
    pub conversation_id: ConversationId,
    pub action_id: ActionId,
    pub started_by: Option<ParticipantId>,
}

/// Command to record action execution completion.
#[derive(Debug, Clone)]
pub struct CompleteActionCommand {
    pub conversation_id: ConversationId,
    pub action_id: ActionId,
    pub result: ActionResult,
    pub completed_by: Option<ParticipantId>,
}

/// Command to record action execution failure.
#[derive(Debug, Clone)]
pub struct FailActionCommand {
    pub conversation_id: ConversationId,
    pub action_id: ActionId,
    pub error_message: String,
    pub failed_by: Option<ParticipantId>,
}

/// Command to mark an agent run as timed out.
#[derive(Debug, Clone)]
pub struct TimeoutAgentRunCommand {
    pub conversation_id: ConversationId,
    pub run_id: String,
    pub timed_out_by: Option<ParticipantId>,
}
