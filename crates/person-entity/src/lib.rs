//! # Person Entity
//!
//! Core types for representing people in AgentOS.
//!
//! This crate defines the domain model for person profiles:
//! - Identity: PersonId, PersonName, PersonAlias
//! - Profile: PersonProfile with typed attributes
//! - Evidence: ProfileEvidence, ProfileConfidence, ProfileSensitivity
//!
//! Design principles:
//! - All profile mutations carry evidence (conversation/message link)
//! - Sensitive attributes require explicit user confirmation
//! - Confidence levels are explicit and auditable
//! - Distinguish Fact / Observation / Hypothesis / Inference

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fmt;

// ---------------------------------------------------------------------------
// Person Identity
// ---------------------------------------------------------------------------

/// Unique identifier for a person.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PersonId(pub String);

impl fmt::Display for PersonId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl From<String> for PersonId {
    fn from(s: String) -> Self {
        Self(s)
    }
}

impl From<&str> for PersonId {
    fn from(s: &str) -> Self {
        Self(s.to_string())
    }
}

/// A person's name with context.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PersonName {
    pub display_name: String,
    pub family_name: Option<String>,
    pub given_name: Option<String>,
    pub language: Option<String>,
}

impl PersonName {
    pub fn new(display_name: impl Into<String>) -> Self {
        Self {
            display_name: display_name.into(),
            family_name: None,
            given_name: None,
            language: None,
        }
    }

    pub fn with_parts(
        display_name: impl Into<String>,
        family: impl Into<String>,
        given: impl Into<String>,
    ) -> Self {
        Self {
            display_name: display_name.into(),
            family_name: Some(family.into()),
            given_name: Some(given.into()),
            language: None,
        }
    }
}

/// An alias or alternate name for a person.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PersonAlias {
    pub alias: String,
    pub context: Option<String>,
}

// ---------------------------------------------------------------------------
// Profile Evidence & Confidence
// ---------------------------------------------------------------------------

/// How confident we are in a profile attribute.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProfileConfidence {
    /// Low-confidence guess.
    LowConfidence,
    /// Inferred from patterns.
    Inferred,
    /// Observed from conversation context.
    Observed,
    /// Directly stated by user.
    Confirmed,
}

impl fmt::Display for ProfileConfidence {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Confirmed => write!(f, "confirmed"),
            Self::Observed => write!(f, "observed"),
            Self::Inferred => write!(f, "inferred"),
            Self::LowConfidence => write!(f, "low_confidence"),
        }
    }
}

/// Where the evidence came from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceSource {
    UserStated,
    ConversationObservation,
    InferredFromPattern,
    ImportedFromExternal,
}

/// Evidence linking a profile attribute to its source.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProfileEvidence {
    pub source: EvidenceSource,
    pub conversation_id: Option<String>,
    pub message_id: Option<String>,
    pub observed_at: DateTime<Utc>,
    pub note: Option<String>,
}

impl ProfileEvidence {
    pub fn user_stated(observed_at: DateTime<Utc>) -> Self {
        Self {
            source: EvidenceSource::UserStated,
            conversation_id: None,
            message_id: None,
            observed_at,
            note: None,
        }
    }

    pub fn from_conversation(
        conversation_id: impl Into<String>,
        message_id: impl Into<String>,
        observed_at: DateTime<Utc>,
    ) -> Self {
        Self {
            source: EvidenceSource::ConversationObservation,
            conversation_id: Some(conversation_id.into()),
            message_id: Some(message_id.into()),
            observed_at,
            note: None,
        }
    }
}

// ---------------------------------------------------------------------------
// Profile Sensitivity
// ---------------------------------------------------------------------------

/// How sensitive a profile attribute is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProfileSensitivity {
    /// Public information (name, company, title).
    Public,
    /// Semi-sensitive (team, project role).
    SemiSensitive,
    /// Sensitive (personality traits, reliability judgments, conflict assessments).
    Sensitive,
}

impl fmt::Display for ProfileSensitivity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Public => write!(f, "public"),
            Self::SemiSensitive => write!(f, "semi_sensitive"),
            Self::Sensitive => write!(f, "sensitive"),
        }
    }
}

// ---------------------------------------------------------------------------
// Person Attribute
// ---------------------------------------------------------------------------

/// A single attribute of a person's profile.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PersonAttribute {
    pub key: String,
    pub value: serde_json::Value,
    pub confidence: ProfileConfidence,
    pub sensitivity: ProfileSensitivity,
    pub evidence: ProfileEvidence,
    pub updated_at: DateTime<Utc>,
}

// ---------------------------------------------------------------------------
// Person Profile
// ---------------------------------------------------------------------------

