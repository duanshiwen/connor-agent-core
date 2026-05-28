//! # Reminder Core
//!
//! Domain types, events, action schemas, and static executor for AgentOS Reminders.
//!
//! The Reminder Entity enables the assistant to create, complete, cancel, and snooze
//! reminders. It integrates with the scheduler and notification subsystems.

use action_core::{
    ActionExecutor, ActionExecutorError, ActionKind, ActionRegistry, ActionRegistryError,
    ActionRequest, ActionResult, ActionResultPayload, ActionSchema, ActionStatus, SideEffectKind,
};
use artifact_core::ArtifactId;
use async_trait::async_trait;
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use std::fmt;

// ---------------------------------------------------------------------------
// Action kind constants
// ---------------------------------------------------------------------------

pub const REMINDER_CREATE_ACTION_KIND: &str = "reminder.create";
pub const REMINDER_COMPLETE_ACTION_KIND: &str = "reminder.complete";
pub const REMINDER_CANCEL_ACTION_KIND: &str = "reminder.cancel";
pub const REMINDER_SNOOZE_ACTION_KIND: &str = "reminder.snooze";

pub fn reminder_create_action_kind() -> ActionKind {
    ActionKind::from(REMINDER_CREATE_ACTION_KIND)
}

pub fn reminder_complete_action_kind() -> ActionKind {
    ActionKind::from(REMINDER_COMPLETE_ACTION_KIND)
}

pub fn reminder_cancel_action_kind() -> ActionKind {
    ActionKind::from(REMINDER_CANCEL_ACTION_KIND)
}

pub fn reminder_snooze_action_kind() -> ActionKind {
    ActionKind::from(REMINDER_SNOOZE_ACTION_KIND)
}

// ---------------------------------------------------------------------------
// Core types
// ---------------------------------------------------------------------------

/// Unique identifier for a reminder.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ReminderId(pub String);

impl fmt::Display for ReminderId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl From<String> for ReminderId {
    fn from(s: String) -> Self {
        Self(s)
    }
}

impl From<&str> for ReminderId {
    fn from(s: &str) -> Self {
        Self(s.to_string())
    }
}

/// Time specification for when a reminder should fire.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ReminderTimeSpec {
    /// Absolute time — fire at this exact datetime.
    Absolute(DateTime<Utc>),
    /// Relative time — fire after this duration from creation.
    Relative(Duration),
}

impl ReminderTimeSpec {
    /// Resolve to an absolute time given a reference "now".
    pub fn resolve(&self, now: DateTime<Utc>) -> DateTime<Utc> {
        match self {
            ReminderTimeSpec::Absolute(dt) => *dt,
            ReminderTimeSpec::Relative(dur) => now + *dur,
        }
    }
}

/// Status of a reminder.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ReminderStatus {
    /// Created but not yet due.
    Pending,
    /// Due — time has arrived, notification pending.
    Due,
    /// User completed the reminder.
    Completed,
    /// User cancelled the reminder.
    Cancelled,
    /// User snoozed — will re-fire after snooze duration.
    Snoozed,
}

impl fmt::Display for ReminderStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ReminderStatus::Pending => write!(f, "Pending"),
            ReminderStatus::Due => write!(f, "Due"),
            ReminderStatus::Completed => write!(f, "Completed"),
            ReminderStatus::Cancelled => write!(f, "Cancelled"),
            ReminderStatus::Snoozed => write!(f, "Snoozed"),
        }
    }
}

/// Recurrence pattern for a reminder.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ReminderRecurrence {
    /// One-time reminder.
    None,
    /// Repeat every day.
    Daily,
    /// Repeat every week.
    Weekly,
    /// Repeat every month.
    Monthly,
    /// Custom interval.
    Custom(Duration),
}

/// Source of the reminder.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ReminderSource {
    /// Created by user request.
    UserCreated,
    /// Created by system.
    SystemGenerated,
}

