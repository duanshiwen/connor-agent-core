//! # Asset Core
//!
//! Core domain types for AgentOS assets.
//!
//! Assets represent binary and metadata resources such as images, documents,
//! spreadsheets, slides, PDFs, video references, and audio files. They are
//! observed, captured, processed, and linked to work objects without becoming
//! foreground conversation participants.

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use tokio::fs::{self, OpenOptions};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

/// Unique identifier for an asset.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
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
    Question,
    Answer,
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

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct AssetHash(pub String);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AssetBlobRef {
    pub uri: String,
    pub content_hash: Option<AssetHash>,
    pub size_bytes: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AssetRecord {
    pub metadata: AssetMetadata,
    pub policy: AssetPolicy,
    pub blob: Option<AssetBlobRef>,
    pub processing_status: AssetProcessingStatus,
    pub extraction: Option<AssetExtractionResult>,
    pub linked_work_objects: Vec<AssetWorkObjectLink>,
    pub updated_at: DateTime<Utc>,
}

impl AssetRecord {
    pub fn new(metadata: AssetMetadata, policy: AssetPolicy, updated_at: DateTime<Utc>) -> Self {
        Self {
            processing_status: AssetProcessingStatus::Observed,
            metadata,
            policy,
            blob: None,
            extraction: None,
            linked_work_objects: Vec::new(),
            updated_at,
        }
    }

    pub fn with_blob(mut self, blob: AssetBlobRef) -> Self {
        self.blob = Some(blob);
        self
    }

    pub fn apply_extraction(mut self, extraction: AssetExtractionResult) -> Self {
        self.processing_status = extraction.status.clone();
        self.extraction = Some(extraction);
        self.updated_at = Utc::now();
        self
    }

    pub fn link_work_object(mut self, link: AssetWorkObjectLink) -> Self {
        if !self.linked_work_objects.contains(&link) {
            self.linked_work_objects.push(link);
            self.updated_at = Utc::now();
        }
        self
    }
}

/// Supported extraction pipeline boundary for assets.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AssetExtractionKind {
    Pdf,
    Docx,
    ImageOcr,
    Spreadsheet,
    Slide,
    VideoMetadata,
}

impl AssetExtractionKind {
    pub fn from_asset_kind(kind: &AssetKind) -> Result<Self, AssetExtractionError> {
        match kind {
            AssetKind::Pdf => Ok(Self::Pdf),
            AssetKind::Document => Ok(Self::Docx),
            AssetKind::Image => Ok(Self::ImageOcr),
            AssetKind::Spreadsheet => Ok(Self::Spreadsheet),
            AssetKind::Slide => Ok(Self::Slide),
            AssetKind::VideoReference => Ok(Self::VideoMetadata),
            unsupported => Err(AssetExtractionError::UnsupportedAssetKind(
                unsupported.clone(),
            )),
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Pdf => "pdf",
            Self::Docx => "docx",
            Self::ImageOcr => "image_ocr",
            Self::Spreadsheet => "spreadsheet",
            Self::Slide => "slide",
            Self::VideoMetadata => "video_metadata",
        }
    }
}

/// Input to an asset processor.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AssetExtractionRequest {
    pub asset_id: AssetId,
    pub asset_kind: AssetKind,
    pub extraction_kind: AssetExtractionKind,
    pub source_uri: Option<String>,
    pub mime_type: Option<String>,
    pub requested_at: DateTime<Utc>,
}

impl AssetExtractionRequest {
    pub fn from_metadata(metadata: AssetMetadata, requested_at: DateTime<Utc>) -> Self {
        let extraction_kind = AssetExtractionKind::from_asset_kind(&metadata.kind)
            .unwrap_or(AssetExtractionKind::VideoMetadata);
        Self {
            asset_id: metadata.id,
            asset_kind: metadata.kind,
            extraction_kind,
            source_uri: metadata.source.uri,
            mime_type: metadata.mime_type,
            requested_at,
        }
    }
}

/// Output from an asset processor.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AssetExtractionResult {
    pub asset_id: AssetId,
    pub extraction_kind: AssetExtractionKind,
    pub status: AssetProcessingStatus,
    pub extracted_text: Option<String>,
    pub metadata: serde_json::Value,
    pub processed_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum AssetExtractionError {
    #[error("unsupported asset kind for extraction: {0:?}")]
    UnsupportedAssetKind(AssetKind),
    #[error("asset extraction failed: {0}")]
    Failed(String),
}

