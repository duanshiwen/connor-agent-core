//! Storage backup and restore primitives.
//!
//! Backup v1 uses a directory layout instead of a packed archive so integrity
//! metadata is transparent and easy to test.

use std::fs;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::{
    AgentOsStorage, ArtifactVerificationReport, FsArtifactStore, STORAGE_LAYOUT_DIRECTORIES,
    StorageError, StorageManifest, StorageResult, create_dir_all, read_manifest,
};

pub const STORAGE_BACKUP_VERSION: u32 = 1;
pub const BACKUP_MANIFEST_FILE: &str = "backup-manifest.json";
pub const BACKUP_DATA_DIR: &str = "data";

/// One file covered by a backup manifest.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BackupFileEntry {
    pub path: String,
    pub byte_len: u64,
    pub sha256: String,
}

/// Manifest describing an exported storage backup.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BackupManifest {
    pub backup_version: u32,
    pub storage_version: u32,
    pub created_at: DateTime<Utc>,
    pub source_manifest: StorageManifest,
    pub files: Vec<BackupFileEntry>,
}

/// Report returned after exporting a storage backup.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BackupReport {
    pub backup_dir: PathBuf,
    pub manifest_path: PathBuf,
    pub file_count: usize,
    pub total_bytes: u64,
}

/// Report returned after restoring a storage backup.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RestoreReport {
    pub target_root: PathBuf,
    pub file_count: usize,
    pub total_bytes: u64,
    pub artifact_reports: Vec<ArtifactVerificationReport>,
}

/// Storage backup/restore operations.
pub struct StorageBackup;

impl StorageBackup {
    pub fn export(
        storage: &AgentOsStorage,
        backup_dir: impl AsRef<Path>,
    ) -> StorageResult<BackupReport> {
        let backup_dir = backup_dir.as_ref().to_path_buf();
        ensure_empty_or_create(&backup_dir)?;
        let data_dir = backup_dir.join(BACKUP_DATA_DIR);
        create_dir_all(&data_dir)?;

        copy_dir_contents(storage.root(), &data_dir, &|path| should_export_path(path))?;

        let mut files = collect_file_entries(&data_dir)?;
        files.sort_by(|a, b| a.path.cmp(&b.path));
        let total_bytes = files.iter().map(|entry| entry.byte_len).sum();
        let source_manifest = read_manifest(&storage.manifest_path())?;
        let manifest = BackupManifest {
            backup_version: STORAGE_BACKUP_VERSION,
            storage_version: source_manifest.storage_version,
            created_at: Utc::now(),
            source_manifest,
            files,
        };
        let manifest_path = backup_dir.join(BACKUP_MANIFEST_FILE);
        write_backup_manifest(&manifest_path, &manifest)?;

        Ok(BackupReport {
            backup_dir,
            manifest_path,
            file_count: manifest.files.len(),
            total_bytes,
        })
    }

    pub fn verify_backup(backup_dir: impl AsRef<Path>) -> StorageResult<BackupManifest> {
        let backup_dir = backup_dir.as_ref();
        let manifest_path = backup_dir.join(BACKUP_MANIFEST_FILE);
        let manifest = read_backup_manifest(&manifest_path)?;
        let data_dir = backup_dir.join(BACKUP_DATA_DIR);

        for entry in &manifest.files {
            let path = data_dir.join(path_from_slash(&entry.path));
            if !path.is_file() {
                return Err(StorageError::BackupFileMissing {
                    path: entry.path.clone(),
                });
            }
            let metadata = fs::metadata(&path).map_err(|source| StorageError::Io {
                path: path.clone(),
                source,
            })?;
            if metadata.len() != entry.byte_len {
                return Err(StorageError::BackupIntegrityMismatch {
                    path: entry.path.clone(),
                    expected: format!("len:{} sha256:{}", entry.byte_len, entry.sha256),
                    actual: format!("len:{} sha256:{}", metadata.len(), sha256_file(&path)?),
                });
            }
            let actual_hash = sha256_file(&path)?;
            if actual_hash != entry.sha256 {
                return Err(StorageError::BackupIntegrityMismatch {
                    path: entry.path.clone(),
                    expected: entry.sha256.clone(),
                    actual: actual_hash,
                });
            }
        }

        Ok(manifest)
    }

    pub fn restore(
        backup_dir: impl AsRef<Path>,
        target_root: impl AsRef<Path>,
    ) -> StorageResult<RestoreReport> {
        let backup_dir = backup_dir.as_ref();
        let target_root = target_root.as_ref().to_path_buf();
        let manifest = Self::verify_backup(backup_dir)?;
        ensure_empty_or_create(&target_root)?;

        let data_dir = backup_dir.join(BACKUP_DATA_DIR);
        copy_dir_contents(&data_dir, &target_root, &|_| true)?;

        verify_restored_layout(&target_root)?;
        let artifact_store = FsArtifactStore::new(target_root.join("artifacts"))?;
        let artifact_reports = artifact_store.verify_all()?;
        if let Some(failed) = artifact_reports.iter().find(|report| !report.verified) {
            return Err(StorageError::RestoreIntegrityFailed {
                message: format!(
                    "artifact {} failed verification with {} issue(s)",
                    failed.artifact_id.0,
                    failed.issues.len()
                ),
            });
        }

        let total_bytes = manifest.files.iter().map(|entry| entry.byte_len).sum();
        Ok(RestoreReport {
            target_root,
            file_count: manifest.files.len(),
            total_bytes,
            artifact_reports,
        })
    }
}

