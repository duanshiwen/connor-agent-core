//! AgentOS durable storage layout primitives.

pub mod artifact_store;
pub mod backup;
pub mod knowledge_engine_store;
pub mod lock;
pub mod migration;
pub mod repair;

use std::fs;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use thiserror::Error;

pub use artifact_store::{
    ArtifactContentMetadata, ArtifactVerificationIssue, ArtifactVerificationReport,
    FsArtifactRecord, FsArtifactStore,
};
pub use backup::{BackupFileEntry, BackupManifest, BackupReport, RestoreReport, StorageBackup};
pub use knowledge_engine_store::{
    FsKnowledgeEngineStore, KnowledgeBlobIntegrity, KnowledgeBlobMetadataRecord,
    KnowledgeBlobPutReport, KnowledgeBlobStorageRef, KnowledgeLogAppendReport,
};
pub use lock::{StorageLockGuard, StorageLockInfo, StorageLockOptions};
pub use migration::{
    MigrationMode, MigrationPlan, MigrationPlanStep, MigrationReport, MigrationStatus,
    StorageMigration, StorageMigrationRegistry,
};
pub use repair::{
    ArtifactReferenceReport, BrokenArtifactReference, ConversationProjectionRebuildFailure,
    ProjectionRebuildReport, StorageRepair, StorageRepairIssue, StorageRepairReport,
    StorageRepairSeverity,
};

pub const STORAGE_LAYOUT_VERSION: u32 = 1;

pub const STORAGE_LAYOUT_DIRECTORIES: [&str; 11] = [
    "config",
    "conversations",
    "runs",
    "actions",
    "approvals",
    "audit",
    "artifacts",
    "knowledge",
    "indexes",
    "identity",
    "connectors",
];

/// Local-first object/relation knowledge engine store directory.
///
/// This is the authoritative knowledge storage root described by the desktop
/// AgentOS storage design. Markdown remains an export/compatibility view; the
/// authoritative layers live under this content-addressed, event-sourced store.
pub const KNOWLEDGE_ENGINE_STORE_DIR: &str = ".ke-store";

pub const KNOWLEDGE_ENGINE_AUTHORITATIVE_LAYERS: [&str; 3] = ["events", "audit", "blobs"];

pub const KNOWLEDGE_ENGINE_PROJECTION_LAYERS: [&str; 4] =
    ["records", "indexes", "snapshots", "derivatives"];

/// v0.1 desktop full-node layout for the object/relation knowledge engine.
pub const KNOWLEDGE_ENGINE_LAYOUT_DIRECTORIES: [&str; 21] = [
    "events",
    "audit",
    "records",
    "blobs",
    "blobs/sha256",
    "derivatives",
    "derivatives/previews",
    "derivatives/thumbnails",
    "derivatives/ocr",
    "derivatives/extracted",
    "derivatives/transcripts",
    "derivatives/keyframes",
    "derivatives/embeddings",
    "indexes",
    "indexes/sqlite",
    "indexes/tantivy",
    "indexes/graph",
    "indexes/vector",
    "snapshots",
    "exports",
    "quarantine",
];

#[derive(Debug, Error)]
pub enum StorageError {
    #[error("storage io error at {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("storage manifest serialization failed at {path}: {source}")]
    ManifestSerde {
        path: PathBuf,
        #[source]
        source: serde_json::Error,
    },

    #[error("storage lock serialization failed at {path}: {source}")]
    LockSerde {
        path: PathBuf,
        #[source]
        source: serde_json::Error,
    },

    #[error("artifact serialization failed at {path}: {source}")]
    ArtifactSerde {
        path: PathBuf,
        #[source]
        source: serde_json::Error,
    },

    #[error("knowledge engine serialization failed at {path}: {source}")]
    KnowledgeEngineSerde {
        path: PathBuf,
        #[source]
        source: serde_json::Error,
    },

    #[error("invalid knowledge blob sha256 hash: {blob_hash}")]
    InvalidKnowledgeBlobHash { blob_hash: String },

