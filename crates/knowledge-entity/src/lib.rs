//! # Knowledge Entity
//!
//! Domain types, deterministic in-memory repository, and action-level seams for
//! AgentOS knowledge entries.
//!
//! This crate provides pure Knowledge Entity abstractions that later action/runtime
//! integrations can use through `ActionRuntime` and `CapabilityPolicy`.
//!
//! It includes both a `MemoryKnowledgeRepository` for tests and a
//! `MarkdownKnowledgeRepository` for real filesystem-backed knowledge bases.

pub mod markdown_repo;

use action_core::{
    ActionExecutor, ActionExecutorError, ActionKind, ActionRegistry, ActionRegistryError,
    ActionRequest, ActionResult, ActionResultPayload, ActionSchema, ActionStatus, SideEffectKind,
};
use artifact_core::ArtifactId;
use asset_core::{AssetId, WorkObjectId};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use enterprise_permission_core::{
    EnterpriseUserId, PermissionAction, PermissionStore, ResourceType,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt;
use std::sync::{Arc, Mutex};

pub const KNOWLEDGE_SEARCH_ACTION_KIND: &str = "knowledge.search";
pub const KNOWLEDGE_GET_ENTRY_ACTION_KIND: &str = "knowledge.get_entry";
pub const KNOWLEDGE_CREATE_DRAFT_ACTION_KIND: &str = "knowledge.create_draft";
pub const KNOWLEDGE_SAVE_ENTRY_ACTION_KIND: &str = "knowledge.save_entry";
pub const KNOWLEDGE_UPDATE_ENTRY_ACTION_KIND: &str = "knowledge.update_entry";

pub fn knowledge_search_action_kind() -> ActionKind {
    ActionKind::from(KNOWLEDGE_SEARCH_ACTION_KIND)
}

pub fn knowledge_get_entry_action_kind() -> ActionKind {
    ActionKind::from(KNOWLEDGE_GET_ENTRY_ACTION_KIND)
}

pub fn knowledge_create_draft_action_kind() -> ActionKind {
    ActionKind::from(KNOWLEDGE_CREATE_DRAFT_ACTION_KIND)
}

pub fn knowledge_save_entry_action_kind() -> ActionKind {
    ActionKind::from(KNOWLEDGE_SAVE_ENTRY_ACTION_KIND)
}

pub fn knowledge_update_entry_action_kind() -> ActionKind {
    ActionKind::from(KNOWLEDGE_UPDATE_ENTRY_ACTION_KIND)
}

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

    pub fn validate_for_write(&self) -> Result<(), KnowledgeValidationError> {
        validate_draft_fields(&self.title, &self.content_markdown)
    }
}

/// Partial update for a knowledge entry.
/// `None` fields保留原值，`Some` 字段更新为新值。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct KnowledgeEntryUpdate {
    pub title: Option<String>,
    pub content_markdown: Option<String>,
    pub tags: Option<Vec<String>>,
    pub source_uri: Option<String>,
    pub metadata: Option<serde_json::Value>,
}

impl KnowledgeEntryUpdate {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_title(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }

    pub fn with_content(mut self, content: impl Into<String>) -> Self {
        self.content_markdown = Some(content.into());
        self
    }

    pub fn with_tags(mut self, tags: Vec<String>) -> Self {
        self.tags = Some(tags);
        self
    }

    pub fn with_source_uri(mut self, uri: impl Into<String>) -> Self {
        self.source_uri = Some(uri.into());
        self
    }

    pub fn with_metadata(mut self, metadata: serde_json::Value) -> Self {
        self.metadata = Some(metadata);
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
    pub permission_required: bool,
    pub confidentiality: Option<String>,
}

/// Full-text query boundary for knowledge indexes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KnowledgeFullTextQuery {
    pub text: String,
    pub tags: Vec<String>,
    pub frontmatter_filters: Vec<(String, String)>,
    pub limit: usize,
}

impl KnowledgeFullTextQuery {
    pub fn new(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            tags: vec![],
            frontmatter_filters: vec![],
            limit: 10,
        }
    }

    pub fn with_tags(mut self, tags: Vec<String>) -> Self {
        self.tags = tags;
        self
    }

    pub fn with_frontmatter_filter(
        mut self,
        key: impl Into<String>,
        value: impl Into<String>,
    ) -> Self {
        self.frontmatter_filters.push((key.into(), value.into()));
        self
    }

    pub fn with_limit(mut self, limit: usize) -> Self {
        self.limit = limit;
        self
    }
}

/// Indexable knowledge document containing search body and metadata.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct KnowledgeIndexDocument {
    pub entry: KnowledgeEntryRef,
    pub body_markdown: String,
    pub tags: Vec<String>,
    pub frontmatter: serde_json::Value,
}

impl KnowledgeIndexDocument {
    pub fn new(entry: KnowledgeEntryRef, body_markdown: impl Into<String>) -> Self {
        Self {
            entry,
            body_markdown: body_markdown.into(),
            tags: vec![],
            frontmatter: serde_json::json!({}),
        }
    }

    pub fn with_tags(mut self, tags: Vec<String>) -> Self {
        self.tags = tags;
        self
    }

    pub fn with_frontmatter(mut self, frontmatter: serde_json::Value) -> Self {
        self.frontmatter = frontmatter;
        self
    }
}

/// Validated embedding vector for semantic knowledge search.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct KnowledgeEmbeddingVector {
    values: Vec<f32>,
}

impl KnowledgeEmbeddingVector {
    pub fn new(values: Vec<f32>) -> Result<Self, KnowledgeIndexError> {
        if values.is_empty() {
            return Err(KnowledgeIndexError::InvalidEmbedding(
                "knowledge embedding vector must not be empty".to_string(),
            ));
        }
        if values.iter().any(|value| !value.is_finite()) {
            return Err(KnowledgeIndexError::InvalidEmbedding(
                "knowledge embedding vector values must be finite".to_string(),
            ));
        }
        Ok(Self { values })
    }

    pub fn values(&self) -> &[f32] {
        &self.values
    }

    pub fn dimensions(&self) -> usize {
        self.values.len()
    }
}

/// Semantic query boundary for embedding-backed knowledge indexes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct KnowledgeSemanticQuery {
    pub embedding: KnowledgeEmbeddingVector,
    pub tags: Vec<String>,
    pub frontmatter_filters: Vec<(String, String)>,
    pub limit: usize,
}

impl KnowledgeSemanticQuery {
    pub fn new(embedding: KnowledgeEmbeddingVector) -> Self {
        Self {
            embedding,
            tags: vec![],
            frontmatter_filters: vec![],
            limit: 10,
        }
    }

    pub fn with_tags(mut self, tags: Vec<String>) -> Self {
        self.tags = tags;
        self
    }

    pub fn with_frontmatter_filter(
        mut self,
        key: impl Into<String>,
        value: impl Into<String>,
    ) -> Self {
        self.frontmatter_filters.push((key.into(), value.into()));
        self
    }

    pub fn with_limit(mut self, limit: usize) -> Self {
        self.limit = limit;
        self
    }
}

/// Indexable semantic embedding document containing entry metadata.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct KnowledgeEmbeddingDocument {
    pub entry: KnowledgeEntryRef,
    pub embedding: KnowledgeEmbeddingVector,
    pub tags: Vec<String>,
    pub frontmatter: serde_json::Value,
}

impl KnowledgeEmbeddingDocument {
    pub fn new(entry: KnowledgeEntryRef, embedding: KnowledgeEmbeddingVector) -> Self {
        Self {
            entry,
            embedding,
            tags: vec![],
            frontmatter: serde_json::json!({}),
        }
    }

    pub fn with_tags(mut self, tags: Vec<String>) -> Self {
        self.tags = tags;
        self
    }

    pub fn with_frontmatter(mut self, frontmatter: serde_json::Value) -> Self {
        self.frontmatter = frontmatter;
        self
    }
}

/// Request to rebuild an index from a complete document set.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct KnowledgeIndexRebuildRequest {
    pub documents: Vec<KnowledgeIndexDocument>,
    pub requested_at: DateTime<Utc>,
}

/// Request to rebuild an embedding index from a complete document set.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct KnowledgeEmbeddingRebuildRequest {
    pub documents: Vec<KnowledgeEmbeddingDocument>,
    pub requested_at: DateTime<Utc>,
}

/// Report returned by a full index rebuild.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KnowledgeIndexRebuildReport {
    pub indexed_count: usize,
    pub deleted_count: usize,
    pub rebuilt_at: DateTime<Utc>,
}

/// Errors from knowledge index operations.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum KnowledgeIndexError {
    #[error("knowledge index lock poisoned")]
    LockPoisoned,
    #[error("invalid knowledge index query: {0}")]
    InvalidQuery(String),
    #[error("invalid knowledge embedding: {0}")]
    InvalidEmbedding(String),
    #[error("knowledge embedding dimension mismatch: expected {expected}, actual {actual}")]
    DimensionMismatch { expected: usize, actual: usize },
}

/// Search backend abstraction for knowledge indexes.
#[async_trait]
pub trait KnowledgeIndex: Send + Sync {
    async fn upsert(&mut self, document: KnowledgeIndexDocument)
    -> Result<(), KnowledgeIndexError>;

    async fn delete(&mut self, id: &KnowledgeEntryId) -> Result<(), KnowledgeIndexError>;

    async fn query(
        &self,
        query: &KnowledgeFullTextQuery,
    ) -> Result<Vec<KnowledgeSearchResult>, KnowledgeIndexError>;

    async fn rebuild(
        &mut self,
        request: KnowledgeIndexRebuildRequest,
    ) -> Result<KnowledgeIndexRebuildReport, KnowledgeIndexError>;
}

/// Semantic search backend abstraction for knowledge embedding indexes.
#[async_trait]
pub trait KnowledgeEmbeddingIndex: Send + Sync {
    async fn upsert_embedding(
        &mut self,
        document: KnowledgeEmbeddingDocument,
    ) -> Result<(), KnowledgeIndexError>;

    async fn delete_embedding(&mut self, id: &KnowledgeEntryId) -> Result<(), KnowledgeIndexError>;

    async fn semantic_query(
        &self,
        query: &KnowledgeSemanticQuery,
    ) -> Result<Vec<KnowledgeSearchResult>, KnowledgeIndexError>;

    async fn rebuild_embeddings(
        &mut self,
        request: KnowledgeEmbeddingRebuildRequest,
    ) -> Result<KnowledgeIndexRebuildReport, KnowledgeIndexError>;
}

/// Weights used to fuse full-text and semantic knowledge scores.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct KnowledgeHybridScoreWeights {
    pub full_text: f32,
    pub semantic: f32,
}

impl Default for KnowledgeHybridScoreWeights {
    fn default() -> Self {
        Self {
            full_text: 0.5,
            semantic: 0.5,
        }
    }
}

/// First-version rerank mode marker for hybrid knowledge search.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KnowledgeHybridRerankMode {
    Disabled,
    DeterministicHook,
}

impl Default for KnowledgeHybridRerankMode {
    fn default() -> Self {
        Self::Disabled
    }
}

/// Query that combines full-text and semantic search paths.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct KnowledgeHybridQuery {
    pub full_text: KnowledgeFullTextQuery,
    pub semantic: KnowledgeSemanticQuery,
    pub weights: KnowledgeHybridScoreWeights,
    pub limit: usize,
    pub rerank: KnowledgeHybridRerankMode,
}

impl KnowledgeHybridQuery {
    pub fn new(full_text: KnowledgeFullTextQuery, semantic: KnowledgeSemanticQuery) -> Self {
        Self {
            full_text,
            semantic,
            weights: KnowledgeHybridScoreWeights::default(),
            limit: 10,
            rerank: KnowledgeHybridRerankMode::default(),
        }
    }

    pub fn with_weights(mut self, weights: KnowledgeHybridScoreWeights) -> Self {
        self.weights = weights;
        self
    }

    pub fn with_limit(mut self, limit: usize) -> Self {
        self.limit = limit;
        self
    }

    pub fn with_rerank(mut self, rerank: KnowledgeHybridRerankMode) -> Self {
        self.rerank = rerank;
        self
    }
}

/// Search result produced by deterministic hybrid score fusion.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct KnowledgeHybridSearchResult {
    pub entry: KnowledgeEntryRef,
    pub fused_score: f32,
    pub full_text_score: Option<f32>,
    pub semantic_score: Option<f32>,
    pub snippet: Option<String>,
}

/// Rerank seam for deterministic tests and future model-backed rerankers.
pub trait KnowledgeHybridReranker: Send + Sync {
    fn rerank(&self, results: &mut Vec<KnowledgeHybridSearchResult>);
}

#[derive(Debug, Clone, Copy, Default)]
pub struct NoopHybridReranker;

impl KnowledgeHybridReranker for NoopHybridReranker {
    fn rerank(&self, _results: &mut Vec<KnowledgeHybridSearchResult>) {}
}

/// Deterministic test-only reranker used to prove the rerank seam can reorder results.
#[derive(Debug, Clone, Copy, Default)]
pub struct ReverseHybridReranker;

impl KnowledgeHybridReranker for ReverseHybridReranker {
    fn rerank(&self, results: &mut Vec<KnowledgeHybridSearchResult>) {
        results.reverse();
    }
}

/// Dependency-free hybrid search coordinator over full-text and semantic indexes.
#[derive(Debug, Clone, Copy)]
pub struct DeterministicHybridKnowledgeSearch<'a, F, S, R = NoopHybridReranker> {
    full_text: &'a F,
    semantic: &'a S,
    reranker: R,
}

impl<'a, F, S> DeterministicHybridKnowledgeSearch<'a, F, S, NoopHybridReranker> {
    pub fn new(full_text: &'a F, semantic: &'a S) -> Self {
        Self {
            full_text,
            semantic,
            reranker: NoopHybridReranker,
        }
    }
}

impl<'a, F, S, R> DeterministicHybridKnowledgeSearch<'a, F, S, R> {
    pub fn with_reranker<NextR>(
        self,
        reranker: NextR,
    ) -> DeterministicHybridKnowledgeSearch<'a, F, S, NextR> {
        DeterministicHybridKnowledgeSearch {
            full_text: self.full_text,
            semantic: self.semantic,
            reranker,
        }
    }
}

impl<'a, F, S, R> DeterministicHybridKnowledgeSearch<'a, F, S, R>
where
    F: KnowledgeIndex,
    S: KnowledgeEmbeddingIndex,
    R: KnowledgeHybridReranker,
{
    pub async fn query(
        &self,
        query: &KnowledgeHybridQuery,
    ) -> Result<Vec<KnowledgeHybridSearchResult>, KnowledgeIndexError> {
        if query.limit == 0 {
            return Err(KnowledgeIndexError::InvalidQuery(
                "knowledge hybrid query limit must be greater than zero".to_string(),
            ));
        }

        let mut full_text_query = query.full_text.clone();
        full_text_query.limit = query.limit;
        let mut semantic_query = query.semantic.clone();
        semantic_query.limit = query.limit;

        let full_text_results = self.full_text.query(&full_text_query).await?;
        let semantic_results = self.semantic.semantic_query(&semantic_query).await?;
        let mut merged: HashMap<KnowledgeEntryId, KnowledgeHybridSearchResult> = HashMap::new();

        for result in full_text_results {
            merged.insert(
                result.entry.id.clone(),
                KnowledgeHybridSearchResult {
                    entry: result.entry,
                    fused_score: query.weights.full_text * result.score,
                    full_text_score: Some(result.score),
                    semantic_score: None,
                    snippet: result.snippet,
                },
            );
        }

        for result in semantic_results {
            merged
                .entry(result.entry.id.clone())
                .and_modify(|existing| {
                    existing.semantic_score = Some(result.score);
                    existing.fused_score += query.weights.semantic * result.score;
                    if existing.snippet.is_none() {
                        existing.snippet = result.snippet.clone();
                    }
                })
                .or_insert_with(|| KnowledgeHybridSearchResult {
                    entry: result.entry,
                    fused_score: query.weights.semantic * result.score,
                    full_text_score: None,
                    semantic_score: Some(result.score),
                    snippet: result.snippet,
                });
        }

        let mut results = merged.into_values().collect::<Vec<_>>();
        results.sort_by(|a, b| {
            b.fused_score
                .partial_cmp(&a.fused_score)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| a.entry.id.0.cmp(&b.entry.id.0))
        });
        results.truncate(query.limit);
        self.reranker.rerank(&mut results);
        results.truncate(query.limit);
        Ok(results)
    }
}

/// Selected full-text backend implementation kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KnowledgeFullTextBackendKind {
    /// Dependency-free deterministic backend used until Tantivy/SQLite FTS is selected.
    DeterministicInProcess,
}

impl Default for KnowledgeFullTextBackendKind {
    fn default() -> Self {
        Self::DeterministicInProcess
    }
}

/// Selected embedding backend implementation kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KnowledgeEmbeddingBackendKind {
    /// Dependency-free deterministic backend used until vector storage is selected.
    DeterministicInProcess,
}

impl Default for KnowledgeEmbeddingBackendKind {
    fn default() -> Self {
        Self::DeterministicInProcess
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct FullTextTermWeights {
    title: u32,
    body: u32,
    tags: u32,
    frontmatter: u32,
}

impl Default for FullTextTermWeights {
    fn default() -> Self {
        Self {
            title: 4,
            body: 2,
            tags: 3,
            frontmatter: 1,
        }
    }
}

/// First full-text backend: deterministic, dependency-free, in-process search.
#[derive(Debug, Clone, Default)]
pub struct DeterministicFullTextKnowledgeBackend {
    documents: HashMap<KnowledgeEntryId, KnowledgeIndexDocument>,
    weights: FullTextTermWeights,
}

impl DeterministicFullTextKnowledgeBackend {
    pub fn new() -> Self {
        Self::default()
    }

    fn matches(document: &KnowledgeIndexDocument, query: &KnowledgeFullTextQuery) -> bool {
        let terms = query_terms(&query.text);
        let searchable = indexed_text(document);
        let matches_text =
            terms.is_empty() || terms.iter().all(|term| searchable.contains(term.as_str()));
        let matches_tags = query
            .tags
            .iter()
            .all(|tag| document.tags.iter().any(|candidate| candidate == tag));
        let matches_frontmatter = query.frontmatter_filters.iter().all(|(key, value)| {
            document
                .frontmatter
                .get(key)
                .and_then(serde_json::Value::as_str)
                .map(|candidate| candidate == value)
                .unwrap_or(false)
        });
        matches_text && matches_tags && matches_frontmatter
    }

    fn score(&self, document: &KnowledgeIndexDocument, query: &KnowledgeFullTextQuery) -> f32 {
        let terms = query_terms(&query.text);
        if terms.is_empty() {
            return 0.0;
        }
        let title = document.entry.title.to_ascii_lowercase();
        let body = document.body_markdown.to_ascii_lowercase();
        let tags = document.tags.join(" ").to_ascii_lowercase();
        let frontmatter = document.frontmatter.to_string().to_ascii_lowercase();
        terms.into_iter().fold(0.0, |score, term| {
            score
                + weighted_contains(&title, &term, self.weights.title)
                + weighted_contains(&body, &term, self.weights.body)
                + weighted_contains(&tags, &term, self.weights.tags)
                + weighted_contains(&frontmatter, &term, self.weights.frontmatter)
        })
    }

    fn snippet(document: &KnowledgeIndexDocument) -> Option<String> {
        Some(document.body_markdown.chars().take(160).collect())
    }
}

fn query_terms(text: &str) -> Vec<String> {
    text.to_ascii_lowercase()
        .split_whitespace()
        .map(str::trim)
        .filter(|term| !term.is_empty())
        .map(str::to_string)
        .collect()
}

fn indexed_text(document: &KnowledgeIndexDocument) -> String {
    format!(
        "{}\n{}\n{}\n{}",
        document.entry.title,
        document.body_markdown,
        document.tags.join(" "),
        document.frontmatter
    )
    .to_ascii_lowercase()
}

fn weighted_contains(text: &str, term: &str, weight: u32) -> f32 {
    if text.contains(term) {
        weight as f32
    } else {
        0.0
    }
}

#[async_trait]
impl KnowledgeIndex for DeterministicFullTextKnowledgeBackend {
    async fn upsert(
        &mut self,
        document: KnowledgeIndexDocument,
    ) -> Result<(), KnowledgeIndexError> {
        self.documents.insert(document.entry.id.clone(), document);
        Ok(())
    }

    async fn delete(&mut self, id: &KnowledgeEntryId) -> Result<(), KnowledgeIndexError> {
        self.documents.remove(id);
        Ok(())
    }

    async fn query(
        &self,
        query: &KnowledgeFullTextQuery,
    ) -> Result<Vec<KnowledgeSearchResult>, KnowledgeIndexError> {
        if query.limit == 0 {
            return Err(KnowledgeIndexError::InvalidQuery(
                "knowledge full-text query limit must be greater than zero".to_string(),
            ));
        }
        let mut results = self
            .documents
            .values()
            .filter(|document| Self::matches(document, query))
            .map(|document| KnowledgeSearchResult {
                entry: document.entry.clone(),
                score: self.score(document, query),
                snippet: Self::snippet(document),
                permission_required: false,
                confidentiality: None,
            })
            .collect::<Vec<_>>();
        results.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| a.entry.id.0.cmp(&b.entry.id.0))
        });
        results.truncate(query.limit);
        Ok(results)
    }

    async fn rebuild(
        &mut self,
        request: KnowledgeIndexRebuildRequest,
    ) -> Result<KnowledgeIndexRebuildReport, KnowledgeIndexError> {
        let deleted_count = self.documents.len();
        self.documents = request
            .documents
            .into_iter()
            .map(|document| (document.entry.id.clone(), document))
            .collect();
        Ok(KnowledgeIndexRebuildReport {
            indexed_count: self.documents.len(),
            deleted_count,
            rebuilt_at: request.requested_at,
        })
    }
}

