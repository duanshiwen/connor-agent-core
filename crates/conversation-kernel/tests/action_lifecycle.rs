use action_core::{
    ActionId, ActionKind, ActionRequest, ActionResult, ActionResultPayload, ActionStatus,
};
use chrono::{DateTime, Utc};
use conversation_core::*;
use conversation_journal::MemoryConversationJournal;
use conversation_kernel::*;
use std::sync::Arc;

struct SequentialIdGenerator {
    counter: std::sync::Mutex<u64>,
}

impl SequentialIdGenerator {
    fn new() -> Self {
        Self {
            counter: std::sync::Mutex::new(0),
        }
    }
}

impl IdGenerator for SequentialIdGenerator {
    fn new_id(&self) -> String {
        let mut counter = self.counter.lock().unwrap();
        *counter += 1;
        format!("id-{counter}")
    }
}

struct FixedClock {
    time: DateTime<Utc>,
}

impl FixedClock {
    fn new(time: DateTime<Utc>) -> Self {
        Self { time }
    }
}

impl Clock for FixedClock {
    fn now(&self) -> DateTime<Utc> {
        self.time
    }
}

fn test_kernel() -> ConversationKernel {
    let journal = Arc::new(MemoryConversationJournal::new());
    ConversationKernel::with_generators(
        journal,
        Arc::new(SequentialIdGenerator::new()),
        Arc::new(FixedClock::new(Utc::now())),
    )
}

fn user() -> Participant {
    Participant {
        id: ParticipantId::from("user-1"),
        kind: ParticipantKind::Human,
        display_name: "User".to_string(),
    }
}

fn agent() -> Participant {
    Participant {
        id: ParticipantId::from("agent-1"),
        kind: ParticipantKind::Agent,
        display_name: "Assistant".to_string(),
    }
}

async fn create_conversation(kernel: &ConversationKernel) -> ConversationId {
    kernel
        .create_conversation(CreateConversationCommand {
            kind: ConversationKind::AgentTask,
            title: Some("Action lifecycle".to_string()),
            participants: vec![user(), agent()],
            actor_id: Some(ParticipantId::from("user-1")),
        })
        .await
        .unwrap()
}

fn action_request(conversation_id: &ConversationId, action_id: &str, kind: &str) -> ActionRequest {
    ActionRequest {
        action_id: ActionId::from(action_id),
        action_kind: ActionKind::from(kind),
        input: serde_json::json!({"query": "agent os"}),
        requested_by: "user-1".to_string(),
        conversation_id: Some(conversation_id.to_string()),
        message_id: None,
        requested_at: Utc::now(),
    }
}

fn action_result() -> ActionResult {
    ActionResult {
        status: ActionStatus::Completed,
        payload: ActionResultPayload::Text("done".to_string()),
        summary: "Action completed".to_string(),
        completed_at: Utc::now(),
    }
}

async fn request_action(kernel: &ConversationKernel, conversation_id: &ConversationId) {
    kernel
        .request_action(RequestActionCommand {
            conversation_id: conversation_id.clone(),
            action_request: action_request(conversation_id, "action-1", "knowledge.search"),
            requested_by: Some(ParticipantId::from("user-1")),
        })
        .await
        .unwrap();
}

#[tokio::test]
async fn action_lifecycle_projects_requested_to_completed() {
    let kernel = test_kernel();
    let conversation_id = create_conversation(&kernel).await;
    request_action(&kernel, &conversation_id).await;

    kernel
        .start_action(StartActionCommand {
            conversation_id: conversation_id.clone(),
            action_id: ActionId::from("action-1"),
            started_by: Some(ParticipantId::from("agent-1")),
        })
        .await
        .unwrap();

    kernel
        .complete_action(CompleteActionCommand {
            conversation_id: conversation_id.clone(),
            action_id: ActionId::from("action-1"),
            result: action_result(),
            completed_by: Some(ParticipantId::from("agent-1")),
        })
        .await
        .unwrap();

    let state = kernel.load_state(&conversation_id).await.unwrap();
    let action = state.actions.get(&ActionId::from("action-1")).unwrap();
    assert_eq!(action.status, ConversationActionStatus::Completed);
    assert_eq!(action.action_kind, ActionKind::from("knowledge.search"));
    assert!(action.result.is_some());
}

#[tokio::test]
async fn action_requires_approval_then_approved_can_start_and_complete() {
    let kernel = test_kernel();
    let conversation_id = create_conversation(&kernel).await;
    request_action(&kernel, &conversation_id).await;

    kernel
        .require_action_approval(RequireActionApprovalCommand {
            conversation_id: conversation_id.clone(),
            action_id: ActionId::from("action-1"),
            reason: "write action requires approval".to_string(),
            required_by: Some(ParticipantId::from("agent-1")),
        })
        .await
        .unwrap();

    kernel
        .approve_action(ApproveActionCommand {
            conversation_id: conversation_id.clone(),
            action_id: ActionId::from("action-1"),
            approved_by: ParticipantId::from("user-1"),
        })
        .await
        .unwrap();

    kernel
        .start_action(StartActionCommand {
            conversation_id: conversation_id.clone(),
            action_id: ActionId::from("action-1"),
            started_by: Some(ParticipantId::from("agent-1")),
        })
        .await
        .unwrap();

    kernel
        .complete_action(CompleteActionCommand {
            conversation_id: conversation_id.clone(),
            action_id: ActionId::from("action-1"),
            result: action_result(),
            completed_by: Some(ParticipantId::from("agent-1")),
        })
        .await
        .unwrap();

    let state = kernel.load_state(&conversation_id).await.unwrap();
    let action = state.actions.get(&ActionId::from("action-1")).unwrap();
    assert_eq!(action.status, ConversationActionStatus::Completed);
    assert_eq!(action.approved_by, Some(ParticipantId::from("user-1")));
    assert_eq!(
        action.approval_required_reason.as_deref(),
        Some("write action requires approval")
    );
}

