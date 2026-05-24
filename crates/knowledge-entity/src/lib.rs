//! # Knowledge Entity
//!
//! Domain types, deterministic in-memory repository, and action-level seams for
//! AgentOS knowledge entries.
//!
//! This crate intentionally does not write to a real Markdown/frontmatter knowledge base.
//! It provides pure Knowledge Entity abstractions that later action/runtime integrations
//! can use through `ActionRuntime` and `CapabilityPolicy`.

use action_core::{
    ActionExecutor, ActionExecutorError, ActionKind, ActionRegistry, ActionRegistryError,
    ActionRequest, ActionResult, ActionResultPayload, ActionSchema, ActionStatus, SideEffectKind,
};
use artifact_core::ArtifactId;
use asset_core::AssetId;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt;
use std::sync::{Arc, Mutex};

pub const KNOWLEDGE_SEARCH_ACTION_KIND: &str = "knowledge.search";
pub const KNOWLEDGE_GET_ENTRY_ACTION_KIND: &str = "knowledge.get_entry";
pub const KNOWLEDGE_CREATE_DRAFT_ACTION_KIND: &str = "knowledge.create_draft";
pub const KNOWLEDGE_SAVE_ENTRY_ACTION_KIND: &str = "knowledge.save_entry";

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

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum KnowledgeValidationError {
    #[error("knowledge draft title cannot be blank")]
    BlankTitle,
    #[error("knowledge draft content cannot be blank")]
    BlankContent,
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
}
