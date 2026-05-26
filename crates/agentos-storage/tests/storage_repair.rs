use agentos_storage::{AgentOsStorage, FsArtifactStore, StorageRepair, StorageRepairIssue};
use artifact_core::{ArtifactDescriptor, ArtifactId, ArtifactKind};
use chrono::{TimeZone, Utc};
use conversation_core::{
    CURRENT_SCHEMA_VERSION, ConversationEvent, ConversationEventEnvelope, ConversationId,
    ConversationKind, ConversationSession, ConversationStatus, EventId,
};
use conversation_journal::{ConversationJournal, JsonlConversationJournal};

fn ts(seconds: i64) -> chrono::DateTime<Utc> {
    Utc.timestamp_opt(seconds, 0).unwrap()
}

fn artifact_descriptor(id: &str) -> ArtifactDescriptor {
    let mut descriptor = ArtifactDescriptor::new(ArtifactId::from(id), ArtifactKind::Image, ts(1));
    descriptor.title = Some(format!("{id}.png"));
    descriptor.mime_type = Some("image/png".to_string());
    descriptor
}

fn conversation_created_envelope(
    conversation_id: &str,
    event_id: &str,
) -> ConversationEventEnvelope {
    let id = ConversationId::from(conversation_id);
    ConversationEventEnvelope {
        schema_version: CURRENT_SCHEMA_VERSION,
        event_id: EventId::from(event_id),
        conversation_id: id.clone(),
        occurred_at: ts(10),
        actor_id: None,
        event: ConversationEvent::ConversationCreated {
            session: ConversationSession {
                id,
                kind: ConversationKind::AgentTask,
                title: Some("repair test".to_string()),
                participants: vec![],
                created_at: ts(10),
                updated_at: ts(10),
                status: ConversationStatus::Active,
            },
        },
    }
}

fn artifact_linked_envelope(
    conversation_id: &str,
    event_id: &str,
    artifact_id: &str,
) -> ConversationEventEnvelope {
    ConversationEventEnvelope {
        schema_version: CURRENT_SCHEMA_VERSION,
        event_id: EventId::from(event_id),
        conversation_id: ConversationId::from(conversation_id),
        occurred_at: ts(11),
        actor_id: None,
        event: ConversationEvent::ArtifactLinkedToConversation {
            artifact: artifact_descriptor(artifact_id),
        },
    }
}

#[tokio::test]
async fn repair_inspect_reports_clean_empty_storage() {
    let dir = tempfile::tempdir().unwrap();
    let storage = AgentOsStorage::init(dir.path()).unwrap();

    let report = StorageRepair::inspect(&storage).await.unwrap();

    assert!(report.is_clean(), "unexpected issues: {:?}", report.issues);
    assert!(report.artifact_references.orphan_artifacts.is_empty());
    assert!(
        report
            .artifact_references
            .broken_artifact_references
            .is_empty()
    );
}

#[tokio::test]
async fn artifact_store_lists_artifact_ids_in_deterministic_order() {
    let dir = tempfile::tempdir().unwrap();
    let storage = AgentOsStorage::init(dir.path()).unwrap();
    let artifacts = FsArtifactStore::for_storage(&storage).unwrap();
    artifacts
        .put_with_content(artifact_descriptor("artifact-c"), b"c")
        .unwrap();
    artifacts
        .put_with_content(artifact_descriptor("artifact-a"), b"a")
        .unwrap();
    artifacts
        .put_with_content(artifact_descriptor("artifact-b"), b"b")
        .unwrap();

    let ids = artifacts.list_artifact_ids().unwrap();

    assert_eq!(
        ids,
        vec![
            ArtifactId::from("artifact-a"),
            ArtifactId::from("artifact-b"),
            ArtifactId::from("artifact-c"),
        ]
    );
}

#[tokio::test]
async fn repair_reports_orphan_artifact() {
    let dir = tempfile::tempdir().unwrap();
    let storage = AgentOsStorage::init(dir.path()).unwrap();
    let artifacts = FsArtifactStore::for_storage(&storage).unwrap();
    artifacts
        .put_with_content(artifact_descriptor("artifact-orphan"), b"orphan")
        .unwrap();

    let report = StorageRepair::inspect(&storage).await.unwrap();

    assert_eq!(
        report.artifact_references.orphan_artifacts,
        vec![ArtifactId::from("artifact-orphan")]
    );
    assert!(report.issues.iter().any(|issue| matches!(
        issue,
        StorageRepairIssue::OrphanArtifact { artifact_id }
            if artifact_id == &ArtifactId::from("artifact-orphan")
    )));
}

#[tokio::test]
async fn repair_reports_broken_artifact_reference() {
    let dir = tempfile::tempdir().unwrap();
    let storage = AgentOsStorage::init(dir.path()).unwrap();
    let journal = JsonlConversationJournal::new(storage.path_for("conversations"));
    journal
        .append(conversation_created_envelope("conv-broken", "evt-1"))
        .await
        .unwrap();
    journal
        .append(artifact_linked_envelope(
            "conv-broken",
            "evt-2",
            "artifact-missing",
        ))
        .await
        .unwrap();

    let report = StorageRepair::inspect(&storage).await.unwrap();

    assert!(report.artifact_references.orphan_artifacts.is_empty());
    assert_eq!(
        report.artifact_references.broken_artifact_references.len(),
        1
    );
    let broken = &report.artifact_references.broken_artifact_references[0];
    assert_eq!(broken.conversation_id, ConversationId::from("conv-broken"));
    assert_eq!(broken.event_id, EventId::from("evt-2"));
    assert_eq!(broken.artifact_id, ArtifactId::from("artifact-missing"));
    assert!(report.issues.iter().any(|issue| matches!(
        issue,
        StorageRepairIssue::BrokenArtifactReference { artifact_id, .. }
            if artifact_id == &ArtifactId::from("artifact-missing")
    )));
}

#[tokio::test]
async fn repair_rebuilds_projection_from_conversation_journal() {
    let dir = tempfile::tempdir().unwrap();
    let storage = AgentOsStorage::init(dir.path()).unwrap();
    let journal = JsonlConversationJournal::new(storage.path_for("conversations"));
    journal
        .append(conversation_created_envelope("conv-projection", "evt-1"))
        .await
        .unwrap();

    let report = StorageRepair::rebuild_conversation_projections(&storage)
        .await
        .unwrap();

    assert_eq!(report.conversations_checked, 1);
    assert_eq!(
        report.rebuilt_conversations,
        vec![ConversationId::from("conv-projection")]
    );
    assert!(report.failed_conversations.is_empty());
}

#[tokio::test]
async fn repair_reports_corrupted_conversation_journal() {
    let dir = tempfile::tempdir().unwrap();
    let storage = AgentOsStorage::init(dir.path()).unwrap();
    let journal = JsonlConversationJournal::new(storage.path_for("conversations"));
    journal
        .append(conversation_created_envelope("conv-corrupt", "evt-1"))
        .await
        .unwrap();
    std::fs::write(
        storage
            .path_for("conversations")
            .join("conv-corrupt")
            .join("segments")
            .join("00000000000000000000.jsonl"),
        b"not valid json\n",
    )
    .unwrap();

    let report = StorageRepair::inspect(&storage).await.unwrap();

    assert!(report.issues.iter().any(|issue| matches!(
        issue,
        StorageRepairIssue::ConversationJournalIntegrityFailed { conversation_id, .. }
            if conversation_id == &ConversationId::from("conv-corrupt")
    )));
}