/// Deterministic in-memory full-text index alias kept for the PR134 test-index boundary.
pub type MemoryFullTextKnowledgeIndex = DeterministicFullTextKnowledgeBackend;

/// First embedding backend: deterministic, dependency-free, in-process cosine search.
#[derive(Debug, Clone, Default)]
pub struct DeterministicEmbeddingKnowledgeBackend {
    documents: HashMap<KnowledgeEntryId, KnowledgeEmbeddingDocument>,
}

impl DeterministicEmbeddingKnowledgeBackend {
    pub fn new() -> Self {
        Self::default()
    }

    fn matches(document: &KnowledgeEmbeddingDocument, query: &KnowledgeSemanticQuery) -> bool {
        let matches_tags = query
            .tags
            .iter()
            .all(|tag| document.tags.iter().any(|candidate| candidate == tag));
        let matches_frontmatter = query.frontmatter_filters.iter().all(|(key, value)| {
            document
                .frontmatter
                .get(key)
                .and_then(serde_json::Value::as_str)
                .map(|candidate| candidate == value)
                .unwrap_or(false)
        });
        matches_tags && matches_frontmatter
    }

    fn cosine_similarity(
        left: &KnowledgeEmbeddingVector,
        right: &KnowledgeEmbeddingVector,
    ) -> Result<f32, KnowledgeIndexError> {
        if left.dimensions() != right.dimensions() {
            return Err(KnowledgeIndexError::DimensionMismatch {
                expected: left.dimensions(),
                actual: right.dimensions(),
            });
        }

        let mut dot = 0.0_f32;
        let mut left_norm = 0.0_f32;
        let mut right_norm = 0.0_f32;
        for (left_value, right_value) in left.values().iter().zip(right.values().iter()) {
            dot += left_value * right_value;
            left_norm += left_value * left_value;
            right_norm += right_value * right_value;
        }

        if left_norm == 0.0 || right_norm == 0.0 {
            return Ok(0.0);
        }

        Ok(dot / (left_norm.sqrt() * right_norm.sqrt()))
    }
}

#[async_trait]
impl KnowledgeEmbeddingIndex for DeterministicEmbeddingKnowledgeBackend {
    async fn upsert_embedding(
        &mut self,
        document: KnowledgeEmbeddingDocument,
    ) -> Result<(), KnowledgeIndexError> {
        if let Some(existing) = self.documents.values().next()
            && existing.embedding.dimensions() != document.embedding.dimensions()
        {
            return Err(KnowledgeIndexError::DimensionMismatch {
                expected: existing.embedding.dimensions(),
                actual: document.embedding.dimensions(),
            });
        }
        self.documents.insert(document.entry.id.clone(), document);
        Ok(())
    }

    async fn delete_embedding(&mut self, id: &KnowledgeEntryId) -> Result<(), KnowledgeIndexError> {
        self.documents.remove(id);
        Ok(())
    }

    async fn semantic_query(
        &self,
        query: &KnowledgeSemanticQuery,
    ) -> Result<Vec<KnowledgeSearchResult>, KnowledgeIndexError> {
        if query.limit == 0 {
            return Err(KnowledgeIndexError::InvalidQuery(
                "knowledge semantic query limit must be greater than zero".to_string(),
            ));
        }

        let mut results = Vec::new();
        for document in self
            .documents
            .values()
            .filter(|document| Self::matches(document, query))
        {
            results.push(KnowledgeSearchResult {
                entry: document.entry.clone(),
                score: Self::cosine_similarity(&document.embedding, &query.embedding)?,
                snippet: None,
                permission_required: false,
                confidentiality: None,
            });
        }
        results.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| a.entry.id.0.cmp(&b.entry.id.0))
        });
        results.truncate(query.limit);
        Ok(results)
    }

    async fn rebuild_embeddings(
        &mut self,
        request: KnowledgeEmbeddingRebuildRequest,
    ) -> Result<KnowledgeIndexRebuildReport, KnowledgeIndexError> {
        if let Some(first) = request.documents.first() {
            let expected = first.embedding.dimensions();
            for document in &request.documents {
                if document.embedding.dimensions() != expected {
                    return Err(KnowledgeIndexError::DimensionMismatch {
                        expected,
                        actual: document.embedding.dimensions(),
                    });
                }
            }
        }

        let deleted_count = self.documents.len();
        self.documents = request
            .documents
            .into_iter()
            .map(|document| (document.entry.id.clone(), document))
            .collect();
        Ok(KnowledgeIndexRebuildReport {
            indexed_count: self.documents.len(),
            deleted_count,
            rebuilt_at: request.requested_at,
        })
    }
}

/// Deterministic in-memory semantic index alias kept for the PR136 test-index boundary.
pub type MemorySemanticKnowledgeIndex = DeterministicEmbeddingKnowledgeBackend;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum KnowledgeValidationError {
    #[error("knowledge draft title cannot be blank")]
    BlankTitle,
    #[error("knowledge draft content cannot be blank")]
    BlankContent,
}

/// Unique identifier for a question ledger entry.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct QuestionLedgerEntryId(pub String);

impl fmt::Display for QuestionLedgerEntryId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl From<String> for QuestionLedgerEntryId {
    fn from(s: String) -> Self {
        Self(s)
    }
}

impl From<&str> for QuestionLedgerEntryId {
    fn from(s: &str) -> Self {
        Self(s.to_string())
    }
}

/// Lightweight answer link for a question ledger entry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct QuestionAnswerRef {
    pub answer_id: String,
    pub answer_cache_ref: Option<String>,
    pub knowledge_entry_id: Option<KnowledgeEntryId>,
}

/// Append-friendly question record linked back to a conversation/message.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct QuestionLedgerEntry {
    pub id: QuestionLedgerEntryId,
    pub question: String,
    pub conversation_id: Option<String>,
    pub message_id: Option<String>,
    pub work_object_id: Option<WorkObjectId>,
    pub answer_ref: Option<QuestionAnswerRef>,
    pub related_knowledge_entry_ids: Vec<KnowledgeEntryId>,
    pub tags: Vec<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Request to create a question ledger entry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct QuestionLedgerCreateRequest {
    pub question: String,
    pub conversation_id: Option<String>,
    pub message_id: Option<String>,
    pub work_object_id: Option<WorkObjectId>,
    pub related_knowledge_entry_ids: Vec<KnowledgeEntryId>,
    pub tags: Vec<String>,
    pub created_at: DateTime<Utc>,
}

impl QuestionLedgerCreateRequest {
    pub fn new(question: impl Into<String>, created_at: DateTime<Utc>) -> Self {
        Self {
            question: question.into(),
            conversation_id: None,
            message_id: None,
            work_object_id: None,
            related_knowledge_entry_ids: vec![],
            tags: vec![],
            created_at,
        }
    }

    pub fn from_conversation_message(
        conversation_id: impl Into<String>,
        message_id: impl Into<String>,
        question: impl Into<String>,
        created_at: DateTime<Utc>,
    ) -> Self {
        Self::new(question, created_at)
            .with_conversation_id(conversation_id)
            .with_message_id(message_id)
    }

    pub fn with_conversation_id(mut self, conversation_id: impl Into<String>) -> Self {
        self.conversation_id = Some(conversation_id.into());
        self
    }

    pub fn with_message_id(mut self, message_id: impl Into<String>) -> Self {
        self.message_id = Some(message_id.into());
        self
    }

    pub fn with_work_object_id(mut self, work_object_id: WorkObjectId) -> Self {
        self.work_object_id = Some(work_object_id);
        self
    }

    pub fn with_related_knowledge_entry_ids(mut self, ids: Vec<KnowledgeEntryId>) -> Self {
        self.related_knowledge_entry_ids = ids;
        self
    }

    pub fn with_tags(mut self, tags: Vec<String>) -> Self {
        self.tags = tags;
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum QuestionLedgerError {
    #[error("question ledger lock poisoned")]
    LockPoisoned,
    #[error("question cannot be blank")]
    BlankQuestion,
    #[error("question ledger entry not found: {0}")]
    NotFound(String),
}

#[async_trait]
pub trait QuestionLedger: Send + Sync {
    async fn create_question(
        &self,
        request: QuestionLedgerCreateRequest,
    ) -> Result<QuestionLedgerEntry, QuestionLedgerError>;

    async fn get_question(
        &self,
        id: &QuestionLedgerEntryId,
    ) -> Result<Option<QuestionLedgerEntry>, QuestionLedgerError>;

    async fn list_by_conversation(
        &self,
        conversation_id: &str,
    ) -> Result<Vec<QuestionLedgerEntry>, QuestionLedgerError>;

    async fn link_answer(
        &self,
        id: &QuestionLedgerEntryId,
        answer_ref: QuestionAnswerRef,
        updated_at: DateTime<Utc>,
    ) -> Result<QuestionLedgerEntry, QuestionLedgerError>;

    async fn list_by_work_object(
        &self,
        work_object_id: &WorkObjectId,
    ) -> Result<Vec<QuestionLedgerEntry>, QuestionLedgerError>;
}

#[derive(Debug, Clone, Default)]
pub struct MemoryQuestionLedger {
    entries: Arc<Mutex<HashMap<QuestionLedgerEntryId, QuestionLedgerEntry>>>,
}

impl MemoryQuestionLedger {
    pub fn new() -> Self {
        Self::default()
    }

    fn next_id(
        entries: &HashMap<QuestionLedgerEntryId, QuestionLedgerEntry>,
    ) -> QuestionLedgerEntryId {
        QuestionLedgerEntryId::from(format!("question-{}", entries.len() + 1))
    }
}

#[async_trait]
impl QuestionLedger for MemoryQuestionLedger {
    async fn create_question(
        &self,
        request: QuestionLedgerCreateRequest,
    ) -> Result<QuestionLedgerEntry, QuestionLedgerError> {
        if request.question.trim().is_empty() {
            return Err(QuestionLedgerError::BlankQuestion);
        }
        let mut entries = self
            .entries
            .lock()
            .map_err(|_| QuestionLedgerError::LockPoisoned)?;
        let entry = QuestionLedgerEntry {
            id: Self::next_id(&entries),
            question: request.question,
            conversation_id: request.conversation_id,
            message_id: request.message_id,
            work_object_id: request.work_object_id,
            answer_ref: None,
            related_knowledge_entry_ids: request.related_knowledge_entry_ids,
            tags: request.tags,
            created_at: request.created_at,
            updated_at: request.created_at,
        };
        entries.insert(entry.id.clone(), entry.clone());
        Ok(entry)
    }

    async fn get_question(
        &self,
        id: &QuestionLedgerEntryId,
    ) -> Result<Option<QuestionLedgerEntry>, QuestionLedgerError> {
        let entries = self
            .entries
            .lock()
            .map_err(|_| QuestionLedgerError::LockPoisoned)?;
        Ok(entries.get(id).cloned())
    }

    async fn list_by_conversation(
        &self,
        conversation_id: &str,
    ) -> Result<Vec<QuestionLedgerEntry>, QuestionLedgerError> {
        let entries = self
            .entries
            .lock()
            .map_err(|_| QuestionLedgerError::LockPoisoned)?;
        let mut results = entries
            .values()
            .filter(|entry| entry.conversation_id.as_deref() == Some(conversation_id))
            .cloned()
            .collect::<Vec<_>>();
        results.sort_by(|a, b| a.id.0.cmp(&b.id.0));
        Ok(results)
    }

    async fn link_answer(
        &self,
        id: &QuestionLedgerEntryId,
        answer_ref: QuestionAnswerRef,
        updated_at: DateTime<Utc>,
    ) -> Result<QuestionLedgerEntry, QuestionLedgerError> {
        let mut entries = self
            .entries
            .lock()
            .map_err(|_| QuestionLedgerError::LockPoisoned)?;
        let entry = entries
            .get_mut(id)
            .ok_or_else(|| QuestionLedgerError::NotFound(id.0.clone()))?;
        if let Some(knowledge_entry_id) = answer_ref.knowledge_entry_id.clone()
            && !entry
                .related_knowledge_entry_ids
                .iter()
                .any(|existing| existing == &knowledge_entry_id)
        {
            entry.related_knowledge_entry_ids.push(knowledge_entry_id);
        }
        entry.answer_ref = Some(answer_ref);
        entry.updated_at = updated_at;
        Ok(entry.clone())
    }

    async fn list_by_work_object(
        &self,
        work_object_id: &WorkObjectId,
    ) -> Result<Vec<QuestionLedgerEntry>, QuestionLedgerError> {
        let entries = self
            .entries
            .lock()
            .map_err(|_| QuestionLedgerError::LockPoisoned)?;
        let mut results = entries
            .values()
            .filter(|entry| entry.work_object_id.as_ref() == Some(work_object_id))
            .cloned()
            .collect::<Vec<_>>();
        results.sort_by(|a, b| a.id.0.cmp(&b.id.0));
        Ok(results)
    }
}

/// Unique identifier for browser snapshot evidence.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct BrowserSnapshotId(pub String);

impl fmt::Display for BrowserSnapshotId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl From<String> for BrowserSnapshotId {
    fn from(s: String) -> Self {
        Self(s)
    }
}

impl From<&str> for BrowserSnapshotId {
    fn from(s: &str) -> Self {
        Self(s.to_string())
    }
}

/// Stable identifier for a registered citation/evidence record.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct EvidenceRefId(pub String);

impl fmt::Display for EvidenceRefId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl From<String> for EvidenceRefId {
    fn from(s: String) -> Self {
        Self(s)
    }
}

impl From<&str> for EvidenceRefId {
    fn from(s: &str) -> Self {
        Self(s.to_string())
    }
}

/// Evidence source reference used by the citation pipeline.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum CitationEvidenceRef {
    SourceUri {
        uri: String,
    },
    Artifact {
        artifact_id: ArtifactId,
    },
    ConversationMessage {
        conversation_id: String,
        message_id: String,
    },
    BrowserSnapshot {
        snapshot_id: BrowserSnapshotId,
        url: Option<String>,
    },
}

impl CitationEvidenceRef {
    pub fn source_uri(uri: impl Into<String>) -> Self {
        Self::SourceUri { uri: uri.into() }
    }

    pub fn artifact(artifact_id: ArtifactId) -> Self {
        Self::Artifact { artifact_id }
    }

    pub fn conversation_message(
        conversation_id: impl Into<String>,
        message_id: impl Into<String>,
    ) -> Self {
        Self::ConversationMessage {
            conversation_id: conversation_id.into(),
            message_id: message_id.into(),
        }
    }

    pub fn browser_snapshot(snapshot_id: BrowserSnapshotId, url: Option<String>) -> Self {
        Self::BrowserSnapshot { snapshot_id, url }
    }
}

/// Registered citation evidence with optional excerpt.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CitationEvidenceRecord {
    pub id: EvidenceRefId,
    pub source_ref: CitationEvidenceRef,
    pub excerpt: Option<String>,
    pub created_at: DateTime<Utc>,
}

impl CitationEvidenceRecord {
    pub fn new(
        id: EvidenceRefId,
        source_ref: CitationEvidenceRef,
        created_at: DateTime<Utc>,
    ) -> Self {
        Self {
            id,
            source_ref,
            excerpt: None,
            created_at,
        }
    }

    pub fn with_excerpt(mut self, excerpt: impl Into<String>) -> Self {
        self.excerpt = Some(excerpt.into());
        self
    }
}

/// Trace result for evidence cited by an answer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AnswerCitationTrace {
    pub answer_id: String,
    pub evidence: Vec<CitationEvidenceRecord>,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CitationEvidenceError {
    #[error("citation evidence lock poisoned")]
    LockPoisoned,
    #[error("answer id cannot be blank")]
    BlankAnswerId,
    #[error("citation evidence not found: {0}")]
    NotFound(String),
}

#[async_trait]
pub trait CitationEvidenceStore: Send + Sync {
    async fn register_evidence(
        &self,
        source_ref: CitationEvidenceRef,
        excerpt: Option<String>,
        created_at: DateTime<Utc>,
    ) -> Result<CitationEvidenceRecord, CitationEvidenceError>;

    async fn cite_answer(
        &self,
        answer_id: &str,
        evidence_id: &EvidenceRefId,
        cited_at: DateTime<Utc>,
    ) -> Result<(), CitationEvidenceError>;

    async fn trace_answer(
        &self,
        answer_id: &str,
    ) -> Result<AnswerCitationTrace, CitationEvidenceError>;
}

#[derive(Debug, Clone, Default)]
pub struct MemoryCitationEvidenceStore {
    evidence: Arc<Mutex<HashMap<EvidenceRefId, CitationEvidenceRecord>>>,
    citations: Arc<Mutex<HashMap<String, Vec<EvidenceRefId>>>>,
}

impl MemoryCitationEvidenceStore {
    pub fn new() -> Self {
        Self::default()
    }

    fn next_id(evidence: &HashMap<EvidenceRefId, CitationEvidenceRecord>) -> EvidenceRefId {
        EvidenceRefId::from(format!("evidence-{}", evidence.len() + 1))
    }
}

#[async_trait]
impl CitationEvidenceStore for MemoryCitationEvidenceStore {
    async fn register_evidence(
        &self,
        source_ref: CitationEvidenceRef,
        excerpt: Option<String>,
        created_at: DateTime<Utc>,
    ) -> Result<CitationEvidenceRecord, CitationEvidenceError> {
        let mut evidence = self
            .evidence
            .lock()
            .map_err(|_| CitationEvidenceError::LockPoisoned)?;
        let mut record =
            CitationEvidenceRecord::new(Self::next_id(&evidence), source_ref, created_at);
        record.excerpt = excerpt;
        evidence.insert(record.id.clone(), record.clone());
        Ok(record)
    }

    async fn cite_answer(
        &self,
        answer_id: &str,
        evidence_id: &EvidenceRefId,
        _cited_at: DateTime<Utc>,
    ) -> Result<(), CitationEvidenceError> {
        if answer_id.trim().is_empty() {
            return Err(CitationEvidenceError::BlankAnswerId);
        }
        {
            let evidence = self
                .evidence
                .lock()
                .map_err(|_| CitationEvidenceError::LockPoisoned)?;
            if !evidence.contains_key(evidence_id) {
                return Err(CitationEvidenceError::NotFound(evidence_id.to_string()));
            }
        }
        let mut citations = self
            .citations
            .lock()
            .map_err(|_| CitationEvidenceError::LockPoisoned)?;
        let ids = citations.entry(answer_id.to_string()).or_default();
        if !ids.iter().any(|id| id == evidence_id) {
            ids.push(evidence_id.clone());
        }
        Ok(())
    }

