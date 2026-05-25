//! # Relationship Core
//!
//! Types for modeling relationships between people in AgentOS.
//!
//! This crate defines the domain model for relationships:
//! - Relationship identity and classification
//! - Observations about relationship dynamics
//! - Evidence linking observations to their sources
//!
//! Design principles:
//! - All observations carry evidence and confidence
//! - Sensitive observations (conflict, reliability) require user confirmation
//! - Relationship strength is a derived concept, not directly stored
//! - Distinguish Fact / Observation / Hypothesis

use chrono::{DateTime, Utc};
use person_entity::{PersonId, ProfileConfidence, ProfileSensitivity};
use serde::{Deserialize, Serialize};
use std::fmt;

// ---------------------------------------------------------------------------
// Relationship Identity
// ---------------------------------------------------------------------------

/// Unique identifier for a relationship.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct RelationshipId(pub String);

impl fmt::Display for RelationshipId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl From<String> for RelationshipId {
    fn from(s: String) -> Self {
        Self(s)
    }
}

impl From<&str> for RelationshipId {
    fn from(s: &str) -> Self {
        Self(s.to_string())
    }
}

// ---------------------------------------------------------------------------
// Relationship Kind
// ---------------------------------------------------------------------------

/// Classification of relationship types.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RelationshipKind {
    /// Same team or organization.
    Colleague,
    /// Direct report / manager.
    ReportsTo,
    /// Manager of.
    Manages,
    /// Cross-team collaborator.
    CrossTeamCollaborator,
    /// External contact (vendor, client).
    ExternalContact,
    /// Personal relationship.
    Personal,
    /// Mentorship.
    Mentor,
    /// Mentee.
    Mentee,
    /// Unknown or unclassified.
    Other(String),
}

impl fmt::Display for RelationshipKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Colleague => write!(f, "colleague"),
            Self::ReportsTo => write!(f, "reports_to"),
            Self::Manages => write!(f, "manages"),
            Self::CrossTeamCollaborator => write!(f, "cross_team_collaborator"),
            Self::ExternalContact => write!(f, "external_contact"),
            Self::Personal => write!(f, "personal"),
            Self::Mentor => write!(f, "mentor"),
            Self::Mentee => write!(f, "mentee"),
            Self::Other(s) => write!(f, "other:{}", s),
        }
    }
}

// ---------------------------------------------------------------------------
// Relationship Observation
// ---------------------------------------------------------------------------

/// An observation about a relationship dynamic.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RelationshipObservation {
    pub id: String,
    pub subject: PersonId,
    pub object: PersonId,
    pub kind: RelationshipKind,
    pub description: String,
    pub confidence: ProfileConfidence,
    pub sensitivity: ProfileSensitivity,
    pub observed_at: DateTime<Utc>,
    pub evidence_conversation_id: Option<String>,
    pub evidence_message_id: Option<String>,
    pub confirmed: bool,
}

impl RelationshipObservation {
    /// Check if this observation requires user confirmation before saving.
    pub fn requires_confirmation(&self) -> bool {
        matches!(self.sensitivity, ProfileSensitivity::Sensitive)
            && !self.confirmed
            && !matches!(self.confidence, ProfileConfidence::Confirmed)
    }
}

// ---------------------------------------------------------------------------
// Relationship Profile
// ---------------------------------------------------------------------------