/// A complete reminder entity.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Reminder {
    pub id: ReminderId,
    pub title: String,
    pub description: String,
    pub time_spec: ReminderTimeSpec,
    pub status: ReminderStatus,
    pub recurrence: ReminderRecurrence,
    pub source: ReminderSource,
    pub artifact_id: Option<ArtifactId>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

// ---------------------------------------------------------------------------
// Action inputs
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReminderCreateInput {
    pub title: String,
    pub description: String,
    pub time_spec: ReminderTimeSpec,
    pub recurrence: ReminderRecurrence,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReminderCompleteInput {
    pub reminder_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReminderCancelInput {
    pub reminder_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReminderSnoozeInput {
    pub reminder_id: String,
    pub snooze_duration: Duration,
}

// ---------------------------------------------------------------------------
// Repository trait
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ReminderRepositoryError {
    #[error("reminder not found: {0}")]
    NotFound(String),
    #[error("invalid state transition: {from} → {to}")]
    InvalidTransition {
        from: ReminderStatus,
        to: ReminderStatus,
    },
    #[error("internal error: {0}")]
    Internal(String),
}

/// Trait for persisting and querying reminders.
#[async_trait]
pub trait ReminderRepository: Send + Sync {
    async fn save(&self, reminder: &Reminder) -> Result<(), ReminderRepositoryError>;
    async fn get(&self, id: &ReminderId) -> Result<Reminder, ReminderRepositoryError>;
    async fn list(&self) -> Result<Vec<Reminder>, ReminderRepositoryError>;
    async fn list_due(&self) -> Result<Vec<Reminder>, ReminderRepositoryError>;
    async fn update_status(
        &self,
        id: &ReminderId,
        status: ReminderStatus,
    ) -> Result<(), ReminderRepositoryError>;
}

// ---------------------------------------------------------------------------
// MemoryReminderRepository
// ---------------------------------------------------------------------------

use std::collections::HashMap;
use std::sync::Mutex;

pub struct MemoryReminderRepository {
    inner: Mutex<HashMap<String, Reminder>>,
}

impl MemoryReminderRepository {
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(HashMap::new()),
        }
    }
}

impl Default for MemoryReminderRepository {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl ReminderRepository for MemoryReminderRepository {
    async fn save(&self, reminder: &Reminder) -> Result<(), ReminderRepositoryError> {
        let mut store = self.inner.lock().unwrap();
        store.insert(reminder.id.0.clone(), reminder.clone());
        Ok(())
    }

    async fn get(&self, id: &ReminderId) -> Result<Reminder, ReminderRepositoryError> {
        let store = self.inner.lock().unwrap();
        store
            .get(&id.0)
            .cloned()
            .ok_or_else(|| ReminderRepositoryError::NotFound(id.0.clone()))
    }

    async fn list(&self) -> Result<Vec<Reminder>, ReminderRepositoryError> {
        let store = self.inner.lock().unwrap();
        Ok(store.values().cloned().collect())
    }

    async fn list_due(&self) -> Result<Vec<Reminder>, ReminderRepositoryError> {
        let store = self.inner.lock().unwrap();
        Ok(store
            .values()
            .filter(|r| r.status == ReminderStatus::Due)
            .cloned()
            .collect())
    }

    async fn update_status(
        &self,
        id: &ReminderId,
        status: ReminderStatus,
    ) -> Result<(), ReminderRepositoryError> {
        let mut store = self.inner.lock().unwrap();
        if let Some(reminder) = store.get_mut(&id.0) {
            reminder.status = status;
            reminder.updated_at = Utc::now();
            Ok(())
        } else {
            Err(ReminderRepositoryError::NotFound(id.0.clone()))
        }
    }
}

// ---------------------------------------------------------------------------
// Action schema registration
// ---------------------------------------------------------------------------

pub fn register_reminder_action_schemas(
    registry: &mut ActionRegistry,
) -> Result<(), ActionRegistryError> {
    registry.register(ActionSchema {
        kind: reminder_create_action_kind(),
        display_name: "Create Reminder".to_string(),
        description: "Create a new reminder.".to_string(),
        side_effect: SideEffectKind::RuntimeStateMutation,
        input_schema: None,
        output_schema: None,
    })?;
    registry.register(ActionSchema {
        kind: reminder_complete_action_kind(),
        display_name: "Complete Reminder".to_string(),
        description: "Mark a reminder as completed.".to_string(),
        side_effect: SideEffectKind::RuntimeStateMutation,
        input_schema: None,
        output_schema: None,
    })?;
    registry.register(ActionSchema {
        kind: reminder_cancel_action_kind(),
        display_name: "Cancel Reminder".to_string(),
        description: "Cancel a reminder.".to_string(),
        side_effect: SideEffectKind::RuntimeStateMutation,
        input_schema: None,
        output_schema: None,
    })?;
    registry.register(ActionSchema {
        kind: reminder_snooze_action_kind(),
        display_name: "Snooze Reminder".to_string(),
        description: "Snooze a reminder for a specified duration.".to_string(),
        side_effect: SideEffectKind::RuntimeStateMutation,
        input_schema: None,
        output_schema: None,
    })?;
    Ok(())
}

// ---------------------------------------------------------------------------
// ReminderActionExecutor
// ---------------------------------------------------------------------------

pub struct ReminderActionExecutor {
    repo: std::sync::Arc<dyn ReminderRepository>,
}

impl ReminderActionExecutor {
    pub fn new(repo: std::sync::Arc<dyn ReminderRepository>) -> Self {
        Self { repo }
    }
}

#[async_trait]
impl ActionExecutor for ReminderActionExecutor {
    async fn execute(&self, request: &ActionRequest) -> Result<ActionResult, ActionExecutorError> {
        let now = Utc::now();
        let payload = match request.action_kind.0.as_str() {
            REMINDER_CREATE_ACTION_KIND => {
                let input: ReminderCreateInput = serde_json::from_value(request.input.clone())
                    .map_err(|e| ActionExecutorError::InvalidInput(e.to_string()))?;
                let _due_at = input.time_spec.resolve(input.created_at);
                let reminder = Reminder {
                    id: ReminderId(request.action_id.to_string()),
                    title: input.title,
                    description: input.description,
                    time_spec: input.time_spec,
                    status: ReminderStatus::Pending,
                    recurrence: input.recurrence,
                    source: ReminderSource::UserCreated,
                    artifact_id: None,
                    created_at: input.created_at,
                    updated_at: input.created_at,
                };
                self.repo
                    .save(&reminder)
                    .await
                    .map_err(|e| ActionExecutorError::ExecutionFailed(e.to_string()))?;
                ActionResultPayload::Json(
                    serde_json::to_value(&reminder)
                        .map_err(|e| ActionExecutorError::ExecutionFailed(e.to_string()))?,
                )
            }
            REMINDER_COMPLETE_ACTION_KIND => {
                let input: ReminderCompleteInput = serde_json::from_value(request.input.clone())
                    .map_err(|e| ActionExecutorError::InvalidInput(e.to_string()))?;
                let id = ReminderId(input.reminder_id);
                self.repo
                    .update_status(&id, ReminderStatus::Completed)
                    .await
                    .map_err(|e| ActionExecutorError::ExecutionFailed(e.to_string()))?;
                let reminder = self
                    .repo
                    .get(&id)
                    .await
                    .map_err(|e| ActionExecutorError::ExecutionFailed(e.to_string()))?;
                ActionResultPayload::Json(
                    serde_json::to_value(&reminder)
                        .map_err(|e| ActionExecutorError::ExecutionFailed(e.to_string()))?,
                )
            }
            REMINDER_CANCEL_ACTION_KIND => {
                let input: ReminderCancelInput = serde_json::from_value(request.input.clone())
                    .map_err(|e| ActionExecutorError::InvalidInput(e.to_string()))?;
                let id = ReminderId(input.reminder_id);
                self.repo
                    .update_status(&id, ReminderStatus::Cancelled)
                    .await
                    .map_err(|e| ActionExecutorError::ExecutionFailed(e.to_string()))?;
                let reminder = self
                    .repo
                    .get(&id)
                    .await
                    .map_err(|e| ActionExecutorError::ExecutionFailed(e.to_string()))?;
                ActionResultPayload::Json(
                    serde_json::to_value(&reminder)
                        .map_err(|e| ActionExecutorError::ExecutionFailed(e.to_string()))?,
                )
            }
            REMINDER_SNOOZE_ACTION_KIND => {
                let input: ReminderSnoozeInput = serde_json::from_value(request.input.clone())
                    .map_err(|e| ActionExecutorError::InvalidInput(e.to_string()))?;
                let id = ReminderId(input.reminder_id);
                self.repo
                    .update_status(&id, ReminderStatus::Snoozed)
                    .await
                    .map_err(|e| ActionExecutorError::ExecutionFailed(e.to_string()))?;
                let reminder = self
                    .repo
                    .get(&id)
                    .await
                    .map_err(|e| ActionExecutorError::ExecutionFailed(e.to_string()))?;
                ActionResultPayload::Json(
                    serde_json::to_value(&reminder)
                        .map_err(|e| ActionExecutorError::ExecutionFailed(e.to_string()))?,
                )
            }
            _ => {
                return Err(ActionExecutorError::NotSupported(
                    request.action_kind.clone(),
                ));
            }
        };

        Ok(ActionResult {
            status: ActionStatus::Completed,
            payload,
            summary: format!("{} completed", request.action_kind),
            completed_at: now,
        })
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use action_core::{ActionId, ActionRequest};
    use capability_policy::CapabilityPolicy;

    fn ts() -> DateTime<Utc> {
        "2026-05-24T12:00:00Z".parse().unwrap()
    }

    fn action_request(kind: ActionKind, input: serde_json::Value) -> ActionRequest {
        ActionRequest {
            action_id: ActionId::from("action-reminder-1"),
            action_kind: kind,
            input,
            requested_by: "user-1".to_string(),
            conversation_id: Some("conversation-1".to_string()),
            message_id: Some("message-1".to_string()),
            requested_at: ts(),
        }
    }

    // ---- Type roundtrip tests ----

    #[test]
    fn reminder_id_roundtrips() {
        let id = ReminderId::from("reminder-1");
        assert_eq!(id.to_string(), "reminder-1");
        let json = serde_json::to_string(&id).unwrap();
        let decoded: ReminderId = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded, id);
    }

    #[test]
    fn reminder_time_spec_absolute_roundtrips() {
        let spec = ReminderTimeSpec::Absolute(ts());
        let json = serde_json::to_string(&spec).unwrap();
        let decoded: ReminderTimeSpec = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded, spec);
    }

    #[test]
    fn reminder_time_spec_relative_roundtrips() {
        let spec = ReminderTimeSpec::Relative(Duration::hours(2));
        let json = serde_json::to_string(&spec).unwrap();
        let decoded: ReminderTimeSpec = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded, spec);
    }

    #[test]
    fn reminder_time_spec_resolves_correctly() {
        let absolute = ReminderTimeSpec::Absolute(ts());
        assert_eq!(absolute.resolve(ts()), ts());

        let relative = ReminderTimeSpec::Relative(Duration::hours(1));
        assert_eq!(relative.resolve(ts()), ts() + Duration::hours(1));
    }

    #[test]
    fn reminder_roundtrips() {
        let reminder = Reminder {
            id: ReminderId::from("reminder-1"),
            title: "Send email".to_string(),
            description: "Remember to send the email".to_string(),
            time_spec: ReminderTimeSpec::Absolute(ts()),
            status: ReminderStatus::Pending,
            recurrence: ReminderRecurrence::None,
            source: ReminderSource::UserCreated,
            artifact_id: None,
            created_at: ts(),
            updated_at: ts(),
        };
        let json = serde_json::to_string_pretty(&reminder).unwrap();
        let decoded: Reminder = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded, reminder);
    }

    #[test]
    fn reminder_recurrence_roundtrips() {
        let variants = vec![
            ReminderRecurrence::None,
            ReminderRecurrence::Daily,
            ReminderRecurrence::Weekly,
            ReminderRecurrence::Monthly,
            ReminderRecurrence::Custom(Duration::hours(6)),
        ];
        for variant in variants {
            let json = serde_json::to_string(&variant).unwrap();
            let decoded: ReminderRecurrence = serde_json::from_str(&json).unwrap();
            assert_eq!(decoded, variant);
        }
    }

    // ---- Status transition tests ----

    #[test]
    fn reminder_status_transitions() {
        assert_ne!(ReminderStatus::Pending, ReminderStatus::Due);
        assert_ne!(ReminderStatus::Due, ReminderStatus::Completed);
        assert_ne!(ReminderStatus::Completed, ReminderStatus::Cancelled);
        assert_ne!(ReminderStatus::Pending, ReminderStatus::Snoozed);
    }

    #[test]
    fn reminder_status_roundtrips() {
        let statuses = vec![
            ReminderStatus::Pending,
            ReminderStatus::Due,
            ReminderStatus::Completed,
            ReminderStatus::Cancelled,
            ReminderStatus::Snoozed,
        ];
        for status in statuses {
            let json = serde_json::to_string(&status).unwrap();
            let decoded: ReminderStatus = serde_json::from_str(&json).unwrap();
            assert_eq!(decoded, status);
        }
    }

    // ---- Schema registration tests ----

    #[test]
    fn register_reminder_action_schemas_adds_expected_actions() {
        let mut registry = ActionRegistry::new();
        register_reminder_action_schemas(&mut registry).unwrap();
        assert!(registry.get(&reminder_create_action_kind()).is_some());
        assert!(registry.get(&reminder_complete_action_kind()).is_some());
        assert!(registry.get(&reminder_cancel_action_kind()).is_some());
        assert!(registry.get(&reminder_snooze_action_kind()).is_some());
    }

    #[test]
    fn reminder_action_schemas_have_correct_side_effects() {
        let mut registry = ActionRegistry::new();
        register_reminder_action_schemas(&mut registry).unwrap();
        assert_eq!(
            registry
                .get(&reminder_create_action_kind())
                .unwrap()
                .side_effect,
            SideEffectKind::RuntimeStateMutation
        );
        assert_eq!(
            registry
                .get(&reminder_complete_action_kind())
                .unwrap()
                .side_effect,
            SideEffectKind::RuntimeStateMutation
        );
        assert_eq!(
            registry
                .get(&reminder_cancel_action_kind())
                .unwrap()
                .side_effect,
            SideEffectKind::RuntimeStateMutation
        );
        assert_eq!(
            registry
                .get(&reminder_snooze_action_kind())
                .unwrap()
                .side_effect,
            SideEffectKind::RuntimeStateMutation
        );
    }

    // ---- Executor tests ----

    #[test]
    fn reminder_executor_create_saves_reminder() {
        let repo = std::sync::Arc::new(MemoryReminderRepository::new());
        let executor = ReminderActionExecutor::new(repo.clone());
        let rt = tokio::runtime::Runtime::new().unwrap();
        let result = rt.block_on(async {
            executor
                .execute(&action_request(
                    reminder_create_action_kind(),
                    serde_json::to_value(ReminderCreateInput {
                        title: "Send email".to_string(),
                        description: "Remember to send the email".to_string(),
                        time_spec: ReminderTimeSpec::Absolute(ts()),
                        recurrence: ReminderRecurrence::None,
                        created_at: ts(),
                    })
                    .unwrap(),
                ))
                .await
                .unwrap()
        });
        assert_eq!(result.status, ActionStatus::Completed);
        if let ActionResultPayload::Json(value) = &result.payload {
            let reminder: Reminder = serde_json::from_value(value.clone()).unwrap();
            assert_eq!(reminder.title, "Send email");
            assert_eq!(reminder.status, ReminderStatus::Pending);
        } else {
            panic!("expected json payload");
        }
        // Verify saved in repo
        let saved = rt.block_on(async { repo.list().await.unwrap() });
        assert_eq!(saved.len(), 1);
    }

    #[test]
    fn reminder_executor_complete_updates_status() {
        let repo = std::sync::Arc::new(MemoryReminderRepository::new());
        // Create first
        let reminder = Reminder {
            id: ReminderId::from("r-1"),
            title: "Test".to_string(),
            description: "".to_string(),
            time_spec: ReminderTimeSpec::Absolute(ts()),
            status: ReminderStatus::Pending,
            recurrence: ReminderRecurrence::None,
            source: ReminderSource::UserCreated,
            artifact_id: None,
            created_at: ts(),
            updated_at: ts(),
        };
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async { repo.save(&reminder).await.unwrap() });

        let executor = ReminderActionExecutor::new(repo.clone());
        let result = rt.block_on(async {
            executor
                .execute(&action_request(
                    reminder_complete_action_kind(),
                    serde_json::json!({"reminder_id": "r-1"}),
                ))
                .await
                .unwrap()
        });
        assert_eq!(result.status, ActionStatus::Completed);
        if let ActionResultPayload::Json(value) = &result.payload {
            let updated: Reminder = serde_json::from_value(value.clone()).unwrap();
            assert_eq!(updated.status, ReminderStatus::Completed);
        } else {
            panic!("expected json payload");
        }
    }

    #[test]
    fn reminder_executor_cancel_updates_status() {
        let repo = std::sync::Arc::new(MemoryReminderRepository::new());
        let reminder = Reminder {
            id: ReminderId::from("r-2"),
            title: "Test".to_string(),
            description: "".to_string(),
            time_spec: ReminderTimeSpec::Absolute(ts()),
            status: ReminderStatus::Pending,
            recurrence: ReminderRecurrence::None,
            source: ReminderSource::UserCreated,
            artifact_id: None,
            created_at: ts(),
            updated_at: ts(),
        };
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async { repo.save(&reminder).await.unwrap() });

        let executor = ReminderActionExecutor::new(repo.clone());
        let result = rt.block_on(async {
            executor
                .execute(&action_request(
                    reminder_cancel_action_kind(),
                    serde_json::json!({"reminder_id": "r-2"}),
                ))
                .await
                .unwrap()
        });
        assert_eq!(result.status, ActionStatus::Completed);
        if let ActionResultPayload::Json(value) = &result.payload {
            let updated: Reminder = serde_json::from_value(value.clone()).unwrap();
            assert_eq!(updated.status, ReminderStatus::Cancelled);
        } else {
            panic!("expected json payload");
        }
    }

    #[test]
    fn reminder_executor_rejects_unknown_action() {
        let repo = std::sync::Arc::new(MemoryReminderRepository::new());
        let executor = ReminderActionExecutor::new(repo);
        let rt = tokio::runtime::Runtime::new().unwrap();
        let result = rt.block_on(async {
            executor
                .execute(&action_request(
                    ActionKind::from("reminder.unknown"),
                    serde_json::json!({}),
                ))
                .await
        });
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            ActionExecutorError::NotSupported(_)
        ));
    }

    // ---- Policy tests ----

    #[test]
    fn reminder_create_is_asked_by_default_safe_policy() {
        let policy = CapabilityPolicy::default_safe();
        let req = action_request(
            reminder_create_action_kind(),
            serde_json::json!({"title": "Test", "description": "", "time_spec": {"Absolute": "2026-05-24T12:00:00Z"}, "recurrence": "None", "created_at": "2026-05-24T12:00:00Z"}),
        );
        assert!(
            policy
                .evaluate(&req, &SideEffectKind::RuntimeStateMutation)
                .is_ask()
        );
    }

    #[test]
    fn reminder_complete_is_asked_by_default_safe_policy() {
        let policy = CapabilityPolicy::default_safe();
        let req = action_request(
            reminder_complete_action_kind(),
            serde_json::json!({"reminder_id": "r-1"}),
        );
        assert!(
            policy
                .evaluate(&req, &SideEffectKind::RuntimeStateMutation)
                .is_ask()
        );
    }
}
