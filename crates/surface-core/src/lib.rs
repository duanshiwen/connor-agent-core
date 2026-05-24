//! # Surface Core
//!
//! Core domain types for AgentOS surfaces.
//!
//! Surfaces are renderable, attachable views over artifacts and conversation content.
//! They represent UI surfaces such as web views, mail views, document previews,
//! image viewers, knowledge entry editors, approval dialogs, and audit timelines.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fmt;

/// Unique identifier for a surface.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SurfaceId(pub String);

impl fmt::Display for SurfaceId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl From<String> for SurfaceId {
    fn from(s: String) -> Self {
        Self(s)
    }
}

impl From<&str> for SurfaceId {
    fn from(s: &str) -> Self {
        Self(s.to_string())
    }
}

/// The broad category of surface.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SurfaceKind {
    WebSurface,
    MailSurface,
    DocumentSurface,
    ImageSurface,
    KnowledgeEntrySurface,
    ApprovalSurface,
    AuditSurface,
}

/// The lifecycle status of a surface.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SurfaceLifecycleStatus {
    Attached,
    Updated,
    Closed,
}

/// Hint for how a surface should be rendered.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SurfaceRendererHint {
    Markdown,
    Html,
    PlainText,
    Image,
    Pdf,
    Table,
    Form,
    Timeline,
    Custom(String),
}

/// Metadata describing a surface available to the Assistant or conversation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SurfaceDescriptor {
    pub id: SurfaceId,
    pub kind: SurfaceKind,
    pub title: Option<String>,
    pub renderer_hint: SurfaceRendererHint,
    pub artifact_id: Option<artifact_core::ArtifactId>,
    pub metadata: serde_json::Value,
    pub created_at: DateTime<Utc>,
}

impl SurfaceDescriptor {
    pub fn new(
        id: impl Into<SurfaceId>,
        kind: SurfaceKind,
        renderer_hint: SurfaceRendererHint,
        created_at: DateTime<Utc>,
    ) -> Self {
        Self {
            id: id.into(),
            kind,
            title: None,
            renderer_hint,
            artifact_id: None,
            metadata: serde_json::json!({}),
            created_at,
        }
    }

    pub fn with_title(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }

    pub fn with_artifact_id(mut self, artifact_id: artifact_core::ArtifactId) -> Self {
        self.artifact_id = Some(artifact_id);
        self
    }

    pub fn with_metadata(mut self, metadata: serde_json::Value) -> Self {
        self.metadata = metadata;
        self
    }
}

/// The current state of a surface, including its lifecycle status and last update time.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SurfaceState {
    pub descriptor: SurfaceDescriptor,
    pub status: SurfaceLifecycleStatus,
    pub updated_at: DateTime<Utc>,
}

impl SurfaceState {
    pub fn attached(descriptor: SurfaceDescriptor, created_at: DateTime<Utc>) -> Self {
        Self {
            descriptor,
            status: SurfaceLifecycleStatus::Attached,
            updated_at: created_at,
        }
    }
}

/// Errors from surface operations.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SurfaceRegistryError {
    #[error("duplicate surface id: {0}")]
    DuplicateSurfaceId(SurfaceId),
}

/// In-memory surface registry for tests and early runtime flows.
#[derive(Debug, Clone, Default)]
pub struct SurfaceRegistry {
    surfaces: std::collections::HashMap<SurfaceId, SurfaceState>,
}

