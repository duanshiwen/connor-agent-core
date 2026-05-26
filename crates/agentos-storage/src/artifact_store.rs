//! Filesystem-backed artifact store.
//!
//! Artifacts are stored under `{storage_root}/artifacts/{artifact_id}` with a
//! descriptor, content bytes, and a record containing content metadata.

use std::fs;
use std::path::{Path, PathBuf};

use artifact_core::{ArtifactDescriptor, ArtifactId, ArtifactStore, ArtifactStoreError};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::{AgentOsStorage, StorageError, StorageResult, create_dir_all};

const DESCRIPTOR_FILE: &str = "descriptor.json";
const CONTENT_FILE: &str = "content.bin";
const RECORD_FILE: &str = "record.json";

/// Content metadata persisted alongside an artifact descriptor.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArtifactContentMetadata {
    pub byte_len: u64,
    pub sha256: String,
}

/// Full filesystem artifact record.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FsArtifactRecord {
    pub descriptor: ArtifactDescriptor,
    pub content: ArtifactContentMetadata,
}

/// Verification issues for a filesystem artifact.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ArtifactVerificationIssue {
    MissingRecord,
    MissingDescriptor,
    MissingContent,
    DescriptorMismatch,
    ContentSizeMismatch { expected: u64, actual: u64 },
    ContentHashMismatch { expected: String, actual: String },
}

/// Verification report for a filesystem artifact.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArtifactVerificationReport {
    pub artifact_id: ArtifactId,
    pub verified: bool,
    pub content: Option<ArtifactContentMetadata>,
    pub issues: Vec<ArtifactVerificationIssue>,
}

/// Filesystem-backed artifact store.
#[derive(Debug, Clone)]
pub struct FsArtifactStore {
    root: PathBuf,
}

impl FsArtifactStore {
    pub fn new(root: impl AsRef<Path>) -> StorageResult<Self> {
        let root = root.as_ref().to_path_buf();
        create_dir_all(&root)?;
        Ok(Self { root })
    }

    pub fn for_storage(storage: &AgentOsStorage) -> StorageResult<Self> {
        Self::new(storage.path_for("artifacts"))
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn put_with_content(
        &self,
        descriptor: ArtifactDescriptor,
        content: impl AsRef<[u8]>,
    ) -> StorageResult<FsArtifactRecord> {
        let artifact_dir = self.artifact_dir(&descriptor.id)?;
        fs::create_dir(&artifact_dir).map_err(|source| {
            if source.kind() == std::io::ErrorKind::AlreadyExists {
                StorageError::ArtifactAlreadyExists {
                    artifact_id: descriptor.id.0.clone(),
                }
            } else {
                StorageError::Io {
                    path: artifact_dir.clone(),
                    source,
                }
            }
        })?;

        let content = content.as_ref();
        let metadata = ArtifactContentMetadata {
            byte_len: content.len() as u64,
            sha256: sha256_hex(content),
        };
        let record = FsArtifactRecord {
            descriptor: descriptor.clone(),
            content: metadata,
        };

        write_json(&artifact_dir.join(DESCRIPTOR_FILE), &descriptor)?;
        write_bytes(&artifact_dir.join(CONTENT_FILE), content)?;
        write_json(&artifact_dir.join(RECORD_FILE), &record)?;

        Ok(record)
    }

    pub fn get_record(&self, artifact_id: &ArtifactId) -> StorageResult<Option<FsArtifactRecord>> {
        let artifact_dir = self.artifact_dir(artifact_id)?;
        let record_path = artifact_dir.join(RECORD_FILE);
        if !record_path.exists() {
            return Ok(None);
        }
        Ok(Some(read_json(&record_path)?))
    }

    pub fn read_content(&self, artifact_id: &ArtifactId) -> StorageResult<Option<Vec<u8>>> {
        let artifact_dir = self.artifact_dir(artifact_id)?;
        let content_path = artifact_dir.join(CONTENT_FILE);
        if !content_path.exists() {
            return Ok(None);
        }
        fs::read(&content_path)
            .map(Some)
            .map_err(|source| StorageError::Io {
                path: content_path,
                source,
            })
    }

    pub fn verify(&self, artifact_id: &ArtifactId) -> StorageResult<ArtifactVerificationReport> {
        let artifact_dir = self.artifact_dir(artifact_id)?;
        let record_path = artifact_dir.join(RECORD_FILE);
        let descriptor_path = artifact_dir.join(DESCRIPTOR_FILE);
        let content_path = artifact_dir.join(CONTENT_FILE);
        let mut issues = Vec::new();
        let mut content_metadata = None;

        let record = if record_path.exists() {
            Some(read_json::<FsArtifactRecord>(&record_path)?)
        } else {
            issues.push(ArtifactVerificationIssue::MissingRecord);
            None
        };

        if !descriptor_path.exists() {
            issues.push(ArtifactVerificationIssue::MissingDescriptor);
        } else if let Some(record) = &record {
            let descriptor: ArtifactDescriptor = read_json(&descriptor_path)?;
            if descriptor != record.descriptor {
                issues.push(ArtifactVerificationIssue::DescriptorMismatch);
            }
        }

        if !content_path.exists() {
            issues.push(ArtifactVerificationIssue::MissingContent);
        } else if let Some(record) = &record {
            let content = fs::read(&content_path).map_err(|source| StorageError::Io {
                path: content_path.clone(),
                source,
            })?;
            let actual = ArtifactContentMetadata {
                byte_len: content.len() as u64,
                sha256: sha256_hex(&content),
            };
            if actual.byte_len != record.content.byte_len {
                issues.push(ArtifactVerificationIssue::ContentSizeMismatch {
                    expected: record.content.byte_len,
                    actual: actual.byte_len,
                });
            }
            if actual.sha256 != record.content.sha256 {
                issues.push(ArtifactVerificationIssue::ContentHashMismatch {
                    expected: record.content.sha256.clone(),
                    actual: actual.sha256.clone(),
                });
            }
            content_metadata = Some(actual);
        }

        Ok(ArtifactVerificationReport {
            artifact_id: artifact_id.clone(),
            verified: issues.is_empty(),
            content: content_metadata,
            issues,
        })
    }

    pub fn verify_all(&self) -> StorageResult<Vec<ArtifactVerificationReport>> {
        let mut reports = Vec::new();
        for artifact_id in self.list_artifact_ids()? {
            reports.push(self.verify(&artifact_id)?);
        }
        reports.sort_by(|a, b| a.artifact_id.0.cmp(&b.artifact_id.0));
        Ok(reports)
    }

    fn list_descriptors(&self) -> StorageResult<Vec<ArtifactDescriptor>> {
        let mut descriptors: Vec<ArtifactDescriptor> = Vec::new();
        for artifact_id in self.list_artifact_ids()? {
            let descriptor_path = self.artifact_dir(&artifact_id)?.join(DESCRIPTOR_FILE);
            if descriptor_path.exists() {
                descriptors.push(read_json(&descriptor_path)?);
            }
        }
        descriptors.sort_by(|a, b| a.id.0.cmp(&b.id.0));
        Ok(descriptors)
    }

    pub fn list_artifact_ids(&self) -> StorageResult<Vec<ArtifactId>> {
        let mut ids = Vec::new();
        if !self.root.exists() {
            return Ok(ids);
        }
        let entries = fs::read_dir(&self.root).map_err(|source| StorageError::Io {
            path: self.root.clone(),
            source,
        })?;
        for entry in entries {
            let entry = entry.map_err(|source| StorageError::Io {
                path: self.root.clone(),
                source,
            })?;
            let path = entry.path();
            if path.is_dir() {
                if let Some(name) = entry.file_name().to_str() {
                    ids.push(ArtifactId::from(name));
                }
            }
        }
        ids.sort_by(|a, b| a.0.cmp(&b.0));
        Ok(ids)
    }

    fn artifact_dir(&self, artifact_id: &ArtifactId) -> StorageResult<PathBuf> {
        validate_artifact_id_path(artifact_id)?;
        Ok(self.root.join(&artifact_id.0))
    }
}

#[async_trait]
impl ArtifactStore for FsArtifactStore {
    async fn put(&self, descriptor: ArtifactDescriptor) -> Result<(), ArtifactStoreError> {
        self.put_with_content(descriptor, [])
            .map(|_| ())
            .map_err(storage_to_artifact_error)
    }

