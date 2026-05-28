use std::{collections::BTreeMap, sync::Arc};

use action_core::{
    ActionId, ActionKind, ActionRegistry, ActionRequest, ActionSchema, SideEffectKind,
    StaticActionExecutor,
};
use action_runtime::ActionRuntimeOutcome;
use agentos_kernel::{
    HostActionDecisionRequest, HostApiErrorResponse, HostExecuteApprovedActionRequest,
    HostProcessActionRequest, KernelHostApi, KernelRuntimeBuilder, StartAgentRunRequest,
    SubmitUserMessageRequest,
};
use audit_log::{AuditLog, MemoryAuditSink};
use capability_policy::CapabilityPolicy;
use chrono::Utc;
use conversation_core::{
    ConversationId, ConversationKind, Participant, ParticipantId, ParticipantKind,
};
use conversation_journal::{ConversationJournal, MemoryConversationJournal};
use conversation_kernel::CreateConversationCommand;
use model_adapter::{ModelAdapter, StaticModelAdapter};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let host = KernelHostApi::new(runtime()?);
    host.runtime().init()?;
    host.runtime().start()?;

    let health = host.runtime().health_check();
    println!(
        "minimal server host readiness: healthy={}, state={}",
        health.healthy,
        health.state.as_str()
    );

    let (conversation_id, user_id) = create_conversation(&host).await?;
    let message = host
        .submit_user_message(SubmitUserMessageRequest {
            conversation_id: conversation_id.clone(),
            user_id: user_id.clone(),
            text: "server host submits a pilot integration message".to_string(),
            actor_context: None,
        })
        .await?;

    let run = host
        .start_agent_run(StartAgentRunRequest {
            conversation_id: conversation_id.clone(),
            trigger_message_id: message.message_id,
            requested_by: user_id.clone(),
            actor_context: None,
        })
        .await?;
    println!(
        "minimal server host run started: run_id={}, status={:?}",
        run.run_id, run.status
    );

    let read_outcome = host
        .process_action(HostProcessActionRequest {
            conversation_id: conversation_id.clone(),
            action_request: action_request(
                "server-read-action",
                "knowledge.search",
                &conversation_id,
                &user_id,
            ),
            requested_by: Some(user_id.clone()),
            runtime_actor: Some(user_id.clone()),
            actor_context: None,
        })
        .await?;
    println!("minimal server host read action: {read_outcome:?}");

    let send_outcome = host
        .process_action(HostProcessActionRequest {
            conversation_id: conversation_id.clone(),
            action_request: action_request(
                "server-send-action",
                "mail.send",
                &conversation_id,
                &user_id,
            ),
            requested_by: Some(user_id.clone()),
            runtime_actor: Some(user_id.clone()),
            actor_context: None,
        })
        .await?;

    if matches!(send_outcome, ActionRuntimeOutcome::ApprovalRequired { .. }) {
        host.approve_action(HostActionDecisionRequest {
            conversation_id: conversation_id.clone(),
            action_id: ActionId::from("server-send-action"),
            decided_by: user_id.clone(),
            reason: Some("server operator approved pilot fixture".to_string()),
            actor_context: None,
        })
        .await?;

        let approved = host
            .execute_approved_action(HostExecuteApprovedActionRequest {
                conversation_id: conversation_id.clone(),
                action_id: ActionId::from("server-send-action"),
                runtime_actor: Some(user_id.clone()),
                actor_context: None,
            })
            .await?;
        println!("minimal server host approved action: {approved:?}");
    }

    let diagnostics = host.runtime().diagnostics_bundle(BTreeMap::new()).await?;
    println!(
        "minimal server host diagnostics: storage={}, audit_events={}",
        diagnostics.storage_manifest.status, diagnostics.recent_audit_summary.total_events
    );

    let missing_run = host
        .get_run_status(conversation_id, "missing-run".to_string())
        .await
        .err()
        .map(|error| HostApiErrorResponse::from(&error));

    if let Some(error_response) = missing_run {
        println!(
            "minimal server host error response: category={:?}, code={}",
            error_response.category, error_response.code
        );
    }

    Ok(())
}

async fn create_conversation(
    host: &KernelHostApi,
) -> anyhow::Result<(ConversationId, ParticipantId)> {
    let user_id = ParticipantId::from("server-user");
    let conversation_id = host
        .runtime()
        .services()
        .conversation_kernel
        .create_conversation(CreateConversationCommand {
            kind: ConversationKind::Direct,
            title: Some("Server host integration example".to_string()),
            participants: vec![Participant {
                id: user_id.clone(),
                kind: ParticipantKind::Human,
                display_name: "Server User".to_string(),
            }],
            actor_id: Some(user_id.clone()),
        })
        .await?;

    Ok((conversation_id, user_id))
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
        input: serde_json::json!({"example": "server-host"}),
        requested_by: requested_by.to_string(),
        conversation_id: Some(conversation_id.to_string()),
        message_id: None,
        requested_at: Utc::now(),
    }
}

fn registry() -> Arc<ActionRegistry> {
    let mut registry = ActionRegistry::new();
    registry
        .register(ActionSchema {
            kind: ActionKind::from("knowledge.search"),
            display_name: "Knowledge Search".to_string(),
            description: "Read-only backend host lookup".to_string(),
            side_effect: SideEffectKind::ReadOnly,
            input_schema: None,
            output_schema: None,
        })
        .unwrap();
    registry
        .register(ActionSchema {
            kind: ActionKind::from("mail.send"),
            display_name: "Send Mail".to_string(),
            description: "Approval-gated backend host side effect".to_string(),
            side_effect: SideEffectKind::NetworkAccess,
            input_schema: None,
            output_schema: None,
        })
        .unwrap();
    Arc::new(registry)
}

fn runtime() -> anyhow::Result<agentos_kernel::KernelRuntime> {
    let journal: Arc<dyn ConversationJournal> = Arc::new(MemoryConversationJournal::new());
    let model_adapter: Arc<dyn ModelAdapter> = Arc::new(StaticModelAdapter::default());
    let audit_log: Arc<dyn AuditLog> = Arc::new(MemoryAuditSink::new());

    Ok(KernelRuntimeBuilder::new()
        .conversation_journal(journal)
        .model_adapter(model_adapter)
        .action_registry(registry())
        .capability_policy(Arc::new(CapabilityPolicy::default_safe()))
        .audit_log(audit_log)
        .action_executor(Arc::new(StaticActionExecutor::new("server host fixture")))
        .build()?)
}
