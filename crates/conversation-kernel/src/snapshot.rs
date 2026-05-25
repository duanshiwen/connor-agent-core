//! Versioned conversation projection snapshots.
//!
//! Snapshots capture a projected `ConversationState` at a known event boundary
//! so later callers can replay only tail events instead of rebuilding from the
//! beginning of the journal.

use crate::state::ConversationState;
use chrono::{DateTime, Utc};
use conversation_core::{ConversationId, EventId};
use serde::{Deserialize, Serialize};

/// Current conversation projection snapshot schema version.
pub const CONVERSATION_SNAPSHOT_VERSION: u32 = 1;

/// A versioned snapshot of a projected conversation state.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ConversationProjectionSnapshot {
    pub snapshot_version: u32,
    pub conversation_id: ConversationId,
    pub created_at: DateTime<Utc>,
    pub last_event_id: Option<EventId>,
    pub event_count: u64,
    pub state: ConversationState,
}

impl ConversationProjectionSnapshot {
    pub fn new(
        conversation_id: ConversationId,
        created_at: DateTime<Utc>,
        last_event_id: Option<EventId>,
        event_count: u64,
        state: ConversationState,
    ) -> Self {
        Self {
            snapshot_version: CONVERSATION_SNAPSHOT_VERSION,
            conversation_id,
            created_at,
            last_event_id,
            event_count,
            state,
        }
    }
}
