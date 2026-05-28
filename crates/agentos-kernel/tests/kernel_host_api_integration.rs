use std::sync::{Arc, Mutex};

use action_core::{
    ActionId, ActionKind, ActionRegistry, ActionRequest, ActionSchema, SideEffectKind,
    StaticActionExecutor,
};
use action_runtime::ActionRuntimeOutcome;
use agentos_kernel::{
    HostActionDecisionRequest, HostActorContext, HostApiError, HostExecuteApprovedActionRequest,
    HostProcessActionRequest, HostRunStatus, KernelHostApi, KernelRuntime, KernelRuntimeBuilder,
    StartAgentRunRequest, SubmitUserMessageRequest,
};
use audit_log::{AuditLog, MemoryAuditSink};
use capability_policy::CapabilityPolicy;
use chrono::Utc;
use conversation_core::{
    ConversationId, ConversationKind, Participant, ParticipantId, ParticipantKind,
};
use conversation_journal::{ConversationJournal, MemoryConversationJournal};
use conversation_kernel::CreateConversationCommand;
use enterprise_permission_core::{
    EnterpriseRole, EnterpriseUserId, PermissionAction, PermissionStore, ResourceId, ResourceType,
};
use model_adapter::{ModelAdapter, StaticModelAdapter};

fn registry() -> Arc<ActionRegistry> {
    let mut registry = ActionRegistry::new();
    registry
        .register(ActionSchema {
            kind: ActionKind::from("knowledge.search"),
            display_name: "Knowledge Search".to_string(),
            description: "Read-only knowledge lookup".to_string(),
            side_effect: SideEffectKind::ReadOnly,
            input_schema: None,
            output_schema: None,
        })
        .unwrap();
    registry
        .register(ActionSchema {
            kind: ActionKind::from("mail.send"),
            display_name: "Send Mail".to_string(),
            description: "Network mail send side effect".to_string(),
            side_effect: SideEffectKind::NetworkAccess,
            input_schema: None,
            output_schema: None,
        })
        .unwrap();
    Arc::new(registry)
}

fn runtime(permission_store: Option<PermissionStore>) -> KernelRuntime {
    let journal: Arc<dyn ConversationJournal> = Arc::new(MemoryConversationJournal::new());
    let model_adapter: Arc<dyn ModelAdapter> = Arc::new(StaticModelAdapter::default());
    let audit_log: Arc<dyn AuditLog> = Arc::new(MemoryAuditSink::new());
    let mut builder = KernelRuntimeBuilder::new()
        .conversation_journal(journal)
        .model_adapter(model_adapter)
        .action_registry(registry())
        .capability_policy(Arc::new(CapabilityPolicy::default_safe()))
        .audit_log(audit_log)
        .action_executor(Arc::new(StaticActionExecutor::new("host api integration")));

    if let Some(permission_store) = permission_store {
        builder = builder.permission_store(Arc::new(Mutex::new(permission_store)));
    }

    builder.build().unwrap()
}

async fn conversation_with_user(runtime: &KernelRuntime) -> (ConversationId, ParticipantId) {
    let user_id = ParticipantId::from("integration-user");
    let conversation_id = runtime
        .services()
        .conversation_kernel
        .create_conversation(CreateConversationCommand {
            kind: ConversationKind::Direct,
            title: Some("KernelHostApi integration".to_string()),
            participants: vec![Participant {
                id: user_id.clone(),
                kind: ParticipantKind::Human,
                display_name: "Integration User".to_string(),
            }],
            actor_id: Some(user_id.clone()),
        })
        .await
        .unwrap();

    (conversation_id, user_id)
}

fn action_request(
    action_id: impl Into<String>,
    action_kind: impl Into<String>,
    conversation_id: &ConversationId,
    requested_by: &ParticipantId,
) -> ActionRequest {
    ActionRequest {
        action_id: ActionId::from(action_id.into()),
        action_kind: ActionKind::from(action_kind.into()),
        input: serde_json::json!({"fixture": true}),
        requested_by: requested_by.to_string(),
        conversation_id: Some(conversation_id.to_string()),
        message_id: None,
        requested_at: Utc::now(),
    }
}

