use std::sync::{Arc, Mutex};

use action_core::ActionRegistry;
use action_core::{ActionId, ActionKind, ActionRequest};
use agentos_kernel::{
    HostActionDecisionRequest, HostActorContext, HostApiError, HostApiResult, HostRunStatus,
    KernelHostApi, KernelRuntime, KernelRuntimeBuilder, StartAgentRunRequest,
    SubmitUserMessageRequest,
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
use enterprise_permission_core::{
    EnterpriseRole, EnterpriseUserId, PermissionAction, PermissionGrant, PermissionStore,
    ResourceId, ResourceType,
};
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

fn runtime_with_permission_store(permission_store: PermissionStore) -> KernelRuntime {
    let journal: Arc<dyn ConversationJournal> = Arc::new(MemoryConversationJournal::new());
    let model_adapter: Arc<dyn ModelAdapter> = Arc::new(FakeModelAdapter::default());
    let audit_log: Arc<dyn AuditLog> = Arc::new(MemoryAuditSink::new());

    KernelRuntimeBuilder::new()
        .conversation_journal(journal)
        .model_adapter(model_adapter)
        .action_registry(Arc::new(ActionRegistry::new()))
        .capability_policy(Arc::new(CapabilityPolicy::default_safe()))
        .audit_log(audit_log)
        .permission_store(Arc::new(Mutex::new(permission_store)))
        .build()
        .unwrap()
}

fn actor_context(user_id: ParticipantId, role: EnterpriseRole) -> HostActorContext {
    HostActorContext {
        enterprise_user_id: EnterpriseUserId(user_id.0.clone()),
        user_id,
        role,
    }
}

fn grant_for(
    grant_id: &str,
    user_id: &ParticipantId,
    conversation_id: &ConversationId,
    actions: Vec<PermissionAction>,
) -> PermissionGrant {
    PermissionGrant {
        grant_id: grant_id.to_string(),
        user_id: EnterpriseUserId(user_id.0.clone()),
        role: EnterpriseRole::User,
        resource_type: ResourceType::Conversation,
        resource_id: ResourceId(conversation_id.0.clone()),
        actions,
        granted_at: Utc::now(),
        expires_at: None,
        revoked: false,
    }
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
            actor_context: None,
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
        actor_context: None,
    })
    .await
    .unwrap();

    let pending = api.list_pending_approvals(conversation_id).await.unwrap();
    assert!(pending.is_empty());
}

#[test]
fn builder_wires_optional_permission_store() {
    let runtime = runtime_with_permission_store(PermissionStore::new());

    assert!(runtime.services().permission_store.is_some());
    assert!(runtime.health_check().permission_store_available);
}

#[tokio::test]
async fn host_api_submit_user_message_allows_actor_with_conversation_write_grant() {
    let runtime = runtime_with_permission_store(PermissionStore::new());
    let api = KernelHostApi::new(runtime.clone());
    let (conversation_id, user_id) = conversation_with_user(&runtime).await;
    runtime
        .services()
        .permission_store
        .as_ref()
        .unwrap()
        .lock()
        .unwrap()
        .add_grant(grant_for(
            "grant-write",
            &user_id,
            &conversation_id,
            vec![PermissionAction::Write],
        ));

    let result = api
        .submit_user_message(SubmitUserMessageRequest {
            conversation_id: conversation_id.clone(),
            user_id: user_id.clone(),
            text: "allowed".to_string(),
            actor_context: Some(actor_context(user_id, EnterpriseRole::User)),
        })
        .await
        .unwrap();

    let state = runtime
        .services()
        .conversation_kernel
        .load_state(&conversation_id)
        .await
        .unwrap();
    assert!(state.messages_by_id.contains_key(&result.message_id));
}

#[tokio::test]
async fn host_api_submit_user_message_denies_actor_without_conversation_write_grant() {
    let runtime = runtime_with_permission_store(PermissionStore::new());
    let api = KernelHostApi::new(runtime.clone());
    let (conversation_id, user_id) = conversation_with_user(&runtime).await;

    let result = api
        .submit_user_message(SubmitUserMessageRequest {
            conversation_id: conversation_id.clone(),
            user_id: user_id.clone(),
            text: "blocked".to_string(),
            actor_context: Some(actor_context(user_id, EnterpriseRole::User)),
        })
        .await;

    assert!(matches!(
        result,
        Err(HostApiError::PermissionDenied { resource_id, action, .. })
            if resource_id == conversation_id.0 && action == "write"
    ));
    let state = runtime
        .services()
        .conversation_kernel
        .load_state(&conversation_id)
        .await
        .unwrap();
    assert!(state.messages_by_id.is_empty());
}

