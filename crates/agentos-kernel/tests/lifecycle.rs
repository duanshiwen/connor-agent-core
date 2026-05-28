use std::sync::Arc;

use action_core::ActionRegistry;
use agentos_kernel::{KernelRuntime, KernelRuntimeBuilder, KernelRuntimeState};
use audit_log::{AuditLog, MemoryAuditSink};
use capability_policy::CapabilityPolicy;
use conversation_journal::{ConversationJournal, MemoryConversationJournal};
use model_adapter::{ModelAdapter, StaticModelAdapter};

fn runtime() -> KernelRuntime {
    let journal: Arc<dyn ConversationJournal> = Arc::new(MemoryConversationJournal::new());
    let model_adapter: Arc<dyn ModelAdapter> = Arc::new(StaticModelAdapter::default());
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

#[test]
fn runtime_starts_in_new_state() {
    let runtime = runtime();

    assert_eq!(runtime.state(), KernelRuntimeState::New);
}

#[test]
fn init_is_idempotent_before_start() {
    let runtime = runtime();

    runtime.init().unwrap();
    runtime.init().unwrap();

    assert_eq!(runtime.state(), KernelRuntimeState::Initialized);
}

#[test]
fn start_transitions_initialized_runtime_to_started() {
    let runtime = runtime();

    runtime.init().unwrap();
    runtime.start().unwrap();
    runtime.start().unwrap();

    assert_eq!(runtime.state(), KernelRuntimeState::Started);
}

#[test]
fn start_from_new_initializes_and_starts_runtime() {
    let runtime = runtime();

    runtime.start().unwrap();

    assert_eq!(runtime.state(), KernelRuntimeState::Started);
}

#[test]
fn recover_from_new_moves_runtime_to_initialized() {
    let runtime = runtime();

    runtime.recover().unwrap();

    assert_eq!(runtime.state(), KernelRuntimeState::Initialized);
}

#[test]
fn recover_is_idempotent_before_shutdown() {
    let runtime = runtime();

    runtime.start().unwrap();
    runtime.recover().unwrap();
    runtime.recover().unwrap();

    assert_eq!(runtime.state(), KernelRuntimeState::Initialized);
}

#[test]
fn recover_after_shutdown_returns_typed_error() {
    let runtime = runtime();

    runtime.shutdown().unwrap();
    let err = runtime.recover().unwrap_err();

    assert_eq!(
        err.to_string(),
        "invalid kernel lifecycle transition: shutdown -> recovering"
    );
}

#[test]
fn recovering_state_has_stable_string() {
    assert_eq!(KernelRuntimeState::Recovering.as_str(), "recovering");
}

#[test]
fn shutdown_is_idempotent() {
    let runtime = runtime();

    runtime.start().unwrap();
    runtime.shutdown().unwrap();
    runtime.shutdown().unwrap();

    assert_eq!(runtime.state(), KernelRuntimeState::Shutdown);
}

#[test]
fn start_after_shutdown_returns_typed_error() {
    let runtime = runtime();

    runtime.shutdown().unwrap();
    let err = runtime.start().unwrap_err();

    assert_eq!(
        err.to_string(),
        "invalid kernel lifecycle transition: shutdown -> started"
    );
}

#[test]
fn health_check_reports_runtime_state_and_core_services() {
    let runtime = runtime();

    runtime.start().unwrap();
    let report = runtime.health_check();

    assert_eq!(report.state, KernelRuntimeState::Started);
    assert!(report.conversation_kernel_available);
    assert!(report.model_adapter_available);
    assert!(report.action_registry_available);
    assert!(report.capability_policy_available);
    assert!(report.audit_log_available);
    assert!(report.healthy);
}