/// Complete profile for a person.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PersonProfile {
    pub id: PersonId,
    pub names: Vec<PersonName>,
    pub aliases: Vec<PersonAlias>,
    pub attributes: Vec<PersonAttribute>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl PersonProfile {
    pub fn new(id: PersonId, name: PersonName, now: DateTime<Utc>) -> Self {
        Self {
            id,
            names: vec![name],
            aliases: Vec::new(),
            attributes: Vec::new(),
            created_at: now,
            updated_at: now,
        }
    }

    /// Add or update an attribute. If the key exists, replace it.
    pub fn set_attribute(&mut self, attr: PersonAttribute) {
        if let Some(existing) = self.attributes.iter_mut().find(|a| a.key == attr.key) {
            *existing = attr;
        } else {
            self.attributes.push(attr);
        }
        self.updated_at = Utc::now();
    }

    /// Get an attribute by key.
    pub fn get_attribute(&self, key: &str) -> Option<&PersonAttribute> {
        self.attributes.iter().find(|a| a.key == key)
    }

    /// Check if a sensitive mutation requires user approval.
    pub fn requires_approval(attr: &PersonAttribute) -> bool {
        matches!(attr.sensitivity, ProfileSensitivity::Sensitive)
            && !matches!(attr.confidence, ProfileConfidence::Confirmed)
    }

    /// Add an alias.
    pub fn add_alias(&mut self, alias: PersonAlias) {
        if !self.aliases.iter().any(|a| a.alias == alias.alias) {
            self.aliases.push(alias);
            self.updated_at = Utc::now();
        }
    }
}

