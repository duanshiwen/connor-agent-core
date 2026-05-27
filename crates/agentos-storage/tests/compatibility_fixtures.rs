use std::path::Path;

use agentos_storage::{
    AgentOsStorage, MigrationMode, MigrationStatus, StorageManifest, StorageMigration,
    StorageMigrationRegistry, StorageResult,
};
use chrono::{TimeZone, Utc};

struct FixtureV0ToV1Migration;

impl StorageMigration for FixtureV0ToV1Migration {
    fn name(&self) -> &str {
        "fixture-v0-to-v1"
    }

    fn from_version(&self) -> u32 {
        0
    }

    fn to_version(&self) -> u32 {
        1
    }

    fn migrate(&self, _root: &Path, manifest: &mut StorageManifest) -> StorageResult<()> {
        manifest.storage_version = 1;
        manifest.layout_directories = agentos_storage::STORAGE_LAYOUT_DIRECTORIES
            .iter()
            .map(|name| (*name).to_string())
            .collect();
        Ok(())
    }
}

fn write_legacy_v0_storage_fixture(root: &Path) {
    std::fs::create_dir_all(root).unwrap();
    std::fs::create_dir_all(root.join("conversations")).unwrap();
    let legacy_manifest = serde_json::json!({
        "storage_version": 0,
        "created_at": "2026-01-01T00:00:00Z",
        "updated_at": "2026-01-01T00:00:00Z",
        "layout_directories": ["conversations"]
    });
    std::fs::write(
        root.join("manifest.json"),
        serde_json::to_vec_pretty(&legacy_manifest).unwrap(),
    )
    .unwrap();
}

fn read_manifest(root: &Path) -> StorageManifest {
    serde_json::from_slice(&std::fs::read(root.join("manifest.json")).unwrap()).unwrap()
}

#[test]
fn old_storage_fixture_can_migrate_to_current_layout_and_reopen() {
    let dir = tempfile::tempdir().unwrap();
    write_legacy_v0_storage_fixture(dir.path());

    let mut registry = StorageMigrationRegistry::new();
    registry.register(FixtureV0ToV1Migration).unwrap();

    let report = registry
        .migrate(
            dir.path(),
            agentos_storage::STORAGE_LAYOUT_VERSION,
            MigrationMode::Apply,
        )
        .unwrap();

    assert_eq!(report.status, MigrationStatus::Applied);
    assert_eq!(report.from_version, 0);
    assert_eq!(report.to_version, agentos_storage::STORAGE_LAYOUT_VERSION);
    assert!(report.backup_manifest_path.unwrap().is_file());

    let manifest = read_manifest(dir.path());
    assert_eq!(
        manifest.storage_version,
        agentos_storage::STORAGE_LAYOUT_VERSION
    );
    for layout_dir in agentos_storage::STORAGE_LAYOUT_DIRECTORIES {
        assert!(
            manifest
                .layout_directories
                .contains(&layout_dir.to_string()),
            "missing layout dir in migrated manifest: {layout_dir}"
        );
    }

    let storage = AgentOsStorage::init(dir.path()).unwrap();
    assert_eq!(
        storage.manifest().storage_version,
        agentos_storage::STORAGE_LAYOUT_VERSION
    );
    assert_eq!(
        storage.manifest().layout_directories.len(),
        agentos_storage::STORAGE_LAYOUT_DIRECTORIES.len()
    );

    // Keep chrono linked in this fixture test so older timestamp shapes remain explicit.
    assert_eq!(
        Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0)
            .unwrap()
            .timestamp(),
        1_767_225_600
    );
}
