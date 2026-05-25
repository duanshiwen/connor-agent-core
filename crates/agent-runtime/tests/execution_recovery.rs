use std::sync::Arc;
use std::time::Duration as StdDuration;

use action_core::{
    ActionId, ActionKind, ActionRequest, ActionResult, ActionResultPayload, ActionStatus,
};
use action_runtime::ActionRuntimeOutcome;
use agent_runtime::{
    ActionStore, AgentRunQueue, AgentRunRecord, AgentRunStore, ApprovalQueue, ApprovalRequest,
    ApprovalStatus, DurableAgentRunStatus, JsonlActionStore, JsonlAgentRunStore,
    JsonlApprovalQueue, ToolLoopCheckpoint, ToolLoopResumePlan, ToolResultCheckpoint,
};
use chrono::{Duration, Utc};
use conversation_core::{ConversationId, MessageId, ParticipantId};

fn run_record(run_id: &str, status: DurableAgentRunStatus) -> AgentRunRecord {
    let now = Utc::now();
    AgentRunRecord {
        run_id: run_id.to_string(),
        conversation_id: ConversationId::from("conv-1"),
        trigger_message_id: MessageId::from("msg-1"),
        requested_by: ParticipantId::from("user-1"),
        status,
        created_at: now,
        updated_at: now,
        error_message: None,
    }
}

fn action_request(id: &str) -> ActionRequest {
    ActionRequest {
        action_id: ActionId(id.to_string()),
        action_kind: ActionKind("knowledge.search".to_string()),
        input: serde_json::json!({"q": "recovery"}),
        requested_by: "agent".to_string(),
        conversation_id: Some("conv-1".to_string()),
        message_id: Some("msg-1".to_string()),
        requested_at: Utc::now(),
    }
}

fn completed_outcome(id: &str) -> ActionRuntimeOutcome {
    ActionRuntimeOutcome::Completed {
        action_id: ActionId(id.to_string()),
        result: ActionResult {
            status: ActionStatus::Completed,
            payload: ActionResultPayload::Text("ok".to_string()),
            summary: "ok".to_string(),
            completed_at: Utc::now(),
        },
    }
}

fn approval_request(action_id: &str) -> ApprovalRequest {
    ApprovalRequest {
        approval_id: format!("approval-{action_id}"),
        action_id: ActionId(action_id.to_string()),
        conversation_id: ConversationId::from("conv-1"),
        requested_by: ParticipantId::from("agent"),
        reason: "needs approval".to_string(),
        requested_at: Utc::now(),
        expires_at: Some(Utc::now() + Duration::minutes(10)),
    }
}

#[tokio::test]
async fn recovery_reloads_agent_run_state_after_restart() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("runs.jsonl");
    let store = JsonlAgentRunStore::open(&path).await.unwrap();
    store
        .insert(run_record("run-1", DurableAgentRunStatus::Queued))
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

    let reloaded = JsonlAgentRunStore::open(&path).await.unwrap();
    let recovered = reloaded.get("run-1").await.unwrap().unwrap();

    assert_eq!(recovered.status, DurableAgentRunStatus::Completed);
}

#[tokio::test]
async fn recovery_requeues_expired_run_queue_lease_after_restart() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("runs.jsonl");
    let store = JsonlAgentRunStore::open(&path).await.unwrap();
    store
        .insert(run_record("run-1", DurableAgentRunStatus::Queued))
        .await
        .unwrap();
    let queue = AgentRunQueue::new(Arc::new(store), StdDuration::from_millis(1));
    let lease = queue.lease().await.unwrap().unwrap();
    assert_eq!(lease.run_id, "run-1");
    tokio::time::sleep(StdDuration::from_millis(5)).await;

    let reloaded_store = Arc::new(JsonlAgentRunStore::open(&path).await.unwrap());
    let recovered_queue = AgentRunQueue::new(reloaded_store, StdDuration::from_millis(1));
    let recovered = recovered_queue.recover_expired_leases().await.unwrap();

    assert_eq!(recovered, vec!["run-1".to_string()]);
    assert_eq!(
        recovered_queue
            .store()
            .get("run-1")
            .await
            .unwrap()
            .unwrap()
            .status,
        DurableAgentRunStatus::Queued
    );
}

#[tokio::test]
async fn recovery_resume_plan_skips_checkpointed_read_only_tool_result() {
    let checkpoints = vec![ToolLoopCheckpoint::tool_result(
        "run-1",
        1,
        ToolResultCheckpoint {
            tool_call_id: "search-1".to_string(),
            action_id: "action-search-1".to_string(),
            result_text: "cached".to_string(),
            read_only: true,
        },
    )];

    let plan = ToolLoopResumePlan::from_checkpoints(checkpoints);

    assert!(plan.should_skip_tool_call("search-1"));
    assert_eq!(plan.completed_tool_result("search-1"), Some("cached"));
}

#[tokio::test]
async fn recovery_reloads_action_outcome_after_restart() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("actions.jsonl");
    let store = JsonlActionStore::open(&path).await.unwrap();
    store
        .insert_request(agent_runtime::ActionRecord::requested(
            action_request("action-1"),
            "audit-1",
            "idem-1",
        ))
        .await
        .unwrap();
    store
        .record_outcome("action-1", completed_outcome("action-1"))
        .await
        .unwrap();

    let reloaded = JsonlActionStore::open(&path).await.unwrap();
    let record = reloaded.get("action-1").await.unwrap().unwrap();

    assert_eq!(record.status, ActionStatus::Completed);
    assert!(matches!(
        record.outcome,
        Some(ActionRuntimeOutcome::Completed { .. })
    ));
}

#[tokio::test]
async fn recovery_reloads_pending_approval_after_restart() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("approvals.jsonl");
    let queue = JsonlApprovalQueue::open(&path).await.unwrap();
    queue.enqueue(approval_request("action-1")).await.unwrap();

    let reloaded = JsonlApprovalQueue::open(&path).await.unwrap();
    let pending = reloaded.pending().await.unwrap();

    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].approval_id, "approval-action-1");
    assert_eq!(pending[0].status, ApprovalStatus::Pending);
}
