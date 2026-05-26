//! Storage schema/layout migration primitives.
//!
//! This module intentionally keeps the first migration framework small: it can
//! plan linear version upgrades, dry-run them, and apply them with a manifest
//! backup boundary. Broader backup/restore and repair workflows are handled by
//! later storage PRs.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use crate::{StorageError, StorageManifest, StorageResult, read_manifest, write_manifest};

/// Controls whether a migration is planned only or applied to disk.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MigrationMode {
    DryRun,
    Apply,
}

/// One step in a storage migration plan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MigrationPlanStep {
    pub from_version: u32,
    pub to_version: u32,
    pub name: String,
}

/// A linear migration plan from the current version to a target version.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MigrationPlan {
    pub current_version: u32,
    pub target_version: u32,
    pub steps: Vec<MigrationPlanStep>,
}

/// Result status for a migration run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MigrationStatus {
    DryRun,
    Applied,
    Noop,
}

/// Report returned by dry-run or applied migrations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MigrationReport {
    pub status: MigrationStatus,
    pub from_version: u32,
    pub to_version: u32,
    pub applied_steps: Vec<MigrationPlanStep>,
    pub backup_manifest_path: Option<PathBuf>,
}

/// A single storage migration between two adjacent storage versions.
#[allow(clippy::wrong_self_convention)]
pub trait StorageMigration: Send + Sync {
    fn name(&self) -> &str;
    fn from_version(&self) -> u32;
    fn to_version(&self) -> u32;

    fn migrate(&self, root: &Path, manifest: &mut StorageManifest) -> StorageResult<()>;
}

/// Registry of known storage migrations.
#[derive(Default)]
pub struct StorageMigrationRegistry {
    migrations: BTreeMap<(u32, u32), Box<dyn StorageMigration>>,
}

impl StorageMigrationRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register<M>(&mut self, migration: M) -> StorageResult<()>
    where
        M: StorageMigration + 'static,
    {
        let name = migration.name().to_string();
        let from_version = migration.from_version();
        let to_version = migration.to_version();

        if to_version <= from_version {
            return Err(StorageError::InvalidMigration {
                name,
                from_version,
                to_version,
            });
        }

        let key = (from_version, to_version);
        if self.migrations.contains_key(&key) {
            return Err(StorageError::DuplicateMigration {
                from_version,
                to_version,
            });
        }

        self.migrations.insert(key, Box::new(migration));
        Ok(())
    }

    pub fn plan(&self, current_version: u32, target_version: u32) -> StorageResult<MigrationPlan> {
        if current_version == target_version {
            return Ok(MigrationPlan {
                current_version,
                target_version,
                steps: vec![],
            });
        }

        if current_version > target_version {
            return Err(StorageError::MigrationPathNotFound {
                from_version: current_version,
                target_version,
            });
        }

        let mut version = current_version;
        let mut steps = Vec::new();
        while version < target_version {
            let next_version = version + 1;
            let migration = self.migrations.get(&(version, next_version)).ok_or(
                StorageError::MigrationPathNotFound {
                    from_version: version,
                    target_version,
                },
            )?;
            steps.push(MigrationPlanStep {
                from_version: version,
                to_version: next_version,
                name: migration.name().to_string(),
            });
            version = next_version;
        }

        Ok(MigrationPlan {
            current_version,
            target_version,
            steps,
        })
    }

    pub fn migrate(
        &self,
        root: impl AsRef<Path>,
        target_version: u32,
        mode: MigrationMode,
    ) -> StorageResult<MigrationReport> {
        let root = root.as_ref();
        let manifest_path = root.join("manifest.json");
        let original_manifest = read_manifest(&manifest_path)?;
        let from_version = original_manifest.storage_version;
        let plan = self.plan(from_version, target_version)?;

        if plan.steps.is_empty() {
            return Ok(MigrationReport {
                status: MigrationStatus::Noop,
                from_version,
                to_version: target_version,
                applied_steps: vec![],
                backup_manifest_path: None,
            });
        }

        if mode == MigrationMode::DryRun {
            return Ok(MigrationReport {
                status: MigrationStatus::DryRun,
                from_version,
                to_version: target_version,
                applied_steps: plan.steps,
                backup_manifest_path: None,
            });
        }

        let backup_manifest_path = backup_manifest(root, from_version)?;
        let mut manifest = original_manifest;

        for step in &plan.steps {
            let migration = self
                .migrations
                .get(&(step.from_version, step.to_version))
                .expect("migration plan referenced a registered migration");
            migration.migrate(root, &mut manifest).map_err(|source| {
                StorageError::MigrationFailed {
                    name: step.name.clone(),
                    message: source.to_string(),
                }
            })?;
            manifest.storage_version = step.to_version;
        }

        write_manifest(&manifest_path, &manifest)?;

        Ok(MigrationReport {
            status: MigrationStatus::Applied,
            from_version,
            to_version: target_version,
            applied_steps: plan.steps,
            backup_manifest_path: Some(backup_manifest_path),
        })
    }
}

fn backup_manifest(root: &Path, from_version: u32) -> StorageResult<PathBuf> {
    let manifest_path = root.join("manifest.json");
    let backup_path = root.join(format!("manifest.v{from_version}.bak.json"));
    fs::copy(&manifest_path, &backup_path).map_err(|source| StorageError::Io {
        path: backup_path.clone(),
        source,
    })?;
    Ok(backup_path)
}
