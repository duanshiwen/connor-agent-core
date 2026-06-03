use agentos_storage::{AgentOsStorage, FsKnowledgeEngineStore, KnowledgeRecordProjectionKind};
use chrono::{TimeZone, Utc};
use serde_json::json;

#[test]
fn appends_and_reads_monthly_event_and_audit_segments() {
    let dir = tempfile::tempdir().unwrap();
    let storage = AgentOsStorage::init(dir.path()).unwrap();
    let store = FsKnowledgeEngineStore::for_storage(&storage).unwrap();
    let ts = Utc.with_ymd_and_hms(2026, 6, 3, 10, 30, 0).unwrap();

    let event = json!({
        "event_id": "evt-1",
        "event_type": "object.created",
        "timestamp": ts,
        "payload": { "object_id": "obj-mass" }
    });
    let audit = json!({
        "audit_id": "aud-1",
        "operation_type": "query",
        "timestamp": ts,
        "result": { "status": "returned" }
    });

    let event_report = store.append_event(ts, &event).unwrap();
    let audit_report = store.append_audit(ts, &audit).unwrap();

    assert!(
        event_report
            .path
            .ends_with("events/2026/06/seg-000001.kevent.ndjson")
    );
    assert!(
        audit_report
            .path
            .ends_with("audit/2026/06/audit-seg-000001.kaudit.ndjson")
    );
    assert_eq!(event_report.line_count, 1);
    assert_eq!(audit_report.line_count, 1);

    let events: Vec<serde_json::Value> = store.read_events(2026, 6).unwrap();
    let audits: Vec<serde_json::Value> = store.read_audit(2026, 6).unwrap();

    assert_eq!(events.len(), 1);
    assert_eq!(events[0]["event_type"], "object.created");
    assert_eq!(audits.len(), 1);
    assert_eq!(audits[0]["operation_type"], "query");
}

#[test]
fn blob_store_is_content_addressed_and_deduplicates() {
    let dir = tempfile::tempdir().unwrap();
    let storage = AgentOsStorage::init(dir.path()).unwrap();
    let store = FsKnowledgeEngineStore::for_storage(&storage).unwrap();
    let ts = Utc.with_ymd_and_hms(2026, 6, 3, 10, 55, 0).unwrap();

    let first = store
        .put_blob(b"kumquat image bytes", "image/jpeg", ts)
        .unwrap();
    let second = store
        .put_blob(b"kumquat image bytes", "image/jpeg", ts)
        .unwrap();

    assert_eq!(first.blob_hash, second.blob_hash);
    assert!(!first.deduplicated);
    assert!(second.deduplicated);
    assert!(first.storage_path.exists());
    assert!(first.metadata_path.exists());
    assert!(
        first
            .storage_path
            .to_string_lossy()
            .contains("blobs/sha256/")
    );

    let bytes = store.read_blob(&first.blob_hash).unwrap().unwrap();
    assert_eq!(bytes, b"kumquat image bytes");

    let metadata = store.read_blob_metadata(&first.blob_hash).unwrap().unwrap();
    assert_eq!(metadata.blob_hash, first.blob_hash);
    assert_eq!(metadata.size_bytes, first.size_bytes);
    assert_eq!(metadata.mime_type_detected, "image/jpeg");
    assert_eq!(metadata.integrity.hash_algorithm, "sha256");
    assert!(!metadata.storage.encrypted);
}

#[test]
fn invalid_blob_hash_is_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let storage = AgentOsStorage::init(dir.path()).unwrap();
    let store = FsKnowledgeEngineStore::for_storage(&storage).unwrap();

    let err = store.read_blob("sha256:not-a-valid-hash").unwrap_err();
    assert!(
        err.to_string()
            .contains("invalid knowledge blob sha256 hash")
    );
}

#[test]
fn projection_records_replace_and_read_current_state() {
    let dir = tempfile::tempdir().unwrap();
    let storage = AgentOsStorage::init(dir.path()).unwrap();
    let store = FsKnowledgeEngineStore::for_storage(&storage).unwrap();

    let records = vec![
        json!({
            "record_type": "object",
            "object_id": "obj-mass",
            "canonical_name": "质量",
            "status": "active"
        }),
        json!({
            "record_type": "object",
            "object_id": "obj-gravity",
            "canonical_name": "万有引力",
            "status": "active"
        }),
    ];

    let report = store
        .replace_projection_records(KnowledgeRecordProjectionKind::Objects, &records)
        .unwrap();

    assert_eq!(report.record_count, 2);
    assert!(report.path.ends_with("records/objects.krec.ndjson"));

    let loaded: Vec<serde_json::Value> = store
        .read_projection_records(KnowledgeRecordProjectionKind::Objects)
        .unwrap();
    assert_eq!(loaded, records);

    let replacement = vec![json!({
        "record_type": "object",
        "object_id": "obj-mass",
        "canonical_name": "质量",
        "status": "active",
        "version": 2
    })];
    store
        .replace_projection_records(KnowledgeRecordProjectionKind::Objects, &replacement)
        .unwrap();
    let loaded_after_replace: Vec<serde_json::Value> = store
        .read_projection_records(KnowledgeRecordProjectionKind::Objects)
        .unwrap();
    assert_eq!(loaded_after_replace, replacement);
}

#[test]
fn empty_projection_read_returns_empty_vec() {
    let dir = tempfile::tempdir().unwrap();
    let storage = AgentOsStorage::init(dir.path()).unwrap();
    let store = FsKnowledgeEngineStore::for_storage(&storage).unwrap();

    let loaded: Vec<serde_json::Value> = store
        .read_projection_records(KnowledgeRecordProjectionKind::Relations)
        .unwrap();
    assert!(loaded.is_empty());
}
