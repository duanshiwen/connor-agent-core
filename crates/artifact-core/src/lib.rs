//! # Artifact Core
//!
//! Core domain types for AgentOS artifacts.
//!
//! Artifacts are durable references to action outputs and external resources such
//! as web pages, extracted text, emails, documents, tool results, or knowledge
//! drafts. They allow conversations and actions to reference rich objects without
//! embedding all content directly inside message text.

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt;
use std::sync::{Arc, Mutex};

/// Unique identifier for an artifact.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ArtifactId(pub String);

impl fmt::Display for ArtifactId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl From<String> for ArtifactId {
    fn from(s: String) -> Self {
        Self(s)
    }
}

impl From<&str> for ArtifactId {
    fn from(s: &str) -> Self {
        Self(s.to_string())
    }
}

/// The broad category of artifact.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ArtifactKind {
    WebPage,
    ExtractedText,
    Email,
    Document,
    Image,
    Pdf,
    ToolResult,
    KnowledgeDraft,
    ActionResult,
}

/// Metadata describing an artifact available to the Assistant or conversation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ArtifactDescriptor {
    pub id: ArtifactId,
    pub kind: ArtifactKind,
    pub title: Option<String>,
    pub source_uri: Option<String>,
    pub mime_type: Option<String>,
    pub metadata: serde_json::Value,
    pub created_at: DateTime<Utc>,
}

impl ArtifactDescriptor {
    pub fn new(id: impl Into<ArtifactId>, kind: ArtifactKind, created_at: DateTime<Utc>) -> Self {
        Self {
            id: id.into(),
            kind,
            title: None,
            source_uri: None,
            mime_type: None,
            metadata: serde_json::json!({}),
            created_at,
        }
    }
}

/// Lightweight reference to an artifact.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArtifactRef {
    pub artifact_id: ArtifactId,
    pub kind: ArtifactKind,
    pub title: Option<String>,
}

impl From<&ArtifactDescriptor> for ArtifactRef {
    fn from(descriptor: &ArtifactDescriptor) -> Self {
        Self {
            artifact_id: descriptor.id.clone(),
            kind: descriptor.kind.clone(),
            title: descriptor.title.clone(),
        }
    }
}

/// Errors from artifact storage.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ArtifactStoreError {
    #[error("duplicate artifact id: {0}")]
    DuplicateArtifactId(ArtifactId),
    #[error("artifact store lock poisoned")]
    LockPoisoned,
    #[error("artifact storage error: {0}")]
    Storage(String),
}

/// Storage abstraction for artifact descriptors.
#[async_trait]
pub trait ArtifactStore: Send + Sync {
    async fn put(&self, descriptor: ArtifactDescriptor) -> Result<(), ArtifactStoreError>;

    async fn get(
        &self,
        artifact_id: &ArtifactId,
    ) -> Result<Option<ArtifactDescriptor>, ArtifactStoreError>;

    async fn list(&self) -> Result<Vec<ArtifactDescriptor>, ArtifactStoreError>;
}

/// Deterministic in-memory artifact store for tests and early runtime flows.
#[derive(Debug, Clone, Default)]
pub struct MemoryArtifactStore {
    artifacts: Arc<Mutex<HashMap<ArtifactId, ArtifactDescriptor>>>,
}

impl MemoryArtifactStore {
    pub fn new() -> Self {
        Self::default()
    }
}

#[async_trait]
impl ArtifactStore for MemoryArtifactStore {
    async fn put(&self, descriptor: ArtifactDescriptor) -> Result<(), ArtifactStoreError> {
        let mut artifacts = self
            .artifacts
            .lock()
            .map_err(|_| ArtifactStoreError::LockPoisoned)?;
        if artifacts.contains_key(&descriptor.id) {
            return Err(ArtifactStoreError::DuplicateArtifactId(descriptor.id));
        }
        artifacts.insert(descriptor.id.clone(), descriptor);
        Ok(())
    }

    async fn get(
        &self,
        artifact_id: &ArtifactId,
    ) -> Result<Option<ArtifactDescriptor>, ArtifactStoreError> {
        let artifacts = self
            .artifacts
            .lock()
            .map_err(|_| ArtifactStoreError::LockPoisoned)?;
        Ok(artifacts.get(artifact_id).cloned())
    }

    async fn list(&self) -> Result<Vec<ArtifactDescriptor>, ArtifactStoreError> {
        let artifacts = self
            .artifacts
            .lock()
            .map_err(|_| ArtifactStoreError::LockPoisoned)?;
        let mut descriptors = artifacts.values().cloned().collect::<Vec<_>>();
        descriptors.sort_by(|a, b| a.id.0.cmp(&b.id.0));
        Ok(descriptors)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn web_page_artifact() -> ArtifactDescriptor {
        ArtifactDescriptor {
            id: ArtifactId::from("artifact-web-1"),
            kind: ArtifactKind::WebPage,
            title: Some("Agent OS Roadmap".to_string()),
            source_uri: Some("https://example.com/agent-os".to_string()),
            mime_type: Some("text/html".to_string()),
            metadata: serde_json::json!({
                "captured_by": "browser-entity",
                "language": "en"
            }),
            created_at: "2026-05-24T12:00:00Z".parse().unwrap(),
        }
    }

    #[test]
    fn artifact_id_roundtrips() {
        let id = ArtifactId::from("artifact-1");
        assert_eq!(id.to_string(), "artifact-1");

        let json = serde_json::to_string(&id).unwrap();
        let decoded: ArtifactId = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded, id);
    }

    #[test]
    fn artifact_kind_serializes_as_snake_case() {
        let json = serde_json::to_string(&ArtifactKind::KnowledgeDraft).unwrap();
        assert_eq!(json, "\"knowledge_draft\"");

        let decoded: ArtifactKind = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded, ArtifactKind::KnowledgeDraft);
    }

    #[test]
    fn artifact_descriptor_roundtrips() {
        let descriptor = web_page_artifact();
        let json = serde_json::to_string_pretty(&descriptor).unwrap();
        let decoded: ArtifactDescriptor = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded, descriptor);
    }

    #[test]
    fn artifact_ref_roundtrips() {
        let descriptor = web_page_artifact();
        let artifact_ref = ArtifactRef::from(&descriptor);

        let json = serde_json::to_string_pretty(&artifact_ref).unwrap();
        let decoded: ArtifactRef = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded, artifact_ref);
        assert_eq!(decoded.artifact_id, descriptor.id);
    }

    #[tokio::test]
    async fn memory_artifact_store_put_get_and_list() {
        let store = MemoryArtifactStore::new();
        let descriptor = web_page_artifact();
        let id = descriptor.id.clone();

        store.put(descriptor.clone()).await.unwrap();

        assert_eq!(store.get(&id).await.unwrap(), Some(descriptor.clone()));
        assert_eq!(store.list().await.unwrap(), vec![descriptor.clone()]);

        let duplicate = store.put(descriptor.clone()).await.unwrap_err();
        assert_eq!(duplicate, ArtifactStoreError::DuplicateArtifactId(id));
    }

    #[tokio::test]
    async fn memory_artifact_store_returns_none_for_missing_artifact() {
        let store = MemoryArtifactStore::new();
        let missing = store.get(&ArtifactId::from("missing")).await.unwrap();
        assert_eq!(missing, None);
    }
}
