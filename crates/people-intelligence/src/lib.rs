//! # People Intelligence
//!
//! Policy and event types for AgentOS People Intelligence.
//!
//! This crate provides the governance layer for people-related data:
//! - CapabilityPolicy: which mutations require user confirmation
//! - PeopleEvent: conversation events related to people
//! - MentionedPerson: extracted person references from text
//! - Draft workflow types for observation → confirmation → persist
//!
//! Design principles:
//! - Sensitive writes must ask user before persisting
//! - All events are audit-logged
//! - Drafts can be confirmed, edited, or rejected
//! - Evidence links are mandatory for observations

use chrono::{DateTime, Utc};
use person_entity::{PersonAttribute, PersonId, ProfileConfidence, ProfileSensitivity};
use relationship_core::RelationshipObservation;
use serde::{Deserialize, Serialize};
use std::fmt;

// ---------------------------------------------------------------------------
// Capability Policy
// ---------------------------------------------------------------------------

/// Decision on whether a mutation is allowed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PolicyDecision {
    /// Auto-save without asking user.
    AutoAllow,
    /// Requires user confirmation before saving.
    RequireConfirmation,
    /// Blocked (should not be saved).
    Blocked,
}

impl fmt::Display for PolicyDecision {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::AutoAllow => write!(f, "auto_allow"),
            Self::RequireConfirmation => write!(f, "require_confirmation"),
            Self::Blocked => write!(f, "blocked"),
        }
    }
}

/// Evaluate policy for a proposed person attribute mutation.
pub fn evaluate_attribute_policy(attr: &PersonAttribute) -> PolicyDecision {
    // Sensitive attributes always require confirmation unless user-confirmed
    if matches!(attr.sensitivity, ProfileSensitivity::Sensitive) {
        if matches!(attr.confidence, ProfileConfidence::Confirmed) {
            PolicyDecision::AutoAllow
        } else {
            PolicyDecision::RequireConfirmation
        }
    } else {
        PolicyDecision::AutoAllow
    }
}

/// Evaluate policy for a proposed relationship observation.
pub fn evaluate_observation_policy(obs: &RelationshipObservation) -> PolicyDecision {
    if obs.requires_confirmation() {
        PolicyDecision::RequireConfirmation
    } else {
        PolicyDecision::AutoAllow
    }
}

// ---------------------------------------------------------------------------
// Mentioned Person
// ---------------------------------------------------------------------------

/// A person mentioned in conversation text.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MentionedPerson {
    pub raw_text: String,
    pub person_id: Option<PersonId>,
    pub context: String,
    pub confidence: ProfileConfidence,
    pub offset_start: usize,
    pub offset_end: usize,
}

// ---------------------------------------------------------------------------
// People Events
// ---------------------------------------------------------------------------

/// Events related to people intelligence.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum PeopleEvent {
    /// A person was mentioned in conversation.
    PersonMentioned {
        event_id: String,
        person: MentionedPerson,
        conversation_id: String,
        message_id: String,
        timestamp: DateTime<Utc>,
    },
    /// A profile observation was proposed.
    PersonProfileObserved {
        event_id: String,
        person_id: PersonId,
        attribute: PersonAttribute,
        draft_id: String,
        timestamp: DateTime<Utc>,
    },
    /// A profile update was proposed (requires confirmation for sensitive).
    PersonProfileUpdateProposed {
        event_id: String,
        person_id: PersonId,
        proposed_attribute: PersonAttribute,
        draft_id: String,
        policy_decision: PolicyDecision,
        timestamp: DateTime<Utc>,
    },
    /// Profile update was confirmed by user.
    PersonProfileUpdated {
        event_id: String,
        person_id: PersonId,
        attribute: PersonAttribute,
        confirmed_by_user: bool,
        timestamp: DateTime<Utc>,
    },
    /// A relationship observation was proposed.
    RelationshipObserved {
        event_id: String,
        observation: RelationshipObservation,
        draft_id: String,
        timestamp: DateTime<Utc>,
    },
    /// Relationship update proposed (requires confirmation for sensitive).
    RelationshipUpdateProposed {
        event_id: String,
        observation: RelationshipObservation,
        draft_id: String,
        policy_decision: PolicyDecision,
        timestamp: DateTime<Utc>,
    },
    /// Relationship update confirmed by user.
    RelationshipUpdated {
        event_id: String,
        observation: RelationshipObservation,
        confirmed_by_user: bool,
        timestamp: DateTime<Utc>,
    },
    /// A behavior observation was created.
    BehaviorObservationCreated {
        event_id: String,
        person_id: PersonId,
        observation: String,
        confidence: ProfileConfidence,
        draft_id: String,
        timestamp: DateTime<Utc>,
    },
    /// Behavior observation confirmed by user.
    BehaviorObservationConfirmed {
        event_id: String,
        draft_id: String,
        person_id: PersonId,
        final_observation: String,
        timestamp: DateTime<Utc>,
    },
    /// Behavior observation rejected by user.
    BehaviorObservationRejected {
        event_id: String,
        draft_id: String,
        person_id: PersonId,
        reason: Option<String>,
        timestamp: DateTime<Utc>,
    },
}

