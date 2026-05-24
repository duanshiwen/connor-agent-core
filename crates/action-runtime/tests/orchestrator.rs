use action_core::{
    ActionExecutor, ActionExecutorError, ActionId, ActionKind, ActionRegistry, ActionRequest,
    ActionResult, ActionResultPayload, ActionSchema, ActionStatus, SideEffectKind,
};
use action_runtime::{
    ActionRuntime, ActionRuntimeOutcome, ArtifactProducingExecutor, ArtifactStoreResolver,
    DeterministicArtifactDescriptorFactory, ProcessActionRequest,
};
use artifact_core::{ArtifactId, ArtifactKind, ArtifactStore, MemoryArtifactStore};
use async_trait::async_trait;
use audit_log::{AuditLog, MemoryAuditSink};
use capability_policy::CapabilityPolicy;
use chrono::{DateTime, Utc};
use conversation_core::*;
use conversation_journal::MemoryConversationJournal;
use conversation_kernel::*;
use knowledge_entity::{
    KnowledgeActionExecutor, KnowledgeCreateDraftActionInput, KnowledgeEntryDraft,
    KnowledgeEntryRef, KnowledgeGetEntryActionInput, KnowledgeSaveEntryActionInput,
    KnowledgeSearchActionInput, KnowledgeSearchQuery, KnowledgeSearchResult,
    MemoryKnowledgeRepository, register_knowledge_action_schemas,
};
use std::sync::{Arc, Mutex};

struct SequentialIdGenerator {
    counter: Mutex<u64>,
}

impl SequentialIdGenerator {
    fn new() -> Self {
        Self {
            counter: Mutex::new(0),
        }
    }
}

impl IdGenerator for SequentialIdGenerator {
    fn new_id(&self) -> String {
        let mut counter = self.counter.lock().unwrap();
        *counter += 1;
        format!("id-{counter}")
    }
}

struct FixedClock {
    time: DateTime<Utc>,
}

impl FixedClock {
    fn new(time: DateTime<Utc>) -> Self {
        Self { time }
    }
}

impl Clock for FixedClock {
    fn now(&self) -> DateTime<Utc> {
        self.time
    }
}

#[derive(Debug)]
enum TestPayload {
    Text,
    ArtifactRef(ArtifactId),
}

impl Default for TestPayload {
    fn default() -> Self {
        Self::Text
    }
}

#[derive(Debug, Default)]
struct CountingExecutor {
    calls: Mutex<u64>,
    fail: bool,
    payload: TestPayload,
}

impl CountingExecutor {
    fn calls(&self) -> u64 {
        *self.calls.lock().unwrap()
    }

    fn failing() -> Self {
        Self {
            calls: Mutex::new(0),
            fail: true,
            payload: TestPayload::Text,
        }
    }

    fn returning_artifact_ref(artifact_id: impl Into<ArtifactId>) -> Self {
        Self {
            calls: Mutex::new(0),
            fail: false,
            payload: TestPayload::ArtifactRef(artifact_id.into()),
        }
    }
}

#[async_trait]
impl ActionExecutor for CountingExecutor {
    async fn execute(&self, request: &ActionRequest) -> Result<ActionResult, ActionExecutorError> {
        *self.calls.lock().unwrap() += 1;
        if self.fail {
            return Err(ActionExecutorError::ExecutionFailed("boom".to_string()));
        }
        let payload = match &self.payload {
            TestPayload::Text => ActionResultPayload::Text(format!("{} ok", request.action_kind)),
            TestPayload::ArtifactRef(artifact_id) => {
                ActionResultPayload::ArtifactRef(artifact_id.clone())
            }
        };
        Ok(ActionResult {
            status: ActionStatus::Completed,
            payload,
            summary: format!("{} completed", request.action_kind),
            completed_at: Utc::now(),
        })
    }
}

fn test_kernel() -> ConversationKernel {
    let journal = Arc::new(MemoryConversationJournal::new());
    ConversationKernel::with_generators(
        journal,
        Arc::new(SequentialIdGenerator::new()),
        Arc::new(FixedClock::new(Utc::now())),
    )
}

