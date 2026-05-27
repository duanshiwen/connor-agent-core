use std::sync::Arc;

use action_core::{
    ActionId, ActionKind, ActionRegistry, ActionRequest, ActionSchema, FakeActionExecutor,
    SideEffectKind,
};
use action_runtime::ActionRuntimeOutcome;
use agentos_kernel::{HostProcessActionRequest, KernelHostApi, KernelRuntimeBuilder};
use audit_log::{AuditLog, MemoryAuditSink};
use capability_policy::CapabilityPolicy;
use chrono::Utc;
use conversation_core::{
    ConversationId, ConversationKind, Participant, ParticipantId, ParticipantKind,
};
use conversation_journal::{ConversationJournal, MemoryConversationJournal};
use conversation_kernel::CreateConversationCommand;
use model_adapter::{FakeModelAdapter, ModelAdapter};

fn conversation_journal() -> Arc<dyn ConversationJournal> {
    Arc::new(MemoryConversationJournal::new())
}

fn model_adapter() -> Arc<dyn ModelAdapter> {
    Arc::new(FakeModelAdapter::default())
}

fn audit_log() -> Arc<dyn AuditLog> {
    Arc::new(MemoryAuditSink::new())
}

fn registry_with_read_only_action() -> Arc<ActionRegistry> {
    let mut registry = ActionRegistry::new();
    registry
        .register(ActionSchema {
            kind: ActionKind::from("test.read"),
            display_name: "Test Read".to_string(),
            description: "A deterministic read-only test action".to_string(),
            side_effect: SideEffectKind::ReadOnly,
            input_schema: None,
            output_schema: None,
        })
        .unwrap();
    Arc::new(registry)
}

fn runtime_with_action_service() -> agentos_kernel::KernelRuntime {
    KernelRuntimeBuilder::new()
        .conversation_journal(conversation_journal())
        .model_adapter(model_adapter())
        .action_registry(registry_with_read_only_action())
        .capability_policy(Arc::new(CapabilityPolicy::default_safe()))
        .audit_log(audit_log())
        .action_executor(Arc::new(FakeActionExecutor::new(
            "from kernel action service",
        )))
        .build()
        .unwrap()
}

#[test]
fn builder_composes_kernel_owned_action_runtime_when_executor_is_configured() {
    let runtime = runtime_with_action_service();

    assert!(runtime.services().action_runtime.is_some());
}

#[tokio::test]
async fn host_api_processes_action_through_kernel_owned_action_runtime() {
    let runtime = runtime_with_action_service();
    let api = KernelHostApi::new(runtime.clone());
    let user = ParticipantId::from("user-1");
    let conversation_id = runtime
        .services()
        .conversation_kernel
        .create_conversation(CreateConversationCommand {
            kind: ConversationKind::Direct,
            title: Some("Action runtime service test".to_string()),
            participants: vec![Participant {
                id: user.clone(),
                kind: ParticipantKind::Human,
                display_name: "User".to_string(),
            }],
            actor_id: Some(user.clone()),
        })
        .await
        .unwrap();

    api.submit_user_message(agentos_kernel::SubmitUserMessageRequest {
        conversation_id: conversation_id.clone(),
        user_id: user.clone(),
        text: "please run the action".to_string(),
        actor_context: None,
    })
    .await
    .unwrap();

    let outcome = api
        .process_action(HostProcessActionRequest {
            conversation_id: conversation_id.clone(),
            action_request: ActionRequest {
                action_id: ActionId::from("action-1"),
                action_kind: ActionKind::from("test.read"),
                input: serde_json::json!({"query": "hello"}),
                requested_by: user.to_string(),
                conversation_id: Some(conversation_id.to_string()),
                message_id: None,
                requested_at: Utc::now(),
            },
            requested_by: Some(user.clone()),
            runtime_actor: Some(user),
            actor_context: None,
        })
        .await
        .unwrap();

    let ActionRuntimeOutcome::Completed { result, .. } = outcome else {
        panic!("expected completed action outcome");
    };
    assert_eq!(result.summary, "test.read completed");
}

#[tokio::test]
async fn host_api_reports_missing_action_runtime_when_executor_is_not_configured() {
    let runtime = KernelRuntimeBuilder::new()
        .conversation_journal(conversation_journal())
        .model_adapter(model_adapter())
        .action_registry(registry_with_read_only_action())
        .capability_policy(Arc::new(CapabilityPolicy::default_safe()))
        .audit_log(audit_log())
        .build()
        .unwrap();
    let api = KernelHostApi::new(runtime);

    let err = api
        .process_action(HostProcessActionRequest {
            conversation_id: ConversationId::from("conversation-missing-action-runtime"),
            action_request: ActionRequest {
                action_id: ActionId::from("action-1"),
                action_kind: ActionKind::from("test.read"),
                input: serde_json::json!({}),
                requested_by: "user-1".to_string(),
                conversation_id: None,
                message_id: None,
                requested_at: Utc::now(),
            },
            requested_by: None,
            runtime_actor: None,
            actor_context: None,
        })
        .await
        .unwrap_err();

    assert_eq!(err.code(), "action_runtime_unavailable");
}