/// A relationship between two people.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RelationshipProfile {
    pub id: RelationshipId,
    pub subject: PersonId,
    pub object: PersonId,
    pub kinds: Vec<RelationshipKind>,
    pub observations: Vec<RelationshipObservation>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl RelationshipProfile {
    pub fn new(
        id: RelationshipId,
        subject: PersonId,
        object: PersonId,
        now: DateTime<Utc>,
    ) -> Self {
        Self {
            id,
            subject,
            object,
            kinds: Vec::new(),
            observations: Vec::new(),
            created_at: now,
            updated_at: now,
        }
    }

    /// Add a kind if not already present.
    pub fn add_kind(&mut self, kind: RelationshipKind) {
        if !self.kinds.contains(&kind) {
            self.kinds.push(kind);
            self.updated_at = Utc::now();
        }
    }

    /// Add an observation.
    pub fn add_observation(&mut self, obs: RelationshipObservation) {
        self.observations.push(obs);
        self.updated_at = Utc::now();
    }

    /// Confirm an observation by id.
    pub fn confirm_observation(&mut self, obs_id: &str) -> bool {
        if let Some(obs) = self.observations.iter_mut().find(|o| o.id == obs_id) {
            obs.confirmed = true;
            self.updated_at = Utc::now();
            true
        } else {
            false
        }
    }

    /// Get unconfirmed sensitive observations.
    pub fn pending_confirmations(&self) -> Vec<&RelationshipObservation> {
        self.observations
            .iter()
            .filter(|o| o.requires_confirmation())
            .collect()
    }
}

