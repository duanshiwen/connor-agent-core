use std::sync::Arc;

use action_core::ActionRegistry;
use agentos_kernel::{KernelHostApi, KernelRuntimeBuilder, SubmitUserMessageRequest};
use audit_log::{AuditLog, MemoryAuditSink};
use capability_policy::CapabilityPolicy;
use conversation_core::{
    ConversationKind, MessageContent, Participant, ParticipantId, ParticipantKind, Visibility,
};
use conversation_journal::{ConversationJournal, MemoryConversationJournal};
use conversation_kernel::{AppendMessageCommand, CreateConversationCommand};
use model_adapter::{ModelAdapter, StaticModelAdapter};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let host = KernelHostApi::new(runtime()?);
    host.runtime().init()?;
    host.runtime().start()?;

    let user_id = ParticipantId::from("cli-user");
    let conversation_id = host
        .runtime()
        .services()
        .conversation_kernel
        .create_conversation(CreateConversationCommand {
            kind: ConversationKind::Direct,
            title: Some("CLI host example".to_string()),
            participants: vec![Participant {
                id: user_id.clone(),
                kind: ParticipantKind::Human,
                display_name: "CLI User".to_string(),
            }],
            actor_id: Some(user_id.clone()),
        })
        .await?;

    let seed_message_id = host
        .runtime()
        .services()
        .conversation_kernel
        .append_message(AppendMessageCommand {
            conversation_id: conversation_id.clone(),
            sender_id: user_id.clone(),
            content: MessageContent::Text {
                text: "hello from CLI host".to_string(),
            },
            reply_to: None,
            thread_id: None,
            visibility: Visibility::Conversation,
        })
        .await?;

    let response = host
        .submit_user_message(SubmitUserMessageRequest {
            conversation_id: conversation_id.clone(),
            user_id: user_id.clone(),
            text: "second message through host API".to_string(),
            actor_context: None,
        })
        .await?;

    println!(
        "minimal CLI host integrated kernel API: conversation={}, seed_message={}, host_message={}",
        conversation_id.0, seed_message_id.0, response.message_id.0
    );
    Ok(())
}

fn runtime() -> anyhow::Result<agentos_kernel::KernelRuntime> {
    let journal: Arc<dyn ConversationJournal> = Arc::new(MemoryConversationJournal::new());
    let model_adapter: Arc<dyn ModelAdapter> = Arc::new(StaticModelAdapter::default());
    let audit_log: Arc<dyn AuditLog> = Arc::new(MemoryAuditSink::new());

    Ok(KernelRuntimeBuilder::new()
        .conversation_journal(journal)
        .model_adapter(model_adapter)
        .action_registry(Arc::new(ActionRegistry::new()))
        .capability_policy(Arc::new(CapabilityPolicy::default_safe()))
        .audit_log(audit_log)
        .build()?)
}
