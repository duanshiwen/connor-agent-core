//! # Action Core
//!
//! Defines the execution model for all external side effects in AgentOS.
//!
//! Every action that changes state outside the conversation kernel — navigating
//! a browser, saving a knowledge entry, sending an email — goes through the
//! action execution pipeline:
//!
//! ```text
//! ActionRequest → ActionRegistry lookup → CapabilityPolicy evaluation →
//!   Allow → execute → ActionCompleted
//!   Ask   → ActionApprovalRequired → wait → execute or deny
//!   Deny  → ActionDenied (never executes)
//! ```

use artifact_core::ArtifactId;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fmt;
use thiserror::Error;

// ────────────────────────────────────────────────────────────────────────────
// IDs
// ────────────────────────────────────────────────────────────────────────────

/// Unique identifier for an action invocation.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ActionId(pub String);

impl fmt::Display for ActionId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl From<String> for ActionId {
    fn from(value: String) -> Self {
        Self(value)
    }
}

impl From<&str> for ActionId {
    fn from(value: &str) -> Self {
        Self(value.to_string())
    }
}

// ────────────────────────────────────────────────────────────────────────────
// Action Kind
// ────────────────────────────────────────────────────────────────────────────

/// The kind of action, expressed as a dotted namespace string.
///
/// Examples: `browser.navigate`, `knowledge.search`, `reminder.create`
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ActionKind(pub String);

impl fmt::Display for ActionKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl From<String> for ActionKind {
    fn from(value: String) -> Self {
        Self(value)
    }
}

impl From<&str> for ActionKind {
    fn from(value: &str) -> Self {
        Self(value.to_string())
    }
}

// ────────────────────────────────────────────────────────────────────────────
// Side Effect Classification
// ────────────────────────────────────────────────────────────────────────────

/// Classifies the side effect severity of an action.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SideEffectKind {
    /// No side effects (pure computation).
    None,
    /// Read-only access to external resources.
    ReadOnly,
    /// Modifies runtime state (in-memory, session-local).
    RuntimeStateMutation,
    /// Modifies the local filesystem.
    FileSystemMutation,
    /// Makes network requests.
    NetworkAccess,
    /// Modifies external systems (APIs, databases).
    ExternalSystemMutation,
    /// UI side effects (showing notifications, changing surfaces).
    UiSideEffect,
    /// Controls device hardware.
    DeviceControl,
    /// Modifies sensitive user profiles or settings.
    SensitiveProfileMutation,
}

// ────────────────────────────────────────────────────────────────────────────
// Action Status
// ────────────────────────────────────────────────────────────────────────────

/// Lifecycle status of an action invocation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActionStatus {
    /// Action has been requested, awaiting policy evaluation.
    Pending,
    /// Policy requires human approval before execution.
    ApprovalRequired,
    /// Action has been approved and is running.
    Running,
    /// Action completed successfully.
    Completed,
    /// Action failed during execution.
    Failed,
    /// Action was denied by policy.
    Denied,
    /// Action was cancelled before or during execution.
    Cancelled,
}

// ────────────────────────────────────────────────────────────────────────────
// Action Result
// ────────────────────────────────────────────────────────────────────────────

/// The result payload of an action execution.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", content = "value")]
pub enum ActionResultPayload {
    /// Plain text result.
    Text(String),
    /// Structured JSON result.
    Json(serde_json::Value),
    /// Reference to an artifact created by this action.
    ArtifactRef(ArtifactId),
    /// Empty result (action had side effects but no data return).
    Empty,
}

/// Complete result of an action execution.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ActionResult {
    pub status: ActionStatus,
    pub payload: ActionResultPayload,
    pub summary: String,
    pub completed_at: DateTime<Utc>,
}

// ────────────────────────────────────────────────────────────────────────────
// Action Request
// ────────────────────────────────────────────────────────────────────────────

/// A request to execute an action.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ActionRequest {
    /// Unique ID for this invocation.
    pub action_id: ActionId,
    /// The kind of action to execute.
    pub action_kind: ActionKind,
    /// Serialized input parameters (action-specific JSON).
    pub input: serde_json::Value,
    /// Who requested this action (participant ID).
    pub requested_by: String,
    /// Related conversation ID.
    pub conversation_id: Option<String>,
    /// Related message ID.
    pub message_id: Option<String>,
    /// When the action was requested.
    pub requested_at: DateTime<Utc>,
}

// ────────────────────────────────────────────────────────────────────────────
// Action Schema (for registry)
// ────────────────────────────────────────────────────────────────────────────

/// Metadata about a registered action kind.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActionSchema {
    pub kind: ActionKind,
    pub display_name: String,
    pub description: String,
    pub side_effect: SideEffectKind,
    pub input_schema: Option<serde_json::Value>,
    pub output_schema: Option<serde_json::Value>,
}

// ────────────────────────────────────────────────────────────────────────────
// Action Registry
// ────────────────────────────────────────────────────────────────────────────

