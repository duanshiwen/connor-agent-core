use std::sync::Arc;

use action_core::ActionRegistry;
use agentos_kernel::{KernelHostApi, KernelRuntimeBuilder};
use audit_log::{AuditLog, MemoryAuditSink};
use capability_policy::CapabilityPolicy;
use conversation_core::{ConversationKind, Participant, ParticipantId, ParticipantKind};
use conversation_journal::{ConversationJournal, MemoryConversationJournal};
use conversation_kernel::CreateConversationCommand;
use model_adapter::{FakeModelAdapter, ModelAdapter};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let host = KernelHostApi::new(runtime()?);
    host.runtime().init()?;
    host.runtime().start()?;

    let desktop_user = ParticipantId::from("desktop-user");
    let conversation_id = host
        .runtime()
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
        .await?;

    let pending = host.list_pending_approvals(conversation_id.clone()).await?;
    println!(
        "minimal desktop host boundary integrated kernel API: conversation={}, pending_approvals={}",
        conversation_id.0,
        pending.len()
    );

    host.shutdown().await?;
    Ok(())
}

fn runtime() -> anyhow::Result<agentos_kernel::KernelRuntime> {
    let journal: Arc<dyn ConversationJournal> = Arc::new(MemoryConversationJournal::new());
    let model_adapter: Arc<dyn ModelAdapter> = Arc::new(FakeModelAdapter::new("desktop host"));
    let audit_log: Arc<dyn AuditLog> = Arc::new(MemoryAuditSink::new());

    Ok(KernelRuntimeBuilder::new()
        .conversation_journal(journal)
        .model_adapter(model_adapter)
        .action_registry(Arc::new(ActionRegistry::new()))
        .capability_policy(Arc::new(CapabilityPolicy::default_safe()))
        .audit_log(audit_log)
        .build()?)
}
