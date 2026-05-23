//! Command structs for the Conversation Kernel.
//!
//! Commands are validated before producing events. Each command contains
//! the data needed to produce one or more `ConversationEvent`s.

use conversation_core::*;

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

/// Command to create a private assistant suggestion.
#[derive(Debug, Clone)]
pub struct CreateAssistantSuggestionCommand {
    pub conversation_id: ConversationId,
    pub target_user_id: ParticipantId,
    pub text: String,
    pub actions: Vec<SuggestedAction>,
    pub trigger: SuggestionTrigger,
}

/// Command to request an agent run (does NOT directly invoke a model).
#[derive(Debug, Clone)]
pub struct RequestAgentRunCommand {
    pub conversation_id: ConversationId,
    pub trigger_message_id: MessageId,
    pub requested_by: ParticipantId,
}

/// Command to mark an agent run as completed.
#[derive(Debug, Clone)]
pub struct CompleteAgentRunCommand {
    pub conversation_id: ConversationId,
    pub run_id: String,
    pub output_message_id: MessageId,
    pub completed_by: ParticipantId,
}
