use std::sync::Arc;

use action_core::ActionRegistry;
use agentos_kernel::{HostApiErrorResponse, KernelHostApi, KernelRuntimeBuilder};
use audit_log::{AuditLog, MemoryAuditSink};
use capability_policy::CapabilityPolicy;
use conversation_journal::{ConversationJournal, MemoryConversationJournal};
use model_adapter::{FakeModelAdapter, ModelAdapter};

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

    let missing_run = host
        .get_run_status("server-conversation".into(), "missing-run".to_string())
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

fn runtime() -> anyhow::Result<agentos_kernel::KernelRuntime> {
    let journal: Arc<dyn ConversationJournal> = Arc::new(MemoryConversationJournal::new());
    let model_adapter: Arc<dyn ModelAdapter> = Arc::new(FakeModelAdapter::default());
    let audit_log: Arc<dyn AuditLog> = Arc::new(MemoryAuditSink::new());

    Ok(KernelRuntimeBuilder::new()
        .conversation_journal(journal)
        .model_adapter(model_adapter)
        .action_registry(Arc::new(ActionRegistry::new()))
        .capability_policy(Arc::new(CapabilityPolicy::default_safe()))
        .audit_log(audit_log)
        .build()?)
}