fn user() -> Participant {
    Participant {
        id: ParticipantId::from("user-1"),
        kind: ParticipantKind::Human,
        display_name: "User".to_string(),
    }
}

fn agent() -> Participant {
    Participant {
        id: ParticipantId::from("agent-1"),
        kind: ParticipantKind::Agent,
        display_name: "Assistant".to_string(),
    }
}

async fn create_conversation(kernel: &ConversationKernel) -> ConversationId {
    kernel
        .create_conversation(CreateConversationCommand {
            kind: ConversationKind::AgentTask,
            title: Some("Action runtime".to_string()),
            participants: vec![user(), agent()],
            actor_id: Some(ParticipantId::from("user-1")),
        })
        .await
        .unwrap()
}

fn registry() -> ActionRegistry {
    let mut registry = ActionRegistry::new();
    for (kind, side_effect) in [
        ("knowledge.search", SideEffectKind::ReadOnly),
        ("knowledge.save_entry", SideEffectKind::RuntimeStateMutation),
        ("mail.send", SideEffectKind::ExternalSystemMutation),
    ] {
        registry
            .register(ActionSchema {
                kind: ActionKind::from(kind),
                display_name: kind.to_string(),
                description: kind.to_string(),
                side_effect,
                input_schema: None,
                output_schema: None,
            })
            .unwrap();
    }
    registry
}

fn action_request(conversation_id: &ConversationId, action_id: &str, kind: &str) -> ActionRequest {
    ActionRequest {
        action_id: ActionId::from(action_id),
        action_kind: ActionKind::from(kind),
        input: serde_json::json!({"query":"agent os"}),
        requested_by: "user-1".to_string(),
        conversation_id: Some(conversation_id.to_string()),
        message_id: None,
        requested_at: Utc::now(),
    }
}

async fn process_action(
    kernel: &ConversationKernel,
    registry: &ActionRegistry,
    executor: &dyn ActionExecutor,
    audit: &dyn AuditLog,
    conversation_id: &ConversationId,
    action_id: &str,
    kind: &str,
) -> ActionRuntimeOutcome {
    process_action_with_artifact_resolver(
        kernel,
        registry,
        executor,
        audit,
        None,
        conversation_id,
        action_id,
        kind,
    )
    .await
}

async fn process_action_with_artifact_resolver(
    kernel: &ConversationKernel,
    registry: &ActionRegistry,
    executor: &dyn ActionExecutor,
    audit: &dyn AuditLog,
    artifact_resolver: Option<&dyn action_runtime::ArtifactResolver>,
    conversation_id: &ConversationId,
    action_id: &str,
    kind: &str,
) -> ActionRuntimeOutcome {
    process_action_with_input_and_artifact_resolver(
        kernel,
        registry,
        executor,
        audit,
        artifact_resolver,
        conversation_id,
        action_id,
        kind,
        serde_json::json!({"query":"agent os"}),
    )
    .await
}

async fn process_action_with_input(
    kernel: &ConversationKernel,
    registry: &ActionRegistry,
    executor: &dyn ActionExecutor,
    audit: &dyn AuditLog,
    conversation_id: &ConversationId,
    action_id: &str,
    kind: &str,
    input: serde_json::Value,
) -> ActionRuntimeOutcome {
    process_action_with_input_and_artifact_resolver(
        kernel,
        registry,
        executor,
        audit,
        None,
        conversation_id,
        action_id,
        kind,
        input,
    )
    .await
}

#[allow(clippy::too_many_arguments)]
async fn process_action_with_input_and_artifact_resolver(
    kernel: &ConversationKernel,
    registry: &ActionRegistry,
    executor: &dyn ActionExecutor,
    audit: &dyn AuditLog,
    artifact_resolver: Option<&dyn action_runtime::ArtifactResolver>,
    conversation_id: &ConversationId,
    action_id: &str,
    kind: &str,
    input: serde_json::Value,
) -> ActionRuntimeOutcome {
    let policy = CapabilityPolicy::default_safe();
    let runtime = ActionRuntime {
        kernel,
        registry,
        policy: &policy,
        executor,
        audit_log: audit,
        artifact_resolver,
    };
    runtime
        .process(ProcessActionRequest {
            conversation_id,
            action_request: ActionRequest {
                input,
                ..action_request(conversation_id, action_id, kind)
            },
            requested_by: Some(ParticipantId::from("user-1")),
            runtime_actor: Some(ParticipantId::from("agent-1")),
        })
        .await
        .unwrap()
}