impl SurfaceRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(&mut self, state: SurfaceState) -> Result<(), SurfaceRegistryError> {
        if self.surfaces.contains_key(&state.descriptor.id) {
            return Err(SurfaceRegistryError::DuplicateSurfaceId(
                state.descriptor.id.clone(),
            ));
        }
        self.surfaces.insert(state.descriptor.id.clone(), state);
        Ok(())
    }

    pub fn get(&self, id: &SurfaceId) -> Option<&SurfaceState> {
        self.surfaces.get(id)
    }

    pub fn contains(&self, id: &SurfaceId) -> bool {
        self.surfaces.contains_key(id)
    }

    pub fn len(&self) -> usize {
        self.surfaces.len()
    }

    pub fn is_empty(&self) -> bool {
        self.surfaces.is_empty()
    }

    pub fn iter(&self) -> impl Iterator<Item = &SurfaceState> {
        self.surfaces.values()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn web_surface_descriptor() -> SurfaceDescriptor {
        SurfaceDescriptor::new(
            "surface-web-1",
            SurfaceKind::WebSurface,
            SurfaceRendererHint::Html,
            "2026-05-24T12:00:00Z".parse().unwrap(),
        )
        .with_title("Example Page")
        .with_artifact_id(artifact_core::ArtifactId::from("artifact-web-1"))
    }

    fn knowledge_surface_descriptor() -> SurfaceDescriptor {
        SurfaceDescriptor::new(
            "surface-knowledge-1",
            SurfaceKind::KnowledgeEntrySurface,
            SurfaceRendererHint::Markdown,
            "2026-05-24T12:00:00Z".parse().unwrap(),
        )
        .with_title("Knowledge Entry")
    }

    #[test]
    fn surface_id_roundtrips() {
        let id = SurfaceId::from("surface-1");
        assert_eq!(id.to_string(), "surface-1");

        let json = serde_json::to_string(&id).unwrap();
        let decoded: SurfaceId = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded, id);
    }

    #[test]
    fn surface_kind_serializes_as_snake_case() {
        let json = serde_json::to_string(&SurfaceKind::KnowledgeEntrySurface).unwrap();
        assert_eq!(json, "\"knowledge_entry_surface\"");

        let decoded: SurfaceKind = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded, SurfaceKind::KnowledgeEntrySurface);
    }

    #[test]
    fn surface_lifecycle_status_serializes_as_snake_case() {
        let json = serde_json::to_string(&SurfaceLifecycleStatus::Closed).unwrap();
        assert_eq!(json, "\"closed\"");

        let decoded: SurfaceLifecycleStatus = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded, SurfaceLifecycleStatus::Closed);
    }

    #[test]
    fn surface_renderer_hint_roundtrips() {
        let hints = vec![
            SurfaceRendererHint::Markdown,
            SurfaceRendererHint::Html,
            SurfaceRendererHint::PlainText,
            SurfaceRendererHint::Image,
            SurfaceRendererHint::Pdf,
            SurfaceRendererHint::Table,
            SurfaceRendererHint::Form,
            SurfaceRendererHint::Timeline,
            SurfaceRendererHint::Custom("custom_layout".to_string()),
        ];

        for hint in hints {
            let json = serde_json::to_string(&hint).unwrap();
            let decoded: SurfaceRendererHint = serde_json::from_str(&json).unwrap();
            assert_eq!(decoded, hint);
        }
    }

    #[test]
    fn surface_descriptor_roundtrips() {
        let descriptor = web_surface_descriptor();
        let json = serde_json::to_string_pretty(&descriptor).unwrap();
        let decoded: SurfaceDescriptor = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded, descriptor);
    }

    #[test]
    fn surface_state_roundtrips() {
        let descriptor = knowledge_surface_descriptor();
        let created_at: DateTime<Utc> = "2026-05-24T12:00:00Z".parse().unwrap();
        let state = SurfaceState::attached(descriptor, created_at);

        let json = serde_json::to_string_pretty(&state).unwrap();
        let decoded: SurfaceState = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded, state);
    }

    #[test]
    fn surface_descriptor_can_reference_artifact_id() {
        let descriptor = web_surface_descriptor();
        assert!(descriptor.artifact_id.is_some());
        assert_eq!(
            descriptor.artifact_id.unwrap(),
            artifact_core::ArtifactId::from("artifact-web-1")
        );
    }

    #[test]
    fn surface_descriptor_without_artifact_id_defaults_to_none() {
        let descriptor = knowledge_surface_descriptor();
        assert!(descriptor.artifact_id.is_none());
    }

    #[test]
    fn surface_registry_register_and_get() {
        let mut registry = SurfaceRegistry::new();
        let descriptor = web_surface_descriptor();
        let created_at: DateTime<Utc> = "2026-05-24T12:00:00Z".parse().unwrap();
        let state = SurfaceState::attached(descriptor, created_at);

        registry.register(state.clone()).unwrap();

        assert_eq!(registry.len(), 1);
        assert!(registry.contains(&SurfaceId::from("surface-web-1")));
        assert_eq!(
            registry.get(&SurfaceId::from("surface-web-1")),
            Some(&state)
        );
    }

    #[test]
    fn surface_registry_duplicate_id_fails() {
        let mut registry = SurfaceRegistry::new();
        let descriptor = web_surface_descriptor();
        let created_at: DateTime<Utc> = "2026-05-24T12:00:00Z".parse().unwrap();
        let state = SurfaceState::attached(descriptor, created_at);

        registry.register(state.clone()).unwrap();

        let err = registry.register(state).unwrap_err();
        assert_eq!(
            err,
            SurfaceRegistryError::DuplicateSurfaceId(SurfaceId::from("surface-web-1"))
        );
    }

    #[test]
    fn surface_registry_returns_none_for_missing_surface() {
        let registry = SurfaceRegistry::new();
        assert_eq!(registry.get(&SurfaceId::from("missing")), None);
    }

    #[test]
    fn surface_state_is_attached_lifecycle() {
        let descriptor = knowledge_surface_descriptor();
        let created_at: DateTime<Utc> = "2026-05-24T12:00:00Z".parse().unwrap();
        let state = SurfaceState::attached(descriptor, created_at);

        assert_eq!(state.status, SurfaceLifecycleStatus::Attached);
        assert_eq!(state.updated_at, created_at);
    }
}
