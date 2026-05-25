use std::sync::Arc;

use action_core::ActionRegistry;
use action_core::{ActionId, ActionKind, ActionRequest};
use agentos_kernel::{
    HostActionDecisionRequest, HostApiError, HostApiResult, HostRunStatus, KernelHostApi,
    KernelRuntime, KernelRuntimeBuilder, StartAgentRunRequest, SubmitUserMessageRequest,
};
use audit_log::{AuditLog, MemoryAuditSink};
use capability_policy::CapabilityPolicy;
use chrono::Utc;
use conversation_core::{
    ConversationId, ConversationKind, MessageContent, Participant, ParticipantId, ParticipantKind,
    Visibility,
};
use conversation_journal::{ConversationJournal, MemoryConversationJournal};
use conversation_kernel::{CreateConversationCommand, RequireActionApprovalCommand};
use model_adapter::{FakeModelAdapter, ModelAdapter};
use serde_json::json;

fn runtime() -> KernelRuntime {
    let journal: Arc<dyn ConversationJournal> = Arc::new(MemoryConversationJournal::new());
    let model_adapter: Arc<dyn ModelAdapter> = Arc::new(FakeModelAdapter::default());
    let audit_log: Arc<dyn AuditLog> = Arc::new(MemoryAuditSink::new());

    KernelRuntimeBuilder::new()
        .conversation_journal(journal)
        .model_adapter(model_adapter)
        .action_registry(Arc::new(ActionRegistry::new()))
        .capability_policy(Arc::new(CapabilityPolicy::default_safe()))
        .audit_log(audit_log)
        .build()
        .unwrap()
}

async fn conversation_with_user(runtime: &KernelRuntime) -> (ConversationId, ParticipantId) {
    let user_id = ParticipantId("user-1".to_string());
    let conversation_id = runtime
        .services()
        .conversation_kernel
        .create_conversation(CreateConversationCommand {
            kind: ConversationKind::Direct,
            title: Some("Host API test".to_string()),
            participants: vec![Participant {
                id: user_id.clone(),
                kind: ParticipantKind::Human,
                display_name: "User".to_string(),
            }],
            actor_id: Some(user_id.clone()),
        })
        .await
        .unwrap();

    (conversation_id, user_id)
}

#[tokio::test]
async fn host_api_submits_user_message_without_exposing_kernel_commands() {
    let runtime = runtime();
    let api = KernelHostApi::new(runtime.clone());
    let (conversation_id, user_id) = conversation_with_user(&runtime).await;

    let result = api
        .submit_user_message(SubmitUserMessageRequest {
            conversation_id: conversation_id.clone(),
            user_id: user_id.clone(),
            text: "hello host api".to_string(),
        })
        .await
        .unwrap();

    let state = runtime
        .services()
        .conversation_kernel
        .load_state(&conversation_id)
        .await
        .unwrap();
    let message = state.messages_by_id.get(&result.message_id).unwrap();

    assert_eq!(message.sender_id, user_id);
    assert_eq!(
        message.content,
        MessageContent::Text {
            text: "hello host api".to_string(),
        }
    );
    assert_eq!(message.visibility, Visibility::Conversation);
}

#[tokio::test]
async fn host_api_starts_agent_run_and_reports_status() {
    let runtime = runtime();
    let api = KernelHostApi::new(runtime.clone());
    let (conversation_id, user_id) = conversation_with_user(&runtime).await;
    let message = api
        .submit_user_message(SubmitUserMessageRequest {
            conversation_id: conversation_id.clone(),
            user_id: user_id.clone(),
            text: "please help".to_string(),
        })
        .await
        .unwrap();

    let run = api
        .start_agent_run(StartAgentRunRequest {
            conversation_id: conversation_id.clone(),
            trigger_message_id: message.message_id,
            requested_by: user_id.clone(),
        })
        .await
        .unwrap();

    assert_eq!(run.status, HostRunStatus::Running);
    assert_eq!(
        api.get_run_status(conversation_id, run.run_id.clone())
            .await
            .unwrap()
            .status,
        HostRunStatus::Running
    );
}

#[tokio::test]
async fn host_api_lists_and_decides_pending_approvals() {
    let runtime = runtime();
    let api = KernelHostApi::new(runtime.clone());
    let (conversation_id, user_id) = conversation_with_user(&runtime).await;
    let action_id = ActionId("action-1".to_string());

    runtime
        .services()
        .conversation_kernel
        .request_action(conversation_kernel::RequestActionCommand {
            conversation_id: conversation_id.clone(),
            action_request: ActionRequest {
                action_id: action_id.clone(),
                action_kind: ActionKind("email.send".to_string()),
                input: json!({"to":"a@example.com"}),
                requested_by: user_id.0.clone(),
                conversation_id: Some(conversation_id.0.clone()),
                message_id: None,
                requested_at: Utc::now(),
            },
            requested_by: Some(user_id.clone()),
        })
        .await
        .unwrap();
    runtime
        .services()
        .conversation_kernel
        .require_action_approval(RequireActionApprovalCommand {
            conversation_id: conversation_id.clone(),
            action_id: action_id.clone(),
            reason: "external side effect".to_string(),
            required_by: Some(user_id.clone()),
        })
        .await
        .unwrap();

    let pending = api
        .list_pending_approvals(conversation_id.clone())
        .await
        .unwrap();
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].action_id, action_id);

    api.approve_action(HostActionDecisionRequest {
        conversation_id: conversation_id.clone(),
        action_id: action_id.clone(),
        decided_by: user_id,
        reason: None,
    })
    .await
    .unwrap();

    let pending = api.list_pending_approvals(conversation_id).await.unwrap();
    assert!(pending.is_empty());
}

#[tokio::test]
async fn host_api_exposes_stable_error_type() {
    let runtime = runtime();
    let api = KernelHostApi::new(runtime);

    let result: HostApiResult<_> = api
        .get_run_status(
            ConversationId("missing-conversation".to_string()),
            "missing-run".to_string(),
        )
        .await;

    assert!(matches!(
        result,
        Err(HostApiError::RunNotFound { run_id }) if run_id == "missing-run"
    ));
}

#[tokio::test]
async fn host_api_shutdown_delegates_to_kernel_runtime() {
    let runtime = runtime();
    let api = KernelHostApi::new(runtime.clone());

    api.shutdown().await.unwrap();

    assert_eq!(runtime.state().as_str(), "shutdown");
}