/// Error type for action registry operations.
#[derive(Debug, Clone, Error, PartialEq, Eq)]
pub enum ActionRegistryError {
    #[error("duplicate action kind: {0}")]
    DuplicateActionKind(ActionKind),
    #[error("action kind not found: {0}")]
    ActionKindNotFound(ActionKind),
}

/// Registry of known action kinds and their schemas.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ActionRegistry {
    actions: std::collections::BTreeMap<String, ActionSchema>,
}

impl ActionRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a new action kind.
    pub fn register(&mut self, schema: ActionSchema) -> Result<(), ActionRegistryError> {
        let key = schema.kind.0.clone();
        if self.actions.contains_key(&key) {
            return Err(ActionRegistryError::DuplicateActionKind(schema.kind));
        }
        self.actions.insert(key, schema);
        Ok(())
    }

    /// Look up an action kind.
    pub fn get(&self, kind: &ActionKind) -> Option<&ActionSchema> {
        self.actions.get(&kind.0)
    }

    /// Get the side effect classification for an action kind.
    pub fn side_effect(&self, kind: &ActionKind) -> Option<&SideEffectKind> {
        self.actions.get(&kind.0).map(|s| &s.side_effect)
    }

    pub fn len(&self) -> usize {
        self.actions.len()
    }

    pub fn is_empty(&self) -> bool {
        self.actions.is_empty()
    }
}

// ────────────────────────────────────────────────────────────────────────────
// Action Executor trait
// ────────────────────────────────────────────────────────────────────────────

/// Trait for executing actions. Implementations are action-kind-specific.
#[async_trait]
pub trait ActionExecutor: Send + Sync {
    /// Execute an action and return the result.
    async fn execute(&self, request: &ActionRequest) -> Result<ActionResult, ActionExecutorError>;
}

/// Errors from action execution.
#[derive(Debug, Clone, Error)]
pub enum ActionExecutorError {
    #[error("action not supported: {0}")]
    NotSupported(ActionKind),
    #[error("execution failed: {0}")]
    ExecutionFailed(String),
    #[error("invalid input: {0}")]
    InvalidInput(String),
}

// ────────────────────────────────────────────────────────────────────────────
// Fake Action Executor (for tests)
// ────────────────────────────────────────────────────────────────────────────

/// Deterministic fake executor for testing.
#[derive(Debug, Clone)]
pub struct FakeActionExecutor {
    response_text: String,
}

impl Default for FakeActionExecutor {
    fn default() -> Self {
        Self {
            response_text: "Action executed successfully".to_string(),
        }
    }
}

impl FakeActionExecutor {
    pub fn new(response_text: impl Into<String>) -> Self {
        Self {
            response_text: response_text.into(),
        }
    }
}

#[async_trait]
impl ActionExecutor for FakeActionExecutor {
    async fn execute(&self, request: &ActionRequest) -> Result<ActionResult, ActionExecutorError> {
        Ok(ActionResult {
            status: ActionStatus::Completed,
            payload: ActionResultPayload::Text(format!(
                "{}: {}",
                request.action_kind, self.response_text
            )),
            summary: format!("{} completed", request.action_kind),
            completed_at: chrono::Utc::now(),
        })
    }
}