    async fn trace_answer(
        &self,
        answer_id: &str,
    ) -> Result<AnswerCitationTrace, CitationEvidenceError> {
        let citation_ids = {
            let citations = self
                .citations
                .lock()
                .map_err(|_| CitationEvidenceError::LockPoisoned)?;
            citations.get(answer_id).cloned().unwrap_or_default()
        };
        let evidence = self
            .evidence
            .lock()
            .map_err(|_| CitationEvidenceError::LockPoisoned)?;
        let mut records = citation_ids
            .into_iter()
            .filter_map(|id| evidence.get(&id).cloned())
            .collect::<Vec<_>>();
        records.sort_by(|a, b| a.id.0.cmp(&b.id.0));
        Ok(AnswerCitationTrace {
            answer_id: answer_id.to_string(),
            evidence: records,
        })
    }
}

/// Governance lifecycle status for knowledge entries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KnowledgeGovernanceStatus {
    Draft,
    Active,
    Deprecated,
    Archived,
}

/// Frontmatter validation errors used by the governance workflow.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, thiserror::Error)]
pub enum KnowledgeGovernanceValidationError {
    #[error("frontmatter title is required")]
    MissingTitle,
    #[error("frontmatter summary is required")]
    MissingSummary,
    #[error("frontmatter tags must contain at least one item")]
    MissingTags,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum KnowledgeGovernanceError {
    #[error("knowledge governance lock poisoned")]
    LockPoisoned,
    #[error("knowledge governance record not found: {0}")]
    NotFound(String),
    #[error("knowledge governance validation failed: {0:?}")]
    ValidationFailed(Vec<KnowledgeGovernanceValidationError>),
}

/// Governance metadata for a knowledge entry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KnowledgeGovernanceRecord {
    pub knowledge_entry_id: KnowledgeEntryId,
    pub status: KnowledgeGovernanceStatus,
    pub frontmatter: serde_json::Value,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl KnowledgeGovernanceRecord {
    pub fn new(
        knowledge_entry_id: KnowledgeEntryId,
        status: KnowledgeGovernanceStatus,
        frontmatter: serde_json::Value,
        created_at: DateTime<Utc>,
    ) -> Self {
        Self {
            knowledge_entry_id,
            status,
            frontmatter,
            created_at,
            updated_at: created_at,
        }
    }

    pub fn validation_errors(&self) -> Vec<KnowledgeGovernanceValidationError> {
        let mut errors = Vec::new();
        if self
            .frontmatter
            .get("title")
            .and_then(serde_json::Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .is_none()
        {
            errors.push(KnowledgeGovernanceValidationError::MissingTitle);
        }
        if self
            .frontmatter
            .get("summary")
            .and_then(serde_json::Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .is_none()
        {
            errors.push(KnowledgeGovernanceValidationError::MissingSummary);
        }
        if self
            .frontmatter
            .get("tags")
            .and_then(serde_json::Value::as_array)
            .filter(|tags| !tags.is_empty())
            .is_none()
        {
            errors.push(KnowledgeGovernanceValidationError::MissingTags);
        }
        errors
    }

    pub fn activate(mut self, updated_at: DateTime<Utc>) -> Result<Self, KnowledgeGovernanceError> {
        let errors = self.validation_errors();
        if !errors.is_empty() {
            return Err(KnowledgeGovernanceError::ValidationFailed(errors));
        }
        self.status = KnowledgeGovernanceStatus::Active;
        self.updated_at = updated_at;
        Ok(self)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KnowledgeReviewQueueItem {
    pub knowledge_entry_id: KnowledgeEntryId,
    pub status: KnowledgeGovernanceStatus,
    pub validation_errors: Vec<KnowledgeGovernanceValidationError>,
    pub updated_at: DateTime<Utc>,
}

impl KnowledgeReviewQueueItem {
    pub fn from_record(record: &KnowledgeGovernanceRecord) -> Self {
        Self {
            knowledge_entry_id: record.knowledge_entry_id.clone(),
            status: record.status,
            validation_errors: record.validation_errors(),
            updated_at: record.updated_at,
        }
    }
}

#[async_trait]
pub trait KnowledgeGovernanceStore: Send + Sync {
    async fn upsert_record(
        &self,
        record: KnowledgeGovernanceRecord,
    ) -> Result<KnowledgeGovernanceRecord, KnowledgeGovernanceError>;

    async fn transition_status(
        &self,
        knowledge_entry_id: &KnowledgeEntryId,
        status: KnowledgeGovernanceStatus,
        updated_at: DateTime<Utc>,
    ) -> Result<KnowledgeGovernanceRecord, KnowledgeGovernanceError>;

    async fn review_queue(&self)
    -> Result<Vec<KnowledgeReviewQueueItem>, KnowledgeGovernanceError>;

    async fn active_index_entry_ids(
        &self,
    ) -> Result<Vec<KnowledgeEntryId>, KnowledgeGovernanceError>;
}

#[derive(Debug, Clone, Default)]
pub struct MemoryKnowledgeGovernanceStore {
    records: Arc<Mutex<HashMap<KnowledgeEntryId, KnowledgeGovernanceRecord>>>,
}

impl MemoryKnowledgeGovernanceStore {
    pub fn new() -> Self {
        Self::default()
    }
}

#[async_trait]
impl KnowledgeGovernanceStore for MemoryKnowledgeGovernanceStore {
    async fn upsert_record(
        &self,
        record: KnowledgeGovernanceRecord,
    ) -> Result<KnowledgeGovernanceRecord, KnowledgeGovernanceError> {
        let mut records = self
            .records
            .lock()
            .map_err(|_| KnowledgeGovernanceError::LockPoisoned)?;
        records.insert(record.knowledge_entry_id.clone(), record.clone());
        Ok(record)
    }

    async fn transition_status(
        &self,
        knowledge_entry_id: &KnowledgeEntryId,
        status: KnowledgeGovernanceStatus,
        updated_at: DateTime<Utc>,
    ) -> Result<KnowledgeGovernanceRecord, KnowledgeGovernanceError> {
        let mut records = self
            .records
            .lock()
            .map_err(|_| KnowledgeGovernanceError::LockPoisoned)?;
        let existing = records
            .get(knowledge_entry_id)
            .cloned()
            .ok_or_else(|| KnowledgeGovernanceError::NotFound(knowledge_entry_id.to_string()))?;
        let updated = if status == KnowledgeGovernanceStatus::Active {
            existing.activate(updated_at)?
        } else {
            KnowledgeGovernanceRecord {
                status,
                updated_at,
                ..existing
            }
        };
        records.insert(knowledge_entry_id.clone(), updated.clone());
        Ok(updated)
    }

    async fn review_queue(
        &self,
    ) -> Result<Vec<KnowledgeReviewQueueItem>, KnowledgeGovernanceError> {
        let records = self
            .records
            .lock()
            .map_err(|_| KnowledgeGovernanceError::LockPoisoned)?;
        let mut queue = records
            .values()
            .filter(|record| record.status != KnowledgeGovernanceStatus::Active)
            .map(KnowledgeReviewQueueItem::from_record)
            .filter(|item| !item.validation_errors.is_empty())
            .collect::<Vec<_>>();
        queue.sort_by(|a, b| a.knowledge_entry_id.0.cmp(&b.knowledge_entry_id.0));
        Ok(queue)
    }

    async fn active_index_entry_ids(
        &self,
    ) -> Result<Vec<KnowledgeEntryId>, KnowledgeGovernanceError> {
        let records = self
            .records
            .lock()
            .map_err(|_| KnowledgeGovernanceError::LockPoisoned)?;
        let mut ids = records
            .values()
            .filter(|record| {
                record.status == KnowledgeGovernanceStatus::Active
                    && record.validation_errors().is_empty()
            })
            .map(|record| record.knowledge_entry_id.clone())
            .collect::<Vec<_>>();
        ids.sort_by(|a, b| a.0.cmp(&b.0));
        Ok(ids)
    }
}

/// Unique identifier for an answer cache package.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct AnswerCachePackageId(pub String);

impl fmt::Display for AnswerCachePackageId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl From<String> for AnswerCachePackageId {
    fn from(s: String) -> Self {
        Self(s)
    }
}

impl From<&str> for AnswerCachePackageId {
    fn from(s: &str) -> Self {
        Self(s.to_string())
    }
}

/// Positive, monotonically increasing package version.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct AnswerCachePackageVersion(u32);

impl AnswerCachePackageVersion {
    pub fn new(version: u32) -> Result<Self, AnswerCacheError> {
        if version == 0 {
            return Err(AnswerCacheError::InvalidVersion);
        }
        Ok(Self(version))
    }

    pub fn value(self) -> u32 {
        self.0
    }
}

/// Freshness policy for cached answers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AnswerFreshnessPolicy {
    pub expires_after_days: Option<u32>,
}

impl AnswerFreshnessPolicy {
    pub fn never_expires() -> Self {
        Self {
            expires_after_days: None,
        }
    }

    pub fn expires_after_days(days: u32) -> Self {
        Self {
            expires_after_days: Some(days),
        }
    }
}

impl Default for AnswerFreshnessPolicy {
    fn default() -> Self {
        Self::never_expires()
    }
}

/// Evidence reference captured inside an answer cache package.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum AnswerEvidenceRef {
    KnowledgeEntry {
        knowledge_entry_id: KnowledgeEntryId,
        quote: Option<String>,
    },
    ConversationMessage {
        conversation_id: String,
        message_id: String,
    },
    Artifact {
        artifact_id: ArtifactId,
    },
    Asset {
        asset_id: AssetId,
    },
}

impl AnswerEvidenceRef {
    pub fn knowledge_entry(knowledge_entry_id: KnowledgeEntryId, quote: Option<String>) -> Self {
        Self::KnowledgeEntry {
            knowledge_entry_id,
            quote,
        }
    }

    pub fn conversation_message(
        conversation_id: impl Into<String>,
        message_id: impl Into<String>,
    ) -> Self {
        Self::ConversationMessage {
            conversation_id: conversation_id.into(),
            message_id: message_id.into(),
        }
    }
}

/// Versioned answer package linked to question ledger and evidence refs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AnswerCachePackage {
    pub id: AnswerCachePackageId,
    pub question_id: Option<QuestionLedgerEntryId>,
    pub work_object_id: Option<WorkObjectId>,
    pub answer_id: String,
    pub version: AnswerCachePackageVersion,
    pub answer_markdown: String,
    pub evidence_refs: Vec<AnswerEvidenceRef>,
    pub related_knowledge_entry_ids: Vec<KnowledgeEntryId>,
    pub freshness_policy: AnswerFreshnessPolicy,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Request to create a new answer cache package version.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AnswerCacheCreateRequest {
    pub question_id: Option<QuestionLedgerEntryId>,
    pub work_object_id: Option<WorkObjectId>,
    pub answer_id: String,
    pub answer_markdown: String,
    pub evidence_refs: Vec<AnswerEvidenceRef>,
    pub freshness_policy: AnswerFreshnessPolicy,
    pub created_at: DateTime<Utc>,
}

impl AnswerCacheCreateRequest {
    pub fn new(
        answer_id: impl Into<String>,
        answer_markdown: impl Into<String>,
        created_at: DateTime<Utc>,
    ) -> Self {
        Self {
            question_id: None,
            work_object_id: None,
            answer_id: answer_id.into(),
            answer_markdown: answer_markdown.into(),
            evidence_refs: vec![],
            freshness_policy: AnswerFreshnessPolicy::default(),
            created_at,
        }
    }

    pub fn with_question_id(mut self, question_id: QuestionLedgerEntryId) -> Self {
        self.question_id = Some(question_id);
        self
    }

    pub fn with_work_object_id(mut self, work_object_id: WorkObjectId) -> Self {
        self.work_object_id = Some(work_object_id);
        self
    }

    pub fn with_evidence_refs(mut self, evidence_refs: Vec<AnswerEvidenceRef>) -> Self {
        self.evidence_refs = evidence_refs;
        self
    }

    pub fn with_freshness_policy(mut self, freshness_policy: AnswerFreshnessPolicy) -> Self {
        self.freshness_policy = freshness_policy;
        self
    }

    pub fn related_knowledge_entry_ids(&self) -> Vec<KnowledgeEntryId> {
        let mut ids = Vec::new();
        for evidence in &self.evidence_refs {
            if let AnswerEvidenceRef::KnowledgeEntry {
                knowledge_entry_id, ..
            } = evidence
                && !ids.iter().any(|existing| existing == knowledge_entry_id)
            {
                ids.push(knowledge_entry_id.clone());
            }
        }
        ids
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum AnswerCacheError {
    #[error("answer cache lock poisoned")]
    LockPoisoned,
    #[error("answer cache package version must be greater than zero")]
    InvalidVersion,
    #[error("answer markdown cannot be blank")]
    BlankAnswer,
    #[error("answer cache package not found: {0}")]
    NotFound(String),
}

#[async_trait]
pub trait AnswerCacheStore: Send + Sync {
    async fn create_package(
        &self,
        request: AnswerCacheCreateRequest,
    ) -> Result<AnswerCachePackage, AnswerCacheError>;

    async fn get_package(
        &self,
        id: &AnswerCachePackageId,
    ) -> Result<Option<AnswerCachePackage>, AnswerCacheError>;

    async fn latest_for_question(
        &self,
        question_id: &QuestionLedgerEntryId,
    ) -> Result<Option<AnswerCachePackage>, AnswerCacheError>;

    async fn list_by_knowledge_entry(
        &self,
        knowledge_entry_id: &KnowledgeEntryId,
    ) -> Result<Vec<AnswerCachePackage>, AnswerCacheError>;

    async fn list_by_work_object(
        &self,
        work_object_id: &WorkObjectId,
    ) -> Result<Vec<AnswerCachePackage>, AnswerCacheError>;
}

#[derive(Debug, Clone, Default)]
pub struct MemoryAnswerCacheStore {
    packages: Arc<Mutex<HashMap<AnswerCachePackageId, AnswerCachePackage>>>,
}

impl MemoryAnswerCacheStore {
    pub fn new() -> Self {
        Self::default()
    }

    fn next_id(
        packages: &HashMap<AnswerCachePackageId, AnswerCachePackage>,
    ) -> AnswerCachePackageId {
        AnswerCachePackageId::from(format!("answer-cache-{}", packages.len() + 1))
    }

    fn next_version_for_answer(
        packages: &HashMap<AnswerCachePackageId, AnswerCachePackage>,
        answer_id: &str,
    ) -> Result<AnswerCachePackageVersion, AnswerCacheError> {
        let next = packages
            .values()
            .filter(|package| package.answer_id == answer_id)
            .map(|package| package.version.value())
            .max()
            .unwrap_or(0)
            + 1;
        AnswerCachePackageVersion::new(next)
    }
}

#[async_trait]
impl AnswerCacheStore for MemoryAnswerCacheStore {
    async fn create_package(
        &self,
        request: AnswerCacheCreateRequest,
    ) -> Result<AnswerCachePackage, AnswerCacheError> {
        if request.answer_markdown.trim().is_empty() {
            return Err(AnswerCacheError::BlankAnswer);
        }
        let mut packages = self
            .packages
            .lock()
            .map_err(|_| AnswerCacheError::LockPoisoned)?;
        let related_knowledge_entry_ids = request.related_knowledge_entry_ids();
        let package = AnswerCachePackage {
            id: Self::next_id(&packages),
            question_id: request.question_id,
            work_object_id: request.work_object_id,
            answer_id: request.answer_id.clone(),
            version: Self::next_version_for_answer(&packages, &request.answer_id)?,
            answer_markdown: request.answer_markdown,
            related_knowledge_entry_ids,
            evidence_refs: request.evidence_refs,
            freshness_policy: request.freshness_policy,
            created_at: request.created_at,
            updated_at: request.created_at,
        };
        packages.insert(package.id.clone(), package.clone());
        Ok(package)
    }

    async fn get_package(
        &self,
        id: &AnswerCachePackageId,
    ) -> Result<Option<AnswerCachePackage>, AnswerCacheError> {
        let packages = self
            .packages
            .lock()
            .map_err(|_| AnswerCacheError::LockPoisoned)?;
        Ok(packages.get(id).cloned())
    }

    async fn latest_for_question(
        &self,
        question_id: &QuestionLedgerEntryId,
    ) -> Result<Option<AnswerCachePackage>, AnswerCacheError> {
        let packages = self
            .packages
            .lock()
            .map_err(|_| AnswerCacheError::LockPoisoned)?;
        let mut results = packages
            .values()
            .filter(|package| package.question_id.as_ref() == Some(question_id))
            .cloned()
            .collect::<Vec<_>>();
        results.sort_by(|a, b| {
            b.version
                .cmp(&a.version)
                .then_with(|| b.updated_at.cmp(&a.updated_at))
                .then_with(|| a.id.0.cmp(&b.id.0))
        });
        Ok(results.into_iter().next())
    }

    async fn list_by_knowledge_entry(
        &self,
        knowledge_entry_id: &KnowledgeEntryId,
    ) -> Result<Vec<AnswerCachePackage>, AnswerCacheError> {
        let packages = self
            .packages
            .lock()
            .map_err(|_| AnswerCacheError::LockPoisoned)?;
        let mut results = packages
            .values()
            .filter(|package| {
                package
                    .related_knowledge_entry_ids
                    .iter()
                    .any(|existing| existing == knowledge_entry_id)
            })
            .cloned()
            .collect::<Vec<_>>();
        results.sort_by(|a, b| a.id.0.cmp(&b.id.0));
        Ok(results)
    }

    async fn list_by_work_object(
        &self,
        work_object_id: &WorkObjectId,
    ) -> Result<Vec<AnswerCachePackage>, AnswerCacheError> {
        let packages = self
            .packages
            .lock()
            .map_err(|_| AnswerCacheError::LockPoisoned)?;
        let mut results = packages
            .values()
            .filter(|package| package.work_object_id.as_ref() == Some(work_object_id))
            .cloned()
            .collect::<Vec<_>>();
        results.sort_by(|a, b| a.id.0.cmp(&b.id.0));
        Ok(results)
    }
}

// ---------------------------------------------------------------------------
// Work Object cross-entity query boundary
// ---------------------------------------------------------------------------

/// Query for all entities linked to a specific work object.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkObjectCrossEntityQuery {
    pub work_object_id: WorkObjectId,
}

impl WorkObjectCrossEntityQuery {
    pub fn new(work_object_id: WorkObjectId) -> Self {
        Self { work_object_id }
    }
}

/// Aggregated result of all entities linked to a work object.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkObjectCrossEntityResult {
    pub work_object_id: WorkObjectId,
    pub question_ids: Vec<QuestionLedgerEntryId>,
    pub answer_ids: Vec<AnswerCachePackageId>,
    pub knowledge_entry_ids: Vec<KnowledgeEntryId>,
}

/// Cross-entity coordinator that queries across question ledger,
/// answer cache, and knowledge entries by work object.
#[derive(Debug, Clone)]
pub struct WorkObjectCrossEntityCoordinator<Q: QuestionLedger, A: AnswerCacheStore> {
    question_ledger: Q,
    answer_cache: A,
}

impl<Q: QuestionLedger, A: AnswerCacheStore> WorkObjectCrossEntityCoordinator<Q, A> {
    pub fn new(question_ledger: Q, answer_cache: A) -> Self {
        Self {
            question_ledger,
            answer_cache,
        }
    }

    /// Query all entities linked to the given work object.
    pub async fn query(
        &self,
        query: &WorkObjectCrossEntityQuery,
    ) -> Result<WorkObjectCrossEntityResult, String> {
        let questions = self
            .question_ledger
            .list_by_work_object(&query.work_object_id)
            .await
            .map_err(|e| e.to_string())?;

        let answers = self
            .answer_cache
            .list_by_work_object(&query.work_object_id)
            .await
            .map_err(|e| e.to_string())?;

        // Collect unique knowledge entry IDs from questions and answers
        let mut knowledge_entry_ids: Vec<KnowledgeEntryId> = Vec::new();
        for q in &questions {
            for kid in &q.related_knowledge_entry_ids {
                if !knowledge_entry_ids.iter().any(|existing| existing == kid) {
                    knowledge_entry_ids.push(kid.clone());
                }
            }
        }
        for a in &answers {
            for kid in &a.related_knowledge_entry_ids {
                if !knowledge_entry_ids.iter().any(|existing| existing == kid) {
                    knowledge_entry_ids.push(kid.clone());
                }
            }
        }
        knowledge_entry_ids.sort_by(|a, b| a.0.cmp(&b.0));

        Ok(WorkObjectCrossEntityResult {
            work_object_id: query.work_object_id.clone(),
            question_ids: questions.iter().map(|q| q.id.clone()).collect(),
            answer_ids: answers.iter().map(|a| a.id.clone()).collect(),
            knowledge_entry_ids,
        })
    }
}

// ---------------------------------------------------------------------------
// Permission-aware knowledge search
// ---------------------------------------------------------------------------

/// Wrapper around any `KnowledgeIndex` that filters results based on
/// enterprise permission grants.
#[derive(Debug, Clone)]
pub struct PermissionAwareKnowledgeSearch<I: KnowledgeIndex> {
    inner: I,
    permission_store: PermissionStore,
}

impl<I: KnowledgeIndex> PermissionAwareKnowledgeSearch<I> {
    pub fn new(inner: I, permission_store: PermissionStore) -> Self {
        Self {
            inner,
            permission_store,
        }
    }

    /// Search with permission filtering.
    /// Entries where the user lacks `Read` on `KnowledgeBase` are excluded.
    pub async fn query_with_permissions(
        &self,
        query: &KnowledgeFullTextQuery,
        user_id: &EnterpriseUserId,
    ) -> Result<Vec<KnowledgeSearchResult>, KnowledgeIndexError> {
        if !self
            .permission_store
            .get_user_lifecycle(user_id)
            .is_active()
        {
            return Ok(Vec::new());
        }

        let results = self.inner.query(query).await?;
        let filtered = results
            .into_iter()
            .filter(|result| {
                let resource_id =
                    enterprise_permission_core::ResourceId::from(result.entry.id.0.clone());
                let decision = self.permission_store.check(
                    user_id,
                    &ResourceType::KnowledgeBase,
                    &resource_id,
                    &PermissionAction::Read,
                    Utc::now(),
                );
                decision.is_allowed()
            })
            .collect();
        Ok(filtered)
    }
}

// ---------------------------------------------------------------------------
// Permission-aware citation evidence filtering
// ---------------------------------------------------------------------------

/// Wrapper around `CitationEvidenceStore` that filters evidence traces
/// based on enterprise permission grants.
#[derive(Debug, Clone)]
pub struct PermissionAwareCitationEvidenceStore<S: CitationEvidenceStore> {
    inner: S,
    permission_store: PermissionStore,
}

impl<S: CitationEvidenceStore> PermissionAwareCitationEvidenceStore<S> {
    pub fn new(inner: S, permission_store: PermissionStore) -> Self {
        Self {
            inner,
            permission_store,
        }
    }

    /// Trace an answer's evidence, filtering out evidence the user cannot access.
    pub async fn trace_answer_with_permissions(
        &self,
        answer_id: &str,
        user_id: &EnterpriseUserId,
    ) -> Result<AnswerCitationTrace, CitationEvidenceError> {
        let trace = self.inner.trace_answer(answer_id).await?;
        let filtered_evidence: Vec<CitationEvidenceRecord> = trace
            .evidence
            .into_iter()
            .filter(|record| match &record.source_ref {
                CitationEvidenceRef::SourceUri { .. } => true,
                CitationEvidenceRef::ConversationMessage { .. } => true,
                CitationEvidenceRef::BrowserSnapshot { .. } => true,
                CitationEvidenceRef::Artifact { artifact_id } => {
                    let resource_id =
                        enterprise_permission_core::ResourceId::from(artifact_id.0.clone());
                    let decision = self.permission_store.check(
                        user_id,
                        &ResourceType::KnowledgeBase,
                        &resource_id,
                        &PermissionAction::Read,
                        Utc::now(),
                    );
                    decision.is_allowed()
                }
            })
            .collect();
        Ok(AnswerCitationTrace {
            answer_id: trace.answer_id,
            evidence: filtered_evidence,
        })
    }
}

/// Input for `knowledge.search`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KnowledgeSearchActionInput {
    pub query: KnowledgeSearchQuery,
}

/// Input for `knowledge.get_entry`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KnowledgeGetEntryActionInput {
    pub id: KnowledgeEntryId,
}

/// Input for `knowledge.create_draft`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct KnowledgeCreateDraftActionInput {
    pub title: String,
    pub content_markdown: String,
    pub source_uri: Option<String>,
    pub source_artifact_id: Option<ArtifactId>,
    pub source_asset_id: Option<AssetId>,
    pub tags: Vec<String>,
    pub metadata: serde_json::Value,
    pub created_at: DateTime<Utc>,
}

