use action_core::{
    ActionId, ActionKind, ActionRequest, ActionResult, ActionResultPayload, ActionStatus,
};
use action_runtime::ActionRuntimeOutcome;
use agent_runtime::{
    ActionRecord, ActionStore, ActionStoreError, JsonlActionStore, MemoryActionStore,
};
use chrono::Utc;

fn request(id: &str) -> ActionRequest {
    ActionRequest {
        action_id: ActionId(id.to_string()),
        action_kind: ActionKind("knowledge.search".to_string()),
        input: serde_json::json!({"q": "agentos"}),
        requested_by: "agent".to_string(),
        conversation_id: Some("conv-1".to_string()),
        message_id: Some("msg-1".to_string()),
        requested_at: Utc::now(),
    }
}

fn completed_outcome(id: &str, summary: &str) -> ActionRuntimeOutcome {
    ActionRuntimeOutcome::Completed {
        action_id: ActionId(id.to_string()),
        result: ActionResult {
            status: ActionStatus::Completed,
            payload: ActionResultPayload::Text(summary.to_string()),
            summary: summary.to_string(),
            completed_at: Utc::now(),
        },
    }
}

#[tokio::test]
async fn memory_action_store_records_request_and_completion() {
    let store = MemoryActionStore::new();
    let request = request("action-1");

    store
        .insert_request(ActionRecord::requested(
            request.clone(),
            "audit-1",
            "idem-1",
        ))
        .await
        .unwrap();
    let completed = store
        .record_outcome("action-1", completed_outcome("action-1", "ok"))
        .await
        .unwrap();

    assert_eq!(completed.action_id, ActionId("action-1".to_string()));
    assert_eq!(completed.status, ActionStatus::Completed);
    assert_eq!(completed.audit_correlation_id, "audit-1");
    assert_eq!(completed.idempotency_key, "idem-1");
    assert!(completed.outcome.is_some());
}

#[tokio::test]
async fn jsonl_action_store_reloads_latest_action_state() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("actions.jsonl");
    let store = JsonlActionStore::open(&path).await.unwrap();

    store
        .insert_request(ActionRecord::requested(
            request("action-1"),
            "audit-1",
            "idem-1",
        ))
        .await
        .unwrap();
    store
        .record_outcome("action-1", completed_outcome("action-1", "persisted"))
        .await
        .unwrap();

    let reloaded = JsonlActionStore::open(&path).await.unwrap();
    let record = reloaded.get("action-1").await.unwrap().unwrap();

    assert_eq!(record.status, ActionStatus::Completed);
    match record.outcome.unwrap() {
        ActionRuntimeOutcome::Completed { action_id, result } => {
            assert_eq!(action_id, ActionId("action-1".to_string()));
            assert_eq!(result.summary, "persisted");
            assert_eq!(
                result.payload,
                ActionResultPayload::Text("persisted".to_string())
            );
        }
        other => panic!("expected completed outcome, got {other:?}"),
    }
    assert_eq!(record.audit_correlation_id, "audit-1");
    assert_eq!(record.idempotency_key, "idem-1");
}

#[tokio::test]
async fn repeated_same_completion_is_idempotent() {
    let store = MemoryActionStore::new();
    let outcome = completed_outcome("action-1", "same");
    store
        .insert_request(ActionRecord::requested(
            request("action-1"),
            "audit-1",
            "idem-1",
        ))
        .await
        .unwrap();

    let first = store
        .record_outcome("action-1", outcome.clone())
        .await
        .unwrap();
    let second = store.record_outcome("action-1", outcome).await.unwrap();

    assert_eq!(first, second);
}

#[tokio::test]
async fn repeated_different_completion_is_rejected() {
    let store = MemoryActionStore::new();
    store
        .insert_request(ActionRecord::requested(
            request("action-1"),
            "audit-1",
            "idem-1",
        ))
        .await
        .unwrap();
    store
        .record_outcome("action-1", completed_outcome("action-1", "first"))
        .await
        .unwrap();

    let err = store
        .record_outcome("action-1", completed_outcome("action-1", "different"))
        .await
        .unwrap_err();

    assert!(matches!(
        err,
        ActionStoreError::ActionAlreadyCompleted { action_id } if action_id == "action-1"
    ));
}
