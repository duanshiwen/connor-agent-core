//! # Knowledge Entity
//!
//! Domain types and deterministic in-memory repository for AgentOS knowledge entries.
//!
//! This crate intentionally does not write to a real Markdown/frontmatter knowledge base.
//! It provides the pure Knowledge Entity seam that later action executors can use through
//! `ActionRuntime` and `CapabilityPolicy`.

use artifact_core::ArtifactId;
use asset_core::AssetId;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt;
use std::sync::{Arc, Mutex};

/// Unique identifier for a knowledge entry.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct KnowledgeEntryId(pub String);

impl fmt::Display for KnowledgeEntryId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl From<String> for KnowledgeEntryId {
    fn from(s: String) -> Self {
        Self(s)
    }
}

impl From<&str> for KnowledgeEntryId {
    fn from(s: &str) -> Self {
        Self(s.to_string())
    }
}

/// Lightweight reference to a saved knowledge entry.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct KnowledgeEntryRef {
    pub id: KnowledgeEntryId,
    pub title: String,
    pub source_uri: Option<String>,
    pub artifact_id: Option<ArtifactId>,
    pub asset_id: Option<AssetId>,
    pub created_at: DateTime<Utc>,
}

/// Draft content that can later be saved as a knowledge entry.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct KnowledgeEntryDraft {
    pub title: String,
    pub content_markdown: String,
    pub source_uri: Option<String>,
    pub source_artifact_id: Option<ArtifactId>,
    pub source_asset_id: Option<AssetId>,
    pub tags: Vec<String>,
    pub metadata: serde_json::Value,
    pub created_at: DateTime<Utc>,
}

impl KnowledgeEntryDraft {
    pub fn new(
        title: impl Into<String>,
        content_markdown: impl Into<String>,
        created_at: DateTime<Utc>,
    ) -> Self {
        Self {
            title: title.into(),
            content_markdown: content_markdown.into(),
            source_uri: None,
            source_artifact_id: None,
            source_asset_id: None,
            tags: vec![],
            metadata: serde_json::json!({}),
            created_at,
        }
    }

    pub fn with_source_uri(mut self, source_uri: impl Into<String>) -> Self {
        self.source_uri = Some(source_uri.into());
        self
    }

    pub fn with_source_artifact_id(mut self, artifact_id: impl Into<ArtifactId>) -> Self {
        self.source_artifact_id = Some(artifact_id.into());
        self
    }

    pub fn with_source_asset_id(mut self, asset_id: impl Into<AssetId>) -> Self {
        self.source_asset_id = Some(asset_id.into());
        self
    }

    pub fn with_tags(mut self, tags: Vec<String>) -> Self {
        self.tags = tags;
        self
    }

    pub fn with_metadata(mut self, metadata: serde_json::Value) -> Self {
        self.metadata = metadata;
        self
    }
}

/// Search query for knowledge entries.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KnowledgeSearchQuery {
    pub text: String,
    pub tags: Vec<String>,
    pub limit: usize,
}

impl KnowledgeSearchQuery {
    pub fn new(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            tags: vec![],
            limit: 10,
        }
    }

    pub fn with_tags(mut self, tags: Vec<String>) -> Self {
        self.tags = tags;
        self
    }

    pub fn with_limit(mut self, limit: usize) -> Self {
        self.limit = limit;
        self
    }
}

/// One knowledge search result.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct KnowledgeSearchResult {
    pub entry: KnowledgeEntryRef,
    pub score: f32,
    pub snippet: Option<String>,
}

/// Errors from knowledge repository operations.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum KnowledgeRepositoryError {
    #[error("knowledge repository lock poisoned")]
    LockPoisoned,
}

/// Storage abstraction for knowledge entry metadata.
#[async_trait]
pub trait KnowledgeRepository: Send + Sync {
    async fn save_draft(
        &self,
        draft: KnowledgeEntryDraft,
    ) -> Result<KnowledgeEntryRef, KnowledgeRepositoryError>;

    async fn get_entry(
        &self,
        id: &KnowledgeEntryId,
    ) -> Result<Option<KnowledgeEntryRef>, KnowledgeRepositoryError>;

    async fn search(
        &self,
        query: &KnowledgeSearchQuery,
    ) -> Result<Vec<KnowledgeSearchResult>, KnowledgeRepositoryError>;

    async fn list_entries(&self) -> Result<Vec<KnowledgeEntryRef>, KnowledgeRepositoryError>;
}