#[tokio::test]
async fn host_api_start_agent_run_denies_actor_without_conversation_write_grant() {
    let runtime = runtime_with_permission_store(PermissionStore::new());
    let api = KernelHostApi::new(runtime.clone());
    let (conversation_id, user_id) = conversation_with_user(&runtime).await;
    let message_id = runtime
        .services()
        .conversation_kernel
        .append_message(conversation_kernel::AppendMessageCommand {
            conversation_id: conversation_id.clone(),
            sender_id: user_id.clone(),
            content: MessageContent::Text {
                text: "trigger".to_string(),
            },
            reply_to: None,
            thread_id: None,
            visibility: Visibility::Conversation,
        })
        .await
        .unwrap();

    let result = api
        .start_agent_run(StartAgentRunRequest {
            conversation_id: conversation_id.clone(),
            trigger_message_id: message_id,
            requested_by: user_id.clone(),
            actor_context: Some(actor_context(user_id, EnterpriseRole::User)),
        })
        .await;

    assert!(matches!(
        result,
        Err(HostApiError::PermissionDenied { resource_id, action, .. })
            if resource_id == conversation_id.0 && action == "write"
    ));
}

#[tokio::test]
async fn host_api_approve_action_requires_admin_permission() {
    let runtime = runtime_with_permission_store(PermissionStore::new());
    let api = KernelHostApi::new(runtime.clone());
    let (conversation_id, user_id) = conversation_with_user(&runtime).await;
    let action_id = ActionId("action-approval".to_string());
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

    let result = api
        .approve_action(HostActionDecisionRequest {
            conversation_id: conversation_id.clone(),
            action_id,
            decided_by: user_id.clone(),
            reason: None,
            actor_context: Some(actor_context(user_id, EnterpriseRole::User)),
        })
        .await;

    assert!(matches!(
        result,
        Err(HostApiError::PermissionDenied { action, .. }) if action == "admin"
    ));
}

#[tokio::test]
async fn host_api_super_admin_bypasses_conversation_grant() {
    let runtime = runtime_with_permission_store(PermissionStore::new());
    let api = KernelHostApi::new(runtime.clone());
    let (conversation_id, user_id) = conversation_with_user(&runtime).await;

    api.submit_user_message(SubmitUserMessageRequest {
        conversation_id,
        user_id: user_id.clone(),
        text: "allowed by super admin".to_string(),
        actor_context: Some(actor_context(user_id, EnterpriseRole::SuperAdmin)),
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn host_api_actor_context_without_permission_store_returns_unavailable_error() {
    let runtime = runtime();
    let api = KernelHostApi::new(runtime.clone());
    let (conversation_id, user_id) = conversation_with_user(&runtime).await;

    let result = api
        .submit_user_message(SubmitUserMessageRequest {
            conversation_id,
            user_id: user_id.clone(),
            text: "needs store".to_string(),
            actor_context: Some(actor_context(user_id, EnterpriseRole::User)),
        })
        .await;

    assert!(matches!(
        result,
        Err(HostApiError::PermissionStoreUnavailable)
    ));
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

#[tokio::test]
async fn host_api_recover_delegates_to_kernel_runtime() {
    let runtime = runtime();
    runtime.start().unwrap();
    let api = KernelHostApi::new(runtime.clone());

    api.recover().await.unwrap();

    assert_eq!(runtime.state().as_str(), "initialized");
}

#[tokio::test]
async fn host_api_recover_after_shutdown_returns_kernel_operation_error() {
    let runtime = runtime();
    let api = KernelHostApi::new(runtime.clone());

    api.shutdown().await.unwrap();
    let result = api.recover().await;

    assert!(matches!(
        result,
        Err(HostApiError::KernelOperationFailed { reason })
            if reason == "invalid kernel lifecycle transition: shutdown -> recovering"
    ));
}
