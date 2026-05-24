//! # Asset Core
//!
//! Core domain types for AgentOS assets.
//!
//! Assets represent binary and metadata resources such as images, documents,
//! spreadsheets, slides, PDFs, video references, and audio files. They are
//! observed, captured, processed, and linked to work objects without becoming
//! foreground conversation participants.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fmt;

/// Unique identifier for an asset.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct AssetId(pub String);

impl fmt::Display for AssetId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl From<String> for AssetId {
    fn from(s: String) -> Self {
        Self(s)
    }
}

impl From<&str> for AssetId {
    fn from(s: &str) -> Self {
        Self(s.to_string())
    }
}

/// Unique identifier for a durable work object an asset can be linked to.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct WorkObjectId(pub String);

impl fmt::Display for WorkObjectId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl From<String> for WorkObjectId {
    fn from(s: String) -> Self {
        Self(s)
    }
}

impl From<&str> for WorkObjectId {
    fn from(s: &str) -> Self {
        Self(s.to_string())
    }
}

/// Durable object categories assets can be linked to.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkObjectType {
    Conversation,
    KnowledgeEntry,
    KnowledgeDraft,
    Project,
    Person,
    MailThread,
    BrowserSession,
    External,
}

/// Why an asset is linked to a work object.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AssetWorkObjectLinkReason {
    Source,
    DerivedFrom,
    Evidence,
    Attachment,
    Related,
}

/// Link from an asset to a durable work object.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AssetWorkObjectLink {
    pub work_object_type: WorkObjectType,
    pub work_object_id: WorkObjectId,
    pub reason: AssetWorkObjectLinkReason,
    pub linked_at: DateTime<Utc>,
}

impl AssetWorkObjectLink {
    pub fn new(
        work_object_type: WorkObjectType,
        work_object_id: impl Into<WorkObjectId>,
        reason: AssetWorkObjectLinkReason,
        linked_at: DateTime<Utc>,
    ) -> Self {
        Self {
            work_object_type,
            work_object_id: work_object_id.into(),
            reason,
            linked_at,
        }
    }
}

/// The broad category of asset.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AssetKind {
    Image,
    Document,
    Spreadsheet,
    Slide,
    Pdf,
    VideoReference,
    Audio,
    Unknown,
}

/// The processing status of an asset.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AssetProcessingStatus {
    Observed,
    Captured,
    Processing,
    Processed,
    Failed,
    Archived,
}

/// How relevant an asset is to the current work or conversation.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AssetRelevance {
    Critical,
    High,
    Medium,
    Low,
    Background,
}

impl AssetRelevance {
    /// Returns true if the asset meets or exceeds the given relevance threshold.
    pub fn meets_threshold(&self, threshold: &AssetRelevance) -> bool {
        self.level() >= threshold.level()
    }

    fn level(&self) -> u8 {
        match self {
            AssetRelevance::Critical => 5,
            AssetRelevance::High => 4,
            AssetRelevance::Medium => 3,
            AssetRelevance::Low => 2,
            AssetRelevance::Background => 1,
        }
    }
}

/// Metadata about where an asset was discovered.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AssetSource {
    /// The URI where the asset was discovered, if applicable.
    pub uri: Option<String>,
    /// When the asset was first observed.
    pub discovered_at: DateTime<Utc>,
    /// Additional context about the discovery.
    pub context: serde_json::Value,
}

impl AssetSource {
    pub fn new(discovered_at: DateTime<Utc>) -> Self {
        Self {
            uri: None,
            discovered_at,
            context: serde_json::json!({}),
        }
    }

    pub fn with_uri(mut self, uri: impl Into<String>) -> Self {
        self.uri = Some(uri.into());
        self
    }

    pub fn with_context(mut self, context: serde_json::Value) -> Self {
        self.context = context;
        self
    }
}

/// Metadata describing an asset.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AssetMetadata {
    pub id: AssetId,
    pub kind: AssetKind,
    pub title: Option<String>,
    pub mime_type: Option<String>,
    pub file_size_bytes: Option<u64>,
    pub source: AssetSource,
    pub relevance: AssetRelevance,
    pub tags: Vec<String>,
    pub created_at: DateTime<Utc>,
}

impl AssetMetadata {
    pub fn new(
        id: impl Into<AssetId>,
        kind: AssetKind,
        source: AssetSource,
        relevance: AssetRelevance,
        created_at: DateTime<Utc>,
    ) -> Self {
        Self {
            id: id.into(),
            kind,
            title: None,
            mime_type: None,
            file_size_bytes: None,
            source,
            relevance,
            tags: vec![],
            created_at,
        }
    }