impl KnowledgeCreateDraftActionInput {
    pub fn into_draft(self) -> Result<KnowledgeEntryDraft, KnowledgeValidationError> {
        validate_draft_fields(&self.title, &self.content_markdown)?;
        Ok(KnowledgeEntryDraft {
            title: self.title,
            content_markdown: self.content_markdown,
            source_uri: self.source_uri,
            source_artifact_id: self.source_artifact_id,
            source_asset_id: self.source_asset_id,
            tags: self.tags,
            metadata: self.metadata,
            created_at: self.created_at,
        })
    }
}

/// Input for `knowledge.save_entry`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct KnowledgeSaveEntryActionInput {
    pub draft: KnowledgeEntryDraft,
}

/// Input for `knowledge.update_entry`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct KnowledgeUpdateEntryActionInput {
    pub id: KnowledgeEntryId,
    pub update: KnowledgeEntryUpdate,
}

fn validate_draft_fields(
    title: &str,
    content_markdown: &str,
) -> Result<(), KnowledgeValidationError> {
    if title.trim().is_empty() {
        return Err(KnowledgeValidationError::BlankTitle);
    }
    if content_markdown.trim().is_empty() {
        return Err(KnowledgeValidationError::BlankContent);
    }
    Ok(())
}

/// Register the first stable knowledge action schemas.
pub fn register_knowledge_action_schemas(
    registry: &mut ActionRegistry,
) -> Result<(), ActionRegistryError> {
    registry.register(ActionSchema {
        kind: knowledge_search_action_kind(),
        display_name: "Search Knowledge".to_string(),
        description: "Search known knowledge entries without mutating state.".to_string(),
        side_effect: SideEffectKind::ReadOnly,
        input_schema: None,
        output_schema: None,
    })?;
    registry.register(ActionSchema {
        kind: knowledge_get_entry_action_kind(),
        display_name: "Get Knowledge Entry".to_string(),
        description: "Get a knowledge entry reference by id without mutating state.".to_string(),
        side_effect: SideEffectKind::ReadOnly,
        input_schema: None,
        output_schema: None,
    })?;
    registry.register(ActionSchema {
        kind: knowledge_create_draft_action_kind(),
        display_name: "Create Knowledge Draft".to_string(),
        description: "Create a session-local knowledge draft payload.".to_string(),
        side_effect: SideEffectKind::RuntimeStateMutation,
        input_schema: None,
        output_schema: None,
    })?;
    registry.register(ActionSchema {
        kind: knowledge_save_entry_action_kind(),
        display_name: "Save Knowledge Entry".to_string(),
        description: "Persist a knowledge draft through the configured repository.".to_string(),
        side_effect: SideEffectKind::FileSystemMutation,
        input_schema: None,
        output_schema: None,
    })?;
    Ok(())
}

/// Errors from knowledge repository operations.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum KnowledgeRepositoryError {
    #[error("knowledge repository lock poisoned")]
    LockPoisoned,
    #[error("io error: {0}")]
    Io(String),
    #[error("frontmatter parse error in {path}: {reason}")]
    FrontmatterParse { path: String, reason: String },
    #[error("entry already exists: {0}")]
    EntryExists(String),
    #[error("invalid entry id: {0}")]
    InvalidId(String),
    #[error("entry not found: {0}")]
    NotFound(String),
}

// ---------------------------------------------------------------------------
// Object/Relation Knowledge Engine v0.1 storage contracts
// ---------------------------------------------------------------------------

/// Stable identifier for an object in the structured object graph projection.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct KnowledgeObjectId(pub String);

impl fmt::Display for KnowledgeObjectId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl From<String> for KnowledgeObjectId {
    fn from(s: String) -> Self {
        Self(s)
    }
}

