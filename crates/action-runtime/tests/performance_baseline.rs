use std::sync::{Arc, Mutex};
use std::time::Instant;

use action_core::{
    ActionExecutor, ActionExecutorError, ActionId, ActionKind, ActionRegistry, ActionRequest,
    ActionResult, ActionResultPayload, ActionSchema, ActionStatus, SideEffectKind,
};
use action_runtime::{ActionRuntime, ActionRuntimeOutcome, ProcessActionRequest};
use async_trait::async_trait;
use audit_log::MemoryAuditSink;
use capability_policy::CapabilityPolicy;
use chrono::{DateTime, Utc};
use conversation_core::{
    ConversationId, ConversationKind, Participant, ParticipantId, ParticipantKind,
};
use conversation_journal::MemoryConversationJournal;
use conversation_kernel::{Clock, ConversationKernel, CreateConversationCommand, IdGenerator};

const ACTION_RUNTIME_ITERATIONS: usize = 50;

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

#[derive(Default)]
struct NoopExecutor;

#[async_trait]
impl ActionExecutor for NoopExecutor {
    async fn execute(&self, request: &ActionRequest) -> Result<ActionResult, ActionExecutorError> {
        Ok(ActionResult {
            status: ActionStatus::Completed,
            payload: ActionResultPayload::Text(format!("{} ok", request.action_kind)),
            summary: format!("{} completed", request.action_kind),
            completed_at: Utc::now(),
        })
    }
}

fn test_kernel() -> ConversationKernel {
    ConversationKernel::with_generators(
        Arc::new(MemoryConversationJournal::new()),
        Arc::new(SequentialIdGenerator::new()),
        Arc::new(FixedClock::new(Utc::now())),
    )
}

fn registry() -> ActionRegistry {
    let mut registry = ActionRegistry::new();
    registry
        .register(ActionSchema {
            kind: ActionKind::from("knowledge.search"),
            display_name: "knowledge.search".to_string(),
            description: "performance baseline action".to_string(),
            side_effect: SideEffectKind::ReadOnly,
            input_schema: None,
            output_schema: None,
        })
        .unwrap();
    registry
}

async fn create_conversation(kernel: &ConversationKernel) -> ConversationId {
    kernel
        .create_conversation(CreateConversationCommand {
            kind: ConversationKind::AgentTask,
            title: Some("Action runtime baseline".to_string()),
            participants: vec![Participant {
                id: ParticipantId::from("user-1"),
                kind: ParticipantKind::Human,
                display_name: "User".to_string(),
            }],
            actor_id: Some(ParticipantId::from("user-1")),
        })
        .await
        .unwrap()
}

fn action_request(conversation_id: &ConversationId, index: usize) -> ActionRequest {
    ActionRequest {
        action_id: ActionId::from(format!("action-{index:04}")),
        action_kind: ActionKind::from("knowledge.search"),
        input: serde_json::json!({"query":"agent os baseline"}),
        requested_by: "user-1".to_string(),
        conversation_id: Some(conversation_id.to_string()),
        message_id: None,
        requested_at: Utc::now(),
    }
}

#[tokio::test]
async fn action_runtime_50_readonly_actions_baseline() {
    let kernel = test_kernel();
    let conversation_id = create_conversation(&kernel).await;
    let registry = registry();
    let policy = CapabilityPolicy::default_safe();
    let executor = NoopExecutor;
    let audit_log = MemoryAuditSink::new();
    let runtime = ActionRuntime {
        kernel: &kernel,
        registry: &registry,
        policy: &policy,
        executor: &executor,
        audit_log: &audit_log,
        artifact_resolver: None,
    };

    let started = Instant::now();
    for index in 0..ACTION_RUNTIME_ITERATIONS {
        let outcome = runtime
            .process(ProcessActionRequest {
                conversation_id: &conversation_id,
                action_request: action_request(&conversation_id, index),
                requested_by: Some(ParticipantId::from("user-1")),
                runtime_actor: Some(ParticipantId::from("user-1")),
            })
            .await
            .unwrap();
        assert!(matches!(outcome, ActionRuntimeOutcome::Completed { .. }));
    }
    let elapsed = started.elapsed();

    assert!(
        elapsed.as_millis() < 1_000,
        "action runtime baseline regressed: processed {ACTION_RUNTIME_ITERATIONS} actions in {elapsed:?}"
    );
    eprintln!(
        "performance baseline: action runtime processed {ACTION_RUNTIME_ITERATIONS} read-only actions in {elapsed:?}"
    );
}