fn actor_context(user_id: ParticipantId) -> HostActorContext {
    HostActorContext {
        user_id: user_id.clone(),
        enterprise_user_id: EnterpriseUserId(user_id.to_string()),
        role: EnterpriseRole::User,
    }
}

#[tokio::test]
async fn kernel_host_api_covers_message_run_action_approval_and_execute_paths() {
    let runtime = runtime(None);
    let api = KernelHostApi::new(runtime.clone());
    let (conversation_id, user_id) = conversation_with_user(&runtime).await;

    let message = api
        .submit_user_message(SubmitUserMessageRequest {
            conversation_id: conversation_id.clone(),
            user_id: user_id.clone(),
            text: "search my knowledge then send a summary".to_string(),
            actor_context: None,
        })
        .await
        .unwrap();

    let run = api
        .start_agent_run(StartAgentRunRequest {
            conversation_id: conversation_id.clone(),
            trigger_message_id: message.message_id,
            requested_by: user_id.clone(),
            actor_context: None,
        })
        .await
        .unwrap();
    assert_eq!(run.status, HostRunStatus::Running);
    assert_eq!(
        api.get_run_status(conversation_id.clone(), run.run_id)
            .await
            .unwrap()
            .status,
        HostRunStatus::Running
    );

    let read_outcome = api
        .process_action(HostProcessActionRequest {
            conversation_id: conversation_id.clone(),
            action_request: action_request(
                "read-action",
                "knowledge.search",
                &conversation_id,
                &user_id,
            ),
            requested_by: Some(user_id.clone()),
            runtime_actor: Some(user_id.clone()),
            actor_context: None,
        })
        .await
        .unwrap();
    assert!(matches!(
        read_outcome,
        ActionRuntimeOutcome::Completed { .. }
    ));

    let send_outcome = api
        .process_action(HostProcessActionRequest {
            conversation_id: conversation_id.clone(),
            action_request: action_request("send-action", "mail.send", &conversation_id, &user_id),
            requested_by: Some(user_id.clone()),
            runtime_actor: Some(user_id.clone()),
            actor_context: None,
        })
        .await
        .unwrap();
    assert!(matches!(
        send_outcome,
        ActionRuntimeOutcome::ApprovalRequired { .. }
    ));

    let pending = api
        .list_pending_approvals(conversation_id.clone())
        .await
        .unwrap();
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].action_id, ActionId::from("send-action"));

    api.approve_action(HostActionDecisionRequest {
        conversation_id: conversation_id.clone(),
        action_id: ActionId::from("send-action"),
        decided_by: user_id.clone(),
        reason: None,
        actor_context: None,
    })
    .await
    .unwrap();

    let approved_outcome = api
        .execute_approved_action(HostExecuteApprovedActionRequest {
            conversation_id: conversation_id.clone(),
            action_id: ActionId::from("send-action"),
            runtime_actor: Some(user_id),
            actor_context: None,
        })
        .await
        .unwrap();
    assert!(matches!(
        approved_outcome,
        ActionRuntimeOutcome::Completed { .. }
    ));

    assert!(
        api.list_pending_approvals(conversation_id)
            .await
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn kernel_host_api_permission_gate_applies_to_action_processing() {
    let runtime = runtime(Some(PermissionStore::new()));
    let api = KernelHostApi::new(runtime.clone());
    let (conversation_id, user_id) = conversation_with_user(&runtime).await;

    let result = api
        .process_action(HostProcessActionRequest {
            conversation_id: conversation_id.clone(),
            action_request: action_request(
                "blocked-read",
                "knowledge.search",
                &conversation_id,
                &user_id,
            ),
            requested_by: Some(user_id.clone()),
            runtime_actor: Some(user_id.clone()),
            actor_context: Some(actor_context(user_id)),
        })
        .await;

    assert!(matches!(
        result,
        Err(HostApiError::PermissionDenied {
            resource_type,
            resource_id,
            action,
            ..
        }) if resource_type == ResourceType::Conversation.to_string()
            && resource_id == ResourceId(conversation_id.to_string()).to_string()
            && action == PermissionAction::Write.to_string()
    ));
}