impl PeopleEvent {
    pub fn event_id(&self) -> &str {
        match self {
            Self::PersonMentioned { event_id, .. }
            | Self::PersonProfileObserved { event_id, .. }
            | Self::PersonProfileUpdateProposed { event_id, .. }
            | Self::PersonProfileUpdated { event_id, .. }
            | Self::RelationshipObserved { event_id, .. }
            | Self::RelationshipUpdateProposed { event_id, .. }
            | Self::RelationshipUpdated { event_id, .. }
            | Self::BehaviorObservationCreated { event_id, .. }
            | Self::BehaviorObservationConfirmed { event_id, .. }
            | Self::BehaviorObservationRejected { event_id, .. } => event_id,
        }
    }

    pub fn timestamp(&self) -> DateTime<Utc> {
        match self {
            Self::PersonMentioned { timestamp, .. }
            | Self::PersonProfileObserved { timestamp, .. }
            | Self::PersonProfileUpdateProposed { timestamp, .. }
            | Self::PersonProfileUpdated { timestamp, .. }
            | Self::RelationshipObserved { timestamp, .. }
            | Self::RelationshipUpdateProposed { timestamp, .. }
            | Self::RelationshipUpdated { timestamp, .. }
            | Self::BehaviorObservationCreated { timestamp, .. }
            | Self::BehaviorObservationConfirmed { timestamp, .. }
            | Self::BehaviorObservationRejected { timestamp, .. } => *timestamp,
        }
    }
}

// ---------------------------------------------------------------------------
// Draft Status
// ---------------------------------------------------------------------------

/// Status of a people intelligence draft.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DraftStatus {
    /// Just created, awaiting review.
    Pending,
    /// User confirmed the draft.
    Confirmed,
    /// User edited and confirmed.
    EditedAndConfirmed,
    /// User rejected the draft.
    Rejected,
}

impl fmt::Display for DraftStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Pending => write!(f, "pending"),
            Self::Confirmed => write!(f, "confirmed"),
            Self::EditedAndConfirmed => write!(f, "edited_and_confirmed"),
            Self::Rejected => write!(f, "rejected"),
        }
    }
}

/// A draft proposal for a people intelligence action.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PeopleDraft {
    pub draft_id: String,
    pub draft_type: DraftType,
    pub status: DraftStatus,
    pub payload: serde_json::Value,
    pub created_at: DateTime<Utc>,
    pub resolved_at: Option<DateTime<Utc>>,
}

/// Type of draft.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DraftType {
    PersonProfileUpdate,
    RelationshipObservation,
    BehaviorObservation,
}