    #[error("artifact already exists: {artifact_id}")]
    ArtifactAlreadyExists { artifact_id: String },

    #[error("invalid artifact id for filesystem path: {artifact_id}")]
    InvalidArtifactIdPath { artifact_id: String },

    #[error("backup manifest serialization failed at {path}: {source}")]
    BackupManifestSerde {
        path: PathBuf,
        #[source]
        source: serde_json::Error,
    },

    #[error("backup/restore target is not empty: {path}")]
    BackupTargetNotEmpty { path: PathBuf },

    #[error("backup file missing: {path}")]
    BackupFileMissing { path: String },

    #[error("backup contains unexpected file: {path}")]
    BackupUnexpectedFile { path: String },

    #[error("backup manifest contains invalid file path: {path}")]
    BackupInvalidFilePath { path: String },

    #[error("backup integrity mismatch at {path}: expected {expected}, actual {actual}")]
    BackupIntegrityMismatch {
        path: String,
        expected: String,
        actual: String,
    },

    #[error("restore integrity failed: {message}")]
    RestoreIntegrityFailed { message: String },

    #[error("storage lock already held at {path} by {owner_id} until {expires_at}")]
    LockAlreadyHeld {
        path: PathBuf,
        owner_id: String,
        expires_at: DateTime<Utc>,
    },

    #[error("unsupported storage version {found}, expected {expected}")]
    UnsupportedVersion { found: u32, expected: u32 },

    #[error("duplicate storage migration {from_version} -> {to_version}")]
    DuplicateMigration { from_version: u32, to_version: u32 },

    #[error("invalid storage migration {name}: {from_version} -> {to_version}")]
    InvalidMigration {
        name: String,
        from_version: u32,
        to_version: u32,
    },

    #[error("storage migration path not found from {from_version} to {target_version}")]
    MigrationPathNotFound {
        from_version: u32,
        target_version: u32,
    },

    #[error("storage migration {name} failed: {message}")]
    MigrationFailed { name: String, message: String },
}

pub type StorageResult<T> = Result<T, StorageError>;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StorageManifest {
    pub storage_version: u32,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub layout_directories: Vec<String>,
    #[serde(default = "default_knowledge_engine_store_dir")]
    pub knowledge_engine_store_dir: String,
    #[serde(default = "default_knowledge_engine_layout_directories")]
    pub knowledge_engine_layout_directories: Vec<String>,
    #[serde(default = "default_knowledge_engine_authoritative_layers")]
    pub knowledge_engine_authoritative_layers: Vec<String>,
    #[serde(default = "default_knowledge_engine_projection_layers")]
    pub knowledge_engine_projection_layers: Vec<String>,
}

impl StorageManifest {
    pub fn new(now: DateTime<Utc>) -> Self {
        Self {
            storage_version: STORAGE_LAYOUT_VERSION,
            created_at: now,
            updated_at: now,
            layout_directories: default_storage_layout_directories(),
            knowledge_engine_store_dir: default_knowledge_engine_store_dir(),
            knowledge_engine_layout_directories: default_knowledge_engine_layout_directories(),
            knowledge_engine_authoritative_layers: default_knowledge_engine_authoritative_layers(),
            knowledge_engine_projection_layers: default_knowledge_engine_projection_layers(),
        }
    }

    fn refreshed(mut self, now: DateTime<Utc>) -> Self {
        self.updated_at = now;
        self.layout_directories = default_storage_layout_directories();
        self.knowledge_engine_store_dir = default_knowledge_engine_store_dir();
        self.knowledge_engine_layout_directories = default_knowledge_engine_layout_directories();
        self.knowledge_engine_authoritative_layers =
            default_knowledge_engine_authoritative_layers();
        self.knowledge_engine_projection_layers = default_knowledge_engine_projection_layers();
        self
    }
}

fn default_storage_layout_directories() -> Vec<String> {
    STORAGE_LAYOUT_DIRECTORIES
        .iter()
        .map(|name| (*name).to_string())
        .collect()
}

