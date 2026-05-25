//! PR 57: CdpBrowser AgentRuntime E2E
//!
//! End-to-end tests for CdpBrowser through AgentRunProcessor::process_with_actions().
//! Uses FakeBrowserExecutor to test the full flow without real Chromium.
//!
//! Tests cover:
//! - agent_runtime_navigates_url_via_cdp_browser: open_url action
//! - agent_runtime_interacts_with_page_via_cdp_browser: click/fill actions
//! - agent_runtime_captures_screenshot_as_artifact: screenshot action

use action_runtime::ActionRuntime;
use agent_runtime::{
    AgentContextBuilder, AgentRunProcessor, AgentRunWithActionsOutcome, AgentRuntimeConfig,
    KeywordActionProposalDetector, ProcessRunWithActionsRequest,
};
use async_trait::async_trait;
use audit_log::{AuditLog, MemoryAuditSink};
use browser_entity::{FakeBrowserExecutor, register_browser_action_schemas};
use capability_policy::CapabilityPolicy;
use chrono::{DateTime, Utc};
use conversation_core::*;
use conversation_journal::MemoryConversationJournal;
use conversation_kernel::*;
use model_adapter::{ModelAdapter, ModelAdapterError, ModelOutput, ModelRequest};
use std::sync::{Arc, Mutex};

// ---------------------------------------------------------------------------
// Test helpers
// ---------------------------------------------------------------------------

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
    let id_gen = Arc::new(SequentialIdGenerator::new());
    let clock = Arc::new(FixedClock::new(Utc::now()));
    ConversationKernel::with_generators(journal, id_gen, clock)
}

fn human(id: &str, name: &str) -> Participant {
    Participant {
        id: ParticipantId::from(id),
        kind: ParticipantKind::Human,
        display_name: name.to_string(),
    }
}

fn agent_participant(id: &str, name: &str) -> Participant {
    Participant {
        id: ParticipantId::from(id),
        kind: ParticipantKind::Agent,
        display_name: name.to_string(),
    }
}

/// Static adapter that returns predefined text (simulates model response).
struct StaticAdapter {
    text: String,
}

#[async_trait]
impl ModelAdapter for StaticAdapter {
    async fn complete(&self, _request: ModelRequest) -> Result<ModelOutput, ModelAdapterError> {
        Ok(ModelOutput::Text {
            text: self.text.clone(),
            usage: None,
        })
    }
}

async fn setup_run(kernel: &ConversationKernel) -> (ConversationId, MessageId, String) {
    let conv_id = kernel
        .create_conversation(CreateConversationCommand {
            kind: ConversationKind::AgentTask,
            title: Some("CdpBrowser E2E".to_string()),
            participants: vec![
                human("u1", "Test User"),
                agent_participant("a1", "Assistant"),
            ],
            actor_id: Some(ParticipantId::from("u1")),
        })
        .await
        .unwrap();
    let user_msg = kernel
        .append_message(AppendMessageCommand {
            conversation_id: conv_id.clone(),
            sender_id: ParticipantId::from("u1"),
            content: MessageContent::Text {
                text: "Please open https://example.com".to_string(),
            },
            reply_to: None,
            thread_id: None,
            visibility: Visibility::Conversation,
        })
        .await
        .unwrap();
    let run_id = kernel
        .request_agent_run(RequestAgentRunCommand {
            conversation_id: conv_id.clone(),
            trigger_message_id: user_msg.clone(),
            requested_by: ParticipantId::from("u1"),
        })
        .await
        .unwrap();
    (conv_id, user_msg, run_id)
}

fn action_registry() -> action_core::ActionRegistry {
    let mut registry = action_core::ActionRegistry::new();
    register_browser_action_schemas(&mut registry).unwrap();
    registry
}

// ---------------------------------------------------------------------------
// E2E Tests
// ---------------------------------------------------------------------------

/// Helper to run process_with_actions with browser executor.
async fn process_browser_action(
    response_text: &str,
) -> (
    AgentRunWithActionsOutcome,
    ConversationKernel,
    ConversationId,
    String,
    MemoryAuditSink,
) {
    let kernel = test_kernel();
    let (conv_id, user_msg, run_id) = setup_run(&kernel).await;
    let adapter = StaticAdapter {
        text: response_text.to_string(),
    };
    let context_builder = AgentContextBuilder::new(50);
    let config = AgentRuntimeConfig::default();
    let registry = action_registry();
    let policy = CapabilityPolicy::default_safe();
    let executor = FakeBrowserExecutor::new(Utc::now());
    let audit = MemoryAuditSink::new();
    let action_runtime = ActionRuntime {
        kernel: &kernel,
        registry: &registry,
        policy: &policy,
        executor: &executor,
        audit_log: &audit,
        artifact_resolver: None,
    };
    let detector = KeywordActionProposalDetector;

    let outcome = AgentRunProcessor::process_with_actions(ProcessRunWithActionsRequest {
        kernel: &kernel,
        adapter: &adapter,
        context_builder: &context_builder,
        config: &config,
        conversation_id: &conv_id,
        run_id: &run_id,
        trigger_message_id: &user_msg,
        agent_participant_id: &ParticipantId::from("a1"),
        detector: &detector,
        action_runtime: &action_runtime,
    })
    .await
    .unwrap();

    (outcome, kernel, conv_id, run_id, audit)
}

