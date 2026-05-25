use agentos_storage::{
    AgentOsStorage, BackupManifest, FsArtifactStore, StorageBackup, StorageError,
};
use artifact_core::{ArtifactDescriptor, ArtifactId, ArtifactKind};

fn screenshot_descriptor(id: &str) -> ArtifactDescriptor {
    let mut descriptor = ArtifactDescriptor::new(
        ArtifactId::from(id),
        ArtifactKind::Image,
        "2026-05-26T00:00:00Z".parse().unwrap(),
    );
    descriptor.title = Some("screenshot.png".to_string());
    descriptor.mime_type = Some("image/png".to_string());
    descriptor.metadata = serde_json::json!({"source": "backup-test"});
    descriptor
}

fn read_backup_manifest(path: &std::path::Path) -> BackupManifest {
    let bytes = std::fs::read(path.join("backup-manifest.json")).unwrap();
    serde_json::from_slice(&bytes).unwrap()
}

#[test]
fn export_restore_preserves_storage_manifest_and_artifact_data() {
    let source_dir = tempfile::tempdir().unwrap();
    let backup_dir = tempfile::tempdir().unwrap();
    let target_dir = tempfile::tempdir().unwrap();
    let backup_path = backup_dir.path().join("backup");
    let target_path = target_dir.path().join("restored");
    let source = AgentOsStorage::init(source_dir.path()).unwrap();
    let source_artifacts = FsArtifactStore::for_storage(&source).unwrap();
    let descriptor = screenshot_descriptor("screenshot-backup");
    let content = b"fake screenshot png bytes";
    source_artifacts
        .put_with_content(descriptor.clone(), content)
        .unwrap();

    let backup_report = StorageBackup::export(&source, &backup_path).unwrap();
    let restore_report = StorageBackup::restore(&backup_path, &target_path).unwrap();

    assert!(backup_report.file_count >= 4);
    assert_eq!(restore_report.file_count, backup_report.file_count);
    assert_eq!(restore_report.total_bytes, backup_report.total_bytes);
    assert!(
        restore_report
            .artifact_reports
            .iter()
            .all(|report| report.verified)
    );

    let source_manifest = std::fs::read(source.manifest_path()).unwrap();
    let target_manifest = std::fs::read(target_path.join("manifest.json")).unwrap();
    assert_eq!(target_manifest, source_manifest);

    let restored_artifacts = FsArtifactStore::new(target_path.join("artifacts")).unwrap();
    assert_eq!(
        restored_artifacts
            .get_record(&ArtifactId::from("screenshot-backup"))
            .unwrap()
            .unwrap()
            .descriptor,
        descriptor
    );
    assert_eq!(
        restored_artifacts
            .read_content(&ArtifactId::from("screenshot-backup"))
            .unwrap(),
        Some(content.to_vec())
    );
    assert!(
        restored_artifacts
            .verify(&ArtifactId::from("screenshot-backup"))
            .unwrap()
            .verified
    );

    let manifest = read_backup_manifest(&backup_path);
    assert_eq!(manifest.backup_version, 1);
    assert_eq!(manifest.storage_version, 1);
    assert!(
        manifest
            .files
            .iter()
            .any(|entry| entry.path == "manifest.json")
    );
    assert!(
        manifest
            .files
            .iter()
            .any(|entry| entry.path == "artifacts/screenshot-backup/content.bin")
    );
}

#[test]
fn restore_rejects_corrupted_backup_file() {
    let source_dir = tempfile::tempdir().unwrap();
    let backup_dir = tempfile::tempdir().unwrap();
    let target_dir = tempfile::tempdir().unwrap();
    let backup_path = backup_dir.path().join("backup");
    let target_path = target_dir.path().join("restored");
    let source = AgentOsStorage::init(source_dir.path()).unwrap();
    let source_artifacts = FsArtifactStore::for_storage(&source).unwrap();
    source_artifacts
        .put_with_content(screenshot_descriptor("screenshot-corrupt"), b"original")
        .unwrap();
    StorageBackup::export(&source, &backup_path).unwrap();
    std::fs::write(
        backup_path
            .join("data")
            .join("artifacts")
            .join("screenshot-corrupt")
            .join("content.bin"),
        b"corrupted",
    )
    .unwrap();

    let error = StorageBackup::restore(&backup_path, &target_path).unwrap_err();

    assert!(matches!(
        error,
        StorageError::BackupIntegrityMismatch { path, .. }
            if path == "artifacts/screenshot-corrupt/content.bin"
    ));
}

