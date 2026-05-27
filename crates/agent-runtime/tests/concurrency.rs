use std::collections::BTreeSet;
use std::sync::Arc;
use std::time::Duration as StdDuration;

use action_core::ActionId;
use agent_runtime::{
    AgentRunQueue, AgentRunRecord, AgentRunStore, ApprovalDecision, ApprovalQueue,
    ApprovalQueueError, ApprovalRequest, ApprovalStatus, DurableAgentRunStatus,
    MemoryAgentRunStore, MemoryApprovalQueue,
};
use chrono::{Duration, Utc};
use conversation_core::{ConversationId, MessageId, ParticipantId};
use tokio::task::JoinSet;

fn run_record(run_id: &str) -> AgentRunRecord {
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

fn approval_request(id: &str) -> ApprovalRequest {
    ApprovalRequest {
        approval_id: format!("approval-{id}"),
        action_id: ActionId(id.to_string()),
        conversation_id: ConversationId::from("conv-1"),
        requested_by: ParticipantId::from("agent"),
        reason: "write action requires approval".to_string(),
        requested_at: Utc::now(),
        expires_at: Some(Utc::now() + Duration::minutes(5)),
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_agent_run_leases_are_unique_and_all_acknowledge() {
    let store = Arc::new(MemoryAgentRunStore::new());
    let queue = Arc::new(AgentRunQueue::new(
        store.clone(),
        StdDuration::from_secs(30),
    ));

    for idx in 0..32 {
        queue
            .enqueue(run_record(&format!("run-{idx:02}")))
            .await
            .unwrap();
    }

    let mut workers = JoinSet::new();
    for _ in 0..32 {
        let queue = queue.clone();
        workers.spawn(async move {
            let lease = queue.lease().await.unwrap().expect("run should be leased");
            queue.ack(&lease.run_id).await.unwrap();
            lease.run_id
        });
    }

    let mut leased = BTreeSet::new();
    while let Some(result) = workers.join_next().await {
        assert!(
            leased.insert(result.unwrap()),
            "run was leased more than once"
        );
    }

    assert_eq!(leased.len(), 32);
    assert!(queue.lease().await.unwrap().is_none());
    assert!(queue.active_leases().await.unwrap().is_empty());

    for run_id in leased {
        assert_eq!(
            store.get(&run_id).await.unwrap().unwrap().status,
            DurableAgentRunStatus::Completed
        );
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_approval_decisions_allow_only_one_terminal_transition() {
    let queue = Arc::new(MemoryApprovalQueue::new());
    queue.enqueue(approval_request("action-1")).await.unwrap();

    let mut workers = JoinSet::new();
    for idx in 0..16 {
        let queue = queue.clone();
        workers.spawn(async move {
            let decision = ApprovalDecision::approved(
                ParticipantId::from(format!("user-{idx}")),
                format!("decision-{idx}"),
            );
            queue.approve("approval-action-1", decision).await
        });
    }

    let mut approved = 0;
    let mut already_decided = 0;
    while let Some(result) = workers.join_next().await {
        match result.unwrap() {
            Ok(record) => {
                approved += 1;
                assert_eq!(record.status, ApprovalStatus::Approved);
            }
            Err(ApprovalQueueError::AlreadyDecided { approval_id }) => {
                already_decided += 1;
                assert_eq!(approval_id, "approval-action-1");
            }
            Err(err) => panic!("unexpected approval queue error: {err}"),
        }
    }

    assert_eq!(approved, 1);
    assert_eq!(already_decided, 15);
    assert_eq!(
        queue
            .get("approval-action-1")
            .await
            .unwrap()
            .unwrap()
            .status,
        ApprovalStatus::Approved
    );
    assert!(queue.pending().await.unwrap().is_empty());
}