#[tokio::test]
async fn read_only_action_auto_executes_and_records_audit() {
    let kernel = test_kernel();
    let conversation_id = create_conversation(&kernel).await;
    let registry = registry();
    let executor = CountingExecutor::default();
    let audit = MemoryAuditSink::new();

    let outcome = process_action(
        &kernel,
        &registry,
        &executor,
        &audit,
        &conversation_id,
        "action-1",
        "knowledge.search",
    )
    .await;

    assert!(matches!(outcome, ActionRuntimeOutcome::Completed { .. }));
    assert_eq!(executor.calls(), 1);

    let state = kernel.load_state(&conversation_id).await.unwrap();
    let action = state.actions.get(&ActionId::from("action-1")).unwrap();
    assert_eq!(action.status, ConversationActionStatus::Completed);
    assert!(action.result.is_some());

    let events = audit.list().await.unwrap();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].policy_decision, "allow");
    assert_eq!(events[0].result_status, "completed");
}

#[tokio::test]
async fn completed_action_can_record_artifact_ref_payload() {
    let kernel = test_kernel();
    let conversation_id = create_conversation(&kernel).await;
    let registry = registry();
    let artifact_id = ArtifactId::from("artifact-action-1");
    let executor = CountingExecutor::returning_artifact_ref(artifact_id.clone());
    let audit = MemoryAuditSink::new();

    let outcome = process_action(
        &kernel,
        &registry,
        &executor,
        &audit,
        &conversation_id,
        "action-1",
        "knowledge.search",
    )
    .await;

    assert!(matches!(outcome, ActionRuntimeOutcome::Completed { .. }));
    assert_eq!(executor.calls(), 1);

    let state = kernel.load_state(&conversation_id).await.unwrap();
    let action = state.actions.get(&ActionId::from("action-1")).unwrap();
    assert_eq!(action.status, ConversationActionStatus::Completed);
    let result = action.result.as_ref().unwrap();
    assert_eq!(
        result.payload,
        ActionResultPayload::ArtifactRef(artifact_id)
    );

    let events = audit.list().await.unwrap();
    assert_eq!(events[0].policy_decision, "allow");
    assert_eq!(events[0].result_status, "completed");
}

#[tokio::test]
async fn artifact_producing_executor_stores_artifact_and_records_ref_payload() {
    let kernel = test_kernel();
    let conversation_id = create_conversation(&kernel).await;
    let registry = registry();
    let store = Arc::new(MemoryArtifactStore::new());
    let executor = ArtifactProducingExecutor::new(
        store.clone(),
        Arc::new(DeterministicArtifactDescriptorFactory::new(
            ArtifactKind::ToolResult,
        )),
    );
    let audit = MemoryAuditSink::new();

    let outcome = process_action(
        &kernel,
        &registry,
        &executor,
        &audit,
        &conversation_id,
        "action-1",
        "knowledge.search",
    )
    .await;

    let artifact_id = ArtifactId::from("artifact-action-1");
    match outcome {
        ActionRuntimeOutcome::Completed { result, .. } => {
            assert_eq!(
                result.payload,
                ActionResultPayload::ArtifactRef(artifact_id.clone())
            );
        }
        other => panic!("expected completed outcome, got {other:?}"),
    }

    let stored = store.get(&artifact_id).await.unwrap().unwrap();
    assert_eq!(stored.id, artifact_id);
    assert_eq!(stored.kind, ArtifactKind::ToolResult);
    assert_eq!(stored.title.as_deref(), Some("knowledge.search result"));
    assert_eq!(stored.metadata["action_id"], "action-1");

    let state = kernel.load_state(&conversation_id).await.unwrap();
    let action = state.actions.get(&ActionId::from("action-1")).unwrap();
    assert_eq!(action.status, ConversationActionStatus::Completed);
    assert_eq!(
        action.result.as_ref().unwrap().payload,
        ActionResultPayload::ArtifactRef(ArtifactId::from("artifact-action-1"))
    );

    assert!(state.linked_artifacts.is_empty());

    let events = audit.list().await.unwrap();
    assert_eq!(events[0].policy_decision, "allow");
    assert_eq!(events[0].result_status, "completed");
}