// ---------------------------------------------------------------------------
// Error
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PersonError {
    #[error("person not found: {0}")]
    NotFound(String),
    #[error("invalid attribute: {0}")]
    InvalidAttribute(String),
    #[error("sensitive mutation requires user confirmation")]
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

    fn make_person() -> PersonProfile {
        let name = PersonName::with_parts("张三", "张", "三");
        PersonProfile::new(PersonId::from("person-001"), name, ts())
    }

    // ---- Identity roundtrips ----

    #[test]
    fn person_id_roundtrips() {
        let id = PersonId::from("person-001");
        let json = serde_json::to_string(&id).unwrap();
        let decoded: PersonId = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded, id);
        assert_eq!(id.to_string(), "person-001");
    }

    #[test]
    fn person_name_roundtrips() {
        let name = PersonName::with_parts("张三", "张", "三");
        let json = serde_json::to_string(&name).unwrap();
        let decoded: PersonName = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded.display_name, "张三");
        assert_eq!(decoded.family_name.as_deref(), Some("张"));
        assert_eq!(decoded.given_name.as_deref(), Some("三"));
    }

    #[test]
    fn person_alias_roundtrips() {
        let alias = PersonAlias {
            alias: "老张".to_string(),
            context: Some("同事称呼".to_string()),
        };
        let json = serde_json::to_string(&alias).unwrap();
        let decoded: PersonAlias = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded.alias, "老张");
    }

    // ---- Confidence & Sensitivity ----

    #[test]
    fn confidence_ordering() {
        assert!(ProfileConfidence::Confirmed > ProfileConfidence::Observed);
        assert!(ProfileConfidence::Observed > ProfileConfidence::Inferred);
        assert!(ProfileConfidence::Inferred > ProfileConfidence::LowConfidence);
    }

    #[test]
    fn confidence_serde() {
        let json = serde_json::to_string(&ProfileConfidence::Confirmed).unwrap();
        assert_eq!(json, "\"confirmed\"");
        let decoded: ProfileConfidence = serde_json::from_str("\"low_confidence\"").unwrap();
        assert_eq!(decoded, ProfileConfidence::LowConfidence);
    }

    #[test]
    fn sensitivity_serde() {
        let json = serde_json::to_string(&ProfileSensitivity::Sensitive).unwrap();
        assert_eq!(json, "\"sensitive\"");
    }

    // ---- Evidence ----

    #[test]
    fn evidence_user_stated() {
        let ev = ProfileEvidence::user_stated(ts());
        assert_eq!(ev.source, EvidenceSource::UserStated);
        assert!(ev.conversation_id.is_none());
    }

    #[test]
    fn evidence_from_conversation() {
        let ev = ProfileEvidence::from_conversation("conv-1", "msg-1", ts());
        assert_eq!(ev.source, EvidenceSource::ConversationObservation);
        assert_eq!(ev.conversation_id.as_deref(), Some("conv-1"));
        assert_eq!(ev.message_id.as_deref(), Some("msg-1"));
    }

    #[test]
    fn evidence_roundtrips() {
        let ev = ProfileEvidence::from_conversation("conv-1", "msg-1", ts());
        let json = serde_json::to_string(&ev).unwrap();
        let decoded: ProfileEvidence = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded, ev);
    }

    // ---- PersonAttribute ----

    #[test]
    fn attribute_roundtrips() {
        let attr = PersonAttribute {
            key: "role".to_string(),
            value: serde_json::json!("工程经理"),
            confidence: ProfileConfidence::Observed,
            sensitivity: ProfileSensitivity::Public,
            evidence: ProfileEvidence::from_conversation("conv-1", "msg-1", ts()),
            updated_at: ts(),
        };
        let json = serde_json::to_string_pretty(&attr).unwrap();
        let decoded: PersonAttribute = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded.key, "role");
        assert_eq!(decoded.value, serde_json::json!("工程经理"));
    }

    // ---- PersonProfile ----

    #[test]
    fn profile_roundtrips() {
        let person = make_person();
        let json = serde_json::to_string_pretty(&person).unwrap();
        let decoded: PersonProfile = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded.id, person.id);
        assert_eq!(decoded.names.len(), 1);
    }

    #[test]
    fn profile_set_attribute() {
        let mut person = make_person();
        let attr = PersonAttribute {
            key: "company".to_string(),
            value: serde_json::json!("字节跳动"),
            confidence: ProfileConfidence::Confirmed,
            sensitivity: ProfileSensitivity::Public,
            evidence: ProfileEvidence::user_stated(ts()),
            updated_at: ts(),
        };
        person.set_attribute(attr);
        assert_eq!(person.get_attribute("company").unwrap().value, "字节跳动");
    }

    #[test]
    fn profile_update_existing_attribute() {
        let mut person = make_person();
        person.set_attribute(PersonAttribute {
            key: "role".to_string(),
            value: serde_json::json!("工程师"),
            confidence: ProfileConfidence::Observed,
            sensitivity: ProfileSensitivity::Public,
            evidence: ProfileEvidence::user_stated(ts()),
            updated_at: ts(),
        });
        person.set_attribute(PersonAttribute {
            key: "role".to_string(),
            value: serde_json::json!("高级工程师"),
            confidence: ProfileConfidence::Confirmed,
            sensitivity: ProfileSensitivity::Public,
            evidence: ProfileEvidence::user_stated(ts()),
            updated_at: ts(),
        });
        assert_eq!(person.attributes.len(), 1);
        assert_eq!(person.get_attribute("role").unwrap().value, "高级工程师");
    }

    #[test]
    fn profile_requires_approval_for_sensitive() {
        let attr = PersonAttribute {
            key: "reliability".to_string(),
            value: serde_json::json!("low"),
            confidence: ProfileConfidence::Inferred,
            sensitivity: ProfileSensitivity::Sensitive,
            evidence: ProfileEvidence::from_conversation("conv-1", "msg-1", ts()),
            updated_at: ts(),
        };
        assert!(PersonProfile::requires_approval(&attr));
    }

    #[test]
    fn profile_no_approval_for_confirmed_sensitive() {
        let attr = PersonAttribute {
            key: "personality".to_string(),
            value: serde_json::json!("内向"),
            confidence: ProfileConfidence::Confirmed,
            sensitivity: ProfileSensitivity::Sensitive,
            evidence: ProfileEvidence::user_stated(ts()),
            updated_at: ts(),
        };
        assert!(!PersonProfile::requires_approval(&attr));
    }

    #[test]
    fn profile_no_approval_for_public() {
        let attr = PersonAttribute {
            key: "company".to_string(),
            value: serde_json::json!("字节跳动"),
            confidence: ProfileConfidence::Inferred,
            sensitivity: ProfileSensitivity::Public,
            evidence: ProfileEvidence::from_conversation("conv-1", "msg-1", ts()),
            updated_at: ts(),
        };
        assert!(!PersonProfile::requires_approval(&attr));
    }

    #[test]
    fn profile_add_alias_dedup() {
        let mut person = make_person();
        person.add_alias(PersonAlias {
            alias: "老张".to_string(),
            context: None,
        });
        person.add_alias(PersonAlias {
            alias: "老张".to_string(),
            context: Some("重复".to_string()),
        });
        assert_eq!(person.aliases.len(), 1);
    }

    #[test]
    fn profile_get_nonexistent_attribute() {
        let person = make_person();
        assert!(person.get_attribute("nonexistent").is_none());
    }

    #[test]
    fn profile_display_ids() {
        assert_eq!(PersonId::from("p1").to_string(), "p1");
        assert_eq!(ProfileConfidence::Observed.to_string(), "observed");
        assert_eq!(ProfileSensitivity::Sensitive.to_string(), "sensitive");
    }
}