#[derive(Debug, Clone)]
struct StoredKnowledgeEntry {
    entry_ref: KnowledgeEntryRef,
    content_markdown: String,
    tags: Vec<String>,
}

/// Deterministic in-memory repository for tests and early runtime flows.
#[derive(Debug, Clone, Default)]
pub struct MemoryKnowledgeRepository {
    entries: Arc<Mutex<HashMap<KnowledgeEntryId, StoredKnowledgeEntry>>>,
}

impl MemoryKnowledgeRepository {
    pub fn new() -> Self {
        Self::default()
    }

    fn next_id(entries: &HashMap<KnowledgeEntryId, StoredKnowledgeEntry>) -> KnowledgeEntryId {
        KnowledgeEntryId::from(format!("knowledge-entry-{}", entries.len() + 1))
    }
}

#[async_trait]
impl KnowledgeRepository for MemoryKnowledgeRepository {
    async fn save_draft(
        &self,
        draft: KnowledgeEntryDraft,
    ) -> Result<KnowledgeEntryRef, KnowledgeRepositoryError> {
        let mut entries = self
            .entries
            .lock()
            .map_err(|_| KnowledgeRepositoryError::LockPoisoned)?;
        let entry_ref = KnowledgeEntryRef {
            id: Self::next_id(&entries),
            title: draft.title,
            source_uri: draft.source_uri,
            artifact_id: draft.source_artifact_id,
            asset_id: draft.source_asset_id,
            created_at: draft.created_at,
        };
        entries.insert(
            entry_ref.id.clone(),
            StoredKnowledgeEntry {
                entry_ref: entry_ref.clone(),
                content_markdown: draft.content_markdown,
                tags: draft.tags,
            },
        );
        Ok(entry_ref)
    }

    async fn get_entry(
        &self,
        id: &KnowledgeEntryId,
    ) -> Result<Option<KnowledgeEntryRef>, KnowledgeRepositoryError> {
        let entries = self
            .entries
            .lock()
            .map_err(|_| KnowledgeRepositoryError::LockPoisoned)?;
        Ok(entries.get(id).map(|entry| entry.entry_ref.clone()))
    }

    async fn search(
        &self,
        query: &KnowledgeSearchQuery,
    ) -> Result<Vec<KnowledgeSearchResult>, KnowledgeRepositoryError> {
        let entries = self
            .entries
            .lock()
            .map_err(|_| KnowledgeRepositoryError::LockPoisoned)?;
        let text = query.text.to_lowercase();
        let mut results = entries
            .values()
            .filter(|entry| {
                let matches_text = text.is_empty()
                    || entry.entry_ref.title.to_lowercase().contains(&text)
                    || entry.content_markdown.to_lowercase().contains(&text);
                let matches_tags = query
                    .tags
                    .iter()
                    .all(|tag| entry.tags.iter().any(|candidate| candidate == tag));
                matches_text && matches_tags
            })
            .map(|entry| KnowledgeSearchResult {
                entry: entry.entry_ref.clone(),
                score: if text.is_empty() { 0.0 } else { 1.0 },
                snippet: Some(entry.content_markdown.chars().take(120).collect()),
            })
            .collect::<Vec<_>>();
        results.sort_by(|a, b| a.entry.id.0.cmp(&b.entry.id.0));
        results.truncate(query.limit);
        Ok(results)
    }

    async fn list_entries(&self) -> Result<Vec<KnowledgeEntryRef>, KnowledgeRepositoryError> {
        let entries = self
            .entries
            .lock()
            .map_err(|_| KnowledgeRepositoryError::LockPoisoned)?;
        let mut entry_refs = entries
            .values()
            .map(|entry| entry.entry_ref.clone())
            .collect::<Vec<_>>();
        entry_refs.sort_by(|a, b| a.id.0.cmp(&b.id.0));
        Ok(entry_refs)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ts() -> DateTime<Utc> {
        "2026-05-24T12:00:00Z".parse().unwrap()
    }

    fn draft(title: &str, content_markdown: &str, tags: Vec<&str>) -> KnowledgeEntryDraft {
        KnowledgeEntryDraft::new(title, content_markdown, ts())
            .with_source_uri("https://example.com/source")
            .with_source_artifact_id("artifact-source-1")
            .with_source_asset_id("asset-source-1")
            .with_tags(tags.into_iter().map(str::to_string).collect())
            .with_metadata(serde_json::json!({ "source": "test" }))
    }

    #[test]
    fn knowledge_entry_id_roundtrips() {
        let id = KnowledgeEntryId::from("knowledge-entry-1");
        assert_eq!(id.to_string(), "knowledge-entry-1");

        let json = serde_json::to_string(&id).unwrap();
        let decoded: KnowledgeEntryId = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded, id);
    }