fn default_knowledge_engine_store_dir() -> String {
    KNOWLEDGE_ENGINE_STORE_DIR.to_string()
}

fn default_knowledge_engine_layout_directories() -> Vec<String> {
    KNOWLEDGE_ENGINE_LAYOUT_DIRECTORIES
        .iter()
        .map(|name| (*name).to_string())
        .collect()
}

fn default_knowledge_engine_authoritative_layers() -> Vec<String> {
    KNOWLEDGE_ENGINE_AUTHORITATIVE_LAYERS
        .iter()
        .map(|name| (*name).to_string())
        .collect()
}

fn default_knowledge_engine_projection_layers() -> Vec<String> {
    KNOWLEDGE_ENGINE_PROJECTION_LAYERS
        .iter()
        .map(|name| (*name).to_string())
        .collect()
}

#[derive(Debug, Clone)]
pub struct AgentOsStorage {
    root: PathBuf,
    manifest: StorageManifest,
}

impl AgentOsStorage {
    pub fn init(root: impl AsRef<Path>) -> StorageResult<Self> {
        let root = root.as_ref().to_path_buf();
        create_dir_all(&root)?;

        for dir in STORAGE_LAYOUT_DIRECTORIES {
            create_dir_all(root.join(dir))?;
        }

        let knowledge_engine_root = root.join(KNOWLEDGE_ENGINE_STORE_DIR);
        create_dir_all(&knowledge_engine_root)?;
        for dir in KNOWLEDGE_ENGINE_LAYOUT_DIRECTORIES {
            create_dir_all(knowledge_engine_root.join(dir))?;
        }

        let manifest_path = root.join("manifest.json");
        let now = Utc::now();
        let manifest = if manifest_path.exists() {
            read_manifest(&manifest_path)?.refreshed(now)
        } else {
            StorageManifest::new(now)
        };

        if manifest.storage_version != STORAGE_LAYOUT_VERSION {
            return Err(StorageError::UnsupportedVersion {
                found: manifest.storage_version,
                expected: STORAGE_LAYOUT_VERSION,
            });
        }

        write_manifest(&manifest_path, &manifest)?;

        Ok(Self { root, manifest })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn manifest(&self) -> &StorageManifest {
        &self.manifest
    }

    pub fn manifest_path(&self) -> PathBuf {
        self.root.join("manifest.json")
    }

    pub fn path_for(&self, layout_dir: &str) -> PathBuf {
        self.root.join(layout_dir)
    }

    pub fn knowledge_engine_root(&self) -> PathBuf {
        self.root.join(KNOWLEDGE_ENGINE_STORE_DIR)
    }

    pub fn knowledge_engine_path_for(&self, layout_dir: &str) -> PathBuf {
        self.knowledge_engine_root().join(layout_dir)
    }

    pub fn acquire_lock(&self, options: StorageLockOptions) -> StorageResult<StorageLockGuard> {
        StorageLockGuard::acquire(&self.root, options)
    }
}

pub(crate) fn create_dir_all(path: impl AsRef<Path>) -> StorageResult<()> {
    let path = path.as_ref();
    fs::create_dir_all(path).map_err(|source| StorageError::Io {
        path: path.to_path_buf(),
        source,
    })
}

pub(crate) fn read_manifest(path: &Path) -> StorageResult<StorageManifest> {
    let bytes = fs::read(path).map_err(|source| StorageError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    serde_json::from_slice(&bytes).map_err(|source| StorageError::ManifestSerde {
        path: path.to_path_buf(),
        source,
    })
}

pub(crate) fn write_manifest(path: &Path, manifest: &StorageManifest) -> StorageResult<()> {
    let bytes =
        serde_json::to_vec_pretty(manifest).map_err(|source| StorageError::ManifestSerde {
            path: path.to_path_buf(),
            source,
        })?;
    fs::write(path, bytes).map_err(|source| StorageError::Io {
        path: path.to_path_buf(),
        source,
    })
}