#[tokio::test]
async fn artifact_ref_result_links_artifact_to_conversation() {
    let kernel = test_kernel();
    let conversation_id = create_conversation(&kernel).await;
    let registry = registry();
    let store = Arc::new(MemoryArtifactStore::new());
    let resolver = ArtifactStoreResolver::new(store.clone());
    let executor = ArtifactProducingExecutor::new(
        store.clone(),
        Arc::new(DeterministicArtifactDescriptorFactory::new(
            ArtifactKind::ToolResult,
        )),
    );
    let audit = MemoryAuditSink::new();

    let outcome = process_action_with_artifact_resolver(
        &kernel,
        &registry,
        &executor,
        &audit,
        Some(&resolver),
        &conversation_id,
        "action-1",
        "knowledge.search",
    )
    .await;

    let artifact_id = ArtifactId::from("artifact-action-1");
    assert!(matches!(outcome, ActionRuntimeOutcome::Completed { .. }));

    let state = kernel.load_state(&conversation_id).await.unwrap();
    let linked_artifact = state.linked_artifacts.get(&artifact_id).unwrap();
    assert_eq!(linked_artifact.id, artifact_id);
    assert_eq!(linked_artifact.kind, ArtifactKind::ToolResult);
    assert_eq!(
        linked_artifact.title.as_deref(),
        Some("knowledge.search result")
    );
    assert_eq!(state.messages.len(), 0);
    assert_eq!(state.participants.len(), 2);
}

#[tokio::test]
async fn missing_artifact_ref_does_not_fail_completed_action() {
    let kernel = test_kernel();
    let conversation_id = create_conversation(&kernel).await;
    let registry = registry();
    let store = Arc::new(MemoryArtifactStore::new());
    let resolver = ArtifactStoreResolver::new(store);
    let artifact_id = ArtifactId::from("artifact-missing");
    let executor = CountingExecutor::returning_artifact_ref(artifact_id.clone());
    let audit = MemoryAuditSink::new();

    let outcome = process_action_with_artifact_resolver(
        &kernel,
        &registry,
        &executor,
        &audit,
        Some(&resolver),
        &conversation_id,
        "action-1",
        "knowledge.search",
    )
    .await;

    assert!(matches!(outcome, ActionRuntimeOutcome::Completed { .. }));

    let state = kernel.load_state(&conversation_id).await.unwrap();
    let action = state.actions.get(&ActionId::from("action-1")).unwrap();
    assert_eq!(action.status, ConversationActionStatus::Completed);
    assert_eq!(
        action.result.as_ref().unwrap().payload,
        ActionResultPayload::ArtifactRef(artifact_id)
    );
    assert!(state.linked_artifacts.is_empty());
}

#[tokio::test]
async fn text_result_does_not_attempt_artifact_linking() {
    let kernel = test_kernel();
    let conversation_id = create_conversation(&kernel).await;
    let registry = registry();
    let store = Arc::new(MemoryArtifactStore::new());
    let resolver = ArtifactStoreResolver::new(store);
    let executor = CountingExecutor::default();
    let audit = MemoryAuditSink::new();

    let outcome = process_action_with_artifact_resolver(
        &kernel,
        &registry,
        &executor,
        &audit,
        Some(&resolver),
        &conversation_id,
        "action-1",
        "knowledge.search",
    )
    .await;

    assert!(matches!(outcome, ActionRuntimeOutcome::Completed { .. }));

    let state = kernel.load_state(&conversation_id).await.unwrap();
    assert!(state.linked_artifacts.is_empty());
}