    pub fn with_title(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }

    pub fn with_mime_type(mut self, mime_type: impl Into<String>) -> Self {
        self.mime_type = Some(mime_type.into());
        self
    }

    pub fn with_file_size_bytes(mut self, file_size_bytes: u64) -> Self {
        self.file_size_bytes = Some(file_size_bytes);
        self
    }

    pub fn with_tags(mut self, tags: Vec<String>) -> Self {
        self.tags = tags;
        self
    }
}

/// Policy constraints for an asset.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AssetPolicy {
    /// Whether the asset requires a local binary copy to be usable.
    pub requires_local_copy: bool,
    /// Whether binary content (as opposed to metadata-only) is allowed.
    pub allow_binary: bool,
    /// Maximum allowed file size in bytes, if any.
    pub max_size_bytes: Option<u64>,
}

impl Default for AssetPolicy {
    fn default() -> Self {
        Self {
            requires_local_copy: false,
            allow_binary: true,
            max_size_bytes: None,
        }
    }
}

impl AssetPolicy {
    /// Returns the default policy for a video reference asset (no local copy required).
    pub fn video_reference() -> Self {
        Self {
            requires_local_copy: false,
            allow_binary: false,
            max_size_bytes: None,
        }
    }
}

/// Errors from asset registry operations.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum AssetRegistryError {
    #[error("duplicate asset id: {0}")]
    DuplicateAssetId(AssetId),
}

/// In-memory asset registry for tests and early runtime flows.
#[derive(Debug, Clone, Default)]
pub struct AssetRegistry {
    assets: std::collections::HashMap<AssetId, AssetMetadata>,
}