// ---------------------------------------------------------------------------
// Error
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PeopleIntelligenceError {
    #[error("draft not found: {0}")]
    DraftNotFound(String),
    #[error("draft already resolved: {0}")]
    DraftAlreadyResolved(String),
    #[error("policy blocked: {0}")]
    PolicyBlocked(String),
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use person_entity::{EvidenceSource, ProfileEvidence};
    use relationship_core::RelationshipKind;

    fn ts() -> DateTime<Utc> {
        "2026-05-25T12:00:00Z".parse().unwrap()
    }

    fn person_id() -> PersonId {
        PersonId::from("person-001")
    }

    fn make_attribute(
        sensitivity: ProfileSensitivity,
        confidence: ProfileConfidence,
    ) -> PersonAttribute {
        PersonAttribute {
            key: "reliability".to_string(),
            value: serde_json::json!("high"),
            confidence,
            sensitivity,
            evidence: ProfileEvidence {
                source: EvidenceSource::ConversationObservation,
                conversation_id: Some("conv-1".to_string()),
                message_id: Some("msg-1".to_string()),
                observed_at: ts(),
                note: None,
            },
            updated_at: ts(),
        }
    }

    fn make_observation(
        sensitivity: ProfileSensitivity,
        confidence: ProfileConfidence,
        confirmed: bool,
    ) -> RelationshipObservation {
        RelationshipObservation {
            id: "obs-1".to_string(),
            subject: PersonId::from("alice"),
            object: PersonId::from("bob"),
            kind: RelationshipKind::Colleague,
            description: "协作良好".to_string(),
            confidence,
            sensitivity,
            observed_at: ts(),
            evidence_conversation_id: Some("conv-1".to_string()),
            evidence_message_id: Some("msg-1".to_string()),
            confirmed,
        }
    }

    // ---- Policy tests ----

    #[test]
    fn policy_auto_allows_public_attributes() {
        let attr = make_attribute(ProfileSensitivity::Public, ProfileConfidence::Inferred);
        assert_eq!(evaluate_attribute_policy(&attr), PolicyDecision::AutoAllow);
    }

    #[test]
    fn policy_requires_confirmation_for_sensitive_inferred() {
        let attr = make_attribute(ProfileSensitivity::Sensitive, ProfileConfidence::Inferred);
        assert_eq!(
            evaluate_attribute_policy(&attr),
            PolicyDecision::RequireConfirmation
        );
    }

    #[test]
    fn policy_auto_allows_sensitive_confirmed() {
        let attr = make_attribute(ProfileSensitivity::Sensitive, ProfileConfidence::Confirmed);
        assert_eq!(evaluate_attribute_policy(&attr), PolicyDecision::AutoAllow);
    }

    #[test]
    fn policy_auto_allows_semi_sensitive() {
        let attr = make_attribute(
            ProfileSensitivity::SemiSensitive,
            ProfileConfidence::Observed,
        );
        assert_eq!(evaluate_attribute_policy(&attr), PolicyDecision::AutoAllow);
    }

    #[test]
    fn observation_policy_requires_confirmation_for_sensitive() {
        let obs = make_observation(
            ProfileSensitivity::Sensitive,
            ProfileConfidence::Inferred,
            false,
        );
        assert_eq!(
            evaluate_observation_policy(&obs),
            PolicyDecision::RequireConfirmation
        );
    }

    #[test]
    fn observation_policy_auto_allows_public() {
        let obs = make_observation(
            ProfileSensitivity::Public,
            ProfileConfidence::Observed,
            false,
        );
        assert_eq!(evaluate_observation_policy(&obs), PolicyDecision::AutoAllow);
    }

    // ---- PolicyDecision display ----

    #[test]
    fn policy_decision_display() {
        assert_eq!(PolicyDecision::AutoAllow.to_string(), "auto_allow");
        assert_eq!(
            PolicyDecision::RequireConfirmation.to_string(),
            "require_confirmation"
        );
    }

    // ---- MentionedPerson roundtrip ----

    #[test]
    fn mentioned_person_roundtrips() {
        let mp = MentionedPerson {
            raw_text: "张三".to_string(),
            person_id: Some(person_id()),
            context: "张三这次又拖延了项目交付".to_string(),
            confidence: ProfileConfidence::Observed,
            offset_start: 0,
            offset_end: 6,
        };
        let json = serde_json::to_string(&mp).unwrap();
        let decoded: MentionedPerson = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded.raw_text, "张三");
    }

    // ---- PeopleEvent roundtrips ----

    #[test]
    fn event_person_mentioned_roundtrips() {
        let event = PeopleEvent::PersonMentioned {
            event_id: "evt-1".to_string(),
            person: MentionedPerson {
                raw_text: "张三".to_string(),
                person_id: None,
                context: "张三来了".to_string(),
                confidence: ProfileConfidence::Observed,
                offset_start: 0,
                offset_end: 6,
            },
            conversation_id: "conv-1".to_string(),
            message_id: "msg-1".to_string(),
            timestamp: ts(),
        };
        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains("person_mentioned"));
        let decoded: PeopleEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded.event_id(), "evt-1");
    }

    #[test]
    fn event_behavior_observation_confirmed_roundtrips() {
        let event = PeopleEvent::BehaviorObservationConfirmed {
            event_id: "evt-2".to_string(),
            draft_id: "draft-1".to_string(),
            person_id: person_id(),
            final_observation: "经常拖延交付".to_string(),
            timestamp: ts(),
        };
        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains("behavior_observation_confirmed"));
        let decoded: PeopleEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded.event_id(), "evt-2");
    }

    #[test]
    fn event_behavior_observation_rejected_roundtrips() {
        let event = PeopleEvent::BehaviorObservationRejected {
            event_id: "evt-3".to_string(),
            draft_id: "draft-1".to_string(),
            person_id: person_id(),
            reason: Some("信息不准确".to_string()),
            timestamp: ts(),
        };
        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains("behavior_observation_rejected"));
        let decoded: PeopleEvent = serde_json::from_str(&json).unwrap();
        match &decoded {
            PeopleEvent::BehaviorObservationRejected { reason, .. } => {
                assert_eq!(reason.as_deref(), Some("信息不准确"));
            }
            _ => panic!("expected rejected event"),
        }
    }

    // ---- Draft types ----

    #[test]
    fn draft_status_display() {
        assert_eq!(DraftStatus::Pending.to_string(), "pending");
        assert_eq!(DraftStatus::Confirmed.to_string(), "confirmed");
        assert_eq!(DraftStatus::Rejected.to_string(), "rejected");
    }

    #[test]
    fn draft_roundtrips() {
        let draft = PeopleDraft {
            draft_id: "draft-1".to_string(),
            draft_type: DraftType::BehaviorObservation,
            status: DraftStatus::Pending,
            payload: serde_json::json!({"observation": "拖延交付"}),
            created_at: ts(),
            resolved_at: None,
        };
        let json = serde_json::to_string_pretty(&draft).unwrap();
        let decoded: PeopleDraft = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded.status, DraftStatus::Pending);
        assert!(decoded.resolved_at.is_none());
    }

    // ---- Low confidence cannot be saved as confirmed fact ----

    #[test]
    fn low_confidence_requires_confirmation() {
        let attr = make_attribute(
            ProfileSensitivity::Sensitive,
            ProfileConfidence::LowConfidence,
        );
        assert_eq!(
            evaluate_attribute_policy(&attr),
            PolicyDecision::RequireConfirmation
        );
    }
}