#[tokio::test]
async fn artifact_producing_executor_duplicate_artifact_id_fails_action() {
    let kernel = test_kernel();
    let conversation_id = create_conversation(&kernel).await;
    let registry = registry();
    let store = Arc::new(MemoryArtifactStore::new());
    let executor = ArtifactProducingExecutor::new(
        store,
        Arc::new(DeterministicArtifactDescriptorFactory::new(
            ArtifactKind::ToolResult,
        )),
    );
    let audit = MemoryAuditSink::new();

    let first = process_action(
        &kernel,
        &registry,
        &executor,
        &audit,
        &conversation_id,
        "action-1",
        "knowledge.search",
    )
    .await;
    assert!(matches!(first, ActionRuntimeOutcome::Completed { .. }));

    let second = process_action(
        &kernel,
        &registry,
        &executor,
        &audit,
        &conversation_id,
        "action-1",
        "knowledge.search",
    )
    .await;
    assert!(matches!(second, ActionRuntimeOutcome::Failed { .. }));

    let state = kernel.load_state(&conversation_id).await.unwrap();
    let action = state.actions.get(&ActionId::from("action-1")).unwrap();
    assert_eq!(action.status, ConversationActionStatus::Failed);
    assert!(
        action
            .error_message
            .as_deref()
            .unwrap()
            .contains("duplicate artifact id")
    );

    let events = audit.list().await.unwrap();
    assert_eq!(events.len(), 2);
    assert_eq!(events[1].policy_decision, "allow");
    assert_eq!(events[1].result_status, "failed");
}

#[tokio::test]
async fn ask_action_requires_approval_and_does_not_execute() {
    let kernel = test_kernel();
    let conversation_id = create_conversation(&kernel).await;
    let registry = registry();
    let executor = CountingExecutor::default();
    let audit = MemoryAuditSink::new();

    let outcome = process_action(
        &kernel,
        &registry,
        &executor,
        &audit,
        &conversation_id,
        "action-1",
        "knowledge.save_entry",
    )
    .await;

    assert!(matches!(
        outcome,
        ActionRuntimeOutcome::ApprovalRequired { .. }
    ));
    assert_eq!(executor.calls(), 0);

    let state = kernel.load_state(&conversation_id).await.unwrap();
    let action = state.actions.get(&ActionId::from("action-1")).unwrap();
    assert_eq!(action.status, ConversationActionStatus::ApprovalRequired);

    let events = audit.list().await.unwrap();
    assert_eq!(events[0].policy_decision, "ask");
    assert_eq!(events[0].result_status, "approval_required");
}

#[tokio::test]
async fn deny_action_records_denied_and_does_not_execute() {
    let kernel = test_kernel();
    let conversation_id = create_conversation(&kernel).await;
    let registry = registry();
    let executor = CountingExecutor::default();
    let audit = MemoryAuditSink::new();

    let outcome = process_action(
        &kernel,
        &registry,
        &executor,
        &audit,
        &conversation_id,
        "action-1",
        "mail.send",
    )
    .await;

    assert!(matches!(outcome, ActionRuntimeOutcome::Denied { .. }));
    assert_eq!(executor.calls(), 0);

    let state = kernel.load_state(&conversation_id).await.unwrap();
    let action = state.actions.get(&ActionId::from("action-1")).unwrap();
    assert_eq!(action.status, ConversationActionStatus::Denied);
    assert!(action.denial_reason.is_some());

    let events = audit.list().await.unwrap();
    assert_eq!(events[0].policy_decision, "deny");
    assert_eq!(events[0].result_status, "denied");
}

#[tokio::test]
async fn missing_registry_entry_denies_safely() {
    let kernel = test_kernel();
    let conversation_id = create_conversation(&kernel).await;
    let registry = registry();
    let executor = CountingExecutor::default();
    let audit = MemoryAuditSink::new();

    let outcome = process_action(
        &kernel,
        &registry,
        &executor,
        &audit,
        &conversation_id,
        "action-1",
        "unknown.action",
    )
    .await;

    assert!(matches!(outcome, ActionRuntimeOutcome::Denied { .. }));
    assert_eq!(executor.calls(), 0);

    let state = kernel.load_state(&conversation_id).await.unwrap();
    let action = state.actions.get(&ActionId::from("action-1")).unwrap();
    assert_eq!(action.status, ConversationActionStatus::Denied);
    assert_eq!(action.action_kind, ActionKind::from("unknown.action"));

    let events = audit.list().await.unwrap();
    assert_eq!(events[0].side_effect, "unknown");
    assert_eq!(events[0].result_status, "denied");
}

