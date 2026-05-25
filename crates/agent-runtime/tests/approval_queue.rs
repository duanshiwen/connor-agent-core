use action_core::ActionId;
use agent_runtime::{
    ApprovalDecision, ApprovalQueue, ApprovalQueueError, ApprovalRequest, ApprovalStatus,
    JsonlApprovalQueue, MemoryApprovalQueue,
};
use chrono::{Duration, Utc};
use conversation_core::{ConversationId, ParticipantId};

fn request(id: &str) -> ApprovalRequest {
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

#[tokio::test]
async fn memory_approval_queue_enqueues_and_approves_pending_request() {
    let queue = MemoryApprovalQueue::new();
    queue.enqueue(request("action-1")).await.unwrap();

    let pending = queue.pending().await.unwrap();
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].status, ApprovalStatus::Pending);

    let decided = queue
        .approve(
            "approval-action-1",
            ApprovalDecision::approved(ParticipantId::from("user-1"), "looks safe"),
        )
        .await
        .unwrap();

    assert_eq!(decided.status, ApprovalStatus::Approved);
    assert_eq!(decided.decision.unwrap().reason, "looks safe");
    assert!(queue.pending().await.unwrap().is_empty());
}

#[tokio::test]
async fn jsonl_approval_queue_reloads_pending_approval_after_restart() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("approvals.jsonl");
    let queue = JsonlApprovalQueue::open(&path).await.unwrap();
    queue.enqueue(request("action-1")).await.unwrap();

    let reloaded = JsonlApprovalQueue::open(&path).await.unwrap();
    let pending = reloaded.pending().await.unwrap();

    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].approval_id, "approval-action-1");
    assert_eq!(pending[0].status, ApprovalStatus::Pending);
}

#[tokio::test]
async fn expired_approval_cannot_be_approved() {
    let queue = MemoryApprovalQueue::new();
    let mut req = request("action-1");
    req.expires_at = Some(Utc::now() - Duration::seconds(1));
    queue.enqueue(req).await.unwrap();

    let err = queue
        .approve(
            "approval-action-1",
            ApprovalDecision::approved(ParticipantId::from("user-1"), "too late"),
        )
        .await
        .unwrap_err();

    assert!(matches!(
        err,
        ApprovalQueueError::ApprovalExpired { approval_id } if approval_id == "approval-action-1"
    ));
    assert_eq!(
        queue
            .get("approval-action-1")
            .await
            .unwrap()
            .unwrap()
            .status,
        ApprovalStatus::Expired
    );
}

#[tokio::test]
async fn approval_can_be_denied_with_reason() {
    let queue = MemoryApprovalQueue::new();
    queue.enqueue(request("action-1")).await.unwrap();

    let decided = queue
        .deny(
            "approval-action-1",
            ApprovalDecision::denied(ParticipantId::from("user-1"), "unsafe"),
        )
        .await
        .unwrap();

    assert_eq!(decided.status, ApprovalStatus::Denied);
    assert_eq!(decided.decision.unwrap().reason, "unsafe");
}

#[tokio::test]
async fn pending_approval_can_be_revoked() {
    let queue = MemoryApprovalQueue::new();
    queue.enqueue(request("action-1")).await.unwrap();

    let revoked = queue
        .revoke("approval-action-1", "superseded by newer request")
        .await
        .unwrap();

    assert_eq!(revoked.status, ApprovalStatus::Revoked);
    assert_eq!(
        revoked.decision.unwrap().reason,
        "superseded by newer request"
    );
}
