use std::path::Path;

use agentos_storage::{
    AgentOsStorage, MigrationMode, MigrationStatus, StorageError, StorageManifest,
    StorageMigration, StorageMigrationRegistry, StorageResult,
};

struct FakeV1ToV2Migration;

impl StorageMigration for FakeV1ToV2Migration {
    fn name(&self) -> &str {
        "fake-v1-to-v2"
    }

    fn from_version(&self) -> u32 {
        1
    }

    fn to_version(&self) -> u32 {
        2
    }

    fn migrate(&self, _root: &Path, manifest: &mut StorageManifest) -> StorageResult<()> {
        manifest.storage_version = 2;
        Ok(())
    }
}

struct FailingV1ToV2Migration;

impl StorageMigration for FailingV1ToV2Migration {
    fn name(&self) -> &str {
        "failing-v1-to-v2"
    }

    fn from_version(&self) -> u32 {
        1
    }

    fn to_version(&self) -> u32 {
        2
    }

    fn migrate(&self, _root: &Path, _manifest: &mut StorageManifest) -> StorageResult<()> {
        Err(StorageError::UnsupportedVersion {
            found: 1,
            expected: 2,
        })
    }
}

fn read_manifest(path: &Path) -> StorageManifest {
    let bytes = std::fs::read(path.join("manifest.json")).unwrap();
    serde_json::from_slice(&bytes).unwrap()
}

#[test]
fn registry_plans_v1_to_v2_migration() {
    let mut registry = StorageMigrationRegistry::new();
    registry.register(FakeV1ToV2Migration).unwrap();

    let plan = registry.plan(1, 2).unwrap();

    assert_eq!(plan.current_version, 1);
    assert_eq!(plan.target_version, 2);
    assert_eq!(plan.steps.len(), 1);
    assert_eq!(plan.steps[0].from_version, 1);
    assert_eq!(plan.steps[0].to_version, 2);
    assert_eq!(plan.steps[0].name, "fake-v1-to-v2");
}

#[test]
fn dry_run_does_not_modify_manifest_or_write_backup() {
    let dir = tempfile::tempdir().unwrap();
    AgentOsStorage::init(dir.path()).unwrap();
    let original_manifest = read_manifest(dir.path());
    let mut registry = StorageMigrationRegistry::new();
    registry.register(FakeV1ToV2Migration).unwrap();

    let report = registry
        .migrate(dir.path(), 2, MigrationMode::DryRun)
        .unwrap();

    let manifest = read_manifest(dir.path());
    assert_eq!(report.status, MigrationStatus::DryRun);
    assert_eq!(report.from_version, 1);
    assert_eq!(report.to_version, 2);
    assert_eq!(report.applied_steps.len(), 1);
    assert_eq!(report.backup_manifest_path, None);
    assert_eq!(manifest, original_manifest);
    assert!(!dir.path().join("manifest.v1.bak.json").exists());
}

#[test]
fn apply_fake_v1_to_v2_updates_manifest_and_writes_backup() {
    let dir = tempfile::tempdir().unwrap();
    AgentOsStorage::init(dir.path()).unwrap();
    let mut registry = StorageMigrationRegistry::new();
    registry.register(FakeV1ToV2Migration).unwrap();

    let report = registry
        .migrate(dir.path(), 2, MigrationMode::Apply)
        .unwrap();

    let manifest = read_manifest(dir.path());
    let backup_path = report.backup_manifest_path.as_ref().unwrap();
    let backup_manifest: StorageManifest =
        serde_json::from_slice(&std::fs::read(backup_path).unwrap()).unwrap();

    assert_eq!(report.status, MigrationStatus::Applied);
    assert_eq!(report.from_version, 1);
    assert_eq!(report.to_version, 2);
    assert_eq!(report.applied_steps.len(), 1);
    assert!(backup_path.is_file());
    assert_eq!(backup_manifest.storage_version, 1);
    assert_eq!(manifest.storage_version, 2);
}

#[test]
fn failed_migration_does_not_modify_original_manifest() {
    let dir = tempfile::tempdir().unwrap();
    AgentOsStorage::init(dir.path()).unwrap();
    let manifest_path = dir.path().join("manifest.json");
    let original_bytes = std::fs::read(&manifest_path).unwrap();
    let mut registry = StorageMigrationRegistry::new();
    registry.register(FailingV1ToV2Migration).unwrap();

    let result = registry.migrate(dir.path(), 2, MigrationMode::Apply);

    assert!(result.is_err());
    assert!(matches!(
        result.unwrap_err(),
        StorageError::MigrationFailed { .. }
    ));
    let current_bytes = std::fs::read(&manifest_path).unwrap();
    assert_eq!(current_bytes, original_bytes);
    assert!(dir.path().join("manifest.v1.bak.json").is_file());
}

#[test]
fn duplicate_migration_is_rejected() {
    let mut registry = StorageMigrationRegistry::new();
    registry.register(FakeV1ToV2Migration).unwrap();

    let result = registry.register(FakeV1ToV2Migration);

    assert!(matches!(
        result.unwrap_err(),
        StorageError::DuplicateMigration {
            from_version: 1,
            to_version: 2
        }
    ));
}

#[test]
fn missing_migration_path_returns_error() {
    let registry = StorageMigrationRegistry::new();

    let result = registry.plan(1, 2);

    assert!(matches!(
        result.unwrap_err(),
        StorageError::MigrationPathNotFound {
            from_version: 1,
            target_version: 2
        }
    ));
}

#[test]
fn migrating_to_same_version_is_noop() {
    let dir = tempfile::tempdir().unwrap();
    AgentOsStorage::init(dir.path()).unwrap();
    let registry = StorageMigrationRegistry::new();

    let report = registry
        .migrate(dir.path(), 1, MigrationMode::Apply)
        .unwrap();

    assert_eq!(report.status, MigrationStatus::Noop);
    assert_eq!(report.from_version, 1);
    assert_eq!(report.to_version, 1);
    assert!(report.applied_steps.is_empty());
    assert_eq!(report.backup_manifest_path, None);
    assert!(!dir.path().join("manifest.v1.bak.json").exists());
}