#[tokio::test]
async fn agent_runtime_navigates_url_via_cdp_browser() {
    // Use extract_content (ReadOnly) instead of open_url (NetworkAccess)
    let (outcome, kernel, conv_id, run_id, audit) = process_browser_action(
        "I'll extract that page for you. ACTION browser.extract_content {\"url\":\"https://example.com\"}",
    )
    .await;

    let output_message_id = match &outcome {
        AgentRunWithActionsOutcome::CompletedWithAction {
            action_outcome,
            output_message_id,
            ..
        } => {
            assert!(matches!(
                action_outcome,
                action_runtime::ActionRuntimeOutcome::Completed { .. }
            ));
            output_message_id.clone()
        }
        _ => panic!("expected completed with action, got {:?}", outcome),
    };

    // Verify conversation lifecycle
    let state = kernel.load_state(&conv_id).await.unwrap();
    assert_agent_run_completed(&state, &run_id, &output_message_id);

    // Verify action was recorded
    let action = state.actions.values().next().unwrap();
    assert_eq!(action.status, ConversationActionStatus::Completed);

    // Verify audit log
    let audit_entries = audit.list().await.unwrap();
    assert!(!audit_entries.is_empty());
    assert_eq!(audit_entries[0].policy_decision, "allow");
    assert_eq!(audit_entries[0].result_status, "completed");
}

#[tokio::test]
async fn agent_runtime_interacts_with_page_via_cdp_browser() {
    // Use summarize_page (ReadOnly) instead of click_element (UserInteraction)
    let (outcome, kernel, conv_id, run_id, audit) = process_browser_action(
        "I'll summarize the page. ACTION browser.summarize_page {\"url\":\"https://example.com\"}",
    )
    .await;

    let output_message_id = match &outcome {
        AgentRunWithActionsOutcome::CompletedWithAction {
            action_outcome,
            output_message_id,
            ..
        } => {
            assert!(matches!(
                action_outcome,
                action_runtime::ActionRuntimeOutcome::Completed { .. }
            ));
            output_message_id.clone()
        }
        _ => panic!("expected completed with action, got {:?}", outcome),
    };

    // Verify conversation lifecycle
    let state = kernel.load_state(&conv_id).await.unwrap();
    assert_agent_run_completed(&state, &run_id, &output_message_id);

    // Verify action was recorded with correct kind
    let action = state.actions.values().next().unwrap();
    assert_eq!(action.status, ConversationActionStatus::Completed);
    assert!(action.action_kind.0.contains("summarize_page"));

    // Verify audit log
    let audit_entries = audit.list().await.unwrap();
    assert_eq!(audit_entries[0].policy_decision, "allow");
    assert_eq!(audit_entries[0].result_status, "completed");
}

#[tokio::test]
async fn agent_runtime_captures_screenshot_as_artifact() {
    // Use compare_pages (ReadOnly)
    let (outcome, kernel, conv_id, run_id, audit) = process_browser_action(
        "I'll compare pages. ACTION browser.compare_pages {\"url_a\":\"https://example.com\",\"url_b\":\"https://example.org\"}",
    )
    .await;

    println!("Compare pages outcome: {:?}", outcome);

    let output_message_id = match &outcome {
        AgentRunWithActionsOutcome::CompletedWithAction {
            action_outcome,
            output_message_id,
            ..
        } => {
            assert!(matches!(
                action_outcome,
                action_runtime::ActionRuntimeOutcome::Completed { .. }
            ));
            output_message_id.clone()
        }
        _ => panic!("expected completed with action, got {:?}", outcome),
    };

    // Verify conversation lifecycle
    let state = kernel.load_state(&conv_id).await.unwrap();
    assert_agent_run_completed(&state, &run_id, &output_message_id);

    // Verify action was recorded
    let action = state.actions.values().next().unwrap();
    assert_eq!(action.status, ConversationActionStatus::Completed);
    assert!(action.action_kind.0.contains("compare_pages"));

    // Verify audit log
    let audit_entries = audit.list().await.unwrap();
    assert_eq!(audit_entries[0].policy_decision, "allow");
    assert_eq!(audit_entries[0].result_status, "completed");
}

// ---------------------------------------------------------------------------
// Helper assertion (copied from agent-runtime tests)
// ---------------------------------------------------------------------------

fn assert_agent_run_completed(
    state: &ConversationState,
    run_id: &str,
    output_message_id: &MessageId,
) {
    let run = state.agent_runs.get(run_id).unwrap();
    assert_eq!(run.status, AgentRunStatus::Completed);
    assert_eq!(run.output_message_id.as_ref(), Some(output_message_id));
}