#[async_trait]
pub trait AssetProcessor: Send + Sync {
    async fn extract(
        &self,
        request: AssetExtractionRequest,
    ) -> Result<AssetExtractionResult, AssetExtractionError>;
}

/// Deterministic test-only processor for tests and early pipeline wiring.
#[derive(Debug, Clone, Default)]
pub struct DeterministicAssetProcessor;

impl DeterministicAssetProcessor {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl AssetProcessor for DeterministicAssetProcessor {
    async fn extract(
        &self,
        request: AssetExtractionRequest,
    ) -> Result<AssetExtractionResult, AssetExtractionError> {
        let extraction_kind = AssetExtractionKind::from_asset_kind(&request.asset_kind)?;
        let metadata = serde_json::json!({
            "processor": "deterministic",
            "asset_kind": request.asset_kind,
            "extraction_kind": extraction_kind.as_str(),
            "source_uri": request.source_uri,
            "mime_type": request.mime_type,
        });
        Ok(AssetExtractionResult {
            extracted_text: Some(format!(
                "fake {} extraction for {}",
                extraction_kind.as_str(),
                request.asset_id
            )),
            asset_id: request.asset_id,
            extraction_kind,
            status: AssetProcessingStatus::Processed,
            metadata,
            processed_at: request.requested_at,
        })
    }
}

/// Errors from asset registry operations.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum AssetRegistryError {
    #[error("duplicate asset id: {0}")]
    DuplicateAssetId(AssetId),
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum AssetStoreError {
    #[error("asset store lock poisoned")]
    LockPoisoned,
    #[error("asset store io error: {0}")]
    Io(String),
    #[error("asset store serialization error: {0}")]
    Serde(String),
}

impl From<std::io::Error> for AssetStoreError {
    fn from(value: std::io::Error) -> Self {
        Self::Io(value.to_string())
    }
}

impl From<serde_json::Error> for AssetStoreError {
    fn from(value: serde_json::Error) -> Self {
        Self::Serde(value.to_string())
    }
}

#[async_trait]
pub trait AssetStore: Send + Sync {
    async fn upsert(&self, metadata: AssetMetadata) -> Result<(), AssetStoreError>;
    async fn get(&self, id: &AssetId) -> Result<Option<AssetMetadata>, AssetStoreError>;
    async fn list(&self) -> Result<Vec<AssetMetadata>, AssetStoreError>;
    async fn len(&self) -> Result<usize, AssetStoreError>;
    async fn is_empty(&self) -> Result<bool, AssetStoreError>;
}

/// JSONL-backed asset metadata store with reloadable in-memory index.
#[derive(Debug, Clone)]
pub struct FsAssetStore {
    path: PathBuf,
    assets: Arc<Mutex<BTreeMap<AssetId, AssetMetadata>>>,
}

impl FsAssetStore {
    pub async fn open(path: impl AsRef<Path>) -> Result<Self, AssetStoreError> {
        let path = path.as_ref().to_path_buf();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).await?;
        }
        let assets = Arc::new(Mutex::new(BTreeMap::new()));
        if fs::try_exists(&path).await? {
            let file = OpenOptions::new().read(true).open(&path).await?;
            let mut lines = BufReader::new(file).lines();
            while let Some(line) = lines.next_line().await? {
                if line.trim().is_empty() {
                    continue;
                }
                let metadata: AssetMetadata = serde_json::from_str(&line)?;
                assets
                    .lock()
                    .map_err(|_| AssetStoreError::LockPoisoned)?
                    .insert(metadata.id.clone(), metadata);
            }
        } else {
            OpenOptions::new()
                .create(true)
                .append(true)
                .open(&path)
                .await?;
        }
        Ok(Self { path, assets })
    }

    async fn append_metadata(&self, metadata: &AssetMetadata) -> Result<(), AssetStoreError> {
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)
            .await?;
        let line = serde_json::to_string(metadata)?;
        file.write_all(line.as_bytes()).await?;
        file.write_all(b"\n").await?;
        file.flush().await?;
        Ok(())
    }
}

#[async_trait]
impl AssetStore for FsAssetStore {
    async fn upsert(&self, metadata: AssetMetadata) -> Result<(), AssetStoreError> {
        self.append_metadata(&metadata).await?;
        self.assets
            .lock()
            .map_err(|_| AssetStoreError::LockPoisoned)?
            .insert(metadata.id.clone(), metadata);
        Ok(())
    }