// ---------------------------------------------------------------------------
// Error
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RelationshipError {
    #[error("relationship not found: {0}")]
    NotFound(String),
    #[error("observation not found: {0}")]
    ObservationNotFound(String),
    #[error("sensitive observation requires confirmation")]
    RequiresConfirmation,
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn ts() -> DateTime<Utc> {
        "2026-05-25T12:00:00Z".parse().unwrap()
    }

    fn person_a() -> PersonId {
        PersonId::from("person-alice")
    }

    fn person_b() -> PersonId {
        PersonId::from("person-bob")
    }

    fn make_observation(id: &str, confirmed: bool) -> RelationshipObservation {
        RelationshipObservation {
            id: id.to_string(),
            subject: person_a(),
            object: person_b(),
            kind: RelationshipKind::Colleague,
            description: "经常一起讨论技术方案".to_string(),
            confidence: ProfileConfidence::Observed,
            sensitivity: ProfileSensitivity::SemiSensitive,
            observed_at: ts(),
            evidence_conversation_id: Some("conv-1".to_string()),
            evidence_message_id: Some("msg-1".to_string()),
            confirmed,
        }
    }

    // ---- Type roundtrips ----

    #[test]
    fn relationship_id_roundtrips() {
        let id = RelationshipId::from("rel-001");
        let json = serde_json::to_string(&id).unwrap();
        let decoded: RelationshipId = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded, id);
        assert_eq!(id.to_string(), "rel-001");
    }

    #[test]
    fn relationship_kind_serde() {
        let kind = RelationshipKind::Colleague;
        assert_eq!(serde_json::to_string(&kind).unwrap(), "\"colleague\"");
        let decoded: RelationshipKind = serde_json::from_str("\"reports_to\"").unwrap();
        assert_eq!(decoded, RelationshipKind::ReportsTo);
    }

    #[test]
    fn relationship_kind_other() {
        let kind = RelationshipKind::Other("供应商".to_string());
        let json = serde_json::to_string(&kind).unwrap();
        assert!(json.contains("供应商"));
        let decoded: RelationshipKind = serde_json::from_str(&json).unwrap();
        match decoded {
            RelationshipKind::Other(s) => assert_eq!(s, "供应商"),
            _ => panic!("expected Other"),
        }
    }

    #[test]
    fn observation_roundtrips() {
        let obs = make_observation("obs-1", false);
        let json = serde_json::to_string_pretty(&obs).unwrap();
        let decoded: RelationshipObservation = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded.id, "obs-1");
        assert!(!decoded.confirmed);
    }

    #[test]
    fn profile_roundtrips() {
        let profile =
            RelationshipProfile::new(RelationshipId::from("rel-1"), person_a(), person_b(), ts());
        let json = serde_json::to_string_pretty(&profile).unwrap();
        let decoded: RelationshipProfile = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded.id, RelationshipId::from("rel-1"));
    }

    // ---- Profile operations ----

    #[test]
    fn profile_add_kind_dedup() {
        let mut profile =
            RelationshipProfile::new(RelationshipId::from("rel-1"), person_a(), person_b(), ts());
        profile.add_kind(RelationshipKind::Colleague);
        profile.add_kind(RelationshipKind::Colleague);
        assert_eq!(profile.kinds.len(), 1);
    }

    #[test]
    fn profile_add_multiple_kinds() {
        let mut profile =
            RelationshipProfile::new(RelationshipId::from("rel-1"), person_a(), person_b(), ts());
        profile.add_kind(RelationshipKind::Colleague);
        profile.add_kind(RelationshipKind::Mentor);
        assert_eq!(profile.kinds.len(), 2);
    }

    #[test]
    fn profile_add_observation() {
        let mut profile =
            RelationshipProfile::new(RelationshipId::from("rel-1"), person_a(), person_b(), ts());
        profile.add_observation(make_observation("obs-1", false));
        assert_eq!(profile.observations.len(), 1);
    }

    #[test]
    fn profile_confirm_observation() {
        let mut profile =
            RelationshipProfile::new(RelationshipId::from("rel-1"), person_a(), person_b(), ts());
        profile.add_observation(make_observation("obs-1", false));
        assert!(profile.confirm_observation("obs-1"));
        assert!(profile.observations[0].confirmed);
    }

    #[test]
    fn profile_confirm_nonexistent_observation() {
        let mut profile =
            RelationshipProfile::new(RelationshipId::from("rel-1"), person_a(), person_b(), ts());
        assert!(!profile.confirm_observation("nonexistent"));
    }

    // ---- Confirmation logic ----

    #[test]
    fn sensitive_observation_requires_confirmation() {
        let obs = RelationshipObservation {
            sensitivity: ProfileSensitivity::Sensitive,
            confidence: ProfileConfidence::Inferred,
            confirmed: false,
            ..make_observation("obs-1", false)
        };
        assert!(obs.requires_confirmation());
    }

    #[test]
    fn confirmed_observation_does_not_require_confirmation() {
        let obs = RelationshipObservation {
            sensitivity: ProfileSensitivity::Sensitive,
            confidence: ProfileConfidence::Inferred,
            confirmed: true,
            ..make_observation("obs-1", true)
        };
        assert!(!obs.requires_confirmation());
    }

    #[test]
    fn public_observation_does_not_require_confirmation() {
        let obs = RelationshipObservation {
            sensitivity: ProfileSensitivity::Public,
            confidence: ProfileConfidence::Inferred,
            confirmed: false,
            ..make_observation("obs-1", false)
        };
        assert!(!obs.requires_confirmation());
    }

    #[test]
    fn pending_confirmations_filters_correctly() {
        let mut profile =
            RelationshipProfile::new(RelationshipId::from("rel-1"), person_a(), person_b(), ts());

        // Add sensitive unconfirmed observation
        profile.add_observation(RelationshipObservation {
            sensitivity: ProfileSensitivity::Sensitive,
            confidence: ProfileConfidence::Inferred,
            confirmed: false,
            ..make_observation("obs-sensitive", false)
        });

        // Add public observation
        profile.add_observation(RelationshipObservation {
            sensitivity: ProfileSensitivity::Public,
            confidence: ProfileConfidence::Observed,
            confirmed: false,
            ..make_observation("obs-public", false)
        });

        // Add sensitive but confirmed observation
        profile.add_observation(RelationshipObservation {
            sensitivity: ProfileSensitivity::Sensitive,
            confidence: ProfileConfidence::Inferred,
            confirmed: true,
            ..make_observation("obs-confirmed", true)
        });

        let pending = profile.pending_confirmations();
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].id, "obs-sensitive");
    }

    // ---- Display ----

    #[test]
    fn relationship_kind_display() {
        assert_eq!(RelationshipKind::Colleague.to_string(), "colleague");
        assert_eq!(RelationshipKind::ReportsTo.to_string(), "reports_to");
        assert_eq!(
            RelationshipKind::Other("test".to_string()).to_string(),
            "other:test"
        );
    }
}