#[test]
fn restore_rejects_non_empty_target_root() {
    let source_dir = tempfile::tempdir().unwrap();
    let backup_dir = tempfile::tempdir().unwrap();
    let target_dir = tempfile::tempdir().unwrap();
    let backup_path = backup_dir.path().join("backup");
    let target_path = target_dir.path().join("restored");
    let source = AgentOsStorage::init(source_dir.path()).unwrap();
    StorageBackup::export(&source, &backup_path).unwrap();
    std::fs::create_dir_all(&target_path).unwrap();
    std::fs::write(target_path.join("existing.txt"), b"already here").unwrap();

    let error = StorageBackup::restore(&backup_path, &target_path).unwrap_err();

    assert!(matches!(
        error,
        StorageError::BackupTargetNotEmpty { path } if path == target_path
    ));
}

#[test]
fn verify_backup_rejects_missing_file() {
    let source_dir = tempfile::tempdir().unwrap();
    let backup_dir = tempfile::tempdir().unwrap();
    let backup_path = backup_dir.path().join("backup");
    let source = AgentOsStorage::init(source_dir.path()).unwrap();
    let source_artifacts = FsArtifactStore::for_storage(&source).unwrap();
    source_artifacts
        .put_with_content(screenshot_descriptor("screenshot-missing"), b"content")
        .unwrap();
    StorageBackup::export(&source, &backup_path).unwrap();
    std::fs::remove_file(
        backup_path
            .join("data")
            .join("artifacts")
            .join("screenshot-missing")
            .join("content.bin"),
    )
    .unwrap();

    let error = StorageBackup::verify_backup(&backup_path).unwrap_err();

    assert!(matches!(
        error,
        StorageError::BackupFileMissing { path }
            if path == "artifacts/screenshot-missing/content.bin"
    ));
}

#[test]
fn restore_rejects_artifact_integrity_failure_after_copy() {
    let backup_dir = tempfile::tempdir().unwrap();
    let target_dir = tempfile::tempdir().unwrap();
    let backup_path = backup_dir.path().join("backup");
    let data_path = backup_path.join("data");
    let source_path = tempfile::tempdir().unwrap();
    let source = AgentOsStorage::init(source_path.path()).unwrap();
    let store = FsArtifactStore::for_storage(&source).unwrap();
    let descriptor = screenshot_descriptor("screenshot-bad-record");
    store
        .put_with_content(descriptor.clone(), b"original-content")
        .unwrap();
    std::fs::write(
        source
            .path_for("artifacts")
            .join("screenshot-bad-record")
            .join("record.json"),
        serde_json::json!({
            "descriptor": descriptor,
            "content": {
                "byte_len": 1,
                "sha256": "not-the-real-hash"
            }
        })
        .to_string(),
    )
    .unwrap();
    std::fs::create_dir_all(&backup_path).unwrap();
    copy_dir_all(source.root(), &data_path);
    let files = collect_entries(&data_path);
    let manifest = serde_json::json!({
        "backup_version": 1,
        "storage_version": 1,
        "created_at": "2026-05-26T00:00:00Z",
        "source_manifest": serde_json::from_slice::<serde_json::Value>(&std::fs::read(source.manifest_path()).unwrap()).unwrap(),
        "files": files
    });
    std::fs::write(
        backup_path.join("backup-manifest.json"),
        serde_json::to_vec_pretty(&manifest).unwrap(),
    )
    .unwrap();

    let error =
        StorageBackup::restore(&backup_path, target_dir.path().join("restored")).unwrap_err();

    assert!(matches!(error, StorageError::RestoreIntegrityFailed { .. }));
}

fn copy_dir_all(source: &std::path::Path, target: &std::path::Path) {
    std::fs::create_dir_all(target).unwrap();
    for entry in std::fs::read_dir(source).unwrap() {
        let entry = entry.unwrap();
        let source_path = entry.path();
        let target_path = target.join(entry.file_name());
        if source_path.is_dir() {
            copy_dir_all(&source_path, &target_path);
        } else {
            std::fs::copy(&source_path, &target_path).unwrap();
        }
    }
}

fn collect_entries(data_path: &std::path::Path) -> Vec<serde_json::Value> {
    let mut entries = Vec::new();
    collect_entries_inner(data_path, data_path, &mut entries);
    entries.sort_by_key(|entry| entry["path"].as_str().unwrap().to_string());
    entries
}

fn collect_entries_inner(
    base: &std::path::Path,
    current: &std::path::Path,
    entries: &mut Vec<serde_json::Value>,
) {
    for entry in std::fs::read_dir(current).unwrap() {
        let entry = entry.unwrap();
        let path = entry.path();
        if path.is_dir() {
            collect_entries_inner(base, &path, entries);
        } else {
            let relative = path
                .strip_prefix(base)
                .unwrap()
                .components()
                .map(|component| component.as_os_str().to_string_lossy())
                .collect::<Vec<_>>()
                .join("/");
            let bytes = std::fs::read(&path).unwrap();
            entries.push(serde_json::json!({
                "path": relative,
                "byte_len": bytes.len() as u64,
                "sha256": sha256(&bytes)
            }));
        }
    }
}

fn sha256(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let digest = Sha256::digest(bytes);
    format!("{digest:x}")
}