// ────────────────────────────────────────────────────────────────────────────
// Tests
// ────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn action_id_serde_roundtrip() {
        let id = ActionId::from("action-001");
        let json = serde_json::to_string(&id).unwrap();
        let decoded: ActionId = serde_json::from_str(&json).unwrap();
        assert_eq!(id, decoded);
    }

    #[test]
    fn action_kind_serde_roundtrip() {
        let kind = ActionKind::from("browser.navigate");
        let json = serde_json::to_string(&kind).unwrap();
        let decoded: ActionKind = serde_json::from_str(&json).unwrap();
        assert_eq!(kind, decoded);
    }

    #[test]
    fn side_effect_kind_serde_roundtrip() {
        let kinds = vec![
            SideEffectKind::None,
            SideEffectKind::ReadOnly,
            SideEffectKind::RuntimeStateMutation,
            SideEffectKind::FileSystemMutation,
            SideEffectKind::NetworkAccess,
            SideEffectKind::ExternalSystemMutation,
            SideEffectKind::UiSideEffect,
            SideEffectKind::DeviceControl,
            SideEffectKind::SensitiveProfileMutation,
        ];
        for kind in kinds {
            let json = serde_json::to_string(&kind).unwrap();
            let decoded: SideEffectKind = serde_json::from_str(&json).unwrap();
            assert_eq!(decoded, kind);
        }
    }

    #[test]
    fn action_status_serde_roundtrip() {
        let statuses = vec![
            ActionStatus::Pending,
            ActionStatus::ApprovalRequired,
            ActionStatus::Running,
            ActionStatus::Completed,
            ActionStatus::Failed,
            ActionStatus::Denied,
            ActionStatus::Cancelled,
        ];
        for status in statuses {
            let json = serde_json::to_string(&status).unwrap();
            let decoded: ActionStatus = serde_json::from_str(&json).unwrap();
            assert_eq!(decoded, status);
        }
    }

    #[test]
    fn action_result_payload_serde_roundtrip() {
        let payloads = vec![
            ActionResultPayload::Text("hello".to_string()),
            ActionResultPayload::Json(serde_json::json!({"key": "value"})),
            ActionResultPayload::ArtifactRef(ArtifactId::from("artifact-001")),
            ActionResultPayload::Empty,
        ];
        for payload in payloads {
            let json = serde_json::to_string(&payload).unwrap();
            let decoded: ActionResultPayload = serde_json::from_str(&json).unwrap();
            assert_eq!(
                serde_json::to_string(&decoded).unwrap(),
                serde_json::to_string(&payload).unwrap()
            );
        }
    }

    #[test]
    fn artifact_ref_payload_uses_stable_json_shape() {
        let payload = ActionResultPayload::ArtifactRef(ArtifactId::from("artifact-001"));
        let json = serde_json::to_value(&payload).unwrap();
        assert_eq!(
            json,
            serde_json::json!({"type":"ArtifactRef","value":"artifact-001"})
        );

        let decoded: ActionResultPayload = serde_json::from_value(json).unwrap();
        assert_eq!(decoded, payload);
    }

    #[test]
    fn action_request_serde_roundtrip() {
        let request = ActionRequest {
            action_id: ActionId::from("action-001"),
            action_kind: ActionKind::from("knowledge.search"),
            input: serde_json::json!({"query": "test"}),
            requested_by: "u1".to_string(),
            conversation_id: Some("conv-1".to_string()),
            message_id: Some("msg-1".to_string()),
            requested_at: chrono::Utc::now(),
        };
        let json = serde_json::to_string(&request).unwrap();
        let decoded: ActionRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded.action_id, request.action_id);
        assert_eq!(decoded.action_kind, request.action_kind);
    }

    #[test]
    fn action_schema_serde_roundtrip() {
        let schema = ActionSchema {
            kind: ActionKind::from("browser.navigate"),
            display_name: "Navigate".to_string(),
            description: "Navigate to a URL".to_string(),
            side_effect: SideEffectKind::NetworkAccess,
            input_schema: None,
            output_schema: None,
        };
        let json = serde_json::to_string(&schema).unwrap();
        let decoded: ActionSchema = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded.kind, schema.kind);
        assert_eq!(decoded.side_effect, schema.side_effect);
    }

    #[test]
    fn registry_register_and_lookup() {
        let mut registry = ActionRegistry::new();
        registry
            .register(ActionSchema {
                kind: ActionKind::from("knowledge.search"),
                display_name: "Search Knowledge".to_string(),
                description: "Search the knowledge base".to_string(),
                side_effect: SideEffectKind::ReadOnly,
                input_schema: None,
                output_schema: None,
            })
            .unwrap();

        let schema = registry.get(&ActionKind::from("knowledge.search")).unwrap();
        assert_eq!(schema.display_name, "Search Knowledge");
        assert_eq!(schema.side_effect, SideEffectKind::ReadOnly);
    }

    #[test]
    fn registry_rejects_duplicate() {
        let mut registry = ActionRegistry::new();
        registry
            .register(ActionSchema {
                kind: ActionKind::from("test.action"),
                display_name: "Test".to_string(),
                description: "Test".to_string(),
                side_effect: SideEffectKind::None,
                input_schema: None,
                output_schema: None,
            })
            .unwrap();

        let result = registry.register(ActionSchema {
            kind: ActionKind::from("test.action"),
            display_name: "Test 2".to_string(),
            description: "Test 2".to_string(),
            side_effect: SideEffectKind::ReadOnly,
            input_schema: None,
            output_schema: None,
        });

        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            ActionRegistryError::DuplicateActionKind(_)
        ));
    }

    #[test]
    fn registry_side_effect_lookup() {
        let mut registry = ActionRegistry::new();
        registry
            .register(ActionSchema {
                kind: ActionKind::from("reminder.create"),
                display_name: "Create Reminder".to_string(),
                description: "Create a reminder".to_string(),
                side_effect: SideEffectKind::RuntimeStateMutation,
                input_schema: None,
                output_schema: None,
            })
            .unwrap();

        let se = registry.side_effect(&ActionKind::from("reminder.create"));
        assert_eq!(se, Some(&SideEffectKind::RuntimeStateMutation));

        let missing = registry.side_effect(&ActionKind::from("nonexistent"));
        assert!(missing.is_none());
    }

    #[tokio::test]
    async fn fake_executor_returns_success() {
        let executor = FakeActionExecutor::new("Search complete");
        let request = ActionRequest {
            action_id: ActionId::from("action-001"),
            action_kind: ActionKind::from("knowledge.search"),
            input: serde_json::json!({"query": "test"}),
            requested_by: "u1".to_string(),
            conversation_id: None,
            message_id: None,
            requested_at: chrono::Utc::now(),
        };

        let result = executor.execute(&request).await.unwrap();
        assert_eq!(result.status, ActionStatus::Completed);
        match &result.payload {
            ActionResultPayload::Text(text) => {
                assert!(text.contains("knowledge.search"));
                assert!(text.contains("Search complete"));
            }
            _ => panic!("expected Text payload"),
        }
    }
}