impl AssetRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(&mut self, metadata: AssetMetadata) -> Result<(), AssetRegistryError> {
        if self.assets.contains_key(&metadata.id) {
            return Err(AssetRegistryError::DuplicateAssetId(metadata.id.clone()));
        }
        self.assets.insert(metadata.id.clone(), metadata);
        Ok(())
    }

    pub fn get(&self, id: &AssetId) -> Option<&AssetMetadata> {
        self.assets.get(id)
    }

    pub fn contains(&self, id: &AssetId) -> bool {
        self.assets.contains_key(id)
    }

    pub fn len(&self) -> usize {
        self.assets.len()
    }

    pub fn is_empty(&self) -> bool {
        self.assets.is_empty()
    }

    pub fn iter(&self) -> impl Iterator<Item = &AssetMetadata> {
        self.assets.values()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn image_asset() -> AssetMetadata {
        AssetMetadata::new(
            "asset-image-1",
            AssetKind::Image,
            AssetSource::new("2026-05-24T12:00:00Z".parse().unwrap())
                .with_uri("https://example.com/photo.jpg"),
            AssetRelevance::High,
            "2026-05-24T12:00:00Z".parse().unwrap(),
        )
        .with_title("Example Photo")
        .with_mime_type("image/jpeg")
        .with_file_size_bytes(1024)
        .with_tags(vec!["photo".to_string(), "example".to_string()])
    }

    fn video_reference_asset() -> AssetMetadata {
        AssetMetadata::new(
            "asset-video-1",
            AssetKind::VideoReference,
            AssetSource::new("2026-05-24T12:00:00Z".parse().unwrap())
                .with_uri("https://example.com/video.mp4"),
            AssetRelevance::Medium,
            "2026-05-24T12:00:00Z".parse().unwrap(),
        )
        .with_title("Example Video")
    }

    #[test]
    fn asset_id_roundtrips() {
        let id = AssetId::from("asset-1");
        assert_eq!(id.to_string(), "asset-1");

        let json = serde_json::to_string(&id).unwrap();
        let decoded: AssetId = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded, id);
    }

    #[test]
    fn asset_kind_serializes_as_snake_case() {
        let kind = AssetKind::VideoReference;
        let json = serde_json::to_string(&kind).unwrap();
        assert_eq!(json, "\"video_reference\"");

        let decoded: AssetKind = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded, AssetKind::VideoReference);
    }

    #[test]
    fn work_object_id_roundtrips() {
        let id = WorkObjectId::from("knowledge-entry-1");
        assert_eq!(id.to_string(), "knowledge-entry-1");

        let json = serde_json::to_string(&id).unwrap();
        let decoded: WorkObjectId = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded, id);
    }

    #[test]
    fn work_object_type_serializes_as_snake_case() {
        let json = serde_json::to_string(&WorkObjectType::KnowledgeEntry).unwrap();
        assert_eq!(json, "\"knowledge_entry\"");

        let decoded: WorkObjectType = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded, WorkObjectType::KnowledgeEntry);
    }

    #[test]
    fn asset_work_object_link_reason_serializes_as_snake_case() {
        let json = serde_json::to_string(&AssetWorkObjectLinkReason::DerivedFrom).unwrap();
        assert_eq!(json, "\"derived_from\"");

        let decoded: AssetWorkObjectLinkReason = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded, AssetWorkObjectLinkReason::DerivedFrom);
    }

    #[test]
    fn asset_work_object_link_roundtrips() {
        let link = AssetWorkObjectLink::new(
            WorkObjectType::KnowledgeEntry,
            "knowledge-entry-1",
            AssetWorkObjectLinkReason::Evidence,
            "2026-05-24T12:00:00Z".parse().unwrap(),
        );

        let json = serde_json::to_string_pretty(&link).unwrap();
        let decoded: AssetWorkObjectLink = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded, link);
    }

    #[test]
    fn asset_relevance_serializes_as_snake_case() {
        let json = serde_json::to_string(&AssetRelevance::Critical).unwrap();
        assert_eq!(json, "\"critical\"");

        let decoded: AssetRelevance = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded, AssetRelevance::Critical);
    }

    #[test]
    fn asset_processing_status_serializes_as_snake_case() {
        let json = serde_json::to_string(&AssetProcessingStatus::Processed).unwrap();
        assert_eq!(json, "\"processed\"");

        let decoded: AssetProcessingStatus = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded, AssetProcessingStatus::Processed);
    }

    #[test]
    fn asset_metadata_roundtrips() {
        let metadata = image_asset();
        let json = serde_json::to_string_pretty(&metadata).unwrap();
        let decoded: AssetMetadata = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded, metadata);
    }

    #[test]
    fn video_reference_asset_does_not_require_local_binary() {
        let metadata = video_reference_asset();
        assert_eq!(metadata.kind, AssetKind::VideoReference);

        let policy = AssetPolicy::video_reference();
        assert!(!policy.requires_local_copy);
        assert!(!policy.allow_binary);
    }

    #[test]
    fn asset_relevance_threshold_classification() {
        let critical = AssetRelevance::Critical;
        let high = AssetRelevance::High;
        let medium = AssetRelevance::Medium;
        let low = AssetRelevance::Low;
        let background = AssetRelevance::Background;

        // Critical meets all thresholds
        assert!(critical.meets_threshold(&critical));
        assert!(critical.meets_threshold(&high));
        assert!(critical.meets_threshold(&medium));
        assert!(critical.meets_threshold(&low));
        assert!(critical.meets_threshold(&background));

        // High meets high and below
        assert!(!high.meets_threshold(&critical));
        assert!(high.meets_threshold(&high));
        assert!(high.meets_threshold(&medium));

        // Background only meets background
        assert!(!background.meets_threshold(&low));
        assert!(background.meets_threshold(&background));
    }

    #[test]
    fn asset_policy_defaults() {
        let policy = AssetPolicy::default();
        assert!(!policy.requires_local_copy);
        assert!(policy.allow_binary);
        assert!(policy.max_size_bytes.is_none());
    }

    #[test]
    fn asset_registry_register_and_get() {
        let mut registry = AssetRegistry::new();
        let metadata = image_asset();

        registry.register(metadata.clone()).unwrap();

        assert_eq!(registry.len(), 1);
        assert!(registry.contains(&AssetId::from("asset-image-1")));
        assert_eq!(
            registry.get(&AssetId::from("asset-image-1")),
            Some(&metadata)
        );
    }

    #[test]
    fn asset_registry_duplicate_id_fails() {
        let mut registry = AssetRegistry::new();
        let metadata = image_asset();

        registry.register(metadata.clone()).unwrap();

        let err = registry.register(metadata).unwrap_err();
        assert_eq!(
            err,
            AssetRegistryError::DuplicateAssetId(AssetId::from("asset-image-1"))
        );
    }

    #[test]
    fn asset_registry_returns_none_for_missing_asset() {
        let registry = AssetRegistry::new();
        assert_eq!(registry.get(&AssetId::from("missing")), None);
    }
}