    async fn get(&self, id: &AssetId) -> Result<Option<AssetMetadata>, AssetStoreError> {
        Ok(self
            .assets
            .lock()
            .map_err(|_| AssetStoreError::LockPoisoned)?
            .get(id)
            .cloned())
    }

    async fn list(&self) -> Result<Vec<AssetMetadata>, AssetStoreError> {
        Ok(self
            .assets
            .lock()
            .map_err(|_| AssetStoreError::LockPoisoned)?
            .values()
            .cloned()
            .collect())
    }

    async fn len(&self) -> Result<usize, AssetStoreError> {
        Ok(self
            .assets
            .lock()
            .map_err(|_| AssetStoreError::LockPoisoned)?
            .len())
    }

    async fn is_empty(&self) -> Result<bool, AssetStoreError> {
        Ok(self
            .assets
            .lock()
            .map_err(|_| AssetStoreError::LockPoisoned)?
            .is_empty())
    }
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

    #[tokio::test]
    async fn fs_asset_store_persists_and_reloads_asset_metadata_after_restart() {
        let tempdir = tempfile::tempdir().unwrap();
        let path = tempdir.path().join("assets.jsonl");
        let first = image_asset();
        let second = video_reference_asset();

        {
            let store = FsAssetStore::open(&path).await.unwrap();
            store.upsert(first.clone()).await.unwrap();
            store.upsert(second.clone()).await.unwrap();
            assert_eq!(store.len().await.unwrap(), 2);
        }

        let restarted = FsAssetStore::open(&path).await.unwrap();
        assert_eq!(restarted.len().await.unwrap(), 2);
        assert_eq!(restarted.get(&first.id).await.unwrap(), Some(first));
        assert_eq!(restarted.get(&second.id).await.unwrap(), Some(second));
    }

    #[tokio::test]
    async fn fs_asset_store_lists_assets_in_stable_id_order() {
        let tempdir = tempfile::tempdir().unwrap();
        let path = tempdir.path().join("assets.jsonl");
        let store = FsAssetStore::open(&path).await.unwrap();
        let mut a = image_asset();
        a.id = AssetId::from("asset-b");
        let mut b = video_reference_asset();
        b.id = AssetId::from("asset-a");

        store.upsert(a).await.unwrap();
        store.upsert(b).await.unwrap();

        let ids = store
            .list()
            .await
            .unwrap()
            .into_iter()
            .map(|asset| asset.id)
            .collect::<Vec<_>>();
        assert_eq!(
            ids,
            vec![AssetId::from("asset-a"), AssetId::from("asset-b")]
        );
    }

    #[tokio::test]
    async fn fs_asset_store_upsert_replaces_index_value_across_restart() {
        let tempdir = tempfile::tempdir().unwrap();
        let path = tempdir.path().join("assets.jsonl");
        let store = FsAssetStore::open(&path).await.unwrap();
        let original = image_asset();
        let updated = original.clone().with_title("Updated Photo");

        store.upsert(original.clone()).await.unwrap();
        store.upsert(updated.clone()).await.unwrap();
        assert_eq!(
            store.get(&original.id).await.unwrap(),
            Some(updated.clone())
        );

        let restarted = FsAssetStore::open(&path).await.unwrap();
        assert_eq!(restarted.len().await.unwrap(), 1);
        assert_eq!(restarted.get(&original.id).await.unwrap(), Some(updated));
    }

    #[tokio::test]
    async fn fs_asset_store_creates_parent_directories_and_empty_index() {
        let tempdir = tempfile::tempdir().unwrap();
        let path = tempdir.path().join("nested/assets/assets.jsonl");

        let store = FsAssetStore::open(&path).await.unwrap();
        assert!(path.exists());
        assert!(store.list().await.unwrap().is_empty());
    }

