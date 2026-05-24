//! # Entity Core
//!
//! Core domain types for AgentOS linked entities.
//!
//! Entities represent Assistant-accessible resources such as Browser, Mail,
//! Knowledge, Reminder, Device, Plugin, or external services. They are not
//! foreground conversation participants by default; conversations can link to
//! entities through `LinkedEntityBinding` metadata.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt;

/// Unique identifier for an Assistant-accessible entity.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct EntityId(pub String);

impl fmt::Display for EntityId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl From<String> for EntityId {
    fn from(s: String) -> Self {
        Self(s)
    }
}

impl From<&str> for EntityId {
    fn from(s: &str) -> Self {
        Self(s.to_string())
    }
}

/// The broad category of entity.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EntityKind {
    Browser,
    Mail,
    Knowledge,
    Reminder,
    Person,
    Device,
    Plugin,
    ExternalService,
    System,
}

/// A named capability exposed by an entity.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct EntityCapability {
    pub name: String,
    pub description: Option<String>,
}

impl EntityCapability {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            description: None,
        }
    }

    pub fn with_description(name: impl Into<String>, description: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            description: Some(description.into()),
        }
    }
}

/// Metadata describing an entity available to the Assistant.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EntityDescriptor {
    pub id: EntityId,
    pub kind: EntityKind,
    pub display_name: String,
    pub capabilities: Vec<EntityCapability>,
    pub default_policy_ref: Option<String>,
}

impl EntityDescriptor {
    pub fn new(id: impl Into<EntityId>, kind: EntityKind, display_name: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            kind,
            display_name: display_name.into(),
            capabilities: vec![],
            default_policy_ref: None,
        }
    }
}

/// Why an entity was linked to a conversation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LinkReason {
    UserRequested,
    AssistantSuggested,
    SystemDefault,
    ContextDetected,
}

/// Metadata binding an entity to a conversation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LinkedEntityBinding {
    pub conversation_id: String,
    pub entity_id: EntityId,
    pub reason: LinkReason,
    pub linked_at: DateTime<Utc>,
}

/// In-memory entity registry.
///
/// This is intentionally small and deterministic. Persistence and capability
/// execution belong in later crates.
#[derive(Debug, Clone, Default)]
pub struct EntityRegistry {
    entities: HashMap<EntityId, EntityDescriptor>,
}

impl EntityRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(&mut self, descriptor: EntityDescriptor) -> Result<(), EntityRegistryError> {
        if self.entities.contains_key(&descriptor.id) {
            return Err(EntityRegistryError::DuplicateEntityId(descriptor.id));
        }
        self.entities.insert(descriptor.id.clone(), descriptor);
        Ok(())
    }

    pub fn get(&self, id: &EntityId) -> Option<&EntityDescriptor> {
        self.entities.get(id)
    }

    pub fn contains(&self, id: &EntityId) -> bool {
        self.entities.contains_key(id)
    }

    pub fn len(&self) -> usize {
        self.entities.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entities.is_empty()
    }

    pub fn iter(&self) -> impl Iterator<Item = &EntityDescriptor> {
        self.entities.values()
    }
}

/// Registry errors.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum EntityRegistryError {
    #[error("duplicate entity id: {0}")]
    DuplicateEntityId(EntityId),
}

#[cfg(test)]
mod tests {
    use super::*;

    fn browser_descriptor() -> EntityDescriptor {
        EntityDescriptor {
            id: EntityId::from("browser-main"),
            kind: EntityKind::Browser,
            display_name: "Browser".to_string(),
            capabilities: vec![
                EntityCapability::with_description("open_url", "Open a URL in the browser"),
                EntityCapability::new("read_page"),
            ],
            default_policy_ref: Some("policy/browser/default".to_string()),
        }
    }

    #[test]
    fn entity_id_display_and_serde_roundtrip() {
        let id = EntityId::from("knowledge-main");
        assert_eq!(id.to_string(), "knowledge-main");

        let json = serde_json::to_string(&id).unwrap();
        let decoded: EntityId = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded, id);
    }

    #[test]
    fn entity_kind_serde_snake_case() {
        let json = serde_json::to_string(&EntityKind::ExternalService).unwrap();
        assert_eq!(json, "\"external_service\"");

        let decoded: EntityKind = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded, EntityKind::ExternalService);
    }

    #[test]
    fn entity_descriptor_serde_roundtrip() {
        let descriptor = browser_descriptor();
        let json = serde_json::to_string_pretty(&descriptor).unwrap();
        let decoded: EntityDescriptor = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded, descriptor);
    }

    #[test]
    fn registry_can_register_and_lookup_entity() {
        let descriptor = browser_descriptor();
        let id = descriptor.id.clone();
        let mut registry = EntityRegistry::new();

        registry.register(descriptor.clone()).unwrap();

        assert_eq!(registry.len(), 1);
        assert!(registry.contains(&id));
        assert_eq!(registry.get(&id), Some(&descriptor));
    }

    #[test]
    fn registry_rejects_duplicate_entity_id() {
        let descriptor = browser_descriptor();
        let mut registry = EntityRegistry::new();

        registry.register(descriptor.clone()).unwrap();
        let result = registry.register(descriptor.clone());

        assert_eq!(
            result.unwrap_err(),
            EntityRegistryError::DuplicateEntityId(descriptor.id)
        );
    }

    #[test]
    fn linked_entity_binding_serde_roundtrip() {
        let binding = LinkedEntityBinding {
            conversation_id: "conv-001".to_string(),
            entity_id: EntityId::from("browser-main"),
            reason: LinkReason::UserRequested,
            linked_at: "2026-05-24T11:00:00Z".parse().unwrap(),
        };

        let json = serde_json::to_string_pretty(&binding).unwrap();
        let decoded: LinkedEntityBinding = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded, binding);
    }
}
