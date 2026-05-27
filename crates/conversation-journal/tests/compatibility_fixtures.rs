use conversation_core::{
    CURRENT_SCHEMA_VERSION, ConversationEvent, ConversationEventEnvelope, ConversationId,
    ConversationKind, ConversationSession, ConversationStatus, EventId,
};
use conversation_journal::{ConversationJournal, JsonlConversationJournal};

fn legacy_conversation_created_event(conversation_id: &str, event_id: &str) -> serde_json::Value {
    serde_json::json!({
        "schema_version": CURRENT_SCHEMA_VERSION,
        "event_id": event_id,
        "conversation_id": conversation_id,
        "occurred_at": "2026-01-01T00:00:00Z",
        "actor_id": null,
        "event": {
            "type": "conversation_created",
            "session": {
                "id": conversation_id,
                "kind": "direct",
                "title": "Legacy fixture conversation",
                "participants": [],
                "created_at": "2026-01-01T00:00:00Z",
                "updated_at": "2026-01-01T00:00:00Z",
                "status": "active"
            }
        }
    })
}

fn write_legacy_v1_journal_fixture(root: &std::path::Path, conversation_id: &str) {
    let conversation_dir = root.join(conversation_id);
    let segments_dir = conversation_dir.join("segments");
    std::fs::create_dir_all(&segments_dir).unwrap();

    let events = [
        legacy_conversation_created_event(conversation_id, "evt-1"),
        legacy_conversation_created_event(conversation_id, "evt-2"),
    ];
    let jsonl = events
        .iter()
        .map(|event| serde_json::to_string(event).unwrap())
        .collect::<Vec<_>>()
        .join("\n")
        + "\n";
    let bytes = jsonl.len() as u64;
    std::fs::write(segments_dir.join("00000000000000000000.jsonl"), jsonl).unwrap();

    // Legacy v1 fixture intentionally omits checksum fields. Current loader accepts this
    // pre-integrity metadata shape and still verifies byte counts and event totals.
    let manifest = serde_json::json!({
        "version": 1,
        "max_segment_bytes": 67108864,
        "active_segment_index": 0,
        "total_events": 2,
        "segments": [{
            "index": 0,
            "file_name": "00000000000000000000.jsonl",
            "event_count": 2,
            "bytes": bytes
        }]
    });
    std::fs::write(
        conversation_dir.join("manifest.json"),
        serde_json::to_vec_pretty(&manifest).unwrap(),
    )
    .unwrap();
}

#[tokio::test]
async fn old_journal_fixture_can_replay_and_accept_new_appends() {
    let dir = tempfile::tempdir().unwrap();
    write_legacy_v1_journal_fixture(dir.path(), "conv-legacy");

    let conversation_id = ConversationId::from("conv-legacy");
    let journal = JsonlConversationJournal::new(dir.path());

    let loaded = journal.load(&conversation_id).await.unwrap();
    assert_eq!(loaded.len(), 2);
    assert_eq!(loaded[0].event_id, EventId::from("evt-1"));
    assert_eq!(loaded[1].event_id, EventId::from("evt-2"));

    let new_event = ConversationEventEnvelope {
        schema_version: CURRENT_SCHEMA_VERSION,
        event_id: EventId::from("evt-3"),
        conversation_id: conversation_id.clone(),
        occurred_at: chrono::Utc::now(),
        actor_id: None,
        event: ConversationEvent::ConversationCreated {
            session: ConversationSession {
                id: conversation_id.clone(),
                kind: ConversationKind::Direct,
                title: Some("Current append".to_string()),
                participants: vec![],
                created_at: chrono::Utc::now(),
                updated_at: chrono::Utc::now(),
                status: ConversationStatus::Active,
            },
        },
    };

    journal.append(new_event).await.unwrap();

    let replayed = journal.load(&conversation_id).await.unwrap();
    assert_eq!(replayed.len(), 3);
    assert_eq!(replayed[2].event_id, EventId::from("evt-3"));
}
