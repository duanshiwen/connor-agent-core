use agentos_storage::{
    AgentOsStorage, KNOWLEDGE_ENGINE_AUTHORITATIVE_LAYERS, KNOWLEDGE_ENGINE_LAYOUT_DIRECTORIES,
    KNOWLEDGE_ENGINE_PROJECTION_LAYERS, KNOWLEDGE_ENGINE_STORE_DIR, STORAGE_LAYOUT_VERSION,
};

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

    let ke_root = dir.path().join(KNOWLEDGE_ENGINE_STORE_DIR);
    assert!(ke_root.is_dir(), "missing knowledge engine root");
    for name in KNOWLEDGE_ENGINE_LAYOUT_DIRECTORIES {
        assert!(ke_root.join(name).is_dir(), "missing .ke-store/{name}");
    }
}

#[test]
fn manifest_contains_storage_version() {
    let dir = tempfile::tempdir().unwrap();
    let storage = AgentOsStorage::init(dir.path()).unwrap();
    let manifest = storage.manifest();

    assert_eq!(manifest.storage_version, STORAGE_LAYOUT_VERSION);
    assert_eq!(manifest.layout_directories.len(), 11);
    assert_eq!(
        manifest.knowledge_engine_store_dir,
        KNOWLEDGE_ENGINE_STORE_DIR
    );
    assert_eq!(
        manifest.knowledge_engine_layout_directories.len(),
        KNOWLEDGE_ENGINE_LAYOUT_DIRECTORIES.len()
    );
    assert_eq!(
        manifest.knowledge_engine_authoritative_layers,
        KNOWLEDGE_ENGINE_AUTHORITATIVE_LAYERS
            .iter()
            .map(|s| s.to_string())
            .collect::<Vec<_>>()
    );
    assert_eq!(
        manifest.knowledge_engine_projection_layers,
        KNOWLEDGE_ENGINE_PROJECTION_LAYERS
            .iter()
            .map(|s| s.to_string())
            .collect::<Vec<_>>()
    );
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

#[test]
fn knowledge_engine_path_helpers_point_inside_ke_store() {
    let dir = tempfile::tempdir().unwrap();
    let storage = AgentOsStorage::init(dir.path()).unwrap();

    assert_eq!(
        storage.knowledge_engine_root(),
        dir.path().join(KNOWLEDGE_ENGINE_STORE_DIR)
    );
    assert_eq!(
        storage.knowledge_engine_path_for("events"),
        dir.path().join(KNOWLEDGE_ENGINE_STORE_DIR).join("events")
    );
}
