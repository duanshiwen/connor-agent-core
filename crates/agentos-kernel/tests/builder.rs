use std::sync::Arc;

use action_core::ActionRegistry;
use agentos_kernel::{
    KernelError, KernelRuntimeBuilder, PolicyProviderRegistry, StorageProviderRegistry,
};
use agentos_storage::AgentOsStorage;
use audit_log::{AuditLog, MemoryAuditSink};
use capability_policy::CapabilityPolicy;
use conversation_journal::{ConversationJournal, MemoryConversationJournal};
use model_adapter::{FakeModelAdapter, ModelAdapter};

fn conversation_journal() -> Arc<dyn ConversationJournal> {
    Arc::new(MemoryConversationJournal::new())
}

fn model_adapter() -> Arc<dyn ModelAdapter> {
    Arc::new(FakeModelAdapter::default())
}

fn action_registry() -> Arc<ActionRegistry> {
    Arc::new(ActionRegistry::new())
}

fn capability_policy() -> Arc<CapabilityPolicy> {
    Arc::new(CapabilityPolicy::default_safe())
}

fn audit_log() -> Arc<dyn AuditLog> {
    Arc::new(MemoryAuditSink::new())
}

fn assert_missing_service(
    result: Result<agentos_kernel::KernelRuntime, KernelError>,
    service: &'static str,
) {
    match result {
        Err(err) => assert_eq!(err, KernelError::MissingService { service }),
        Ok(_) => panic!("expected missing service error for {service}"),
    }
}

#[test]
fn builder_requires_conversation_journal() {
    assert_missing_service(
        KernelRuntimeBuilder::new()
            .model_adapter(model_adapter())
            .action_registry(action_registry())
            .capability_policy(capability_policy())
            .audit_log(audit_log())
            .build(),
        "conversation_journal",
    );
}

#[test]
fn builder_requires_model_adapter() {
    assert_missing_service(
        KernelRuntimeBuilder::new()
            .conversation_journal(conversation_journal())
            .action_registry(action_registry())
            .capability_policy(capability_policy())
            .audit_log(audit_log())
            .build(),
        "model_adapter",
    );
}

#[test]
fn builder_requires_action_registry() {
    assert_missing_service(
        KernelRuntimeBuilder::new()
            .conversation_journal(conversation_journal())
            .model_adapter(model_adapter())
            .capability_policy(capability_policy())
            .audit_log(audit_log())
            .build(),
        "action_registry",
    );
}

#[test]
fn builder_requires_capability_policy() {
    assert_missing_service(
        KernelRuntimeBuilder::new()
            .conversation_journal(conversation_journal())
            .model_adapter(model_adapter())
            .action_registry(action_registry())
            .audit_log(audit_log())
            .build(),
        "capability_policy",
    );
}

#[test]
fn builder_requires_audit_log() {
    assert_missing_service(
        KernelRuntimeBuilder::new()
            .conversation_journal(conversation_journal())
            .model_adapter(model_adapter())
            .action_registry(action_registry())
            .capability_policy(capability_policy())
            .build(),
        "audit_log",
    );
}

#[test]
fn builder_accepts_optional_storage() {
    let temp_dir = tempfile::tempdir().unwrap();
    let storage = Arc::new(AgentOsStorage::init(temp_dir.path()).unwrap());

    let runtime = KernelRuntimeBuilder::new()
        .conversation_journal(conversation_journal())
        .model_adapter(model_adapter())
        .action_registry(action_registry())
        .capability_policy(capability_policy())
        .audit_log(audit_log())
        .storage(storage)
        .build()
        .unwrap();

    assert!(runtime.services().storage.is_some());
}

#[test]
fn builder_accepts_optional_storage_and_policy_provider_registries() {
    let runtime = KernelRuntimeBuilder::new()
        .conversation_journal(conversation_journal())
        .model_adapter(model_adapter())
        .action_registry(action_registry())
        .capability_policy(capability_policy())
        .audit_log(audit_log())
        .storage_provider_registry(Arc::new(StorageProviderRegistry::new()))
        .policy_provider_registry(Arc::new(PolicyProviderRegistry::new()))
        .build()
        .unwrap();

    assert!(runtime.services().storage_provider_registry.is_some());
    assert!(runtime.services().policy_provider_registry.is_some());
}

#[test]
fn builder_constructs_runtime_when_all_required_services_are_present() {
    let runtime = KernelRuntimeBuilder::new()
        .conversation_journal(conversation_journal())
        .model_adapter(model_adapter())
        .action_registry(action_registry())
        .capability_policy(capability_policy())
        .audit_log(audit_log())
        .build()
        .unwrap();

    assert!(runtime.services().action_registry.is_empty());
}
