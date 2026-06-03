//! Filesystem-backed AgentOS knowledge engine store.
//!
//! This module implements the first durable layer under `.ke-store`: append-only
//! JSONL event/audit segments and a content-addressed SHA-256 blob store. It is
//! intentionally storage-focused and accepts any serde-serializable event/audit
//! envelope so domain crates can evolve their contracts without creating a hard
//! dependency cycle.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Datelike, Utc};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::{AgentOsStorage, StorageError, StorageResult, create_dir_all};

const EVENT_SEGMENT_FILE: &str = "seg-000001.kevent.ndjson";
const AUDIT_SEGMENT_FILE: &str = "audit-seg-000001.kaudit.ndjson";
const BLOB_EXTENSION: &str = "blob";
const BLOB_META_EXTENSION: &str = "meta.json";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KnowledgeRecordProjectionKind {
    Objects,
    Aliases,
    Attributes,
    Relations,
    Claims,
    Evidence,
    Mechanisms,
    Evaluations,
    MetaphorMappings,
    Assets,
    AssetPropertyBindings,
    Observations,
    Tools,
}

impl KnowledgeRecordProjectionKind {
    pub fn filename(self) -> &'static str {
        match self {
            Self::Objects => "objects.krec.ndjson",
            Self::Aliases => "aliases.krec.ndjson",
            Self::Attributes => "attributes.krec.ndjson",
            Self::Relations => "relations.krec.ndjson",
            Self::Claims => "claims.krec.ndjson",
            Self::Evidence => "evidence.krec.ndjson",
            Self::Mechanisms => "mechanisms.krec.ndjson",
            Self::Evaluations => "evaluations.krec.ndjson",
            Self::MetaphorMappings => "metaphor_mappings.krec.ndjson",
            Self::Assets => "assets.krec.ndjson",
            Self::AssetPropertyBindings => "asset_property_bindings.krec.ndjson",
            Self::Observations => "observations.krec.ndjson",
            Self::Tools => "tools.krec.ndjson",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KnowledgeProjectionWriteReport {
    pub path: PathBuf,
    pub record_count: u64,
    pub byte_len: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KnowledgeLogAppendReport {
    pub path: PathBuf,
    pub byte_len: u64,
    pub line_count: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct KnowledgeChainedLogAppendReport {
    pub append: KnowledgeLogAppendReport,
    pub record: Value,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KnowledgeBlobPutReport {
    pub blob_hash: String,
    pub size_bytes: u64,
    pub mime_type_detected: String,
    pub storage_path: PathBuf,
    pub metadata_path: PathBuf,
    pub deduplicated: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KnowledgeBlobMetadataRecord {
    pub blob_hash: String,
    pub size_bytes: u64,
    pub mime_type_detected: String,
    pub compression: String,
    pub created_at: DateTime<Utc>,
    pub integrity: KnowledgeBlobIntegrity,
    pub storage: KnowledgeBlobStorageRef,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KnowledgeBlobIntegrity {
    pub hash_algorithm: String,
    pub verified_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KnowledgeBlobStorageRef {
    pub path: String,
    pub encrypted: bool,
}

/// Durable filesystem implementation for the AgentOS `.ke-store` layout.
#[derive(Debug, Clone)]
pub struct FsKnowledgeEngineStore {
    root: PathBuf,
}

impl FsKnowledgeEngineStore {
    pub fn new(root: impl AsRef<Path>) -> StorageResult<Self> {
        let root = root.as_ref().to_path_buf();
        create_dir_all(&root)?;
        Ok(Self { root })
    }

    pub fn for_storage(storage: &AgentOsStorage) -> StorageResult<Self> {
        Self::new(storage.knowledge_engine_root())
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn append_event<T: Serialize>(
        &self,
        timestamp: DateTime<Utc>,
        event: &T,
    ) -> StorageResult<KnowledgeLogAppendReport> {
        let path = self.segment_path("events", timestamp, EVENT_SEGMENT_FILE);
        append_jsonl(&path, event)
    }

    pub fn append_event_chained(
        &self,
        timestamp: DateTime<Utc>,
        event: Value,
    ) -> StorageResult<KnowledgeChainedLogAppendReport> {
        let path = self.segment_path("events", timestamp, EVENT_SEGMENT_FILE);
        append_jsonl_chained(&path, event, "prev_event_hash", "event_hash")
    }

    pub fn append_audit<T: Serialize>(
        &self,
        timestamp: DateTime<Utc>,
        audit: &T,
    ) -> StorageResult<KnowledgeLogAppendReport> {
        let path = self.segment_path("audit", timestamp, AUDIT_SEGMENT_FILE);
        append_jsonl(&path, audit)
    }

    pub fn append_audit_chained(
        &self,
        timestamp: DateTime<Utc>,
        audit: Value,
    ) -> StorageResult<KnowledgeChainedLogAppendReport> {
        let path = self.segment_path("audit", timestamp, AUDIT_SEGMENT_FILE);
        append_jsonl_chained(&path, audit, "prev_audit_hash", "audit_hash")
    }

    pub fn read_events<T: DeserializeOwned>(&self, year: i32, month: u32) -> StorageResult<Vec<T>> {
        read_jsonl(&self.month_segment_path("events", year, month, EVENT_SEGMENT_FILE))
    }

    pub fn read_audit<T: DeserializeOwned>(&self, year: i32, month: u32) -> StorageResult<Vec<T>> {
        read_jsonl(&self.month_segment_path("audit", year, month, AUDIT_SEGMENT_FILE))
    }

    pub fn replace_projection_records<T: Serialize>(
        &self,
        kind: KnowledgeRecordProjectionKind,
        records: &[T],
    ) -> StorageResult<KnowledgeProjectionWriteReport> {
        let path = self.projection_path(kind);
        write_jsonl_replace(&path, records)
    }

    pub fn read_projection_records<T: DeserializeOwned>(
        &self,
        kind: KnowledgeRecordProjectionKind,
    ) -> StorageResult<Vec<T>> {
        read_jsonl(&self.projection_path(kind))
    }

    pub fn projection_path(&self, kind: KnowledgeRecordProjectionKind) -> PathBuf {
        self.root.join("records").join(kind.filename())
    }

    pub fn put_blob(
        &self,
        bytes: impl AsRef<[u8]>,
        mime_type_detected: impl Into<String>,
        created_at: DateTime<Utc>,
    ) -> StorageResult<KnowledgeBlobPutReport> {
        let bytes = bytes.as_ref();
        let hash_hex = sha256_hex(bytes);
        let blob_hash = format!("sha256:{hash_hex}");
        let blob_path = self.blob_path_for_hash_hex(&hash_hex);
        let metadata_path = self.blob_metadata_path_for_hash_hex(&hash_hex);
        if let Some(parent) = blob_path.parent() {
            create_dir_all(parent)?;
        }

        let deduplicated = blob_path.exists();
        if !deduplicated {
            fs::write(&blob_path, bytes).map_err(|source| StorageError::Io {
                path: blob_path.clone(),
                source,
            })?;
        }

        let metadata = KnowledgeBlobMetadataRecord {
            blob_hash: blob_hash.clone(),
            size_bytes: bytes.len() as u64,
            mime_type_detected: mime_type_detected.into(),
            compression: "none".to_string(),
            created_at,
            integrity: KnowledgeBlobIntegrity {
                hash_algorithm: "sha256".to_string(),
                verified_at: created_at,
            },
            storage: KnowledgeBlobStorageRef {
                path: relative_to_root(&self.root, &blob_path),
                encrypted: false,
            },
        };
        write_json(&metadata_path, &metadata)?;

        Ok(KnowledgeBlobPutReport {
            blob_hash,
            size_bytes: bytes.len() as u64,
            mime_type_detected: metadata.mime_type_detected,
            storage_path: blob_path,
            metadata_path,
            deduplicated,
        })
    }

    pub fn read_blob(&self, blob_hash: &str) -> StorageResult<Option<Vec<u8>>> {
        let hash_hex = normalize_sha256_hash(blob_hash)?;
        let path = self.blob_path_for_hash_hex(&hash_hex);
        if !path.exists() {
            return Ok(None);
        }
        fs::read(&path)
            .map(Some)
            .map_err(|source| StorageError::Io { path, source })
    }

    pub fn read_blob_metadata(
        &self,
        blob_hash: &str,
    ) -> StorageResult<Option<KnowledgeBlobMetadataRecord>> {
        let hash_hex = normalize_sha256_hash(blob_hash)?;
        let path = self.blob_metadata_path_for_hash_hex(&hash_hex);
        if !path.exists() {
            return Ok(None);
        }
        read_json(&path).map(Some)
    }

    fn segment_path(&self, layer: &str, timestamp: DateTime<Utc>, filename: &str) -> PathBuf {
        self.month_segment_path(layer, timestamp.year(), timestamp.month(), filename)
    }

    fn month_segment_path(&self, layer: &str, year: i32, month: u32, filename: &str) -> PathBuf {
        self.root
            .join(layer)
            .join(format!("{year:04}"))
            .join(format!("{month:02}"))
            .join(filename)
    }

    fn blob_path_for_hash_hex(&self, hash_hex: &str) -> PathBuf {
        self.root
            .join("blobs")
            .join("sha256")
            .join(&hash_hex[0..2])
            .join(&hash_hex[2..4])
            .join(format!("{hash_hex}.{BLOB_EXTENSION}"))
    }

    fn blob_metadata_path_for_hash_hex(&self, hash_hex: &str) -> PathBuf {
        self.root
            .join("blobs")
            .join("sha256")
            .join(&hash_hex[0..2])
            .join(&hash_hex[2..4])
            .join(format!("{hash_hex}.{BLOB_META_EXTENSION}"))
    }
}

fn append_jsonl_chained(
    path: &Path,
    mut record: Value,
    prev_hash_field: &str,
    hash_field: &str,
) -> StorageResult<KnowledgeChainedLogAppendReport> {
    let prev_hash = latest_jsonl_hash(path, hash_field)?;
    {
        let object =
            record
                .as_object_mut()
                .ok_or_else(|| StorageError::KnowledgeEngineRecordShape {
                    path: path.to_path_buf(),
                    reason: "chained log record must be a JSON object".to_string(),
                })?;
        object.insert(
            prev_hash_field.to_string(),
            prev_hash.map(Value::String).unwrap_or(Value::Null),
        );
        object.insert(hash_field.to_string(), Value::Null);
    }

    let hash_input =
        serde_json::to_vec(&record).map_err(|source| StorageError::KnowledgeEngineSerde {
            path: path.to_path_buf(),
            source,
        })?;
    let object =
        record
            .as_object_mut()
            .ok_or_else(|| StorageError::KnowledgeEngineRecordShape {
                path: path.to_path_buf(),
                reason: "chained log record must be a JSON object".to_string(),
            })?;
    object.insert(
        hash_field.to_string(),
        Value::String(format!("sha256:{}", sha256_hex(&hash_input))),
    );

    let append = append_jsonl(path, &record)?;
    Ok(KnowledgeChainedLogAppendReport { append, record })
}

fn latest_jsonl_hash(path: &Path, hash_field: &str) -> StorageResult<Option<String>> {
    if !path.exists() {
        return Ok(None);
    }
    let text = fs::read_to_string(path).map_err(|source| StorageError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    let Some(last_line) = text.lines().rev().find(|line| !line.trim().is_empty()) else {
        return Ok(None);
    };
    let value: Value =
        serde_json::from_str(last_line).map_err(|source| StorageError::KnowledgeEngineSerde {
            path: path.to_path_buf(),
            source,
        })?;
    Ok(value
        .get(hash_field)
        .and_then(Value::as_str)
        .map(|hash| hash.to_string()))
}

fn write_jsonl_replace<T: Serialize>(
    path: &Path,
    records: &[T],
) -> StorageResult<KnowledgeProjectionWriteReport> {
    if let Some(parent) = path.parent() {
        create_dir_all(parent)?;
    }
    let tmp_path = path.with_extension("tmp");
    let mut bytes = Vec::new();
    for record in records {
        let mut line =
            serde_json::to_vec(record).map_err(|source| StorageError::KnowledgeEngineSerde {
                path: path.to_path_buf(),
                source,
            })?;
        line.push(b'\n');
        bytes.extend(line);
    }
    fs::write(&tmp_path, &bytes).map_err(|source| StorageError::Io {
        path: tmp_path.clone(),
        source,
    })?;
    fs::rename(&tmp_path, path).map_err(|source| StorageError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    Ok(KnowledgeProjectionWriteReport {
        path: path.to_path_buf(),
        record_count: records.len() as u64,
        byte_len: bytes.len() as u64,
    })
}

fn append_jsonl<T: Serialize>(path: &Path, record: &T) -> StorageResult<KnowledgeLogAppendReport> {
    if let Some(parent) = path.parent() {
        create_dir_all(parent)?;
    }
    let mut line =
        serde_json::to_vec(record).map_err(|source| StorageError::KnowledgeEngineSerde {
            path: path.to_path_buf(),
            source,
        })?;
    line.push(b'\n');
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map_err(|source| StorageError::Io {
            path: path.to_path_buf(),
            source,
        })?;
    file.write_all(&line).map_err(|source| StorageError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    Ok(KnowledgeLogAppendReport {
        path: path.to_path_buf(),
        byte_len: line.len() as u64,
        line_count: 1,
    })
}

fn read_jsonl<T: DeserializeOwned>(path: &Path) -> StorageResult<Vec<T>> {
    if !path.exists() {
        return Ok(Vec::new());
    }
    let text = fs::read_to_string(path).map_err(|source| StorageError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    let mut records = Vec::new();
    for line in text.lines().filter(|line| !line.trim().is_empty()) {
        records.push(serde_json::from_str(line).map_err(|source| {
            StorageError::KnowledgeEngineSerde {
                path: path.to_path_buf(),
                source,
            }
        })?);
    }
    Ok(records)
}

fn write_json<T: Serialize>(path: &Path, value: &T) -> StorageResult<()> {
    let bytes =
        serde_json::to_vec_pretty(value).map_err(|source| StorageError::KnowledgeEngineSerde {
            path: path.to_path_buf(),
            source,
        })?;
    fs::write(path, bytes).map_err(|source| StorageError::Io {
        path: path.to_path_buf(),
        source,
    })
}

fn read_json<T: DeserializeOwned>(path: &Path) -> StorageResult<T> {
    let bytes = fs::read(path).map_err(|source| StorageError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    serde_json::from_slice(&bytes).map_err(|source| StorageError::KnowledgeEngineSerde {
        path: path.to_path_buf(),
        source,
    })
}

fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    let digest = hasher.finalize();
    digest.iter().map(|b| format!("{b:02x}")).collect()
}

fn normalize_sha256_hash(blob_hash: &str) -> StorageResult<String> {
    let hash = blob_hash.strip_prefix("sha256:").unwrap_or(blob_hash);
    if hash.len() != 64 || !hash.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(StorageError::InvalidKnowledgeBlobHash {
            blob_hash: blob_hash.to_string(),
        });
    }
    Ok(hash.to_ascii_lowercase())
}

fn relative_to_root(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}