    #[test]
    fn asset_extraction_kind_roundtrips() {
        let kinds = vec![
            AssetExtractionKind::Pdf,
            AssetExtractionKind::Docx,
            AssetExtractionKind::ImageOcr,
            AssetExtractionKind::Spreadsheet,
            AssetExtractionKind::Slide,
            AssetExtractionKind::VideoMetadata,
        ];
        let json = serde_json::to_string(&kinds).unwrap();
        let decoded: Vec<AssetExtractionKind> = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded, kinds);
    }

    #[test]
    fn asset_extraction_request_infers_kind_from_asset_metadata() {
        let pdf = AssetMetadata::new(
            "asset-pdf-1",
            AssetKind::Pdf,
            AssetSource::new("2026-05-24T12:00:00Z".parse().unwrap()),
            AssetRelevance::High,
            "2026-05-24T12:00:00Z".parse().unwrap(),
        );
        let request =
            AssetExtractionRequest::from_metadata(pdf, "2026-05-24T12:00:00Z".parse().unwrap());
        assert_eq!(request.extraction_kind, AssetExtractionKind::Pdf);
        assert_eq!(request.asset_id, AssetId::from("asset-pdf-1"));
    }

    #[tokio::test]
    async fn deterministic_asset_processor_extracts_pdf_docx_ocr_spreadsheet_slide_and_video_metadata()
     {
        let processor = DeterministicAssetProcessor::new();
        let assets = vec![
            ("asset-pdf", AssetKind::Pdf, AssetExtractionKind::Pdf),
            ("asset-docx", AssetKind::Document, AssetExtractionKind::Docx),
            (
                "asset-image",
                AssetKind::Image,
                AssetExtractionKind::ImageOcr,
            ),
            (
                "asset-sheet",
                AssetKind::Spreadsheet,
                AssetExtractionKind::Spreadsheet,
            ),
            ("asset-slide", AssetKind::Slide, AssetExtractionKind::Slide),
            (
                "asset-video",
                AssetKind::VideoReference,
                AssetExtractionKind::VideoMetadata,
            ),
        ];

        for (id, kind, extraction_kind) in assets {
            let metadata = AssetMetadata::new(
                id,
                kind,
                AssetSource::new("2026-05-24T12:00:00Z".parse().unwrap()),
                AssetRelevance::High,
                "2026-05-24T12:00:00Z".parse().unwrap(),
            );
            let result = processor
                .extract(AssetExtractionRequest::from_metadata(
                    metadata,
                    "2026-05-24T12:00:00Z".parse().unwrap(),
                ))
                .await
                .unwrap();

            assert_eq!(result.asset_id, AssetId::from(id));
            assert_eq!(result.extraction_kind, extraction_kind);
            assert_eq!(result.status, AssetProcessingStatus::Processed);
            assert!(result.extracted_text.as_ref().unwrap().contains(id));
            assert_eq!(result.metadata["processor"], "deterministic");
        }
    }

    #[tokio::test]
    async fn deterministic_asset_processor_rejects_unsupported_unknown_assets() {
        let processor = DeterministicAssetProcessor::new();
        let metadata = AssetMetadata::new(
            "asset-unknown",
            AssetKind::Unknown,
            AssetSource::new("2026-05-24T12:00:00Z".parse().unwrap()),
            AssetRelevance::Low,
            "2026-05-24T12:00:00Z".parse().unwrap(),
        );

        assert_eq!(
            processor
                .extract(AssetExtractionRequest::from_metadata(
                    metadata,
                    "2026-05-24T12:00:00Z".parse().unwrap(),
                ))
                .await
                .unwrap_err(),
            AssetExtractionError::UnsupportedAssetKind(AssetKind::Unknown)
        );
    }

    #[tokio::test]
    async fn deterministic_asset_processor_records_processed_at_and_metadata() {
        let processor = DeterministicAssetProcessor::new();
        let metadata = AssetMetadata::new(
            "asset-video",
            AssetKind::VideoReference,
            AssetSource::new("2026-05-24T12:00:00Z".parse().unwrap())
                .with_uri("https://example.com/video.mp4"),
            AssetRelevance::Medium,
            "2026-05-24T12:00:00Z".parse().unwrap(),
        );

        let result = processor
            .extract(AssetExtractionRequest::from_metadata(
                metadata,
                "2026-05-24T13:00:00Z".parse().unwrap(),
            ))
            .await
            .unwrap();

        assert_eq!(
            result.processed_at,
            "2026-05-24T13:00:00Z".parse::<DateTime<Utc>>().unwrap()
        );
        assert_eq!(
            result.metadata["source_uri"],
            "https://example.com/video.mp4"
        );
        assert_eq!(result.metadata["extraction_kind"], "video_metadata");
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