    #[test]
    fn knowledge_entry_ref_roundtrips() {
        let entry_ref = KnowledgeEntryRef {
            id: KnowledgeEntryId::from("knowledge-entry-1"),
            title: "AgentOS Notes".to_string(),
            source_uri: Some("https://example.com/source".to_string()),
            artifact_id: Some(ArtifactId::from("artifact-source-1")),
            asset_id: Some(AssetId::from("asset-source-1")),
            created_at: ts(),
        };

        let json = serde_json::to_string_pretty(&entry_ref).unwrap();
        let decoded: KnowledgeEntryRef = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded, entry_ref);
    }

    #[test]
    fn knowledge_entry_draft_roundtrips() {
        let draft = draft(
            "AgentOS Notes",
            "# AgentOS\n\nFoundation notes",
            vec!["agent-os"],
        );

        let json = serde_json::to_string_pretty(&draft).unwrap();
        let decoded: KnowledgeEntryDraft = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded, draft);
    }

    #[test]
    fn knowledge_search_query_roundtrips() {
        let query = KnowledgeSearchQuery::new("agentos")
            .with_tags(vec!["agent-os".to_string()])
            .with_limit(5);

        let json = serde_json::to_string_pretty(&query).unwrap();
        let decoded: KnowledgeSearchQuery = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded, query);
    }

    #[test]
    fn knowledge_search_result_roundtrips() {
        let result = KnowledgeSearchResult {
            entry: KnowledgeEntryRef {
                id: KnowledgeEntryId::from("knowledge-entry-1"),
                title: "AgentOS Notes".to_string(),
                source_uri: None,
                artifact_id: None,
                asset_id: None,
                created_at: ts(),
            },
            score: 1.0,
            snippet: Some("Foundation notes".to_string()),
        };

        let json = serde_json::to_string_pretty(&result).unwrap();
        let decoded: KnowledgeSearchResult = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded, result);
    }

    #[tokio::test]
    async fn memory_repository_saves_and_gets_entry() {
        let repository = MemoryKnowledgeRepository::new();
        let saved = repository
            .save_draft(draft("AgentOS Notes", "# AgentOS", vec!["agent-os"]))
            .await
            .unwrap();

        assert_eq!(saved.id, KnowledgeEntryId::from("knowledge-entry-1"));
        assert_eq!(saved.title, "AgentOS Notes");
        assert_eq!(
            repository.get_entry(&saved.id).await.unwrap(),
            Some(saved.clone())
        );
    }

    #[tokio::test]
    async fn memory_repository_lists_entries_sorted_by_id() {
        let repository = MemoryKnowledgeRepository::new();
        let first = repository
            .save_draft(draft("First", "first content", vec![]))
            .await
            .unwrap();
        let second = repository
            .save_draft(draft("Second", "second content", vec![]))
            .await
            .unwrap();

        assert_eq!(
            repository.list_entries().await.unwrap(),
            vec![first, second]
        );
    }

    #[tokio::test]
    async fn memory_repository_searches_by_title() {
        let repository = MemoryKnowledgeRepository::new();
        repository
            .save_draft(draft(
                "AgentOS Notes",
                "foundation content",
                vec!["agent-os"],
            ))
            .await
            .unwrap();
        repository
            .save_draft(draft("Browser Notes", "browser content", vec!["browser"]))
            .await
            .unwrap();

        let results = repository
            .search(&KnowledgeSearchQuery::new("agentos"))
            .await
            .unwrap();

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].entry.title, "AgentOS Notes");
    }

    #[tokio::test]
    async fn memory_repository_searches_by_content() {
        let repository = MemoryKnowledgeRepository::new();
        repository
            .save_draft(draft(
                "Architecture",
                "contains event sourcing notes",
                vec!["architecture"],
            ))
            .await
            .unwrap();
        repository
            .save_draft(draft(
                "Browser",
                "contains page extraction notes",
                vec!["browser"],
            ))
            .await
            .unwrap();

        let results = repository
            .search(&KnowledgeSearchQuery::new("event sourcing"))
            .await
            .unwrap();

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].entry.title, "Architecture");
    }

    #[tokio::test]
    async fn memory_repository_returns_none_for_missing_entry() {
        let repository = MemoryKnowledgeRepository::new();

        assert_eq!(
            repository
                .get_entry(&KnowledgeEntryId::from("missing"))
                .await
                .unwrap(),
            None
        );
    }
}