fn ensure_empty_or_create(path: &Path) -> StorageResult<()> {
    if path.exists() {
        let mut entries = fs::read_dir(path).map_err(|source| StorageError::Io {
            path: path.to_path_buf(),
            source,
        })?;
        if entries.next().is_some() {
            return Err(StorageError::BackupTargetNotEmpty {
                path: path.to_path_buf(),
            });
        }
    } else {
        create_dir_all(path)?;
    }
    Ok(())
}

fn should_export_path(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .map(|name| name != ".agentos-storage.lock")
        .unwrap_or(true)
}

fn copy_dir_contents(
    source: &Path,
    target: &Path,
    should_copy: &dyn Fn(&Path) -> bool,
) -> StorageResult<()> {
    create_dir_all(target)?;
    for entry in fs::read_dir(source).map_err(|source_err| StorageError::Io {
        path: source.to_path_buf(),
        source: source_err,
    })? {
        let entry = entry.map_err(|source_err| StorageError::Io {
            path: source.to_path_buf(),
            source: source_err,
        })?;
        let source_path = entry.path();
        if !should_copy(&source_path) {
            continue;
        }
        let target_path = target.join(entry.file_name());
        let file_type = entry.file_type().map_err(|source_err| StorageError::Io {
            path: source_path.clone(),
            source: source_err,
        })?;
        if file_type.is_dir() {
            copy_dir_contents(&source_path, &target_path, should_copy)?;
        } else if file_type.is_file() {
            fs::copy(&source_path, &target_path).map_err(|source_err| StorageError::Io {
                path: target_path,
                source: source_err,
            })?;
        }
    }
    Ok(())
}

fn collect_file_entries(data_dir: &Path) -> StorageResult<Vec<BackupFileEntry>> {
    let mut entries = Vec::new();
    collect_file_entries_inner(data_dir, data_dir, &mut entries)?;
    Ok(entries)
}

fn collect_file_entries_inner(
    base: &Path,
    current: &Path,
    entries: &mut Vec<BackupFileEntry>,
) -> StorageResult<()> {
    for entry in fs::read_dir(current).map_err(|source| StorageError::Io {
        path: current.to_path_buf(),
        source,
    })? {
        let entry = entry.map_err(|source| StorageError::Io {
            path: current.to_path_buf(),
            source,
        })?;
        let path = entry.path();
        let file_type = entry.file_type().map_err(|source| StorageError::Io {
            path: path.clone(),
            source,
        })?;
        if file_type.is_dir() {
            collect_file_entries_inner(base, &path, entries)?;
        } else if file_type.is_file() {
            let relative = path.strip_prefix(base).expect("path is under base");
            let relative = slash_path(relative);
            let bytes = fs::read(&path).map_err(|source| StorageError::Io {
                path: path.clone(),
                source,
            })?;
            entries.push(BackupFileEntry {
                path: relative,
                byte_len: bytes.len() as u64,
                sha256: sha256_bytes(&bytes),
            });
        }
    }
    Ok(())
}

fn verify_restored_layout(root: &Path) -> StorageResult<()> {
    let manifest_path = root.join("manifest.json");
    let _manifest = read_manifest(&manifest_path)?;
    for dir in STORAGE_LAYOUT_DIRECTORIES {
        let dir_path = root.join(dir);
        if !dir_path.is_dir() {
            return Err(StorageError::RestoreIntegrityFailed {
                message: format!("missing layout directory: {dir}"),
            });
        }
    }
    Ok(())
}

fn read_backup_manifest(path: &Path) -> StorageResult<BackupManifest> {
    let bytes = fs::read(path).map_err(|source| StorageError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    serde_json::from_slice(&bytes).map_err(|source| StorageError::BackupManifestSerde {
        path: path.to_path_buf(),
        source,
    })
}

fn write_backup_manifest(path: &Path, manifest: &BackupManifest) -> StorageResult<()> {
    let bytes = serde_json::to_vec_pretty(manifest).map_err(|source| {
        StorageError::BackupManifestSerde {
            path: path.to_path_buf(),
            source,
        }
    })?;
    fs::write(path, bytes).map_err(|source| StorageError::Io {
        path: path.to_path_buf(),
        source,
    })
}

fn sha256_file(path: &Path) -> StorageResult<String> {
    let bytes = fs::read(path).map_err(|source| StorageError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    Ok(sha256_bytes(&bytes))
}

fn sha256_bytes(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    format!("{digest:x}")
}

fn slash_path(path: &Path) -> String {
    path.components()
        .map(|component| component.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
}

fn path_from_slash(path: &str) -> PathBuf {
    path.split('/').collect()
}