fn knowledge_registry() -> ActionRegistry {
    let mut registry = ActionRegistry::new();
    register_knowledge_action_schemas(&mut registry).unwrap();
    registry
}

fn knowledge_draft(title: &str, content_markdown: &str) -> KnowledgeEntryDraft {
    KnowledgeEntryDraft::new(title, content_markdown, Utc::now())
        .with_tags(vec!["agent-os".to_string()])
}

#[tokio::test]
async fn knowledge_search_action_executes_through_action_runtime() {
    let kernel = test_kernel();
    let conversation_id = create_conversation(&kernel).await;
    let registry = knowledge_registry();
    let repository = Arc::new(MemoryKnowledgeRepository::new());
    knowledge_entity::KnowledgeRepository::save_draft(
        repository.as_ref(),
        knowledge_draft("AgentOS Notes", "foundation content"),
    )
    .await
    .unwrap();
    let executor = KnowledgeActionExecutor::new(repository);
    let audit = MemoryAuditSink::new();

    let outcome = process_action_with_input(
        &kernel,
        &registry,
        &executor,
        &audit,
        &conversation_id,
        "action-knowledge-search",
        "knowledge.search",
        serde_json::to_value(KnowledgeSearchActionInput {
            query: KnowledgeSearchQuery::new("agentos"),
        })
        .unwrap(),
    )
    .await;

    let ActionRuntimeOutcome::Completed { result, .. } = outcome else {
        panic!("expected completed outcome");
    };
    let ActionResultPayload::Json(value) = result.payload else {
        panic!("expected json payload");
    };
    let results: Vec<KnowledgeSearchResult> = serde_json::from_value(value).unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].entry.title, "AgentOS Notes");

    let state = kernel.load_state(&conversation_id).await.unwrap();
    let action = state
        .actions
        .get(&ActionId::from("action-knowledge-search"))
        .unwrap();
    assert_eq!(action.status, ConversationActionStatus::Completed);

    let events = audit.list().await.unwrap();
    assert_eq!(events[0].policy_decision, "allow");
    assert_eq!(events[0].result_status, "completed");
}

#[tokio::test]
async fn knowledge_get_entry_action_executes_through_action_runtime() {
    let kernel = test_kernel();
    let conversation_id = create_conversation(&kernel).await;
    let registry = knowledge_registry();
    let repository = Arc::new(MemoryKnowledgeRepository::new());
    let saved = knowledge_entity::KnowledgeRepository::save_draft(
        repository.as_ref(),
        knowledge_draft("AgentOS Notes", "foundation content"),
    )
    .await
    .unwrap();
    let executor = KnowledgeActionExecutor::new(repository);
    let audit = MemoryAuditSink::new();

    let outcome = process_action_with_input(
        &kernel,
        &registry,
        &executor,
        &audit,
        &conversation_id,
        "action-knowledge-get",
        "knowledge.get_entry",
        serde_json::to_value(KnowledgeGetEntryActionInput {
            id: saved.id.clone(),
        })
        .unwrap(),
    )
    .await;

    let ActionRuntimeOutcome::Completed { result, .. } = outcome else {
        panic!("expected completed outcome");
    };
    let ActionResultPayload::Json(value) = result.payload else {
        panic!("expected json payload");
    };
    let entry: Option<KnowledgeEntryRef> = serde_json::from_value(value).unwrap();
    assert_eq!(entry, Some(saved));

    let state = kernel.load_state(&conversation_id).await.unwrap();
    let action = state
        .actions
        .get(&ActionId::from("action-knowledge-get"))
        .unwrap();
    assert_eq!(action.status, ConversationActionStatus::Completed);
}

