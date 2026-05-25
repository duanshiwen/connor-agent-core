//! Filesystem lock primitives for AgentOS storage roots.
//!
//! The lock uses an atomically-created metadata file as a local single-writer
//! guard. It is intentionally scoped to local filesystems; distributed locking
//! and heartbeat renewal can be layered on later.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};

use crate::{StorageError, StorageResult};

pub const STORAGE_LOCK_VERSION: u32 = 1;
pub const STORAGE_LOCK_FILE_NAME: &str = ".agentos-storage.lock";

/// Metadata written to the storage lock file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StorageLockInfo {
    pub lock_version: u32,
    pub owner_id: String,
    pub process_id: u32,
    pub acquired_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
}

/// Options used when acquiring a storage lock.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StorageLockOptions {
    pub owner_id: String,
    pub ttl: Duration,
}

impl StorageLockOptions {
    pub fn new(owner_id: impl Into<String>, ttl: Duration) -> Self {
        Self {
            owner_id: owner_id.into(),
            ttl,
        }
    }
}

/// RAII guard for an acquired storage lock.
#[derive(Debug)]
pub struct StorageLockGuard {
    lock_path: PathBuf,
    info: StorageLockInfo,
    released: bool,
}

impl StorageLockGuard {
    /// Acquire a single-writer lock for a storage root.
    pub fn acquire(root: impl AsRef<Path>, options: StorageLockOptions) -> StorageResult<Self> {
        let root = root.as_ref();
        let lock_path = root.join(STORAGE_LOCK_FILE_NAME);
        let now = Utc::now();

        if lock_path.exists() {
            match read_lock_info(&lock_path) {
                Ok(existing) if existing.expires_at > now => {
                    return Err(StorageError::LockAlreadyHeld {
                        path: lock_path,
                        owner_id: existing.owner_id,
                        expires_at: existing.expires_at,
                    });
                }
                Ok(_) => remove_lock_file(&lock_path)?,
                Err(err) => return Err(err),
            }
        }

        let info = StorageLockInfo {
            lock_version: STORAGE_LOCK_VERSION,
            owner_id: options.owner_id,
            process_id: std::process::id(),
            acquired_at: now,
            expires_at: now + options.ttl,
        };
        let bytes = serde_json::to_vec_pretty(&info).map_err(|source| StorageError::LockSerde {
            path: lock_path.clone(),
            source,
        })?;

        let mut file = match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&lock_path)
        {
            Ok(file) => file,
            Err(source) if source.kind() == std::io::ErrorKind::AlreadyExists => {
                let existing = read_lock_info(&lock_path)?;
                return Err(StorageError::LockAlreadyHeld {
                    path: lock_path,
                    owner_id: existing.owner_id,
                    expires_at: existing.expires_at,
                });
            }
            Err(source) => {
                return Err(StorageError::Io {
                    path: lock_path,
                    source,
                });
            }
        };
        file.write_all(&bytes).map_err(|source| StorageError::Io {
            path: lock_path.clone(),
            source,
        })?;
        file.sync_all().map_err(|source| StorageError::Io {
            path: lock_path.clone(),
            source,
        })?;

        Ok(Self {
            lock_path,
            info,
            released: false,
        })
    }

    pub fn info(&self) -> &StorageLockInfo {
        &self.info
    }

    pub fn lock_path(&self) -> &Path {
        &self.lock_path
    }

    /// Explicitly release the storage lock.
    pub fn release(mut self) -> StorageResult<()> {
        self.release_inner()?;
        self.released = true;
        Ok(())
    }

    fn release_inner(&mut self) -> StorageResult<()> {
        if self.released || !self.lock_path.exists() {
            return Ok(());
        }

        let current = match read_lock_info(&self.lock_path) {
            Ok(current) => current,
            Err(_) => return Ok(()),
        };

        if current.owner_id == self.info.owner_id
            && current.process_id == self.info.process_id
            && current.acquired_at == self.info.acquired_at
        {
            remove_lock_file(&self.lock_path)?;
        }

        Ok(())
    }
}

impl Drop for StorageLockGuard {
    fn drop(&mut self) {
        let _ = self.release_inner();
        self.released = true;
    }
}

pub(crate) fn read_lock_info(path: &Path) -> StorageResult<StorageLockInfo> {
    let bytes = fs::read(path).map_err(|source| StorageError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    serde_json::from_slice(&bytes).map_err(|source| StorageError::LockSerde {
        path: path.to_path_buf(),
        source,
    })
}

fn remove_lock_file(path: &Path) -> StorageResult<()> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(source) if source.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(source) => Err(StorageError::Io {
            path: path.to_path_buf(),
            source,
        }),
    }
}