    async fn get(
        &self,
        artifact_id: &ArtifactId,
    ) -> Result<Option<ArtifactDescriptor>, ArtifactStoreError> {
        self.get_record(artifact_id)
            .map(|record| record.map(|record| record.descriptor))
            .map_err(storage_to_artifact_error)
    }

    async fn list(&self) -> Result<Vec<ArtifactDescriptor>, ArtifactStoreError> {
        self.list_descriptors().map_err(storage_to_artifact_error)
    }
}

fn validate_artifact_id_path(artifact_id: &ArtifactId) -> StorageResult<()> {
    let id = artifact_id.0.as_str();
    if id.is_empty()
        || id == "."
        || id == ".."
        || id.contains("..")
        || !id
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' || ch == '.')
    {
        return Err(StorageError::InvalidArtifactIdPath {
            artifact_id: artifact_id.0.clone(),
        });
    }
    Ok(())
}

fn read_json<T>(path: &Path) -> StorageResult<T>
where
    T: for<'de> Deserialize<'de>,
{
    let bytes = fs::read(path).map_err(|source| StorageError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    serde_json::from_slice(&bytes).map_err(|source| StorageError::ArtifactSerde {
        path: path.to_path_buf(),
        source,
    })
}

fn write_json<T>(path: &Path, value: &T) -> StorageResult<()>
where
    T: Serialize,
{
    let bytes = serde_json::to_vec_pretty(value).map_err(|source| StorageError::ArtifactSerde {
        path: path.to_path_buf(),
        source,
    })?;
    write_bytes(path, &bytes)
}

fn write_bytes(path: &Path, bytes: &[u8]) -> StorageResult<()> {
    fs::write(path, bytes).map_err(|source| StorageError::Io {
        path: path.to_path_buf(),
        source,
    })
}

fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    format!("{digest:x}")
}

fn storage_to_artifact_error(error: StorageError) -> ArtifactStoreError {
    match error {
        StorageError::ArtifactAlreadyExists { artifact_id } => {
            ArtifactStoreError::DuplicateArtifactId(ArtifactId::from(artifact_id))
        }
        other => ArtifactStoreError::Storage(other.to_string()),
    }
}