#[tokio::test]
async fn knowledge_create_draft_requires_approval_through_action_runtime() {
    let kernel = test_kernel();
    let conversation_id = create_conversation(&kernel).await;
    let registry = knowledge_registry();
    let repository = Arc::new(MemoryKnowledgeRepository::new());
    let executor = KnowledgeActionExecutor::new(repository.clone());
    let audit = MemoryAuditSink::new();

    let outcome = process_action_with_input(
        &kernel,
        &registry,
        &executor,
        &audit,
        &conversation_id,
        "action-knowledge-create-draft",
        "knowledge.create_draft",
        serde_json::to_value(KnowledgeCreateDraftActionInput {
            title: "AgentOS Notes".to_string(),
            content_markdown: "draft content".to_string(),
            source_uri: None,
            source_artifact_id: None,
            source_asset_id: None,
            tags: vec!["agent-os".to_string()],
            metadata: serde_json::json!({}),
            created_at: Utc::now(),
        })
        .unwrap(),
    )
    .await;

    assert!(matches!(
        outcome,
        ActionRuntimeOutcome::ApprovalRequired { .. }
    ));
    assert!(
        knowledge_entity::KnowledgeRepository::list_entries(repository.as_ref())
            .await
            .unwrap()
            .is_empty()
    );

    let state = kernel.load_state(&conversation_id).await.unwrap();
    let action = state
        .actions
        .get(&ActionId::from("action-knowledge-create-draft"))
        .unwrap();
    assert_eq!(action.status, ConversationActionStatus::ApprovalRequired);

    let events = audit.list().await.unwrap();
    assert_eq!(events[0].policy_decision, "ask");
    assert_eq!(events[0].result_status, "approval_required");
}

#[tokio::test]
async fn knowledge_save_entry_denied_by_default_safe_policy_through_action_runtime() {
    let kernel = test_kernel();
    let conversation_id = create_conversation(&kernel).await;
    let registry = knowledge_registry();
    let repository = Arc::new(MemoryKnowledgeRepository::new());
    let executor = KnowledgeActionExecutor::new(repository.clone());
    let audit = MemoryAuditSink::new();

    let outcome = process_action_with_input(
        &kernel,
        &registry,
        &executor,
        &audit,
        &conversation_id,
        "action-knowledge-save",
        "knowledge.save_entry",
        serde_json::to_value(KnowledgeSaveEntryActionInput {
            draft: knowledge_draft("AgentOS Notes", "saved content"),
        })
        .unwrap(),
    )
    .await;

    assert!(matches!(outcome, ActionRuntimeOutcome::Denied { .. }));
    assert!(
        knowledge_entity::KnowledgeRepository::list_entries(repository.as_ref())
            .await
            .unwrap()
            .is_empty()
    );

    let state = kernel.load_state(&conversation_id).await.unwrap();
    let action = state
        .actions
        .get(&ActionId::from("action-knowledge-save"))
        .unwrap();
    assert_eq!(action.status, ConversationActionStatus::Denied);

    let events = audit.list().await.unwrap();
    assert_eq!(events[0].policy_decision, "deny");
    assert_eq!(events[0].result_status, "denied");
}

#[tokio::test]
async fn executor_failure_records_action_failed_and_audit() {
    let kernel = test_kernel();
    let conversation_id = create_conversation(&kernel).await;
    let registry = registry();
    let executor = CountingExecutor::failing();
    let audit = MemoryAuditSink::new();

    let outcome = process_action(
        &kernel,
        &registry,
        &executor,
        &audit,
        &conversation_id,
        "action-1",
        "knowledge.search",
    )
    .await;

    assert!(matches!(outcome, ActionRuntimeOutcome::Failed { .. }));
    assert_eq!(executor.calls(), 1);

    let state = kernel.load_state(&conversation_id).await.unwrap();
    let action = state.actions.get(&ActionId::from("action-1")).unwrap();
    assert_eq!(action.status, ConversationActionStatus::Failed);
    assert!(action.error_message.as_deref().unwrap().contains("boom"));

    let events = audit.list().await.unwrap();
    assert_eq!(events[0].policy_decision, "allow");
    assert_eq!(events[0].result_status, "failed");
}