#[tokio::test]
async fn denied_action_is_terminal() {
    let kernel = test_kernel();
    let conversation_id = create_conversation(&kernel).await;
    request_action(&kernel, &conversation_id).await;

    kernel
        .deny_action(DenyActionCommand {
            conversation_id: conversation_id.clone(),
            action_id: ActionId::from("action-1"),
            reason: "denied by user".to_string(),
            denied_by: Some(ParticipantId::from("user-1")),
        })
        .await
        .unwrap();

    let err = kernel
        .start_action(StartActionCommand {
            conversation_id: conversation_id.clone(),
            action_id: ActionId::from("action-1"),
            started_by: Some(ParticipantId::from("agent-1")),
        })
        .await
        .unwrap_err();
    assert!(err.to_string().contains("action already terminal"));

    let state = kernel.load_state(&conversation_id).await.unwrap();
    let action = state.actions.get(&ActionId::from("action-1")).unwrap();
    assert_eq!(action.status, ConversationActionStatus::Denied);
    assert_eq!(action.denial_reason.as_deref(), Some("denied by user"));
}

#[tokio::test]
async fn approval_required_action_cannot_start_before_approval() {
    let kernel = test_kernel();
    let conversation_id = create_conversation(&kernel).await;
    request_action(&kernel, &conversation_id).await;

    kernel
        .require_action_approval(RequireActionApprovalCommand {
            conversation_id: conversation_id.clone(),
            action_id: ActionId::from("action-1"),
            reason: "needs approval".to_string(),
            required_by: Some(ParticipantId::from("agent-1")),
        })
        .await
        .unwrap();

    let err = kernel
        .start_action(StartActionCommand {
            conversation_id,
            action_id: ActionId::from("action-1"),
            started_by: Some(ParticipantId::from("agent-1")),
        })
        .await
        .unwrap_err();
    assert!(err.to_string().contains("requires approval"));
}

#[tokio::test]
async fn complete_requires_started_action() {
    let kernel = test_kernel();
    let conversation_id = create_conversation(&kernel).await;
    request_action(&kernel, &conversation_id).await;

    let err = kernel
        .complete_action(CompleteActionCommand {
            conversation_id,
            action_id: ActionId::from("action-1"),
            result: action_result(),
            completed_by: Some(ParticipantId::from("agent-1")),
        })
        .await
        .unwrap_err();
    assert!(err.to_string().contains("action is not started"));
}

#[tokio::test]
async fn failed_action_records_error() {
    let kernel = test_kernel();
    let conversation_id = create_conversation(&kernel).await;
    request_action(&kernel, &conversation_id).await;

    kernel
        .start_action(StartActionCommand {
            conversation_id: conversation_id.clone(),
            action_id: ActionId::from("action-1"),
            started_by: Some(ParticipantId::from("agent-1")),
        })
        .await
        .unwrap();

    kernel
        .fail_action(FailActionCommand {
            conversation_id: conversation_id.clone(),
            action_id: ActionId::from("action-1"),
            error_message: "executor failed".to_string(),
            failed_by: Some(ParticipantId::from("agent-1")),
        })
        .await
        .unwrap();

    let state = kernel.load_state(&conversation_id).await.unwrap();
    let action = state.actions.get(&ActionId::from("action-1")).unwrap();
    assert_eq!(action.status, ConversationActionStatus::Failed);
    assert_eq!(action.error_message.as_deref(), Some("executor failed"));
    assert!(action.result.is_none());
}

#[tokio::test]
async fn action_projection_is_deterministic() {
    let kernel = test_kernel();
    let conversation_id = create_conversation(&kernel).await;
    request_action(&kernel, &conversation_id).await;

    kernel
        .start_action(StartActionCommand {
            conversation_id: conversation_id.clone(),
            action_id: ActionId::from("action-1"),
            started_by: Some(ParticipantId::from("agent-1")),
        })
        .await
        .unwrap();

    let events = kernel.load_events(&conversation_id).await.unwrap();
    let state1 = ConversationProjector::project(&events).unwrap();
    let state2 = ConversationProjector::project(&events).unwrap();

    let action1 = state1.actions.get(&ActionId::from("action-1")).unwrap();
    let action2 = state2.actions.get(&ActionId::from("action-1")).unwrap();
    assert_eq!(action1.status, action2.status);
    assert_eq!(action1.action_id, action2.action_id);
    assert_eq!(action1.action_kind, action2.action_kind);
}