impl From<&str> for KnowledgeObjectId {
    fn from(s: &str) -> Self {
        Self(s.to_string())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct KnowledgeRelationId(pub String);

impl From<&str> for KnowledgeRelationId {
    fn from(s: &str) -> Self {
        Self(s.to_string())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct KnowledgeAttributeId(pub String);

impl From<&str> for KnowledgeAttributeId {
    fn from(s: &str) -> Self {
        Self(s.to_string())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct KnowledgeClaimId(pub String);

impl From<&str> for KnowledgeClaimId {
    fn from(s: &str) -> Self {
        Self(s.to_string())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct KnowledgeAssetBindingId(pub String);

impl From<&str> for KnowledgeAssetBindingId {
    fn from(s: &str) -> Self {
        Self(s.to_string())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct KnowledgeEventId(pub String);

impl From<&str> for KnowledgeEventId {
    fn from(s: &str) -> Self {
        Self(s.to_string())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct KnowledgeAuditId(pub String);

impl From<&str> for KnowledgeAuditId {
    fn from(s: &str) -> Self {
        Self(s.to_string())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct KnowledgeTransactionId(pub String);

impl From<&str> for KnowledgeTransactionId {
    fn from(s: &str) -> Self {
        Self(s.to_string())
    }
}

/// Actor envelope used by event and audit records. Human, LLM, tool and engine
/// actors all enter the same deterministic boundary; LLM writes remain proposals.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KnowledgeActor {
    pub actor_type: String,
    pub actor_id: String,
    pub delegated_by: Option<String>,
}

impl KnowledgeActor {
    pub fn engine(delegated_by: impl Into<String>) -> Self {
        Self {
            actor_type: "engine".to_string(),
            actor_id: "knowledge-engine".to_string(),
            delegated_by: Some(delegated_by.into()),
        }
    }

    pub fn llm(actor_id: impl Into<String>, delegated_by: impl Into<String>) -> Self {
        Self {
            actor_type: "llm".to_string(),
            actor_id: actor_id.into(),
            delegated_by: Some(delegated_by.into()),
        }
    }
}

/// Append-only event envelope. The event log is the authoritative history for
/// object, attribute, relation, claim, evidence, asset and blob mutations.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct KnowledgeEventEnvelope {
    pub event_id: KnowledgeEventId,
    pub event_type: String,
    pub schema_version: String,
    pub timestamp: DateTime<Utc>,
    pub transaction_id: KnowledgeTransactionId,
    pub causation_id: Option<String>,
    pub correlation_id: Option<String>,
    pub actor: KnowledgeActor,
    pub payload: serde_json::Value,
    pub evidence_refs: Vec<EvidenceRefId>,
    pub audit_refs: Vec<KnowledgeAuditId>,
    pub prev_event_hash: Option<String>,
    pub event_hash: Option<String>,
}

impl KnowledgeEventEnvelope {
    pub fn new(
        event_id: impl Into<KnowledgeEventId>,
        event_type: impl Into<String>,
        transaction_id: impl Into<KnowledgeTransactionId>,
        actor: KnowledgeActor,
        payload: serde_json::Value,
        timestamp: DateTime<Utc>,
    ) -> Self {
        Self {
            event_id: event_id.into(),
            event_type: event_type.into(),
            schema_version: "0.1.0".to_string(),
            timestamp,
            transaction_id: transaction_id.into(),
            causation_id: None,
            correlation_id: None,
            actor,
            payload,
            evidence_refs: Vec::new(),
            audit_refs: Vec::new(),
            prev_event_hash: None,
            event_hash: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct KnowledgeObjectRecord {
    pub record_type: String,
    pub object_id: KnowledgeObjectId,
    pub canonical_name: String,
    pub object_types: Vec<String>,
    pub status: String,
    pub identity: serde_json::Value,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub version: u64,
    pub last_event_id: KnowledgeEventId,
    pub trust_score: f32,
}

impl KnowledgeObjectRecord {
    pub fn new(
        object_id: impl Into<KnowledgeObjectId>,
        canonical_name: impl Into<String>,
        object_types: Vec<String>,
        event_id: impl Into<KnowledgeEventId>,
        now: DateTime<Utc>,
    ) -> Self {
        let canonical_name = canonical_name.into();
        Self {
            record_type: "object".to_string(),
            object_id: object_id.into(),
            identity: serde_json::json!({ "canonical_name": canonical_name, "aliases": [] }),
            canonical_name,
            object_types,
            status: "active".to_string(),
            created_at: now,
            updated_at: now,
            version: 1,
            last_event_id: event_id.into(),
            trust_score: 0.0,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct KnowledgeAttributeRecord {
    pub record_type: String,
    pub attribute_id: KnowledgeAttributeId,
    pub object_id: KnowledgeObjectId,
    pub attribute_key: String,
    pub attribute_type: String,
    pub value: serde_json::Value,
    pub constraints: serde_json::Value,
    pub status: String,
    pub confidence: f32,
    pub evidence_refs: Vec<EvidenceRefId>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub last_event_id: KnowledgeEventId,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct KnowledgeRelationRecord {
    pub record_type: String,
    pub relation_id: KnowledgeRelationId,
    pub from_object_id: KnowledgeObjectId,
    pub relation_type: String,
    pub to_object_id: KnowledgeObjectId,
    pub direction: String,
    pub relation_attributes: serde_json::Value,
    pub status: String,
    pub confidence: f32,
    pub evidence_refs: Vec<EvidenceRefId>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub last_event_id: KnowledgeEventId,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct KnowledgeClaimRecord {
    pub record_type: String,
    pub claim_id: KnowledgeClaimId,
    pub subject: serde_json::Value,
    pub claim_text: String,
    pub claim_status: String,
    pub confidence: f32,
    pub evidence_refs: Vec<EvidenceRefId>,
    pub negotiation_refs: Vec<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KnowledgeAssetStatus {
    ImportedUnbound,
    CandidateBound,
    ActiveBound,
    EvidenceBound,
    Deprecated,
    GarbageCollectable,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KnowledgeAssetRepresentationKind {
    MetadataOnly,
    BindingSummary,
    SemanticSummary,
    OcrText,
    Transcript,
    Thumbnail,
    Preview,
    OptimizedBlob,
    RawBlob,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "target_type")]
pub enum KnowledgeBindingTarget {
    Object {
        object_id: KnowledgeObjectId,
    },
    ObjectAttribute {
        object_id: KnowledgeObjectId,
        attribute_key: String,
    },
    ObjectState {
        object_id: KnowledgeObjectId,
        state_key: String,
    },
    Relation {
        relation_id: KnowledgeRelationId,
    },
    Claim {
        claim_id: KnowledgeClaimId,
    },
    Evidence {
        evidence_id: EvidenceRefId,
    },
    Event {
        event_id: KnowledgeEventId,
    },
}

impl KnowledgeBindingTarget {
    pub fn object_attribute(
        object_id: impl Into<KnowledgeObjectId>,
        attribute_key: impl Into<String>,
    ) -> Self {
        Self::ObjectAttribute {
            object_id: object_id.into(),
            attribute_key: attribute_key.into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct KnowledgeAssetVisibility {
    pub llm_access: String,
    pub raw_blob_access: bool,
}

impl Default for KnowledgeAssetVisibility {
    fn default() -> Self {
        Self {
            llm_access: "metadata_and_derivatives_only".to_string(),
            raw_blob_access: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KnowledgeBlobMetadata {
    pub blob_hash: String,
    pub size_bytes: u64,
    pub mime_type_detected: String,
    pub compression: String,
    pub created_at: DateTime<Utc>,
    pub integrity: serde_json::Value,
    pub storage: serde_json::Value,
}

impl KnowledgeBlobMetadata {
    pub fn sha256(
        hash: impl Into<String>,
        size_bytes: u64,
        mime_type_detected: impl Into<String>,
        relative_path: impl Into<String>,
        created_at: DateTime<Utc>,
    ) -> Self {
        let hash = hash.into();
        Self {
            blob_hash: hash.clone(),
            size_bytes,
            mime_type_detected: mime_type_detected.into(),
            compression: "none".to_string(),
            created_at,
            integrity: serde_json::json!({
                "hash_algorithm": "sha256",
                "verified_at": created_at,
            }),
            storage: serde_json::json!({
                "path": relative_path.into(),
                "encrypted": false,
            }),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct KnowledgeAssetRecord {
    pub record_type: String,
    pub asset_id: AssetId,
    pub asset_type: String,
    pub mime_type: String,
    pub original_filename: Option<String>,
    pub blob_refs: HashMap<String, String>,
    pub title: Option<String>,
    pub description: Option<String>,
    pub source: serde_json::Value,
    pub technical_metadata: serde_json::Value,
    pub semantic_metadata: serde_json::Value,
    pub status: KnowledgeAssetStatus,
    pub visibility: KnowledgeAssetVisibility,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub last_event_id: KnowledgeEventId,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct KnowledgeAssetBindingConfidence {
    pub extraction_confidence: f32,
    pub binding_confidence: f32,
    pub truth_confidence: f32,
}

impl KnowledgeAssetBindingConfidence {
    pub fn candidate(extraction_confidence: f32, binding_confidence: f32) -> Self {
        Self {
            extraction_confidence,
            binding_confidence,
            truth_confidence: 0.0,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct KnowledgeAssetPropertyBindingRecord {
    pub record_type: String,
    pub binding_id: KnowledgeAssetBindingId,
    pub asset_id: AssetId,
    pub binding_target: KnowledgeBindingTarget,
    pub evidence_mode: String,
    pub observed_value: serde_json::Value,
    pub localization: serde_json::Value,
    pub confidence: KnowledgeAssetBindingConfidence,
    pub status: String,
    pub extracted_by: serde_json::Value,
    pub evidence_refs: Vec<EvidenceRefId>,
    pub negotiation_refs: Vec<String>,
    pub created_at: DateTime<Utc>,
}

impl KnowledgeAssetPropertyBindingRecord {
    pub fn candidate(
        binding_id: impl Into<KnowledgeAssetBindingId>,
        asset_id: impl Into<AssetId>,
        binding_target: KnowledgeBindingTarget,
        evidence_mode: impl Into<String>,
        observed_value: serde_json::Value,
        confidence: KnowledgeAssetBindingConfidence,
        created_at: DateTime<Utc>,
    ) -> Self {
        Self {
            record_type: "asset_property_binding".to_string(),
            binding_id: binding_id.into(),
            asset_id: asset_id.into(),
            binding_target,
            evidence_mode: evidence_mode.into(),
            observed_value,
            localization: serde_json::json!({}),
            confidence,
            status: "candidate".to_string(),
            extracted_by: serde_json::json!({}),
            evidence_refs: Vec::new(),
            negotiation_refs: Vec::new(),
            created_at,
        }
    }
}

/// Immutable audit record for both query and command operations. Query audit is
/// intentionally first-class because retrieved context can shape downstream LLM decisions.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct KnowledgeAuditRecord {
    pub audit_id: KnowledgeAuditId,
    pub timestamp: DateTime<Utc>,
    pub operation_type: String,
    pub operation_id: String,
    pub transaction_id: Option<KnowledgeTransactionId>,
    pub actor: KnowledgeActor,
    pub interface: serde_json::Value,
    pub target: serde_json::Value,
    pub request: serde_json::Value,
    pub process: serde_json::Value,
    pub result: serde_json::Value,
    pub prev_audit_hash: Option<String>,
    pub audit_hash: Option<String>,
}

impl KnowledgeAuditRecord {
    pub fn query(
        audit_id: impl Into<KnowledgeAuditId>,
        operation_id: impl Into<String>,
        actor: KnowledgeActor,
        endpoint: impl Into<String>,
        params_hash: impl Into<String>,
        result_count: usize,
        returned_ids: Vec<String>,
        timestamp: DateTime<Utc>,
    ) -> Self {
        Self {
            audit_id: audit_id.into(),
            timestamp,
            operation_type: "query".to_string(),
            operation_id: operation_id.into(),
            transaction_id: None,
            actor,
            interface: serde_json::json!({ "kind": "engine_api", "endpoint": endpoint.into() }),
            target: serde_json::json!({}),
            request: serde_json::json!({ "params_hash": params_hash.into() }),
            process: serde_json::json!({}),
            result: serde_json::json!({
                "status": "returned",
                "result_summary": {
                    "result_count": result_count,
                    "returned_ids": returned_ids,
                    "truncated": false,
                }
            }),
            prev_audit_hash: None,
            audit_hash: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KnowledgeEngineAccessMode {
    /// LLM receives controlled packets/derivatives only; raw files stay behind the engine.
    LlmControlledPacket,
    /// Human or advanced tooling uses the same engine API, with audit.
    EngineApi,
    /// Direct filesystem access is represented only so validators can reject it.
    DirectFilesystem,
}

impl KnowledgeEngineAccessMode {
    pub fn is_allowed(self) -> bool {
        !matches!(self, Self::DirectFilesystem)
    }
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

    async fn update_entry(
        &self,
        id: &KnowledgeEntryId,
        update: KnowledgeEntryUpdate,
    ) -> Result<KnowledgeEntryRef, KnowledgeRepositoryError>;
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
                permission_required: false,
                confidentiality: None,
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

    async fn update_entry(
        &self,
        id: &KnowledgeEntryId,
        update: KnowledgeEntryUpdate,
    ) -> Result<KnowledgeEntryRef, KnowledgeRepositoryError> {
        let mut entries = self
            .entries
            .lock()
            .map_err(|_| KnowledgeRepositoryError::LockPoisoned)?;

        let stored = entries
            .get_mut(id)
            .ok_or_else(|| KnowledgeRepositoryError::NotFound(id.0.clone()))?;

        if let Some(title) = update.title {
            stored.entry_ref.title = title;
        }
        if let Some(content) = update.content_markdown {
            stored.content_markdown = content;
        }
        if let Some(tags) = update.tags {
            stored.tags = tags;
        }
        if let Some(source_uri) = update.source_uri {
            stored.entry_ref.source_uri = Some(source_uri);
        }
        // metadata is not stored in MemoryKnowledgeRepository's StoredKnowledgeEntry

        Ok(stored.entry_ref.clone())
    }
}

/// Deterministic action executor for the first knowledge action seam.
pub struct KnowledgeActionExecutor {
    repository: Arc<dyn KnowledgeRepository>,
}

impl KnowledgeActionExecutor {
    pub fn new(repository: Arc<dyn KnowledgeRepository>) -> Self {
        Self { repository }
    }
}

#[async_trait]
impl ActionExecutor for KnowledgeActionExecutor {
    async fn execute(&self, request: &ActionRequest) -> Result<ActionResult, ActionExecutorError> {
        let payload = match request.action_kind.0.as_str() {
            KNOWLEDGE_SEARCH_ACTION_KIND => {
                let input: KnowledgeSearchActionInput = parse_action_input(&request.input)?;
                let results = self
                    .repository
                    .search(&input.query)
                    .await
                    .map_err(repository_error_to_executor_error)?;
                ActionResultPayload::Json(serde_json::to_value(results).map_err(json_error)?)
            }
            KNOWLEDGE_GET_ENTRY_ACTION_KIND => {
                let input: KnowledgeGetEntryActionInput = parse_action_input(&request.input)?;
                let entry = self
                    .repository
                    .get_entry(&input.id)
                    .await
                    .map_err(repository_error_to_executor_error)?;
                ActionResultPayload::Json(serde_json::to_value(entry).map_err(json_error)?)
            }
            KNOWLEDGE_CREATE_DRAFT_ACTION_KIND => {
                let input: KnowledgeCreateDraftActionInput = parse_action_input(&request.input)?;
                let draft = input
                    .into_draft()
                    .map_err(validation_error_to_executor_error)?;
                ActionResultPayload::Json(serde_json::to_value(draft).map_err(json_error)?)
            }
            KNOWLEDGE_SAVE_ENTRY_ACTION_KIND => {
                let input: KnowledgeSaveEntryActionInput = parse_action_input(&request.input)?;
                input
                    .draft
                    .validate_for_write()
                    .map_err(validation_error_to_executor_error)?;
                let entry = self
                    .repository
                    .save_draft(input.draft)
                    .await
                    .map_err(repository_error_to_executor_error)?;
                ActionResultPayload::Json(serde_json::to_value(entry).map_err(json_error)?)
            }
            KNOWLEDGE_UPDATE_ENTRY_ACTION_KIND => {
                let input: KnowledgeUpdateEntryActionInput = parse_action_input(&request.input)?;
                let entry = self
                    .repository
                    .update_entry(&input.id, input.update)
                    .await
                    .map_err(repository_error_to_executor_error)?;
                ActionResultPayload::Json(serde_json::to_value(entry).map_err(json_error)?)
            }
            _ => {
                return Err(ActionExecutorError::NotSupported(
                    request.action_kind.clone(),
                ));
            }
        };

        Ok(ActionResult {
            status: ActionStatus::Completed,
            payload,
            summary: format!("{} completed", request.action_kind),
            completed_at: Utc::now(),
        })
    }
}

fn parse_action_input<T: for<'de> Deserialize<'de>>(
    input: &serde_json::Value,
) -> Result<T, ActionExecutorError> {
    serde_json::from_value(input.clone()).map_err(json_error)
}

fn json_error(err: serde_json::Error) -> ActionExecutorError {
    ActionExecutorError::InvalidInput(err.to_string())
}

fn validation_error_to_executor_error(err: KnowledgeValidationError) -> ActionExecutorError {
    ActionExecutorError::InvalidInput(err.to_string())
}

fn repository_error_to_executor_error(err: KnowledgeRepositoryError) -> ActionExecutorError {
    ActionExecutorError::ExecutionFailed(err.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use action_core::{ActionId, ActionRequest};
    use capability_policy::{CapabilityPolicy, PolicyDecision};

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

    fn create_draft_input(title: &str, content_markdown: &str) -> KnowledgeCreateDraftActionInput {
        KnowledgeCreateDraftActionInput {
            title: title.to_string(),
            content_markdown: content_markdown.to_string(),
            source_uri: Some("https://example.com/source".to_string()),
            source_artifact_id: Some(ArtifactId::from("artifact-source-1")),
            source_asset_id: Some(AssetId::from("asset-source-1")),
            tags: vec!["agent-os".to_string()],
            metadata: serde_json::json!({ "source": "test" }),
            created_at: ts(),
        }
    }

    fn action_request(kind: ActionKind, input: serde_json::Value) -> ActionRequest {
        ActionRequest {
            action_id: ActionId::from("action-knowledge-1"),
            action_kind: kind,
            input,
            requested_by: "user-1".to_string(),
            conversation_id: Some("conversation-1".to_string()),
            message_id: Some("message-1".to_string()),
            requested_at: ts(),
        }
    }

    fn registry_with_knowledge_actions() -> ActionRegistry {
        let mut registry = ActionRegistry::new();
        register_knowledge_action_schemas(&mut registry).unwrap();
        registry
    }

    fn sample_entry(id: &str, title: &str) -> KnowledgeEntryRef {
        KnowledgeEntryRef {
            id: KnowledgeEntryId::from(id),
            title: title.to_string(),
            source_uri: None,
            artifact_id: None,
            asset_id: None,
            created_at: ts(),
        }
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
            permission_required: false,
            confidentiality: None,
        };

        let json = serde_json::to_string_pretty(&result).unwrap();
        let decoded: KnowledgeSearchResult = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded, result);
    }

    #[test]
    fn knowledge_action_inputs_roundtrip() {
        let search = KnowledgeSearchActionInput {
            query: KnowledgeSearchQuery::new("agentos"),
        };
        let get = KnowledgeGetEntryActionInput {
            id: KnowledgeEntryId::from("knowledge-entry-1"),
        };
        let create = create_draft_input("AgentOS Notes", "content");
        let save = KnowledgeSaveEntryActionInput {
            draft: draft("AgentOS Notes", "content", vec!["agent-os"]),
        };

        assert_eq!(
            serde_json::from_str::<KnowledgeSearchActionInput>(
                &serde_json::to_string(&search).unwrap()
            )
            .unwrap(),
            search
        );
        assert_eq!(
            serde_json::from_str::<KnowledgeGetEntryActionInput>(
                &serde_json::to_string(&get).unwrap()
            )
            .unwrap(),
            get
        );
        assert_eq!(
            serde_json::from_str::<KnowledgeCreateDraftActionInput>(
                &serde_json::to_string(&create).unwrap()
            )
            .unwrap(),
            create
        );
        assert_eq!(
            serde_json::from_str::<KnowledgeSaveEntryActionInput>(
                &serde_json::to_string(&save).unwrap()
            )
            .unwrap(),
            save
        );
    }

    #[test]
    fn register_knowledge_action_schemas_adds_expected_actions() {
        let registry = registry_with_knowledge_actions();

        assert_eq!(registry.len(), 4);
        assert!(registry.get(&knowledge_search_action_kind()).is_some());
        assert!(registry.get(&knowledge_get_entry_action_kind()).is_some());
        assert!(
            registry
                .get(&knowledge_create_draft_action_kind())
                .is_some()
        );
        assert!(registry.get(&knowledge_save_entry_action_kind()).is_some());
    }

    #[test]
    fn knowledge_action_schema_side_effects_match_policy_contract() {
        let registry = registry_with_knowledge_actions();

        assert_eq!(
            registry.side_effect(&knowledge_search_action_kind()),
            Some(&SideEffectKind::ReadOnly)
        );
        assert_eq!(
            registry.side_effect(&knowledge_get_entry_action_kind()),
            Some(&SideEffectKind::ReadOnly)
        );
        assert_eq!(
            registry.side_effect(&knowledge_create_draft_action_kind()),
            Some(&SideEffectKind::RuntimeStateMutation)
        );
        assert_eq!(
            registry.side_effect(&knowledge_save_entry_action_kind()),
            Some(&SideEffectKind::FileSystemMutation)
        );
    }

    #[test]
    fn knowledge_search_and_get_are_allowed_by_default_safe_policy() {
        let registry = registry_with_knowledge_actions();
        let policy = CapabilityPolicy::default_safe();

        for kind in [
            knowledge_search_action_kind(),
            knowledge_get_entry_action_kind(),
        ] {
            let request = action_request(kind, serde_json::json!({}));
            assert_eq!(
                policy.evaluate_with_registry(&request, &registry),
                PolicyDecision::Allow
            );
        }
    }

    #[test]
    fn knowledge_create_draft_requires_approval_by_default_safe_policy() {
        let registry = registry_with_knowledge_actions();
        let policy = CapabilityPolicy::default_safe();
        let request = action_request(knowledge_create_draft_action_kind(), serde_json::json!({}));

        assert!(policy.evaluate_with_registry(&request, &registry).is_ask());
    }

    #[test]
    fn knowledge_save_entry_is_denied_by_default_safe_policy() {
        let registry = registry_with_knowledge_actions();
        let policy = CapabilityPolicy::default_safe();
        let request = action_request(knowledge_save_entry_action_kind(), serde_json::json!({}));

        assert!(
            policy
                .evaluate_with_registry(&request, &registry)
                .is_denied()
        );
    }

    #[test]
    fn knowledge_fulltext_backend_kind_defaults_to_deterministic_in_process() {
        assert_eq!(
            KnowledgeFullTextBackendKind::default(),
            KnowledgeFullTextBackendKind::DeterministicInProcess
        );
    }

    #[tokio::test]
    async fn deterministic_fulltext_backend_ranks_title_matches_above_body_matches() {
        let mut backend = DeterministicFullTextKnowledgeBackend::new();
        backend
            .rebuild(KnowledgeIndexRebuildRequest {
                documents: vec![
                    KnowledgeIndexDocument::new(
                        KnowledgeEntryRef {
                            id: KnowledgeEntryId::from("knowledge-entry-1"),
                            title: "Architecture Notes".to_string(),
                            source_uri: None,
                            artifact_id: None,
                            asset_id: None,
                            created_at: ts(),
                        },
                        "agent memory architecture details".to_string(),
                    ),
                    KnowledgeIndexDocument::new(
                        KnowledgeEntryRef {
                            id: KnowledgeEntryId::from("knowledge-entry-2"),
                            title: "Memory Architecture".to_string(),
                            source_uri: None,
                            artifact_id: None,
                            asset_id: None,
                            created_at: ts(),
                        },
                        "short note".to_string(),
                    ),
                ],
                requested_at: ts(),
            })
            .await
            .unwrap();

        let results = backend
            .query(&KnowledgeFullTextQuery::new("memory"))
            .await
            .unwrap();

        assert_eq!(results.len(), 2);
        assert_eq!(
            results[0].entry.id,
            KnowledgeEntryId::from("knowledge-entry-2")
        );
        assert!(results[0].score > results[1].score);
    }

    #[tokio::test]
    async fn deterministic_fulltext_backend_indexes_title_body_tags_and_frontmatter() {
        let mut backend = DeterministicFullTextKnowledgeBackend::new();
        backend
            .rebuild(KnowledgeIndexRebuildRequest {
                documents: vec![
                    KnowledgeIndexDocument::new(
                        KnowledgeEntryRef {
                            id: KnowledgeEntryId::from("knowledge-entry-1"),
                            title: "AgentOS Memory".to_string(),
                            source_uri: None,
                            artifact_id: None,
                            asset_id: None,
                            created_at: ts(),
                        },
                        "long-term governance".to_string(),
                    )
                    .with_tags(vec!["knowledge".to_string()])
                    .with_frontmatter(serde_json::json!({ "domain": "memory" })),
                ],
                requested_at: ts(),
            })
            .await
            .unwrap();

        for query in ["agentos", "governance", "knowledge", "memory"] {
            let results = backend
                .query(&KnowledgeFullTextQuery::new(query))
                .await
                .unwrap();
            assert_eq!(
                results.len(),
                1,
                "query should match indexed field: {query}"
            );
            assert_eq!(
                results[0].entry.id,
                KnowledgeEntryId::from("knowledge-entry-1")
            );
        }
    }

    #[tokio::test]
    async fn deterministic_fulltext_backend_breaks_score_ties_by_entry_id() {
        let mut backend = DeterministicFullTextKnowledgeBackend::new();
        backend
            .rebuild(KnowledgeIndexRebuildRequest {
                documents: vec![
                    KnowledgeIndexDocument::new(
                        KnowledgeEntryRef {
                            id: KnowledgeEntryId::from("knowledge-entry-b"),
                            title: "Memory".to_string(),
                            source_uri: None,
                            artifact_id: None,
                            asset_id: None,
                            created_at: ts(),
                        },
                        "body".to_string(),
                    ),
                    KnowledgeIndexDocument::new(
                        KnowledgeEntryRef {
                            id: KnowledgeEntryId::from("knowledge-entry-a"),
                            title: "Memory".to_string(),
                            source_uri: None,
                            artifact_id: None,
                            asset_id: None,
                            created_at: ts(),
                        },
                        "body".to_string(),
                    ),
                ],
                requested_at: ts(),
            })
            .await
            .unwrap();

        let results = backend
            .query(&KnowledgeFullTextQuery::new("memory"))
            .await
            .unwrap();

        assert_eq!(
            results
                .iter()
                .map(|result| &result.entry.id)
                .collect::<Vec<_>>(),
            vec![
                &KnowledgeEntryId::from("knowledge-entry-a"),
                &KnowledgeEntryId::from("knowledge-entry-b")
            ]
        );
    }

    #[tokio::test]
    async fn memory_fulltext_index_rebuilds_and_queries_entries() {
        let mut index = MemoryFullTextKnowledgeIndex::new();
        let first = KnowledgeIndexDocument::new(
            KnowledgeEntryRef {
                id: KnowledgeEntryId::from("knowledge-entry-1"),
                title: "AgentOS Memory".to_string(),
                source_uri: None,
                artifact_id: None,
                asset_id: None,
                created_at: ts(),
            },
            "long-term memory and asset governance".to_string(),
        )
        .with_tags(vec!["memory".to_string(), "agent-os".to_string()])
        .with_frontmatter(serde_json::json!({ "status": "active" }));
        let second = KnowledgeIndexDocument::new(
            KnowledgeEntryRef {
                id: KnowledgeEntryId::from("knowledge-entry-2"),
                title: "Browser Kernel".to_string(),
                source_uri: None,
                artifact_id: None,
                asset_id: None,
                created_at: ts(),
            },
            "browser security policy and takeover boundary".to_string(),
        )
        .with_tags(vec!["browser".to_string()]);

        let report = index
            .rebuild(KnowledgeIndexRebuildRequest {
                documents: vec![first, second],
                requested_at: ts(),
            })
            .await
            .unwrap();

        assert_eq!(report.indexed_count, 2);
        assert_eq!(report.deleted_count, 0);
        assert_eq!(report.rebuilt_at, ts());

        let results = index
            .query(&KnowledgeFullTextQuery::new("memory governance").with_limit(5))
            .await
            .unwrap();

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].entry.title, "AgentOS Memory");
        assert!(results[0].score > 0.0);
        assert!(results[0].snippet.as_ref().unwrap().contains("memory"));
    }

    #[tokio::test]
    async fn memory_fulltext_index_filters_by_tags_and_frontmatter() {
        let mut index = MemoryFullTextKnowledgeIndex::new();
        index
            .rebuild(KnowledgeIndexRebuildRequest {
                documents: vec![
                    KnowledgeIndexDocument::new(
                        KnowledgeEntryRef {
                            id: KnowledgeEntryId::from("knowledge-entry-1"),
                            title: "Published Memory".to_string(),
                            source_uri: None,
                            artifact_id: None,
                            asset_id: None,
                            created_at: ts(),
                        },
                        "memory index content".to_string(),
                    )
                    .with_tags(vec!["memory".to_string()])
                    .with_frontmatter(serde_json::json!({ "status": "published" })),
                    KnowledgeIndexDocument::new(
                        KnowledgeEntryRef {
                            id: KnowledgeEntryId::from("knowledge-entry-2"),
                            title: "Draft Memory".to_string(),
                            source_uri: None,
                            artifact_id: None,
                            asset_id: None,
                            created_at: ts(),
                        },
                        "memory index content".to_string(),
                    )
                    .with_tags(vec!["memory".to_string()])
                    .with_frontmatter(serde_json::json!({ "status": "draft" })),
                ],
                requested_at: ts(),
            })
            .await
            .unwrap();

        let results = index
            .query(
                &KnowledgeFullTextQuery::new("memory")
                    .with_tags(vec!["memory".to_string()])
                    .with_frontmatter_filter("status", "published"),
            )
            .await
            .unwrap();

        assert_eq!(results.len(), 1);
        assert_eq!(
            results[0].entry.id,
            KnowledgeEntryId::from("knowledge-entry-1")
        );
    }

    #[tokio::test]
    async fn memory_fulltext_index_upserts_and_deletes_documents() {
        let mut index = MemoryFullTextKnowledgeIndex::new();
        let doc = KnowledgeIndexDocument::new(
            KnowledgeEntryRef {
                id: KnowledgeEntryId::from("knowledge-entry-1"),
                title: "Original".to_string(),
                source_uri: None,
                artifact_id: None,
                asset_id: None,
                created_at: ts(),
            },
            "original content".to_string(),
        );
        index.upsert(doc).await.unwrap();
        index
            .upsert(KnowledgeIndexDocument::new(
                KnowledgeEntryRef {
                    id: KnowledgeEntryId::from("knowledge-entry-1"),
                    title: "Updated".to_string(),
                    source_uri: None,
                    artifact_id: None,
                    asset_id: None,
                    created_at: ts(),
                },
                "updated content".to_string(),
            ))
            .await
            .unwrap();

        let results = index
            .query(&KnowledgeFullTextQuery::new("updated"))
            .await
            .unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].entry.title, "Updated");

        index
            .delete(&KnowledgeEntryId::from("knowledge-entry-1"))
            .await
            .unwrap();
        assert!(
            index
                .query(&KnowledgeFullTextQuery::new("updated"))
                .await
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn knowledge_fulltext_query_defaults_are_stable() {
        let query = KnowledgeFullTextQuery::new("agent memory");

        assert_eq!(query.text, "agent memory");
        assert_eq!(query.limit, 10);
        assert!(query.tags.is_empty());
        assert!(query.frontmatter_filters.is_empty());
    }

    #[test]
    fn citation_evidence_ref_roundtrips_all_supported_sources() {
        let refs = vec![
            CitationEvidenceRef::source_uri("https://example.com/source"),
            CitationEvidenceRef::artifact(ArtifactId::from("artifact-1")),
            CitationEvidenceRef::conversation_message("conversation-1", "message-1"),
            CitationEvidenceRef::browser_snapshot(
                BrowserSnapshotId::from("snapshot-1"),
                Some("https://example.com/page".to_string()),
            ),
        ];

        let json = serde_json::to_string_pretty(&refs).unwrap();
        let decoded: Vec<CitationEvidenceRef> = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded, refs);
    }

    #[test]
    fn citation_evidence_record_preserves_excerpt_and_ref() {
        let evidence = CitationEvidenceRecord::new(
            EvidenceRefId::from("evidence-1"),
            CitationEvidenceRef::conversation_message("conversation-1", "message-1"),
            ts(),
        )
        .with_excerpt("User asked for durable answer traceability.");

        let json = serde_json::to_string_pretty(&evidence).unwrap();
        let decoded: CitationEvidenceRecord = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded, evidence);
        assert_eq!(
            decoded.excerpt.as_deref(),
            Some("User asked for durable answer traceability.")
        );
    }

    #[tokio::test]
    async fn memory_citation_store_links_answer_to_traceable_evidence() {
        let store = MemoryCitationEvidenceStore::new();
        let source = store
            .register_evidence(
                CitationEvidenceRef::source_uri("https://example.com/source"),
                Some("source excerpt".to_string()),
                ts(),
            )
            .await
            .unwrap();
        let artifact = store
            .register_evidence(
                CitationEvidenceRef::artifact(ArtifactId::from("artifact-1")),
                None,
                ts(),
            )
            .await
            .unwrap();
        let conversation = store
            .register_evidence(
                CitationEvidenceRef::conversation_message("conversation-1", "message-1"),
                None,
                ts(),
            )
            .await
            .unwrap();
        let browser = store
            .register_evidence(
                CitationEvidenceRef::browser_snapshot(
                    BrowserSnapshotId::from("snapshot-1"),
                    Some("https://example.com/page".to_string()),
                ),
                None,
                ts(),
            )
            .await
            .unwrap();

        store
            .cite_answer("answer-1", &browser.id, ts())
            .await
            .unwrap();
        store
            .cite_answer("answer-1", &source.id, ts())
            .await
            .unwrap();
        store
            .cite_answer("answer-1", &conversation.id, ts())
            .await
            .unwrap();
        store
            .cite_answer("answer-1", &artifact.id, ts())
            .await
            .unwrap();

        let trace = store.trace_answer("answer-1").await.unwrap();
        assert_eq!(trace.answer_id, "answer-1");
        assert_eq!(
            trace
                .evidence
                .iter()
                .map(|e| e.id.clone())
                .collect::<Vec<_>>(),
            vec![source.id, artifact.id, conversation.id, browser.id]
        );
        assert!(
            trace
                .evidence
                .iter()
                .any(|e| matches!(e.source_ref, CitationEvidenceRef::SourceUri { .. }))
        );
        assert!(
            trace
                .evidence
                .iter()
                .any(|e| matches!(e.source_ref, CitationEvidenceRef::Artifact { .. }))
        );
        assert!(trace.evidence.iter().any(|e| matches!(
            e.source_ref,
            CitationEvidenceRef::ConversationMessage { .. }
        )));
        assert!(
            trace
                .evidence
                .iter()
                .any(|e| matches!(e.source_ref, CitationEvidenceRef::BrowserSnapshot { .. }))
        );
    }

    #[tokio::test]
    async fn memory_citation_store_rejects_missing_evidence_and_blank_answer() {
        let store = MemoryCitationEvidenceStore::new();
        assert_eq!(
            store
                .cite_answer("answer-1", &EvidenceRefId::from("missing"), ts())
                .await
                .unwrap_err(),
            CitationEvidenceError::NotFound("missing".to_string())
        );

        let evidence = store
            .register_evidence(
                CitationEvidenceRef::source_uri("https://example.com/source"),
                None,
                ts(),
            )
            .await
            .unwrap();
        assert_eq!(
            store
                .cite_answer("   ", &evidence.id, ts())
                .await
                .unwrap_err(),
            CitationEvidenceError::BlankAnswerId
        );
    }

    #[tokio::test]
    async fn memory_citation_store_dedupes_answer_citations() {
        let store = MemoryCitationEvidenceStore::new();
        let evidence = store
            .register_evidence(
                CitationEvidenceRef::source_uri("https://example.com/source"),
                None,
                ts(),
            )
            .await
            .unwrap();

        store
            .cite_answer("answer-1", &evidence.id, ts())
            .await
            .unwrap();
        store
            .cite_answer("answer-1", &evidence.id, ts())
            .await
            .unwrap();

        let trace = store.trace_answer("answer-1").await.unwrap();
        assert_eq!(trace.evidence, vec![evidence]);
    }

    #[test]
    fn governance_status_roundtrips() {
        let statuses = vec![
            KnowledgeGovernanceStatus::Draft,
            KnowledgeGovernanceStatus::Active,
            KnowledgeGovernanceStatus::Deprecated,
            KnowledgeGovernanceStatus::Archived,
        ];
        let json = serde_json::to_string(&statuses).unwrap();
        let decoded: Vec<KnowledgeGovernanceStatus> = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded, statuses);
    }

    #[test]
    fn governance_validates_required_frontmatter_for_active_entries() {
        let valid = KnowledgeGovernanceRecord::new(
            KnowledgeEntryId::from("knowledge-entry-1"),
            KnowledgeGovernanceStatus::Draft,
            serde_json::json!({ "title": "AgentOS", "summary": "Durable memory", "tags": ["memory"] }),
            ts(),
        );
        assert!(valid.validation_errors().is_empty());
        assert_eq!(
            valid.activate(ts()).unwrap().status,
            KnowledgeGovernanceStatus::Active
        );

        let invalid = KnowledgeGovernanceRecord::new(
            KnowledgeEntryId::from("knowledge-entry-2"),
            KnowledgeGovernanceStatus::Draft,
            serde_json::json!({ "title": "   ", "summary": "", "tags": [] }),
            ts(),
        );
        assert_eq!(
            invalid.validation_errors(),
            vec![
                KnowledgeGovernanceValidationError::MissingTitle,
                KnowledgeGovernanceValidationError::MissingSummary,
                KnowledgeGovernanceValidationError::MissingTags,
            ]
        );
        assert_eq!(
            invalid.activate(ts()).unwrap_err(),
            KnowledgeGovernanceError::ValidationFailed(vec![
                KnowledgeGovernanceValidationError::MissingTitle,
                KnowledgeGovernanceValidationError::MissingSummary,
                KnowledgeGovernanceValidationError::MissingTags,
            ])
        );
    }

    #[tokio::test]
    async fn memory_governance_review_queue_lists_non_active_invalid_records() {
        let store = MemoryKnowledgeGovernanceStore::new();
        let invalid = store
            .upsert_record(KnowledgeGovernanceRecord::new(
                KnowledgeEntryId::from("knowledge-entry-1"),
                KnowledgeGovernanceStatus::Draft,
                serde_json::json!({ "title": "", "summary": "", "tags": [] }),
                ts(),
            ))
            .await
            .unwrap();
        store
            .upsert_record(KnowledgeGovernanceRecord::new(
                KnowledgeEntryId::from("knowledge-entry-2"),
                KnowledgeGovernanceStatus::Active,
                serde_json::json!({ "title": "Ready", "summary": "Ready summary", "tags": ["ready"] }),
                ts(),
            ))
            .await
            .unwrap();

        let queue = store.review_queue().await.unwrap();
        assert_eq!(queue, vec![KnowledgeReviewQueueItem::from_record(&invalid)]);
    }

    #[tokio::test]
    async fn memory_governance_rejects_invalid_activation_and_keeps_out_of_active_index() {
        let store = MemoryKnowledgeGovernanceStore::new();
        store
            .upsert_record(KnowledgeGovernanceRecord::new(
                KnowledgeEntryId::from("knowledge-entry-1"),
                KnowledgeGovernanceStatus::Draft,
                serde_json::json!({ "title": "", "summary": "", "tags": [] }),
                ts(),
            ))
            .await
            .unwrap();

        let err = store
            .transition_status(
                &KnowledgeEntryId::from("knowledge-entry-1"),
                KnowledgeGovernanceStatus::Active,
                ts(),
            )
            .await
            .unwrap_err();
        assert!(matches!(err, KnowledgeGovernanceError::ValidationFailed(_)));
        assert!(
            store.active_index_entry_ids().await.unwrap().is_empty(),
            "invalid frontmatter must not enter active index"
        );
    }

    #[tokio::test]
    async fn memory_governance_active_index_contains_only_valid_active_records() {
        let store = MemoryKnowledgeGovernanceStore::new();
        store
            .upsert_record(KnowledgeGovernanceRecord::new(
                KnowledgeEntryId::from("knowledge-entry-1"),
                KnowledgeGovernanceStatus::Draft,
                serde_json::json!({ "title": "Ready", "summary": "Ready summary", "tags": ["ready"] }),
                ts(),
            ))
            .await
            .unwrap();
        store
            .transition_status(
                &KnowledgeEntryId::from("knowledge-entry-1"),
                KnowledgeGovernanceStatus::Active,
                ts(),
            )
            .await
            .unwrap();
        store
            .upsert_record(KnowledgeGovernanceRecord::new(
                KnowledgeEntryId::from("knowledge-entry-2"),
                KnowledgeGovernanceStatus::Deprecated,
                serde_json::json!({ "title": "Old", "summary": "Old summary", "tags": ["old"] }),
                ts(),
            ))
            .await
            .unwrap();

        assert_eq!(
            store.active_index_entry_ids().await.unwrap(),
            vec![KnowledgeEntryId::from("knowledge-entry-1")]
        );
    }

    #[test]
    fn answer_cache_package_roundtrips_with_version_evidence_and_freshness() {
        let package = AnswerCachePackage {
            id: AnswerCachePackageId::from("answer-cache-1"),
            question_id: Some(QuestionLedgerEntryId::from("question-1")),
            work_object_id: None,
            answer_id: "answer-1".to_string(),
            version: AnswerCachePackageVersion::new(2).unwrap(),
            answer_markdown: "# Answer\n\nUse durable storage.".to_string(),
            evidence_refs: vec![AnswerEvidenceRef::knowledge_entry(
                KnowledgeEntryId::from("knowledge-entry-1"),
                Some("storage section".to_string()),
            )],
            related_knowledge_entry_ids: vec![KnowledgeEntryId::from("knowledge-entry-1")],
            freshness_policy: AnswerFreshnessPolicy::expires_after_days(30),
            created_at: ts(),
            updated_at: ts(),
        };

        let json = serde_json::to_string_pretty(&package).unwrap();
        let decoded: AnswerCachePackage = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded, package);
    }

    #[test]
    fn answer_cache_package_version_rejects_zero() {
        assert_eq!(
            AnswerCachePackageVersion::new(0).unwrap_err(),
            AnswerCacheError::InvalidVersion
        );
        assert_eq!(AnswerCachePackageVersion::new(1).unwrap().value(), 1);
    }

    #[test]
    fn answer_cache_create_request_collects_related_knowledge_from_evidence() {
        let request = AnswerCacheCreateRequest::new("answer-1", "Use durable storage.", ts())
            .with_question_id(QuestionLedgerEntryId::from("question-1"))
            .with_evidence_refs(vec![
                AnswerEvidenceRef::knowledge_entry(
                    KnowledgeEntryId::from("knowledge-entry-1"),
                    None,
                ),
                AnswerEvidenceRef::conversation_message("conversation-1", "message-1"),
            ])
            .with_freshness_policy(AnswerFreshnessPolicy::expires_after_days(14));

        assert_eq!(
            request.question_id,
            Some(QuestionLedgerEntryId::from("question-1"))
        );
        assert_eq!(request.evidence_refs.len(), 2);
        assert_eq!(
            request.related_knowledge_entry_ids(),
            vec![KnowledgeEntryId::from("knowledge-entry-1")]
        );
        assert_eq!(request.freshness_policy.expires_after_days, Some(14));
    }

    #[tokio::test]
    async fn memory_answer_cache_creates_versions_and_gets_latest_for_question() {
        let store = MemoryAnswerCacheStore::new();
        let first = store
            .create_package(
                AnswerCacheCreateRequest::new("answer-1", "First answer", ts())
                    .with_question_id(QuestionLedgerEntryId::from("question-1")),
            )
            .await
            .unwrap();
        let second = store
            .create_package(
                AnswerCacheCreateRequest::new("answer-1", "Second answer", ts())
                    .with_question_id(QuestionLedgerEntryId::from("question-1")),
            )
            .await
            .unwrap();

        assert_eq!(first.version.value(), 1);
        assert_eq!(second.version.value(), 2);
        let latest = store
            .latest_for_question(&QuestionLedgerEntryId::from("question-1"))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(latest.id, second.id);
        assert_eq!(latest.answer_markdown, "Second answer");
    }

    #[tokio::test]
    async fn memory_answer_cache_gets_package_and_searches_by_knowledge_entry() {
        let store = MemoryAnswerCacheStore::new();
        let package = store
            .create_package(
                AnswerCacheCreateRequest::new("answer-1", "Evidence-backed answer", ts())
                    .with_question_id(QuestionLedgerEntryId::from("question-1"))
                    .with_evidence_refs(vec![AnswerEvidenceRef::knowledge_entry(
                        KnowledgeEntryId::from("knowledge-entry-1"),
                        Some("quote".to_string()),
                    )]),
            )
            .await
            .unwrap();

        assert_eq!(
            store.get_package(&package.id).await.unwrap(),
            Some(package.clone())
        );
        let by_knowledge = store
            .list_by_knowledge_entry(&KnowledgeEntryId::from("knowledge-entry-1"))
            .await
            .unwrap();
        assert_eq!(by_knowledge, vec![package]);
    }

    #[tokio::test]
    async fn memory_answer_cache_rejects_blank_answers() {
        let store = MemoryAnswerCacheStore::new();
        assert_eq!(
            store
                .create_package(AnswerCacheCreateRequest::new("answer-1", "   ", ts()))
                .await
                .unwrap_err(),
            AnswerCacheError::BlankAnswer
        );
    }

    #[test]
    fn question_ledger_entry_roundtrips_with_conversation_and_answer_refs() {
        let entry = QuestionLedgerEntry {
            id: QuestionLedgerEntryId::from("question-1"),
            question: "How should AgentOS store durable memory?".to_string(),
            conversation_id: Some("conversation-1".to_string()),
            message_id: Some("message-1".to_string()),
            work_object_id: None,
            answer_ref: Some(QuestionAnswerRef {
                answer_id: "answer-1".to_string(),
                answer_cache_ref: Some("answer-cache/2026/05/question-1/v1".to_string()),
                knowledge_entry_id: Some(KnowledgeEntryId::from("knowledge-entry-1")),
            }),
            related_knowledge_entry_ids: vec![KnowledgeEntryId::from("knowledge-entry-2")],
            tags: vec!["memory".to_string()],
            created_at: ts(),
            updated_at: ts(),
        };

        let json = serde_json::to_string_pretty(&entry).unwrap();
        let decoded: QuestionLedgerEntry = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded, entry);
    }

    #[test]
    fn question_ledger_create_request_from_conversation_message_is_stable() {
        let request = QuestionLedgerCreateRequest::from_conversation_message(
            "conversation-1",
            "message-1",
            "What should we do next?",
            ts(),
        )
        .with_tags(vec!["planning".to_string()])
        .with_related_knowledge_entry_ids(vec![KnowledgeEntryId::from("knowledge-entry-1")]);

        assert_eq!(request.question, "What should we do next?");
        assert_eq!(request.conversation_id.as_deref(), Some("conversation-1"));
        assert_eq!(request.message_id.as_deref(), Some("message-1"));
        assert_eq!(request.tags, vec!["planning".to_string()]);
        assert_eq!(
            request.related_knowledge_entry_ids,
            vec![KnowledgeEntryId::from("knowledge-entry-1")]
        );
    }

    #[tokio::test]
    async fn memory_question_ledger_creates_entries_from_conversation_messages() {
        let ledger = MemoryQuestionLedger::new();
        let entry = ledger
            .create_question(QuestionLedgerCreateRequest::from_conversation_message(
                "conversation-1",
                "message-1",
                "What should we do next?",
                ts(),
            ))
            .await
            .unwrap();

        assert_eq!(entry.id, QuestionLedgerEntryId::from("question-1"));
        assert_eq!(entry.conversation_id.as_deref(), Some("conversation-1"));
        assert_eq!(entry.message_id.as_deref(), Some("message-1"));

        let by_conversation = ledger.list_by_conversation("conversation-1").await.unwrap();
        assert_eq!(by_conversation, vec![entry]);
    }

    #[tokio::test]
    async fn memory_question_ledger_links_answer_and_knowledge_refs() {
        let ledger = MemoryQuestionLedger::new();
        let entry = ledger
            .create_question(QuestionLedgerCreateRequest::from_conversation_message(
                "conversation-1",
                "message-1",
                "What is the answer?",
                ts(),
            ))
            .await
            .unwrap();

        let linked = ledger
            .link_answer(
                &entry.id,
                QuestionAnswerRef {
                    answer_id: "answer-1".to_string(),
                    answer_cache_ref: Some("answer-cache/2026/05/question-1/v1".to_string()),
                    knowledge_entry_id: Some(KnowledgeEntryId::from("knowledge-entry-1")),
                },
                ts(),
            )
            .await
            .unwrap();

        assert_eq!(linked.answer_ref.as_ref().unwrap().answer_id, "answer-1");
        assert_eq!(
            linked.answer_ref.as_ref().unwrap().knowledge_entry_id,
            Some(KnowledgeEntryId::from("knowledge-entry-1"))
        );
        assert_eq!(linked.updated_at, ts());
    }

    #[tokio::test]
    async fn memory_question_ledger_rejects_blank_questions_and_missing_ids() {
        let ledger = MemoryQuestionLedger::new();
        assert_eq!(
            ledger
                .create_question(QuestionLedgerCreateRequest::new("   ", ts()))
                .await
                .unwrap_err(),
            QuestionLedgerError::BlankQuestion
        );
        assert_eq!(
            ledger
                .link_answer(
                    &QuestionLedgerEntryId::from("missing"),
                    QuestionAnswerRef {
                        answer_id: "answer-1".to_string(),
                        answer_cache_ref: None,
                        knowledge_entry_id: None,
                    },
                    ts(),
                )
                .await
                .unwrap_err(),
            QuestionLedgerError::NotFound("missing".to_string())
        );
    }

    #[test]
    fn knowledge_hybrid_query_defaults_are_stable() {
        let full_text = KnowledgeFullTextQuery::new("agent memory");
        let semantic =
            KnowledgeSemanticQuery::new(KnowledgeEmbeddingVector::new(vec![1.0, 0.0]).unwrap());
        let query = KnowledgeHybridQuery::new(full_text, semantic);

        assert_eq!(query.limit, 10);
        assert_eq!(query.weights.full_text, 0.5);
        assert_eq!(query.weights.semantic, 0.5);
        assert_eq!(query.rerank, KnowledgeHybridRerankMode::Disabled);
    }

    #[tokio::test]
    async fn hybrid_search_merges_and_dedupes_fulltext_and_semantic_hits() {
        let mut full_text = MemoryFullTextKnowledgeIndex::new();
        let mut semantic = MemorySemanticKnowledgeIndex::new();
        let entry_a = sample_entry("knowledge-entry-a", "AgentOS Memory");
        let entry_b = sample_entry("knowledge-entry-b", "AgentOS Runtime");
        let entry_c = sample_entry("knowledge-entry-c", "Browser Kernel");

        full_text
            .upsert(KnowledgeIndexDocument::new(
                entry_a.clone(),
                "agent memory captures reusable context",
            ))
            .await
            .unwrap();
        full_text
            .upsert(KnowledgeIndexDocument::new(
                entry_c.clone(),
                "agent memory browser automation notes",
            ))
            .await
            .unwrap();
        semantic
            .upsert_embedding(KnowledgeEmbeddingDocument::new(
                entry_a.clone(),
                KnowledgeEmbeddingVector::new(vec![1.0, 0.0]).unwrap(),
            ))
            .await
            .unwrap();
        semantic
            .upsert_embedding(KnowledgeEmbeddingDocument::new(
                entry_b.clone(),
                KnowledgeEmbeddingVector::new(vec![0.8, 0.2]).unwrap(),
            ))
            .await
            .unwrap();

        let engine = DeterministicHybridKnowledgeSearch::new(&full_text, &semantic);
        let results = engine
            .query(&KnowledgeHybridQuery::new(
                KnowledgeFullTextQuery::new("agent memory"),
                KnowledgeSemanticQuery::new(KnowledgeEmbeddingVector::new(vec![1.0, 0.0]).unwrap()),
            ))
            .await
            .unwrap();

        assert_eq!(results.len(), 3);
        assert_eq!(
            results[0].entry.id,
            KnowledgeEntryId::from("knowledge-entry-a")
        );
        assert!(results[0].full_text_score.is_some());
        assert!(results[0].semantic_score.is_some());
        assert!(results.iter().any(|result| {
            result.entry.id == KnowledgeEntryId::from("knowledge-entry-b")
                && result.full_text_score.is_none()
                && result.semantic_score.is_some()
        }));
        assert!(results.iter().any(|result| {
            result.entry.id == KnowledgeEntryId::from("knowledge-entry-c")
                && result.full_text_score.is_some()
                && result.semantic_score.is_none()
        }));
    }

    #[tokio::test]
    async fn hybrid_search_breaks_fused_score_ties_by_entry_id() {
        let mut full_text = MemoryFullTextKnowledgeIndex::new();
        let mut semantic = MemorySemanticKnowledgeIndex::new();
        let entry_b = sample_entry("knowledge-entry-b", "Tie B");
        let entry_a = sample_entry("knowledge-entry-a", "Tie A");

        for entry in [entry_b.clone(), entry_a.clone()] {
            full_text
                .upsert(KnowledgeIndexDocument::new(entry.clone(), "same"))
                .await
                .unwrap();
            semantic
                .upsert_embedding(KnowledgeEmbeddingDocument::new(
                    entry,
                    KnowledgeEmbeddingVector::new(vec![1.0, 0.0]).unwrap(),
                ))
                .await
                .unwrap();
        }

        let engine = DeterministicHybridKnowledgeSearch::new(&full_text, &semantic);
        let results = engine
            .query(&KnowledgeHybridQuery::new(
                KnowledgeFullTextQuery::new("same"),
                KnowledgeSemanticQuery::new(KnowledgeEmbeddingVector::new(vec![1.0, 0.0]).unwrap()),
            ))
            .await
            .unwrap();

        assert_eq!(
            results[0].entry.id,
            KnowledgeEntryId::from("knowledge-entry-a")
        );
        assert_eq!(
            results[1].entry.id,
            KnowledgeEntryId::from("knowledge-entry-b")
        );
    }

    #[tokio::test]
    async fn hybrid_search_applies_tags_and_frontmatter_to_both_paths() {
        let mut full_text = MemoryFullTextKnowledgeIndex::new();
        let mut semantic = MemorySemanticKnowledgeIndex::new();
        let matching = sample_entry("knowledge-entry-a", "AgentOS Memory");
        let filtered = sample_entry("knowledge-entry-b", "AgentOS Memory Filtered");

        for (entry, tier) in [(matching.clone(), "gold"), (filtered.clone(), "silver")] {
            full_text
                .upsert(
                    KnowledgeIndexDocument::new(entry.clone(), "agent memory")
                        .with_tags(vec!["memory".to_string()])
                        .with_frontmatter(serde_json::json!({ "tier": tier })),
                )
                .await
                .unwrap();
            semantic
                .upsert_embedding(
                    KnowledgeEmbeddingDocument::new(
                        entry,
                        KnowledgeEmbeddingVector::new(vec![1.0, 0.0]).unwrap(),
                    )
                    .with_tags(vec!["memory".to_string()])
                    .with_frontmatter(serde_json::json!({ "tier": tier })),
                )
                .await
                .unwrap();
        }

        let engine = DeterministicHybridKnowledgeSearch::new(&full_text, &semantic);
        let results = engine
            .query(&KnowledgeHybridQuery::new(
                KnowledgeFullTextQuery::new("agent")
                    .with_tags(vec!["memory".to_string()])
                    .with_frontmatter_filter("tier", "gold"),
                KnowledgeSemanticQuery::new(KnowledgeEmbeddingVector::new(vec![1.0, 0.0]).unwrap())
                    .with_tags(vec!["memory".to_string()])
                    .with_frontmatter_filter("tier", "gold"),
            ))
            .await
            .unwrap();

        assert_eq!(results.len(), 1);
        assert_eq!(
            results[0].entry.id,
            KnowledgeEntryId::from("knowledge-entry-a")
        );
    }

    #[tokio::test]
    async fn hybrid_rerank_hook_can_reorder_but_not_add_entries() {
        let mut full_text = MemoryFullTextKnowledgeIndex::new();
        let mut semantic = MemorySemanticKnowledgeIndex::new();
        let entry_a = sample_entry("knowledge-entry-a", "A");
        let entry_b = sample_entry("knowledge-entry-b", "B");

        for entry in [entry_a.clone(), entry_b.clone()] {
            full_text
                .upsert(KnowledgeIndexDocument::new(entry.clone(), "same"))
                .await
                .unwrap();
            semantic
                .upsert_embedding(KnowledgeEmbeddingDocument::new(
                    entry,
                    KnowledgeEmbeddingVector::new(vec![1.0, 0.0]).unwrap(),
                ))
                .await
                .unwrap();
        }

        let engine = DeterministicHybridKnowledgeSearch::new(&full_text, &semantic)
            .with_reranker(ReverseHybridReranker);
        let results = engine
            .query(&KnowledgeHybridQuery::new(
                KnowledgeFullTextQuery::new("same"),
                KnowledgeSemanticQuery::new(KnowledgeEmbeddingVector::new(vec![1.0, 0.0]).unwrap()),
            ))
            .await
            .unwrap();

        assert_eq!(results.len(), 2);
        assert_eq!(
            results[0].entry.id,
            KnowledgeEntryId::from("knowledge-entry-b")
        );
        assert_eq!(
            results[1].entry.id,
            KnowledgeEntryId::from("knowledge-entry-a")
        );
    }

    #[tokio::test]
    async fn hybrid_search_propagates_semantic_dimension_mismatch() {
        let mut full_text = MemoryFullTextKnowledgeIndex::new();
        let mut semantic = MemorySemanticKnowledgeIndex::new();
        let entry = sample_entry("knowledge-entry-a", "AgentOS Memory");
        full_text
            .upsert(KnowledgeIndexDocument::new(entry.clone(), "agent memory"))
            .await
            .unwrap();
        semantic
            .upsert_embedding(KnowledgeEmbeddingDocument::new(
                entry,
                KnowledgeEmbeddingVector::new(vec![1.0, 0.0]).unwrap(),
            ))
            .await
            .unwrap();

        let engine = DeterministicHybridKnowledgeSearch::new(&full_text, &semantic);
        let error = engine
            .query(&KnowledgeHybridQuery::new(
                KnowledgeFullTextQuery::new("agent"),
                KnowledgeSemanticQuery::new(
                    KnowledgeEmbeddingVector::new(vec![1.0, 0.0, 0.0]).unwrap(),
                ),
            ))
            .await
            .unwrap_err();

        assert_eq!(
            error,
            KnowledgeIndexError::DimensionMismatch {
                expected: 2,
                actual: 3
            }
        );
    }

    #[test]
    fn knowledge_embedding_vector_rejects_empty_vectors() {
        assert_eq!(
            KnowledgeEmbeddingVector::new(vec![]).unwrap_err(),
            KnowledgeIndexError::InvalidEmbedding(
                "knowledge embedding vector must not be empty".to_string()
            )
        );
    }

    #[test]
    fn knowledge_embedding_vector_rejects_non_finite_values() {
        assert_eq!(
            KnowledgeEmbeddingVector::new(vec![1.0, f32::NAN]).unwrap_err(),
            KnowledgeIndexError::InvalidEmbedding(
                "knowledge embedding vector values must be finite".to_string()
            )
        );
    }

    #[test]
    fn knowledge_semantic_query_defaults_are_stable() {
        let query =
            KnowledgeSemanticQuery::new(KnowledgeEmbeddingVector::new(vec![1.0, 0.0]).unwrap());

        assert_eq!(query.embedding.values(), &[1.0, 0.0]);
        assert_eq!(query.limit, 10);
        assert!(query.tags.is_empty());
        assert!(query.frontmatter_filters.is_empty());
    }

    #[test]
    fn knowledge_embedding_backend_kind_defaults_to_deterministic_in_process() {
        assert_eq!(
            KnowledgeEmbeddingBackendKind::default(),
            KnowledgeEmbeddingBackendKind::DeterministicInProcess
        );
    }

    #[tokio::test]
    async fn memory_semantic_index_ranks_by_cosine_similarity() {
        let mut index = MemorySemanticKnowledgeIndex::new();
        index
            .rebuild_embeddings(KnowledgeEmbeddingRebuildRequest {
                documents: vec![
                    KnowledgeEmbeddingDocument::new(
                        KnowledgeEntryRef {
                            id: KnowledgeEntryId::from("knowledge-entry-1"),
                            title: "Browser Kernel".to_string(),
                            source_uri: None,
                            artifact_id: None,
                            asset_id: None,
                            created_at: ts(),
                        },
                        KnowledgeEmbeddingVector::new(vec![0.0, 1.0]).unwrap(),
                    ),
                    KnowledgeEmbeddingDocument::new(
                        KnowledgeEntryRef {
                            id: KnowledgeEntryId::from("knowledge-entry-2"),
                            title: "AgentOS Memory".to_string(),
                            source_uri: None,
                            artifact_id: None,
                            asset_id: None,
                            created_at: ts(),
                        },
                        KnowledgeEmbeddingVector::new(vec![1.0, 0.0]).unwrap(),
                    ),
                ],
                requested_at: ts(),
            })
            .await
            .unwrap();

        let results = index
            .semantic_query(&KnowledgeSemanticQuery::new(
                KnowledgeEmbeddingVector::new(vec![1.0, 0.0]).unwrap(),
            ))
            .await
            .unwrap();

        assert_eq!(results.len(), 2);
        assert_eq!(
            results[0].entry.id,
            KnowledgeEntryId::from("knowledge-entry-2")
        );
        assert!(results[0].score > results[1].score);
    }

    #[tokio::test]
    async fn memory_semantic_index_filters_by_tags_and_frontmatter() {
        let mut index = MemorySemanticKnowledgeIndex::new();
        index
            .rebuild_embeddings(KnowledgeEmbeddingRebuildRequest {
                documents: vec![
                    KnowledgeEmbeddingDocument::new(
                        KnowledgeEntryRef {
                            id: KnowledgeEntryId::from("knowledge-entry-1"),
                            title: "Published Memory".to_string(),
                            source_uri: None,
                            artifact_id: None,
                            asset_id: None,
                            created_at: ts(),
                        },
                        KnowledgeEmbeddingVector::new(vec![1.0, 0.0]).unwrap(),
                    )
                    .with_tags(vec!["memory".to_string()])
                    .with_frontmatter(serde_json::json!({ "status": "published" })),
                    KnowledgeEmbeddingDocument::new(
                        KnowledgeEntryRef {
                            id: KnowledgeEntryId::from("knowledge-entry-2"),
                            title: "Draft Memory".to_string(),
                            source_uri: None,
                            artifact_id: None,
                            asset_id: None,
                            created_at: ts(),
                        },
                        KnowledgeEmbeddingVector::new(vec![1.0, 0.0]).unwrap(),
                    )
                    .with_tags(vec!["memory".to_string()])
                    .with_frontmatter(serde_json::json!({ "status": "draft" })),
                ],
                requested_at: ts(),
            })
            .await
            .unwrap();

        let results = index
            .semantic_query(
                &KnowledgeSemanticQuery::new(
                    KnowledgeEmbeddingVector::new(vec![1.0, 0.0]).unwrap(),
                )
                .with_tags(vec!["memory".to_string()])
                .with_frontmatter_filter("status", "published"),
            )
            .await
            .unwrap();

        assert_eq!(results.len(), 1);
        assert_eq!(
            results[0].entry.id,
            KnowledgeEntryId::from("knowledge-entry-1")
        );
    }

    #[tokio::test]
    async fn memory_semantic_index_rejects_dimension_mismatch() {
        let mut index = MemorySemanticKnowledgeIndex::new();
        index
            .upsert_embedding(KnowledgeEmbeddingDocument::new(
                KnowledgeEntryRef {
                    id: KnowledgeEntryId::from("knowledge-entry-1"),
                    title: "AgentOS Memory".to_string(),
                    source_uri: None,
                    artifact_id: None,
                    asset_id: None,
                    created_at: ts(),
                },
                KnowledgeEmbeddingVector::new(vec![1.0, 0.0]).unwrap(),
            ))
            .await
            .unwrap();

        assert_eq!(
            index
                .semantic_query(&KnowledgeSemanticQuery::new(
                    KnowledgeEmbeddingVector::new(vec![1.0, 0.0, 0.0]).unwrap(),
                ))
                .await
                .unwrap_err(),
            KnowledgeIndexError::DimensionMismatch {
                expected: 2,
                actual: 3,
            }
        );
    }

    #[tokio::test]
    async fn memory_semantic_index_upserts_and_deletes_embeddings() {
        let mut index = MemorySemanticKnowledgeIndex::new();
        index
            .upsert_embedding(KnowledgeEmbeddingDocument::new(
                KnowledgeEntryRef {
                    id: KnowledgeEntryId::from("knowledge-entry-1"),
                    title: "Original".to_string(),
                    source_uri: None,
                    artifact_id: None,
                    asset_id: None,
                    created_at: ts(),
                },
                KnowledgeEmbeddingVector::new(vec![0.0, 1.0]).unwrap(),
            ))
            .await
            .unwrap();
        index
            .upsert_embedding(KnowledgeEmbeddingDocument::new(
                KnowledgeEntryRef {
                    id: KnowledgeEntryId::from("knowledge-entry-1"),
                    title: "Updated".to_string(),
                    source_uri: None,
                    artifact_id: None,
                    asset_id: None,
                    created_at: ts(),
                },
                KnowledgeEmbeddingVector::new(vec![1.0, 0.0]).unwrap(),
            ))
            .await
            .unwrap();

        let results = index
            .semantic_query(&KnowledgeSemanticQuery::new(
                KnowledgeEmbeddingVector::new(vec![1.0, 0.0]).unwrap(),
            ))
            .await
            .unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].entry.title, "Updated");

        index
            .delete_embedding(&KnowledgeEntryId::from("knowledge-entry-1"))
            .await
            .unwrap();
        assert!(
            index
                .semantic_query(&KnowledgeSemanticQuery::new(
                    KnowledgeEmbeddingVector::new(vec![1.0, 0.0]).unwrap(),
                ))
                .await
                .unwrap()
                .is_empty()
        );
    }

    #[tokio::test]
    async fn semantic_ranking_ties_break_by_entry_id() {
        let mut index = MemorySemanticKnowledgeIndex::new();
        index
            .rebuild_embeddings(KnowledgeEmbeddingRebuildRequest {
                documents: vec![
                    KnowledgeEmbeddingDocument::new(
                        KnowledgeEntryRef {
                            id: KnowledgeEntryId::from("knowledge-entry-b"),
                            title: "Memory B".to_string(),
                            source_uri: None,
                            artifact_id: None,
                            asset_id: None,
                            created_at: ts(),
                        },
                        KnowledgeEmbeddingVector::new(vec![1.0, 0.0]).unwrap(),
                    ),
                    KnowledgeEmbeddingDocument::new(
                        KnowledgeEntryRef {
                            id: KnowledgeEntryId::from("knowledge-entry-a"),
                            title: "Memory A".to_string(),
                            source_uri: None,
                            artifact_id: None,
                            asset_id: None,
                            created_at: ts(),
                        },
                        KnowledgeEmbeddingVector::new(vec![1.0, 0.0]).unwrap(),
                    ),
                ],
                requested_at: ts(),
            })
            .await
            .unwrap();

        let results = index
            .semantic_query(&KnowledgeSemanticQuery::new(
                KnowledgeEmbeddingVector::new(vec![1.0, 0.0]).unwrap(),
            ))
            .await
            .unwrap();

        assert_eq!(
            results
                .iter()
                .map(|result| &result.entry.id)
                .collect::<Vec<_>>(),
            vec![
                &KnowledgeEntryId::from("knowledge-entry-a"),
                &KnowledgeEntryId::from("knowledge-entry-b")
            ]
        );
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

    #[tokio::test]
    async fn knowledge_action_executor_searches_repository() {
        let repository = Arc::new(MemoryKnowledgeRepository::new());
        repository
            .save_draft(draft(
                "AgentOS Notes",
                "foundation content",
                vec!["agent-os"],
            ))
            .await
            .unwrap();
        let executor = KnowledgeActionExecutor::new(repository);
        let request = action_request(
            knowledge_search_action_kind(),
            serde_json::to_value(KnowledgeSearchActionInput {
                query: KnowledgeSearchQuery::new("agentos"),
            })
            .unwrap(),
        );

        let result = executor.execute(&request).await.unwrap();
        let ActionResultPayload::Json(value) = result.payload else {
            panic!("expected json payload");
        };
        let results: Vec<KnowledgeSearchResult> = serde_json::from_value(value).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].entry.title, "AgentOS Notes");
    }

    #[tokio::test]
    async fn knowledge_action_executor_gets_entry() {
        let repository = Arc::new(MemoryKnowledgeRepository::new());
        let saved = repository
            .save_draft(draft(
                "AgentOS Notes",
                "foundation content",
                vec!["agent-os"],
            ))
            .await
            .unwrap();
        let executor = KnowledgeActionExecutor::new(repository);
        let request = action_request(
            knowledge_get_entry_action_kind(),
            serde_json::to_value(KnowledgeGetEntryActionInput {
                id: saved.id.clone(),
            })
            .unwrap(),
        );

        let result = executor.execute(&request).await.unwrap();
        let ActionResultPayload::Json(value) = result.payload else {
            panic!("expected json payload");
        };
        let entry: Option<KnowledgeEntryRef> = serde_json::from_value(value).unwrap();
        assert_eq!(entry, Some(saved));
    }

    #[tokio::test]
    async fn knowledge_action_executor_creates_draft_without_saving() {
        let repository = Arc::new(MemoryKnowledgeRepository::new());
        let executor = KnowledgeActionExecutor::new(repository.clone());
        let request = action_request(
            knowledge_create_draft_action_kind(),
            serde_json::to_value(create_draft_input("AgentOS Notes", "draft content")).unwrap(),
        );

        let result = executor.execute(&request).await.unwrap();
        let ActionResultPayload::Json(value) = result.payload else {
            panic!("expected json payload");
        };
        let draft: KnowledgeEntryDraft = serde_json::from_value(value).unwrap();
        assert_eq!(draft.title, "AgentOS Notes");
        assert!(repository.list_entries().await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn knowledge_action_executor_saves_entry_when_executed() {
        let repository = Arc::new(MemoryKnowledgeRepository::new());
        let executor = KnowledgeActionExecutor::new(repository.clone());
        let request = action_request(
            knowledge_save_entry_action_kind(),
            serde_json::to_value(KnowledgeSaveEntryActionInput {
                draft: draft("AgentOS Notes", "saved content", vec!["agent-os"]),
            })
            .unwrap(),
        );

        let result = executor.execute(&request).await.unwrap();
        let ActionResultPayload::Json(value) = result.payload else {
            panic!("expected json payload");
        };
        let entry: KnowledgeEntryRef = serde_json::from_value(value).unwrap();
        assert_eq!(entry.id, KnowledgeEntryId::from("knowledge-entry-1"));
        assert_eq!(repository.list_entries().await.unwrap(), vec![entry]);
    }

    #[tokio::test]
    async fn knowledge_action_executor_rejects_blank_draft_title() {
        let repository = Arc::new(MemoryKnowledgeRepository::new());
        let executor = KnowledgeActionExecutor::new(repository);
        let request = action_request(
            knowledge_create_draft_action_kind(),
            serde_json::to_value(create_draft_input("   ", "draft content")).unwrap(),
        );

        let err = executor.execute(&request).await.unwrap_err();
        assert!(matches!(err, ActionExecutorError::InvalidInput(_)));
    }

    #[tokio::test]
    async fn knowledge_action_executor_rejects_unknown_action_kind() {
        let repository = Arc::new(MemoryKnowledgeRepository::new());
        let executor = KnowledgeActionExecutor::new(repository);
        let request = action_request(ActionKind::from("knowledge.unknown"), serde_json::json!({}));

        let err = executor.execute(&request).await.unwrap_err();
        assert!(matches!(err, ActionExecutorError::NotSupported(_)));
    }

    // -----------------------------------------------------------------------
    // update_entry tests for MemoryKnowledgeRepository
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn memory_update_entry_updates_title() {
        let repo = MemoryKnowledgeRepository::new();
        let draft = KnowledgeEntryDraft::new("Original Title", "# Content", ts());
        let entry = repo.save_draft(draft).await.unwrap();

        let update = KnowledgeEntryUpdate::new().with_title("New Title");
        let updated = repo.update_entry(&entry.id, update).await.unwrap();

        assert_eq!(updated.title, "New Title");
        assert_eq!(updated.id, entry.id);
    }

    #[tokio::test]
    async fn memory_update_entry_updates_content() {
        let repo = MemoryKnowledgeRepository::new();
        let draft = KnowledgeEntryDraft::new("Title", "# Original", ts());
        let entry = repo.save_draft(draft).await.unwrap();

        let update = KnowledgeEntryUpdate::new().with_content("# Updated");
        let _updated = repo.update_entry(&entry.id, update).await.unwrap();

        // Verify content was updated by searching for new content
        let query = KnowledgeSearchQuery::new("Updated");
        let results = repo.search(&query).await.unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].entry.id, entry.id);
    }

    #[tokio::test]
    async fn memory_update_entry_updates_tags() {
        let repo = MemoryKnowledgeRepository::new();
        let draft = KnowledgeEntryDraft::new("Title", "# Content", ts());
        let entry = repo.save_draft(draft).await.unwrap();

        let update = KnowledgeEntryUpdate::new().with_tags(vec!["new-tag".to_string()]);
        let _updated = repo.update_entry(&entry.id, update).await.unwrap();

        // Verify tags were updated by searching with new tag
        let query = KnowledgeSearchQuery::new("").with_tags(vec!["new-tag".to_string()]);
        let results = repo.search(&query).await.unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].entry.id, entry.id);
    }

    #[tokio::test]
    async fn memory_update_entry_not_found() {
        let repo = MemoryKnowledgeRepository::new();
        let update = KnowledgeEntryUpdate::new().with_title("New");
        let result = repo
            .update_entry(&KnowledgeEntryId::from("nonexistent"), update)
            .await;
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            KnowledgeRepositoryError::NotFound(_)
        ));
    }

    #[tokio::test]
    async fn memory_update_entry_preserves_unspecified_fields() {
        let repo = MemoryKnowledgeRepository::new();
        let draft = KnowledgeEntryDraft::new("Title", "# Content", ts())
            .with_source_uri("https://example.com");
        let entry = repo.save_draft(draft).await.unwrap();

        // Only update title, source_uri should be preserved
        let update = KnowledgeEntryUpdate::new().with_title("New Title");
        let updated = repo.update_entry(&entry.id, update).await.unwrap();

        assert_eq!(updated.title, "New Title");
        assert_eq!(updated.source_uri, Some("https://example.com".to_string()));
    }

    // -----------------------------------------------------------------------
    // PR 144: Work Object integration
    // -----------------------------------------------------------------------

    #[test]
    fn work_object_type_question_and_answer_serialize_as_snake_case() {
        use asset_core::WorkObjectType;
        assert_eq!(
            serde_json::to_string(&WorkObjectType::Question).unwrap(),
            "\"question\""
        );
        assert_eq!(
            serde_json::to_string(&WorkObjectType::Answer).unwrap(),
            "\"answer\""
        );
        let decoded_q: WorkObjectType = serde_json::from_str("\"question\"").unwrap();
        assert_eq!(decoded_q, WorkObjectType::Question);
        let decoded_a: WorkObjectType = serde_json::from_str("\"answer\"").unwrap();
        assert_eq!(decoded_a, WorkObjectType::Answer);
    }

    #[test]
    fn question_ledger_entry_with_work_object_id_roundtrips() {
        let entry = QuestionLedgerEntry {
            id: QuestionLedgerEntryId::from("question-1"),
            question: "What is the architecture?".to_string(),
            conversation_id: None,
            message_id: None,
            work_object_id: Some(WorkObjectId::from("project-alpha")),
            answer_ref: None,
            related_knowledge_entry_ids: vec![],
            tags: vec!["architecture".to_string()],
            created_at: ts(),
            updated_at: ts(),
        };
        let json = serde_json::to_string_pretty(&entry).unwrap();
        let decoded: QuestionLedgerEntry = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded, entry);
        assert_eq!(decoded.work_object_id.unwrap().0, "project-alpha");
    }

    #[tokio::test]
    async fn question_ledger_create_with_work_object_id_and_list_by_work_object() {
        let ledger = MemoryQuestionLedger::new();
        let wo_id = WorkObjectId::from("project-alpha");

        ledger
            .create_question(
                QuestionLedgerCreateRequest::new("Q1", ts()).with_work_object_id(wo_id.clone()),
            )
            .await
            .unwrap();
        ledger
            .create_question(QuestionLedgerCreateRequest::new("Q2", ts()))
            .await
            .unwrap();
        ledger
            .create_question(
                QuestionLedgerCreateRequest::new("Q3", ts()).with_work_object_id(wo_id.clone()),
            )
            .await
            .unwrap();

        let results = ledger.list_by_work_object(&wo_id).await.unwrap();
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].question, "Q1");
        assert_eq!(results[1].question, "Q3");

        let other = WorkObjectId::from("other-project");
        assert!(ledger.list_by_work_object(&other).await.unwrap().is_empty());
    }

    #[test]
    fn answer_cache_package_with_work_object_id_roundtrips() {
        let package = AnswerCachePackage {
            id: AnswerCachePackageId::from("answer-cache-1"),
            question_id: None,
            work_object_id: Some(WorkObjectId::from("project-alpha")),
            answer_id: "answer-1".to_string(),
            version: AnswerCachePackageVersion::new(1).unwrap(),
            answer_markdown: "# Answer".to_string(),
            evidence_refs: vec![],
            related_knowledge_entry_ids: vec![],
            freshness_policy: AnswerFreshnessPolicy::never_expires(),
            created_at: ts(),
            updated_at: ts(),
        };
        let json = serde_json::to_string_pretty(&package).unwrap();
        let decoded: AnswerCachePackage = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded, package);
        assert_eq!(decoded.work_object_id.unwrap().0, "project-alpha");
    }

    #[tokio::test]
    async fn answer_cache_create_with_work_object_id_and_list_by_work_object() {
        let store = MemoryAnswerCacheStore::new();
        let wo_id = WorkObjectId::from("project-alpha");

        store
            .create_package(
                AnswerCacheCreateRequest::new("answer-1", "First answer", ts())
                    .with_work_object_id(wo_id.clone()),
            )
            .await
            .unwrap();
        store
            .create_package(AnswerCacheCreateRequest::new(
                "answer-2",
                "Second answer",
                ts(),
            ))
            .await
            .unwrap();
        store
            .create_package(
                AnswerCacheCreateRequest::new("answer-3", "Third answer", ts())
                    .with_work_object_id(wo_id.clone()),
            )
            .await
            .unwrap();

        let results = store.list_by_work_object(&wo_id).await.unwrap();
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].answer_id, "answer-1");
        assert_eq!(results[1].answer_id, "answer-3");

        let other = WorkObjectId::from("other-project");
        assert!(store.list_by_work_object(&other).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn work_object_cross_entity_coordinator_aggregates_questions_and_answers() {
        let ledger = MemoryQuestionLedger::new();
        let cache = MemoryAnswerCacheStore::new();
        let wo_id = WorkObjectId::from("project-alpha");

        // Create question with work object
        let q = ledger
            .create_question(
                QuestionLedgerCreateRequest::new("What is X?", ts())
                    .with_work_object_id(wo_id.clone())
                    .with_related_knowledge_entry_ids(vec![KnowledgeEntryId::from(
                        "knowledge-entry-1",
                    )]),
            )
            .await
            .unwrap();

        // Create answer with work object
        cache
            .create_package(
                AnswerCacheCreateRequest::new("answer-1", "X is Y", ts())
                    .with_work_object_id(wo_id.clone()),
            )
            .await
            .unwrap();

        // Create unrelated question
        ledger
            .create_question(QuestionLedgerCreateRequest::new("Unrelated", ts()))
            .await
            .unwrap();

        let coordinator = WorkObjectCrossEntityCoordinator::new(ledger, cache);
        let result = coordinator
            .query(&WorkObjectCrossEntityQuery::new(wo_id.clone()))
            .await
            .unwrap();

        assert_eq!(result.work_object_id, wo_id);
        assert_eq!(result.question_ids.len(), 1);
        assert_eq!(result.question_ids[0], q.id);
        assert_eq!(result.answer_ids.len(), 1);
        assert_eq!(
            result.knowledge_entry_ids,
            vec![KnowledgeEntryId::from("knowledge-entry-1")]
        );
    }

    #[tokio::test]
    async fn work_object_cross_entity_coordinator_returns_empty_for_unknown_work_object() {
        let ledger = MemoryQuestionLedger::new();
        let cache = MemoryAnswerCacheStore::new();
        let coordinator = WorkObjectCrossEntityCoordinator::new(ledger, cache);
        let result = coordinator
            .query(&WorkObjectCrossEntityQuery::new(WorkObjectId::from(
                "nonexistent",
            )))
            .await
            .unwrap();

        assert!(result.question_ids.is_empty());
        assert!(result.answer_ids.is_empty());
        assert!(result.knowledge_entry_ids.is_empty());
    }

    // -----------------------------------------------------------------------
    // PR 145: Knowledge permission filtering
    // -----------------------------------------------------------------------

    #[test]
    fn knowledge_search_result_permission_fields_roundtrip() {
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
            permission_required: true,
            confidentiality: Some("internal".to_string()),
        };
        let json = serde_json::to_string_pretty(&result).unwrap();
        let decoded: KnowledgeSearchResult = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded, result);
        assert!(decoded.permission_required);
        assert_eq!(decoded.confidentiality.as_deref(), Some("internal"));
    }

    #[tokio::test]
    async fn permission_aware_search_filters_unauthorized_entries() {
        use enterprise_permission_core::{EnterpriseRole, PermissionGrant, ResourceId};

        let mut index = MemoryFullTextKnowledgeIndex::default();
        index
            .upsert(KnowledgeIndexDocument {
                entry: KnowledgeEntryRef {
                    id: KnowledgeEntryId::from("public-entry"),
                    title: "Public Knowledge".to_string(),
                    source_uri: None,
                    artifact_id: None,
                    asset_id: None,
                    created_at: ts(),
                },
                body_markdown: "public content".to_string(),
                tags: vec![],
                frontmatter: serde_json::json!({}),
            })
            .await
            .unwrap();
        index
            .upsert(KnowledgeIndexDocument {
                entry: KnowledgeEntryRef {
                    id: KnowledgeEntryId::from("secret-entry"),
                    title: "Secret Knowledge".to_string(),
                    source_uri: None,
                    artifact_id: None,
                    asset_id: None,
                    created_at: ts(),
                },
                body_markdown: "secret content".to_string(),
                tags: vec![],
                frontmatter: serde_json::json!({}),
            })
            .await
            .unwrap();

        let mut store = PermissionStore::new();
        store.add_grant(PermissionGrant {
            grant_id: "grant-1".to_string(),
            user_id: EnterpriseUserId::from("user-1"),
            role: EnterpriseRole::User,
            resource_type: ResourceType::KnowledgeBase,
            resource_id: ResourceId::from("public-entry"),
            actions: vec![PermissionAction::Read],
            granted_at: ts(),
            expires_at: None,
            revoked: false,
        });

        let search = PermissionAwareKnowledgeSearch::new(index, store);
        let user = EnterpriseUserId::from("user-1");
        let query = KnowledgeFullTextQuery::new("knowledge");

        let results = search.query_with_permissions(&query, &user).await.unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].entry.id.0, "public-entry");
    }

    #[tokio::test]
    async fn permission_aware_search_allows_super_admin() {
        use enterprise_permission_core::{EnterpriseRole, PermissionGrant, ResourceId};

        let mut index = MemoryFullTextKnowledgeIndex::default();
        index
            .upsert(KnowledgeIndexDocument {
                entry: KnowledgeEntryRef {
                    id: KnowledgeEntryId::from("secret-entry"),
                    title: "Secret Knowledge".to_string(),
                    source_uri: None,
                    artifact_id: None,
                    asset_id: None,
                    created_at: ts(),
                },
                body_markdown: "secret content".to_string(),
                tags: vec![],
                frontmatter: serde_json::json!({}),
            })
            .await
            .unwrap();

        let mut store = PermissionStore::new();
        store.add_grant(PermissionGrant {
            grant_id: "grant-1".to_string(),
            user_id: EnterpriseUserId::from("admin-1"),
            role: EnterpriseRole::SuperAdmin,
            resource_type: ResourceType::KnowledgeBase,
            resource_id: ResourceId::from("secret-entry"),
            actions: vec![PermissionAction::Read, PermissionAction::Admin],
            granted_at: ts(),
            expires_at: None,
            revoked: false,
        });

        let search = PermissionAwareKnowledgeSearch::new(index, store);
        let user = EnterpriseUserId::from("admin-1");
        let query = KnowledgeFullTextQuery::new("knowledge");

        let results = search.query_with_permissions(&query, &user).await.unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].entry.id.0, "secret-entry");
    }

    #[tokio::test]
    async fn permission_aware_knowledge_denies_offboarded_user() {
        use enterprise_permission_core::{
            EnterpriseRole, EnterpriseUserLifecycle, EnterpriseUserStatus, PermissionGrant,
            ResourceId,
        };

        let mut index = MemoryFullTextKnowledgeIndex::default();
        index
            .upsert(KnowledgeIndexDocument {
                entry: KnowledgeEntryRef {
                    id: KnowledgeEntryId::from("kb-1"),
                    title: "Offboarded knowledge".to_string(),
                    source_uri: None,
                    artifact_id: None,
                    asset_id: None,
                    created_at: ts(),
                },
                body_markdown: "knowledge body".to_string(),
                tags: vec![],
                frontmatter: serde_json::json!({}),
            })
            .await
            .unwrap();

        let mut store = PermissionStore::new();
        store.add_grant(PermissionGrant {
            grant_id: "grant-1".to_string(),
            user_id: EnterpriseUserId::from("user-1"),
            role: EnterpriseRole::User,
            resource_type: ResourceType::KnowledgeBase,
            resource_id: ResourceId::from("kb-1"),
            actions: vec![PermissionAction::Read],
            granted_at: Utc::now(),
            expires_at: None,
            revoked: false,
        });
        store.set_user_lifecycle(EnterpriseUserStatus {
            user_id: EnterpriseUserId::from("user-1"),
            lifecycle: EnterpriseUserLifecycle::Offboarded,
            changed_at: Utc::now(),
            reason: None,
        });

        let search = PermissionAwareKnowledgeSearch::new(index, store);
        let query = KnowledgeFullTextQuery::new("knowledge");
        let results = search
            .query_with_permissions(&query, &EnterpriseUserId::from("user-1"))
            .await
            .unwrap();
        assert!(results.is_empty());
    }

    #[tokio::test]
    async fn permission_aware_citation_filters_unauthorized_evidence() {
        let store_inner = MemoryCitationEvidenceStore::new();
        let evidence = store_inner
            .register_evidence(
                CitationEvidenceRef::artifact(ArtifactId::from("artifact-1")),
                Some("excerpt".to_string()),
                ts(),
            )
            .await
            .unwrap();
        store_inner
            .cite_answer("answer-1", &evidence.id, ts())
            .await
            .unwrap();

        let perm_store = PermissionStore::new();
        // No grant for artifact-1 -> should be filtered

        let wrapper = PermissionAwareCitationEvidenceStore::new(store_inner, perm_store);
        let user = EnterpriseUserId::from("user-1");
        let trace = wrapper
            .trace_answer_with_permissions("answer-1", &user)
            .await
            .unwrap();
        assert!(
            trace.evidence.is_empty(),
            "evidence without grant must be filtered"
        );
    }

    #[tokio::test]
    async fn permission_aware_citation_allows_authorized_evidence() {
        use enterprise_permission_core::{EnterpriseRole, PermissionGrant, ResourceId};

        let store_inner = MemoryCitationEvidenceStore::new();
        let evidence = store_inner
            .register_evidence(
                CitationEvidenceRef::artifact(ArtifactId::from("artifact-1")),
                Some("excerpt".to_string()),
                ts(),
            )
            .await
            .unwrap();
        store_inner
            .cite_answer("answer-1", &evidence.id, ts())
            .await
            .unwrap();

        let mut perm_store = PermissionStore::new();
        perm_store.add_grant(PermissionGrant {
            grant_id: "grant-1".to_string(),
            user_id: EnterpriseUserId::from("user-1"),
            role: EnterpriseRole::User,
            resource_type: ResourceType::KnowledgeBase,
            resource_id: ResourceId::from("artifact-1"),
            actions: vec![PermissionAction::Read],
            granted_at: ts(),
            expires_at: None,
            revoked: false,
        });

        let wrapper = PermissionAwareCitationEvidenceStore::new(store_inner, perm_store);
        let user = EnterpriseUserId::from("user-1");
        let trace = wrapper
            .trace_answer_with_permissions("answer-1", &user)
            .await
            .unwrap();
        assert_eq!(trace.evidence.len(), 1, "authorized evidence must pass");
    }

    #[tokio::test]
    async fn permission_aware_citation_allows_non_permission_refs() {
        let store_inner = MemoryCitationEvidenceStore::new();
        let evidence = store_inner
            .register_evidence(
                CitationEvidenceRef::source_uri("https://example.com"),
                Some("excerpt".to_string()),
                ts(),
            )
            .await
            .unwrap();
        store_inner
            .cite_answer("answer-1", &evidence.id, ts())
            .await
            .unwrap();

        let perm_store = PermissionStore::new();
        let wrapper = PermissionAwareCitationEvidenceStore::new(store_inner, perm_store);
        let user = EnterpriseUserId::from("user-1");
        let trace = wrapper
            .trace_answer_with_permissions("answer-1", &user)
            .await
            .unwrap();
        assert_eq!(
            trace.evidence.len(),
            1,
            "source_uri evidence should always pass"
        );
    }

    #[test]
    fn knowledge_engine_event_and_audit_contracts_are_json_safe() {
        let now = ts();
        let event = KnowledgeEventEnvelope::new(
            "evt-1",
            "object.created",
            "txn-1",
            KnowledgeActor::engine("user:shiwen"),
            serde_json::json!({"object_id":"obj_mass","canonical_name":"质量"}),
            now,
        );
        let event_json = serde_json::to_value(&event).unwrap();
        assert_eq!(event_json["schema_version"], "0.1.0");
        assert_eq!(event_json["event_type"], "object.created");

        let audit = KnowledgeAuditRecord::query(
            "aud-1",
            "op-1",
            KnowledgeActor::llm("llm_gateway:test", "user:shiwen"),
            "object.get",
            "sha256:params",
            1,
            vec!["obj_mass".to_string()],
            now,
        );
        let audit_json = serde_json::to_value(&audit).unwrap();
        assert_eq!(audit_json["operation_type"], "query");
        assert_eq!(
            audit_json["result"]["result_summary"]["returned_ids"][0],
            "obj_mass"
        );
    }

    #[test]
    fn asset_property_binding_separates_extraction_binding_and_truth_confidence() {
        let binding = KnowledgeAssetPropertyBindingRecord::candidate(
            "apb-1",
            "asset-1",
            KnowledgeBindingTarget::object_attribute("obj_kumquat", "color"),
            "visual_observation",
            serde_json::json!({ "kind": "string", "data": "orange" }),
            KnowledgeAssetBindingConfidence::candidate(0.78, 0.84),
            ts(),
        );

        assert_eq!(binding.status, "candidate");
        assert_eq!(binding.confidence.extraction_confidence, 0.78);
        assert_eq!(binding.confidence.binding_confidence, 0.84);
        assert_eq!(binding.confidence.truth_confidence, 0.0);
        assert!(matches!(
            binding.binding_target,
            KnowledgeBindingTarget::ObjectAttribute { .. }
        ));
    }

    #[test]
    fn direct_filesystem_access_is_explicitly_rejected_boundary() {
        assert!(KnowledgeEngineAccessMode::EngineApi.is_allowed());
        assert!(KnowledgeEngineAccessMode::LlmControlledPacket.is_allowed());
        assert!(!KnowledgeEngineAccessMode::DirectFilesystem.is_allowed());
    }
}
