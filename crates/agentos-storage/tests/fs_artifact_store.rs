use agentos_storage::{AgentOsStorage, ArtifactVerificationIssue, FsArtifactStore, StorageError};
use artifact_core::{
    ArtifactDescriptor, ArtifactId, ArtifactKind, ArtifactStore, ArtifactStoreError,
};

fn screenshot_descriptor(id: &str) -> ArtifactDescriptor {
    let mut descriptor = ArtifactDescriptor::new(
        ArtifactId::from(id),
        ArtifactKind::Image,
        "2026-05-26T00:00:00Z".parse().unwrap(),
    );
    descriptor.title = Some("screenshot.png".to_string());
    descriptor.mime_type = Some("image/png".to_string());
    descriptor.metadata = serde_json::json!({
        "width": 2,
        "height": 2,
        "captured_by": "test"
    });
    descriptor
}

fn png_like_bytes() -> Vec<u8> {
    vec![
        0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, b'I', b'H', b'D',
        b'R',
    ]
}

#[test]
fn screenshot_artifact_can_be_persisted_and_verified() {
    let dir = tempfile::tempdir().unwrap();
    let storage = AgentOsStorage::init(dir.path()).unwrap();
    let store = FsArtifactStore::for_storage(&storage).unwrap();
    let descriptor = screenshot_descriptor("screenshot-1");
    let content = png_like_bytes();

    let record = store
        .put_with_content(descriptor.clone(), &content)
        .unwrap();

    let artifact_dir = storage.path_for("artifacts").join("screenshot-1");
    assert!(artifact_dir.join("descriptor.json").is_file());
    assert!(artifact_dir.join("content.bin").is_file());
    assert!(artifact_dir.join("record.json").is_file());
    assert_eq!(record.descriptor, descriptor);
    assert_eq!(record.content.byte_len, content.len() as u64);
    assert_eq!(record.content.sha256.len(), 64);
    assert!(
        record
            .content
            .sha256
            .chars()
            .all(|ch| ch.is_ascii_hexdigit())
    );

    let report = store.verify(&ArtifactId::from("screenshot-1")).unwrap();
    assert!(report.verified);
    assert!(report.issues.is_empty());
    assert_eq!(report.content, Some(record.content));
}

#[tokio::test]
async fn fs_artifact_store_get_list_and_read_content() {
    let dir = tempfile::tempdir().unwrap();
    let storage = AgentOsStorage::init(dir.path()).unwrap();
    let store = FsArtifactStore::for_storage(&storage).unwrap();
    let first = screenshot_descriptor("screenshot-b");
    let second = screenshot_descriptor("screenshot-a");
    let content = png_like_bytes();

    store.put_with_content(first.clone(), &content).unwrap();
    store.put_with_content(second.clone(), b"other").unwrap();

    let record = store.get_record(&first.id).unwrap().unwrap();
    assert_eq!(record.descriptor, first);
    assert_eq!(store.read_content(&first.id).unwrap(), Some(content));

    assert_eq!(store.get(&first.id).await.unwrap(), Some(first.clone()));
    let listed = store.list().await.unwrap();
    assert_eq!(listed, vec![second, first]);
}

#[tokio::test]
async fn fs_artifact_store_trait_put_writes_empty_content() {
    let dir = tempfile::tempdir().unwrap();
    let storage = AgentOsStorage::init(dir.path()).unwrap();
    let store = FsArtifactStore::for_storage(&storage).unwrap();
    let descriptor = screenshot_descriptor("descriptor-only");

    store.put(descriptor.clone()).await.unwrap();

    let record = store.get_record(&descriptor.id).unwrap().unwrap();
    assert_eq!(record.descriptor, descriptor);
    assert_eq!(record.content.byte_len, 0);
    assert_eq!(
        store
            .read_content(&ArtifactId::from("descriptor-only"))
            .unwrap(),
        Some(vec![])
    );
}

#[test]
fn verify_detects_content_tampering() {
    let dir = tempfile::tempdir().unwrap();
    let storage = AgentOsStorage::init(dir.path()).unwrap();
    let store = FsArtifactStore::for_storage(&storage).unwrap();
    let descriptor = screenshot_descriptor("screenshot-tamper");
    store
        .put_with_content(descriptor.clone(), png_like_bytes())
        .unwrap();
    std::fs::write(
        storage
            .path_for("artifacts")
            .join("screenshot-tamper")
            .join("content.bin"),
        b"tampered",
    )
    .unwrap();

    let report = store.verify(&descriptor.id).unwrap();

    assert!(!report.verified);
    assert!(report.issues.iter().any(|issue| matches!(
        issue,
        ArtifactVerificationIssue::ContentSizeMismatch { .. }
            | ArtifactVerificationIssue::ContentHashMismatch { .. }
    )));
}

#[test]
fn verify_detects_missing_content() {
    let dir = tempfile::tempdir().unwrap();
    let storage = AgentOsStorage::init(dir.path()).unwrap();
    let store = FsArtifactStore::for_storage(&storage).unwrap();
    let descriptor = screenshot_descriptor("screenshot-missing-content");
    store
        .put_with_content(descriptor.clone(), png_like_bytes())
        .unwrap();
    std::fs::remove_file(
        storage
            .path_for("artifacts")
            .join("screenshot-missing-content")
            .join("content.bin"),
    )
    .unwrap();

    let report = store.verify(&descriptor.id).unwrap();

    assert!(!report.verified);
    assert!(
        report
            .issues
            .contains(&ArtifactVerificationIssue::MissingContent)
    );
}

#[test]
fn verify_all_reports_artifacts_sorted_by_id() {
    let dir = tempfile::tempdir().unwrap();
    let storage = AgentOsStorage::init(dir.path()).unwrap();
    let store = FsArtifactStore::for_storage(&storage).unwrap();
    store
        .put_with_content(screenshot_descriptor("screenshot-b"), b"b")
        .unwrap();
    store
        .put_with_content(screenshot_descriptor("screenshot-a"), b"a")
        .unwrap();

    let reports = store.verify_all().unwrap();

    assert_eq!(reports.len(), 2);
    assert_eq!(reports[0].artifact_id, ArtifactId::from("screenshot-a"));
    assert_eq!(reports[1].artifact_id, ArtifactId::from("screenshot-b"));
    assert!(reports.iter().all(|report| report.verified));
}

#[tokio::test]
async fn duplicate_artifact_is_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let storage = AgentOsStorage::init(dir.path()).unwrap();
    let store = FsArtifactStore::for_storage(&storage).unwrap();
    let descriptor = screenshot_descriptor("screenshot-dup");
    store.put_with_content(descriptor.clone(), b"one").unwrap();

    let storage_error = store
        .put_with_content(descriptor.clone(), b"two")
        .unwrap_err();
    assert!(matches!(
        storage_error,
        StorageError::ArtifactAlreadyExists { artifact_id } if artifact_id == "screenshot-dup"
    ));

    let trait_error = store.put(descriptor.clone()).await.unwrap_err();
    assert_eq!(
        trait_error,
        ArtifactStoreError::DuplicateArtifactId(descriptor.id)
    );
}

#[test]
fn invalid_artifact_id_path_is_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let storage = AgentOsStorage::init(dir.path()).unwrap();
    let store = FsArtifactStore::for_storage(&storage).unwrap();
    let descriptor = screenshot_descriptor("../escape");

    let error = store.put_with_content(descriptor, b"bad").unwrap_err();

    assert!(matches!(
        error,
        StorageError::InvalidArtifactIdPath { artifact_id } if artifact_id == "../escape"
    ));
}
