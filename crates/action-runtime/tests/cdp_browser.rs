//! Integration tests for CdpBrowserExecutor through ActionRuntime.
//!
//! These tests verify the routing and policy boundary for CDP browser actions.
//! The current CdpBrowserExecutor is a skeleton that returns ChromiumNotAvailable
//! for all known browser actions. Real Chromium integration tests are gated behind
//! the `browser-integration` feature flag.

use action_core::{ActionExecutor, ActionId, ActionRegistry, ActionRequest, SideEffectKind};
use action_runtime::ActionRuntime;
use audit_log::MemoryAuditSink;
use browser_entity::register_browser_action_schemas;
use browser_kernel_core::{CdpBrowserConfig, CdpBrowserExecutor, ChromiumLifecycleManager};
use capability_policy::CapabilityPolicy;
use chrono::{DateTime, Utc};
use conversation_core::*;
use conversation_journal::MemoryConversationJournal;
use conversation_kernel::*;
use std::sync::{Arc, Mutex};

// ---------------------------------------------------------------------------
// Test helpers (same pattern as static_browser.rs)
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
            title: Some("CDP browser test".to_string()),
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

fn cdp_executor() -> CdpBrowserExecutor {
    let config = CdpBrowserConfig::default();
    let lifecycle = ChromiumLifecycleManager::new(config);
    CdpBrowserExecutor::new(lifecycle, Utc::now())
}

fn browser_action_request(kind_str: &str) -> ActionRequest {
    ActionRequest {
        action_id: ActionId::from("cdp-test-1"),
        action_kind: action_core::ActionKind::from(kind_str),
        input: serde_json::json!({"url": "https://example.com"}),
        requested_by: "user-1".to_string(),
        conversation_id: Some("conv-1".to_string()),
        message_id: Some("msg-1".to_string()),
        requested_at: Utc::now(),
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn cdp_browser_executor_compiles_and_implements_action_executor() {
    let executor = cdp_executor();
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

#[tokio::test]
async fn cdp_browser_open_url_returns_chromium_not_available() {
    let executor = cdp_executor();
    let request = browser_action_request("browser.open_url");

    let result = executor.execute(&request).await;
    assert!(result.is_err(), "skeleton executor should return error");

    let err = result.unwrap_err();
    let err_msg = err.to_string();
    assert!(
        err_msg.contains("Chromium") || err_msg.contains("not available"),
        "expected ChromiumNotAvailable error, got: {}",
        err_msg
    );
}

#[tokio::test]
async fn cdp_browser_extract_content_returns_chromium_not_available() {
    let executor = cdp_executor();
    let request = browser_action_request("browser.extract_content");

    let result = executor.execute(&request).await;
    assert!(result.is_err());

    let err = result.unwrap_err();
    assert!(
        err.to_string().contains("Chromium") || err.to_string().contains("not available"),
        "expected ChromiumNotAvailable, got: {}",
        err
    );
}

#[tokio::test]
async fn cdp_browser_click_element_returns_chromium_not_available() {
    let executor = cdp_executor();
    let request = browser_action_request("browser.click_element");

    let result = executor.execute(&request).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn cdp_browser_fill_form_returns_chromium_not_available() {
    let executor = cdp_executor();
    let request = browser_action_request("browser.fill_form");

    let result = executor.execute(&request).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn cdp_browser_screenshot_returns_chromium_not_available() {
    let executor = cdp_executor();
    let request = browser_action_request("browser.get_page_screenshot");

    let result = executor.execute(&request).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn cdp_browser_execute_js_returns_chromium_not_available() {
    let executor = cdp_executor();
    let request = browser_action_request("browser.execute_js");

    let result = executor.execute(&request).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn cdp_browser_unknown_action_returns_not_supported() {
    let executor = cdp_executor();
    let request = browser_action_request("browser.nonexistent_action");

    let result = executor.execute(&request).await;
    assert!(result.is_err(), "unknown action should return error");

    let err = result.unwrap_err();
    assert!(
        err.to_string().contains("not supported"),
        "expected NotSupported, got: {}",
        err
    );
}

#[test]
fn cdp_browser_open_url_is_read_only_allowed() {
    let policy = CapabilityPolicy::default_safe();
    let request = browser_action_request("browser.open_url");

    // open_url is read-only in the action schema
    assert!(
        policy
            .evaluate(&request, &SideEffectKind::ReadOnly)
            .is_allowed()
    );
}

#[test]
fn cdp_browser_extract_content_is_read_only_allowed() {
    let policy = CapabilityPolicy::default_safe();
    let request = browser_action_request("browser.extract_content");

    assert!(
        policy
            .evaluate(&request, &SideEffectKind::ReadOnly)
            .is_allowed()
    );
}

#[test]
fn cdp_browser_click_element_requires_approval() {
    let policy = CapabilityPolicy::default_safe();
    let request = browser_action_request("browser.click_element");

    // click_element mutates external system — denied under default_safe
    let decision = policy.evaluate(&request, &SideEffectKind::ExternalSystemMutation);
    assert!(
        !decision.is_allowed(),
        "click_element with ExternalSystemMutation should be denied"
    );
}

#[test]
fn cdp_browser_fill_form_requires_approval() {
    let policy = CapabilityPolicy::default_safe();
    let request = browser_action_request("browser.fill_form");

    // fill_form mutates external system — denied under default_safe
    let decision = policy.evaluate(&request, &SideEffectKind::ExternalSystemMutation);
    assert!(
        !decision.is_allowed(),
        "fill_form with ExternalSystemMutation should be denied"
    );
}

#[test]
fn cdp_browser_execute_js_requires_approval() {
    let policy = CapabilityPolicy::default_safe();
    let request = browser_action_request("browser.execute_js");

    // execute_js has network access — asks for approval under default_safe
    let decision = policy.evaluate(&request, &SideEffectKind::NetworkAccess);
    assert!(
        decision.is_ask(),
        "execute_js with NetworkAccess should ask for approval"
    );
}
