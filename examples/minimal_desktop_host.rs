use std::{collections::BTreeMap, sync::Arc};

use action_core::{
    ActionId, ActionKind, ActionRegistry, ActionRequest, ActionSchema, SideEffectKind,
};
use agentos_kernel::{
    HostActionDecisionRequest, HostProcessActionRequest, KernelHostApi, KernelRuntimeBuilder,
};
use agentos_storage::AgentOsStorage;
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
    let storage_root = std::env::temp_dir().join("agentos-minimal-desktop-host-storage");
    let storage = Arc::new(AgentOsStorage::init(&storage_root)?);
    let credential_backend = "macos-keychain-or-host-selected-secure-store";

    let host = KernelHostApi::new(runtime(storage.clone())?);
    host.runtime().init()?;
    host.runtime().start()?;

    let desktop_user = ParticipantId::from("desktop-user");
    let conversation_id = create_conversation(&host, desktop_user.clone()).await?;

    let approval_outcome = host
        .process_action(HostProcessActionRequest {
            conversation_id: conversation_id.clone(),
            action_request: action_request(
                "desktop-open-external",
                "browser.open_external",
                &conversation_id,
                &desktop_user,
            ),
            requested_by: Some(desktop_user.clone()),
            runtime_actor: Some(desktop_user.clone()),
            actor_context: None,
        })
        .await?;
    println!("minimal desktop approval handoff: {approval_outcome:?}");

    let pending = host.list_pending_approvals(conversation_id.clone()).await?;
    println!(
        "minimal desktop host boundary integrated kernel API: conversation={}, pending_approvals={}, credential_backend={}",
        conversation_id.0,
        pending.len(),
        credential_backend
    );

    if let Some(pending) = pending.first() {
        host.deny_action(HostActionDecisionRequest {
            conversation_id: conversation_id.clone(),
            action_id: pending.action_id.clone(),
            decided_by: desktop_user,
            reason: Some("desktop user declined example side effect".to_string()),
            actor_context: None,
        })
        .await?;
    }

    let diagnostics = host.runtime().diagnostics_bundle(BTreeMap::new()).await?;
    println!(
        "minimal desktop diagnostics bundle: storage_status={}, storage_root={:?}, audit_events={}",
        diagnostics.storage_manifest.status,
        storage.root(),
        diagnostics.recent_audit_summary.total_events
    );

    host.shutdown().await?;
    Ok(())
}

async fn create_conversation(
    host: &KernelHostApi,
    desktop_user: ParticipantId,
) -> anyhow::Result<ConversationId> {
    host.runtime()
        .services()
        .conversation_kernel
        .create_conversation(CreateConversationCommand {
            kind: ConversationKind::Direct,
            title: Some("Desktop host boundary example".to_string()),
            participants: vec![Participant {
                id: desktop_user.clone(),
                kind: ParticipantKind::Human,
                display_name: "Desktop User".to_string(),
            }],
            actor_id: Some(desktop_user),
        })
        .await
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
        input: serde_json::json!({"example": "desktop-host"}),
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
            kind: ActionKind::from("browser.open_external"),
            display_name: "Open External Browser".to_string(),
            description: "Desktop UX handoff side effect requiring approval".to_string(),
            side_effect: SideEffectKind::NetworkAccess,
            input_schema: None,
            output_schema: None,
        })
        .unwrap();
    Arc::new(registry)
}

fn runtime(storage: Arc<AgentOsStorage>) -> anyhow::Result<agentos_kernel::KernelRuntime> {
    let journal: Arc<dyn ConversationJournal> = Arc::new(MemoryConversationJournal::new());
    let model_adapter: Arc<dyn ModelAdapter> = Arc::new(StaticModelAdapter::new("desktop host"));
    let audit_log: Arc<dyn AuditLog> = Arc::new(MemoryAuditSink::new());

    Ok(KernelRuntimeBuilder::new()
        .conversation_journal(journal)
        .model_adapter(model_adapter)
        .action_registry(registry())
        .capability_policy(Arc::new(CapabilityPolicy::default_safe()))
        .audit_log(audit_log)
        .storage(storage)
        .build()?)
}
