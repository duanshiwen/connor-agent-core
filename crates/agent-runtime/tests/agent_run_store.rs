use agent_runtime::{
    AgentRunRecord, AgentRunStore, DurableAgentRunStatus, JsonlAgentRunStore, MemoryAgentRunStore,
};
use chrono::Utc;
use conversation_core::{ConversationId, MessageId, ParticipantId};

fn record(run_id: &str) -> AgentRunRecord {
    AgentRunRecord {
        run_id: run_id.to_string(),
        conversation_id: ConversationId("conversation-1".to_string()),
        trigger_message_id: MessageId("message-1".to_string()),
        requested_by: ParticipantId("user-1".to_string()),
        status: DurableAgentRunStatus::Queued,
        created_at: Utc::now(),
        updated_at: Utc::now(),
        error_message: None,
    }
}

#[tokio::test]
async fn memory_agent_run_store_tracks_valid_status_transitions() {
    let store = MemoryAgentRunStore::new();
    store.insert(record("run-1")).await.unwrap();

    store
        .transition("run-1", DurableAgentRunStatus::Running)
        .await
        .unwrap();
    store
        .transition("run-1", DurableAgentRunStatus::WaitingForApproval)
        .await
        .unwrap();
    store
        .transition("run-1", DurableAgentRunStatus::Running)
        .await
        .unwrap();
    store
        .transition("run-1", DurableAgentRunStatus::Completed)
        .await
        .unwrap();

    let loaded = store.get("run-1").await.unwrap().unwrap();
    assert_eq!(loaded.status, DurableAgentRunStatus::Completed);
}

#[tokio::test]
async fn memory_agent_run_store_rejects_invalid_terminal_transition() {
    let store = MemoryAgentRunStore::new();
    store.insert(record("run-1")).await.unwrap();
    store
        .transition("run-1", DurableAgentRunStatus::Running)
        .await
        .unwrap();
    store
        .transition("run-1", DurableAgentRunStatus::Completed)
        .await
        .unwrap();

    let err = store
        .transition("run-1", DurableAgentRunStatus::Running)
        .await
        .unwrap_err();

    assert_eq!(err.run_id(), "run-1");
    assert!(err.to_string().contains("invalid agent run transition"));
}

#[tokio::test]
async fn memory_agent_run_store_lists_runs_in_deterministic_order() {
    let store = MemoryAgentRunStore::new();
    store.insert(record("run-b")).await.unwrap();
    store.insert(record("run-a")).await.unwrap();

    let runs = store.list().await.unwrap();

    assert_eq!(
        runs.iter()
            .map(|run| run.run_id.as_str())
            .collect::<Vec<_>>(),
        vec!["run-a", "run-b"]
    );
}

#[tokio::test]
async fn jsonl_agent_run_store_reloads_latest_run_state() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("agent-runs.jsonl");

    let store = JsonlAgentRunStore::open(&path).await.unwrap();
    store.insert(record("run-1")).await.unwrap();
    store
        .transition("run-1", DurableAgentRunStatus::Running)
        .await
        .unwrap();
    store
        .transition("run-1", DurableAgentRunStatus::WaitingForApproval)
        .await
        .unwrap();

    let reloaded = JsonlAgentRunStore::open(&path).await.unwrap();
    let loaded = reloaded.get("run-1").await.unwrap().unwrap();

    assert_eq!(loaded.status, DurableAgentRunStatus::WaitingForApproval);
    assert_eq!(reloaded.list().await.unwrap().len(), 1);
}

#[tokio::test]
async fn jsonl_agent_run_store_persists_terminal_state_after_reload() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("agent-runs.jsonl");

    let store = JsonlAgentRunStore::open(&path).await.unwrap();
    store.insert(record("run-1")).await.unwrap();
    store
        .transition("run-1", DurableAgentRunStatus::Running)
        .await
        .unwrap();
    store
        .transition("run-1", DurableAgentRunStatus::Failed)
        .await
        .unwrap();

    let reloaded = JsonlAgentRunStore::open(&path).await.unwrap();
    let err = reloaded
        .transition("run-1", DurableAgentRunStatus::Running)
        .await
        .unwrap_err();

    assert_eq!(err.run_id(), "run-1");
    assert_eq!(
        reloaded.get("run-1").await.unwrap().unwrap().status,
        DurableAgentRunStatus::Failed
    );
}
