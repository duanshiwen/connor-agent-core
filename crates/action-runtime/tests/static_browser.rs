//! Integration tests for StaticHtmlBrowserExecutor through ActionRuntime.

use action_core::{ActionId, ActionRegistry, SideEffectKind};
use action_runtime::ActionRuntime;
use audit_log::MemoryAuditSink;
use browser_entity::{
    StaticHtmlBrowserExecutor, browser_extract_content_action_kind,
    browser_summarize_page_action_kind, register_browser_action_schemas,
};
use capability_policy::CapabilityPolicy;
use chrono::{DateTime, Utc};
use conversation_core::*;
use conversation_journal::MemoryConversationJournal;
use conversation_kernel::*;
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
            title: Some("Static browser test".to_string()),
            participants: vec![user(), agent()],
            actor_id: Some(ParticipantId::from("user-1")),
        })
        .await
        .unwrap()
}

fn browser_registry() -> ActionRegistry {
    let mut registry = ActionRegistry::new();
    register_browser_action_schemas(&mut registry).unwrap();
    registry
}

#[tokio::test]
async fn static_browser_executor_compiles_and_implements_action_executor() {
    let executor = StaticHtmlBrowserExecutor::new(Utc::now());
    let registry = browser_registry();
    let policy = CapabilityPolicy::default_safe();
    let kernel = test_kernel();
    let audit = MemoryAuditSink::new();

    let _conv_id = create_conversation(&kernel).await;

    // Verify the executor can be used in ActionRuntime without errors
    let _action_runtime = ActionRuntime {
        kernel: &kernel,
        registry: &registry,
        policy: &policy,
        executor: &executor,
        audit_log: &audit,
        artifact_resolver: None,
    };
}

#[test]
fn static_browser_extract_content_is_read_only_allowed() {
    let policy = CapabilityPolicy::default_safe();
    let req = action_core::ActionRequest {
        action_id: ActionId::from("test-extract"),
        action_kind: browser_extract_content_action_kind(),
        input: serde_json::json!({"url": "https://example.com"}),
        requested_by: "user-1".to_string(),
        conversation_id: Some("conv-1".to_string()),
        message_id: Some("msg-1".to_string()),
        requested_at: Utc::now(),
    };
    assert!(
        policy
            .evaluate(&req, &SideEffectKind::ReadOnly)
            .is_allowed()
    );
}

#[test]
fn static_browser_summarize_is_read_only_allowed() {
    let policy = CapabilityPolicy::default_safe();
    let req = action_core::ActionRequest {
        action_id: ActionId::from("test-summarize"),
        action_kind: browser_summarize_page_action_kind(),
        input: serde_json::json!({"url": "https://example.com", "max_length": 200}),
        requested_by: "user-1".to_string(),
        conversation_id: Some("conv-1".to_string()),
        message_id: Some("msg-1".to_string()),
        requested_at: Utc::now(),
    };
    assert!(
        policy
            .evaluate(&req, &SideEffectKind::ReadOnly)
            .is_allowed()
    );
}
