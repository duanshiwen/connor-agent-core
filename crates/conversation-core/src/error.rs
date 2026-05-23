//! Error types for conversation-core.

use thiserror::Error;

/// Errors that can occur in conversation-core operations.
#[derive(Debug, Error)]
pub enum CoreError {
    #[error("participant not found: {0}")]
    ParticipantNotFound(String),

    #[error("message not found: {0}")]
    MessageNotFound(String),

    #[error("conversation not found: {0}")]
    ConversationNotFound(String),

    #[error("invalid operation: {0}")]
    InvalidOperation(String),

    #[error("serialization error: {0}")]
    Serialization(#[from] serde_json::Error),
}
