use agentos_storage::{AgentOsStorage, STORAGE_LAYOUT_VERSION};

#[test]
fn storage_init_creates_complete_layout() {
    let dir = tempfile::tempdir().unwrap();
    let storage = AgentOsStorage::init(dir.path()).unwrap();

    assert_eq!(storage.root(), dir.path());
    assert!(dir.path().join("manifest.json").is_file());
    for name in [
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
    ] {
        assert!(dir.path().join(name).is_dir(), "missing {name}");
    }
}

#[test]
fn manifest_contains_storage_version() {
    let dir = tempfile::tempdir().unwrap();
    let storage = AgentOsStorage::init(dir.path()).unwrap();
    let manifest = storage.manifest();

    assert_eq!(manifest.storage_version, STORAGE_LAYOUT_VERSION);
    assert_eq!(manifest.layout_directories.len(), 11);
    assert!(manifest.created_at <= manifest.updated_at);
}

#[test]
fn storage_init_is_idempotent_and_preserves_created_at() {
    let dir = tempfile::tempdir().unwrap();
    let first = AgentOsStorage::init(dir.path()).unwrap();
    let created_at = first.manifest().created_at;

    let second = AgentOsStorage::init(dir.path()).unwrap();

    assert_eq!(second.manifest().created_at, created_at);
    assert!(second.manifest().updated_at >= created_at);
    assert_eq!(second.manifest().storage_version, STORAGE_LAYOUT_VERSION);
}
