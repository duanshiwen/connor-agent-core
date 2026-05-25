//! # Mail Entity
//!
//! Domain types, action schemas, and fake executor for AgentOS Mail Entity.
//!
//! The Mail Entity enables the assistant to help users triage, read, draft,
//! and send emails. It is accessed through the ActionRuntime as a linked
//! entity, not as a foreground participant.
//!
//! This crate provides:
//! - Core domain types (`MailAccountId`, `MailMessageId`, `MailMessageSummary`, etc.)
//! - Mail action schemas registered with `ActionRegistry`
//! - `FakeMailExecutor` for testing and early runtime flows
//!
//! Future work:
//! - IMAP/SMTP integration
//! - Feishu/DingTalk/WeCom connectors

use action_core::{
    ActionExecutor, ActionExecutorError, ActionKind, ActionRegistry, ActionRegistryError,
    ActionRequest, ActionResult, ActionResultPayload, ActionSchema, ActionStatus, SideEffectKind,
};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fmt;

// ---------------------------------------------------------------------------
// Action kind constants
// ---------------------------------------------------------------------------

pub const MAIL_LIST_RECENT_ACTION_KIND: &str = "mail.list_recent";
pub const MAIL_GET_MESSAGE_ACTION_KIND: &str = "mail.get_message";
pub const MAIL_SUMMARIZE_THREAD_ACTION_KIND: &str = "mail.summarize_thread";
pub const MAIL_CREATE_DRAFT_REPLY_ACTION_KIND: &str = "mail.create_draft_reply";
pub const MAIL_SEND_ACTION_KIND: &str = "mail.send";

pub fn mail_list_recent_action_kind() -> ActionKind {
    ActionKind::from(MAIL_LIST_RECENT_ACTION_KIND)
}

pub fn mail_get_message_action_kind() -> ActionKind {
    ActionKind::from(MAIL_GET_MESSAGE_ACTION_KIND)
}

pub fn mail_summarize_thread_action_kind() -> ActionKind {
    ActionKind::from(MAIL_SUMMARIZE_THREAD_ACTION_KIND)
}

pub fn mail_create_draft_reply_action_kind() -> ActionKind {
    ActionKind::from(MAIL_CREATE_DRAFT_REPLY_ACTION_KIND)
}

pub fn mail_send_action_kind() -> ActionKind {
    ActionKind::from(MAIL_SEND_ACTION_KIND)
}

// ---------------------------------------------------------------------------
// Core types
// ---------------------------------------------------------------------------

/// Unique identifier for a mail account.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct MailAccountId(pub String);

impl fmt::Display for MailAccountId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl From<String> for MailAccountId {
    fn from(s: String) -> Self {
        Self(s)
    }
}

impl From<&str> for MailAccountId {
    fn from(s: &str) -> Self {
        Self(s.to_string())
    }
}

/// Unique identifier for a mail message.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct MailMessageId(pub String);

impl fmt::Display for MailMessageId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl From<String> for MailMessageId {
    fn from(s: String) -> Self {
        Self(s)
    }
}

impl From<&str> for MailMessageId {
    fn from(s: &str) -> Self {
        Self(s.to_string())
    }
}

/// Unique identifier for a mail thread / conversation.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct MailThreadId(pub String);

impl fmt::Display for MailThreadId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl From<String> for MailThreadId {
    fn from(s: String) -> Self {
        Self(s)
    }
}

impl From<&str> for MailThreadId {
    fn from(s: &str) -> Self {
        Self(s.to_string())
    }
}

/// Sender or recipient of a mail message.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MailAddress {
    pub email: String,
    pub display_name: Option<String>,
}

impl MailAddress {
    pub fn new(email: impl Into<String>) -> Self {
        Self {
            email: email.into(),
            display_name: None,
        }
    }

    pub fn with_display_name(mut self, name: impl Into<String>) -> Self {
        self.display_name = Some(name.into());
        self
    }
}

impl fmt::Display for MailAddress {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.display_name {
            Some(name) => write!(f, "{} <{}>", name, self.email),
            None => f.write_str(&self.email),
        }
    }
}

/// Summary of a mail message (lightweight representation for listing/triage).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MailMessageSummary {
    pub id: MailMessageId,
    pub thread_id: MailThreadId,
    pub account_id: MailAccountId,
    pub subject: String,
    pub from: MailAddress,
    pub to: Vec<MailAddress>,
    pub snippet: String,
    pub received_at: DateTime<Utc>,
    pub is_read: bool,
    pub has_attachments: bool,
}

/// Full mail message content.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MailMessage {
    pub id: MailMessageId,
    pub thread_id: MailThreadId,
    pub account_id: MailAccountId,
    pub subject: String,
    pub from: MailAddress,
    pub to: Vec<MailAddress>,
    pub cc: Vec<MailAddress>,
    pub body_text: String,
    pub body_html: Option<String>,
    pub received_at: DateTime<Utc>,
    pub is_read: bool,
    pub has_attachments: bool,
    pub attachments: Vec<MailAttachment>,
}

/// Metadata for a mail attachment.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MailAttachment {
    pub filename: String,
    pub content_type: String,
    pub size_bytes: u64,
}

/// Triage category for a mail message.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TriageCategory {
    Urgent,
    Important,
    Newsletter,
    Notification,
    Social,
    Spam,
    LowPriority,
}

/// Result of triaging a mail message.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MailTriageResult {
    pub message_id: MailMessageId,
    pub category: TriageCategory,
    pub reason: String,
    pub suggested_action: Option<String>,
}

/// A draft reply to a mail message.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MailDraft {
    pub reply_to: MailMessageId,
    pub to: Vec<MailAddress>,
    pub subject: String,
    pub body_text: String,
    pub created_at: DateTime<Utc>,
}

// ---------------------------------------------------------------------------
// Action inputs
// ---------------------------------------------------------------------------

/// Input for `mail.list_recent`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MailListRecentActionInput {
    pub account_id: String,
    pub max_results: Option<usize>,
    pub unread_only: Option<bool>,
}

/// Input for `mail.get_message`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MailGetMessageActionInput {
    pub message_id: String,
}

/// Input for `mail.summarize_thread`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MailSummarizeThreadActionInput {
    pub thread_id: String,
}

/// Input for `mail.create_draft_reply`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MailCreateDraftReplyActionInput {
    pub message_id: String,
    pub body_text: String,
}

/// Input for `mail.send`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MailSendActionInput {
    pub draft: MailDraft,
}

// ---------------------------------------------------------------------------
// Action schema registration
// ---------------------------------------------------------------------------

/// Register all mail action schemas with the given registry.
pub fn register_mail_action_schemas(
    registry: &mut ActionRegistry,
) -> Result<(), ActionRegistryError> {
    registry.register(ActionSchema {
        kind: mail_list_recent_action_kind(),
        display_name: "List Recent Mail".to_string(),
        description: "List recent mail messages from an account.".to_string(),
        side_effect: SideEffectKind::ReadOnly,
        input_schema: None,
        output_schema: None,
    })?;
    registry.register(ActionSchema {
        kind: mail_get_message_action_kind(),
        display_name: "Get Mail Message".to_string(),
        description: "Get a full mail message by id.".to_string(),
        side_effect: SideEffectKind::ReadOnly,
        input_schema: None,
        output_schema: None,
    })?;
    registry.register(ActionSchema {
        kind: mail_summarize_thread_action_kind(),
        display_name: "Summarize Thread".to_string(),
        description: "Summarize a mail thread / conversation.".to_string(),
        side_effect: SideEffectKind::ReadOnly,
        input_schema: None,
        output_schema: None,
    })?;
    registry.register(ActionSchema {
        kind: mail_create_draft_reply_action_kind(),
        display_name: "Create Draft Reply".to_string(),
        description: "Create a draft reply to a mail message.".to_string(),
        side_effect: SideEffectKind::RuntimeStateMutation,
        input_schema: None,
        output_schema: None,
    })?;
    registry.register(ActionSchema {
        kind: mail_send_action_kind(),
        display_name: "Send Mail".to_string(),
        description: "Send a mail message (requires approval).".to_string(),
        side_effect: SideEffectKind::ExternalSystemMutation,
        input_schema: None,
        output_schema: None,
    })?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Mail repository trait
// ---------------------------------------------------------------------------

/// Errors from mail repository operations.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum MailRepositoryError {
    #[error("account not found: {0}")]
    AccountNotFound(String),
    #[error("message not found: {0}")]
    MessageNotFound(String),
    #[error("thread not found: {0}")]
    ThreadNotFound(String),
    #[error("io error: {0}")]
    Io(String),
}

/// Storage abstraction for mail data.
#[async_trait]
pub trait MailRepository: Send + Sync {
    async fn list_recent(
        &self,
        account_id: &str,
        max_results: usize,
        unread_only: bool,
    ) -> Result<Vec<MailMessageSummary>, MailRepositoryError>;

    async fn get_message(
        &self,
        message_id: &str,
    ) -> Result<Option<MailMessage>, MailRepositoryError>;

    async fn get_thread_messages(
        &self,
        thread_id: &str,
    ) -> Result<Vec<MailMessage>, MailRepositoryError>;
}

// ---------------------------------------------------------------------------
// FakeMailRepository
// ---------------------------------------------------------------------------

/// Deterministic in-memory mail repository for tests.
#[derive(Debug, Clone, Default)]
pub struct FakeMailRepository {
    messages: Vec<MailMessage>,
    summaries: Vec<MailMessageSummary>,
}

impl FakeMailRepository {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_messages(mut self, messages: Vec<MailMessage>) -> Self {
        self.messages = messages;
        self
    }

    pub fn with_summaries(mut self, summaries: Vec<MailMessageSummary>) -> Self {
        self.summaries = summaries;
        self
    }
}

#[async_trait]
impl MailRepository for FakeMailRepository {
    async fn list_recent(
        &self,
        _account_id: &str,
        max_results: usize,
        unread_only: bool,
    ) -> Result<Vec<MailMessageSummary>, MailRepositoryError> {
        let mut results: Vec<_> = self
            .summaries
            .iter()
            .filter(|s| !unread_only || !s.is_read)
            .cloned()
            .collect();
        results.sort_by(|a, b| b.received_at.cmp(&a.received_at));
        results.truncate(max_results);
        Ok(results)
    }

    async fn get_message(
        &self,
        message_id: &str,
    ) -> Result<Option<MailMessage>, MailRepositoryError> {
        Ok(self.messages.iter().find(|m| m.id.0 == message_id).cloned())
    }

    async fn get_thread_messages(
        &self,
        thread_id: &str,
    ) -> Result<Vec<MailMessage>, MailRepositoryError> {
        let mut msgs: Vec<_> = self
            .messages
            .iter()
            .filter(|m| m.thread_id.0 == thread_id)
            .cloned()
            .collect();
        msgs.sort_by(|a, b| a.received_at.cmp(&b.received_at));
        Ok(msgs)
    }
}

// ---------------------------------------------------------------------------
// FakeMailExecutor
// ---------------------------------------------------------------------------

/// Deterministic action executor for mail actions.
pub struct FakeMailExecutor {
    repository: FakeMailRepository,
    now: DateTime<Utc>,
}

impl FakeMailExecutor {
    pub fn new(repository: FakeMailRepository, now: DateTime<Utc>) -> Self {
        Self { repository, now }
    }
}

#[async_trait]
impl ActionExecutor for FakeMailExecutor {
    async fn execute(&self, request: &ActionRequest) -> Result<ActionResult, ActionExecutorError> {
        let payload = match request.action_kind.0.as_str() {
            MAIL_LIST_RECENT_ACTION_KIND => {
                let input: MailListRecentActionInput =
                    serde_json::from_value(request.input.clone())
                        .map_err(|e| ActionExecutorError::InvalidInput(e.to_string()))?;
                let max = input.max_results.unwrap_or(20);
                let unread = input.unread_only.unwrap_or(false);
                let results = self
                    .repository
                    .list_recent(&input.account_id, max, unread)
                    .await
                    .map_err(repo_err)?;
                ActionResultPayload::Json(serde_json::to_value(results).map_err(json_err)?)
            }
            MAIL_GET_MESSAGE_ACTION_KIND => {
                let input: MailGetMessageActionInput =
                    serde_json::from_value(request.input.clone())
                        .map_err(|e| ActionExecutorError::InvalidInput(e.to_string()))?;
                let msg = self
                    .repository
                    .get_message(&input.message_id)
                    .await
                    .map_err(repo_err)?;
                ActionResultPayload::Json(serde_json::to_value(msg).map_err(json_err)?)
            }
            MAIL_SUMMARIZE_THREAD_ACTION_KIND => {
                let input: MailSummarizeThreadActionInput =
                    serde_json::from_value(request.input.clone())
                        .map_err(|e| ActionExecutorError::InvalidInput(e.to_string()))?;
                let messages = self
                    .repository
                    .get_thread_messages(&input.thread_id)
                    .await
                    .map_err(repo_err)?;
                let summary = if messages.is_empty() {
                    format!("Thread {} not found or empty.", input.thread_id)
                } else {
                    format!(
                        "Thread {} has {} message(s). Latest from: {}",
                        input.thread_id,
                        messages.len(),
                        messages.last().unwrap().from
                    )
                };
                ActionResultPayload::Text(summary)
            }
            MAIL_CREATE_DRAFT_REPLY_ACTION_KIND => {
                let input: MailCreateDraftReplyActionInput =
                    serde_json::from_value(request.input.clone())
                        .map_err(|e| ActionExecutorError::InvalidInput(e.to_string()))?;
                let original = self
                    .repository
                    .get_message(&input.message_id)
                    .await
                    .map_err(repo_err)?
                    .ok_or_else(|| {
                        ActionExecutorError::ExecutionFailed(format!(
                            "message not found: {}",
                            input.message_id
                        ))
                    })?;
                let draft = MailDraft {
                    reply_to: original.id.clone(),
                    to: vec![original.from.clone()],
                    subject: format!("Re: {}", original.subject),
                    body_text: input.body_text,
                    created_at: self.now,
                };
                ActionResultPayload::Json(serde_json::to_value(draft).map_err(json_err)?)
            }
            MAIL_SEND_ACTION_KIND => {
                // In a real implementation, this would call SMTP/API.
                // For fake, we just acknowledge the send.
                let _input: MailSendActionInput = serde_json::from_value(request.input.clone())
                    .map_err(|e| ActionExecutorError::InvalidInput(e.to_string()))?;
                ActionResultPayload::Json(
                    serde_json::json!({ "status": "sent", "sent_at": self.now }),
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
            completed_at: self.now,
        })
    }
}

fn repo_err(e: MailRepositoryError) -> ActionExecutorError {
    ActionExecutorError::ExecutionFailed(e.to_string())
}

fn json_err(e: serde_json::Error) -> ActionExecutorError {
    ActionExecutorError::InvalidInput(e.to_string())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use action_core::{ActionId, ActionRequest};
    use capability_policy::{CapabilityPolicy, PolicyDecision};

    fn ts() -> DateTime<Utc> {
        "2026-05-25T12:00:00Z".parse().unwrap()
    }

    fn action_request(kind: ActionKind, input: serde_json::Value) -> ActionRequest {
        ActionRequest {
            action_id: ActionId::from("action-mail-1"),
            action_kind: kind,
            input,
            requested_by: "user-1".to_string(),
            conversation_id: Some("conversation-1".to_string()),
            message_id: Some("message-1".to_string()),
            requested_at: ts(),
        }
    }

    fn test_summary(id: &str, subject: &str, is_read: bool) -> MailMessageSummary {
        MailMessageSummary {
            id: MailMessageId::from(id),
            thread_id: MailThreadId::from("thread-1"),
            account_id: MailAccountId::from("account-1"),
            subject: subject.to_string(),
            from: MailAddress::new("sender@example.com").with_display_name("Sender"),
            to: vec![MailAddress::new("user@example.com")],
            snippet: format!("Snippet for {}", subject),
            received_at: ts(),
            is_read,
            has_attachments: false,
        }
    }

    fn test_message(id: &str, subject: &str, from_email: &str) -> MailMessage {
        MailMessage {
            id: MailMessageId::from(id),
            thread_id: MailThreadId::from("thread-1"),
            account_id: MailAccountId::from("account-1"),
            subject: subject.to_string(),
            from: MailAddress::new(from_email).with_display_name("Sender"),
            to: vec![MailAddress::new("user@example.com")],
            cc: vec![],
            body_text: format!("Body of {}", subject),
            body_html: None,
            received_at: ts(),
            is_read: false,
            has_attachments: false,
            attachments: vec![],
        }
    }

    // ---- Type roundtrip tests ----

    #[test]
    fn mail_account_id_roundtrips() {
        let id = MailAccountId::from("account-1");
        assert_eq!(id.to_string(), "account-1");
        let json = serde_json::to_string(&id).unwrap();
        let decoded: MailAccountId = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded, id);
    }

    #[test]
    fn mail_message_id_roundtrips() {
        let id = MailMessageId::from("msg-1");
        assert_eq!(id.to_string(), "msg-1");
        let json = serde_json::to_string(&id).unwrap();
        let decoded: MailMessageId = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded, id);
    }

    #[test]
    fn mail_thread_id_roundtrips() {
        let id = MailThreadId::from("thread-1");
        assert_eq!(id.to_string(), "thread-1");
        let json = serde_json::to_string(&id).unwrap();
        let decoded: MailThreadId = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded, id);
    }

    #[test]
    fn mail_address_display_with_name() {
        let addr = MailAddress::new("user@example.com").with_display_name("User");
        assert_eq!(addr.to_string(), "User <user@example.com>");
    }

    #[test]
    fn mail_address_display_without_name() {
        let addr = MailAddress::new("user@example.com");
        assert_eq!(addr.to_string(), "user@example.com");
    }

    #[test]
    fn mail_message_summary_roundtrips() {
        let summary = test_summary("msg-1", "Test Subject", false);
        let json = serde_json::to_string_pretty(&summary).unwrap();
        let decoded: MailMessageSummary = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded, summary);
    }

    #[test]
    fn mail_message_roundtrips() {
        let msg = test_message("msg-1", "Test", "sender@example.com");
        let json = serde_json::to_string_pretty(&msg).unwrap();
        let decoded: MailMessage = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded, msg);
    }

    #[test]
    fn triage_category_serde_uses_snake_case() {
        let json = serde_json::to_string(&TriageCategory::LowPriority).unwrap();
        assert_eq!(json, "\"low_priority\"");
    }

    #[test]
    fn mail_draft_roundtrips() {
        let draft = MailDraft {
            reply_to: MailMessageId::from("msg-1"),
            to: vec![MailAddress::new("recipient@example.com")],
            subject: "Re: Test".to_string(),
            body_text: "My reply".to_string(),
            created_at: ts(),
        };
        let json = serde_json::to_string_pretty(&draft).unwrap();
        let decoded: MailDraft = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded, draft);
    }

    // ---- Schema registration tests ----

    #[test]
    fn register_mail_action_schemas_adds_expected_actions() {
        let mut registry = ActionRegistry::new();
        register_mail_action_schemas(&mut registry).unwrap();
        assert_eq!(registry.len(), 5);
        assert!(registry.get(&mail_list_recent_action_kind()).is_some());
        assert!(registry.get(&mail_get_message_action_kind()).is_some());
        assert!(registry.get(&mail_summarize_thread_action_kind()).is_some());
        assert!(
            registry
                .get(&mail_create_draft_reply_action_kind())
                .is_some()
        );
        assert!(registry.get(&mail_send_action_kind()).is_some());
    }

    #[test]
    fn mail_action_schema_side_effects_match_policy_contract() {
        let mut registry = ActionRegistry::new();
        register_mail_action_schemas(&mut registry).unwrap();

        assert_eq!(
            registry.side_effect(&mail_list_recent_action_kind()),
            Some(&SideEffectKind::ReadOnly)
        );
        assert_eq!(
            registry.side_effect(&mail_get_message_action_kind()),
            Some(&SideEffectKind::ReadOnly)
        );
        assert_eq!(
            registry.side_effect(&mail_summarize_thread_action_kind()),
            Some(&SideEffectKind::ReadOnly)
        );
        assert_eq!(
            registry.side_effect(&mail_create_draft_reply_action_kind()),
            Some(&SideEffectKind::RuntimeStateMutation)
        );
        assert_eq!(
            registry.side_effect(&mail_send_action_kind()),
            Some(&SideEffectKind::ExternalSystemMutation)
        );
    }

    // ---- Policy tests ----

    #[test]
    fn mail_list_and_get_are_allowed_by_default_safe_policy() {
        let policy = CapabilityPolicy::default_safe();
        for kind in [
            mail_list_recent_action_kind(),
            mail_get_message_action_kind(),
            mail_summarize_thread_action_kind(),
        ] {
            let req = action_request(kind, serde_json::json!({}));
            assert_eq!(
                policy.evaluate(&req, &SideEffectKind::ReadOnly),
                PolicyDecision::Allow
            );
        }
    }

    #[test]
    fn mail_create_draft_requires_approval_by_default_safe_policy() {
        let policy = CapabilityPolicy::default_safe();
        let req = action_request(mail_create_draft_reply_action_kind(), serde_json::json!({}));
        assert!(matches!(
            policy.evaluate(&req, &SideEffectKind::RuntimeStateMutation),
            PolicyDecision::Ask { .. }
        ));
    }

    #[test]
    fn mail_send_is_denied_by_default_safe_policy() {
        let policy = CapabilityPolicy::default_safe();
        let req = action_request(mail_send_action_kind(), serde_json::json!({}));
        assert!(
            policy
                .evaluate(&req, &SideEffectKind::ExternalSystemMutation)
                .is_denied()
        );
    }

    // ---- FakeMailRepository tests ----

    #[tokio::test]
    async fn fake_repo_lists_summaries() {
        let repo = FakeMailRepository::new().with_summaries(vec![
            test_summary("msg-1", "First", false),
            test_summary("msg-2", "Second", true),
        ]);

        let results = repo.list_recent("account-1", 10, false).await.unwrap();
        assert_eq!(results.len(), 2);
    }

    #[tokio::test]
    async fn fake_repo_filters_unread_only() {
        let repo = FakeMailRepository::new().with_summaries(vec![
            test_summary("msg-1", "Read", true),
            test_summary("msg-2", "Unread", false),
        ]);

        let results = repo.list_recent("account-1", 10, true).await.unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].subject, "Unread");
    }

    #[tokio::test]
    async fn fake_repo_truncates_results() {
        let summaries: Vec<_> = (0..5)
            .map(|i| test_summary(&format!("msg-{}", i), &format!("Subject {}", i), false))
            .collect();
        let repo = FakeMailRepository::new().with_summaries(summaries);

        let results = repo.list_recent("account-1", 3, false).await.unwrap();
        assert_eq!(results.len(), 3);
    }

    #[tokio::test]
    async fn fake_repo_get_message_returns_none_for_missing() {
        let repo = FakeMailRepository::new();
        assert!(repo.get_message("missing").await.unwrap().is_none());
    }

    #[tokio::test]
    async fn fake_repo_get_thread_messages() {
        let repo = FakeMailRepository::new().with_messages(vec![
            test_message("msg-1", "First", "a@example.com"),
            test_message("msg-2", "Second", "b@example.com"),
        ]);

        let msgs = repo.get_thread_messages("thread-1").await.unwrap();
        assert_eq!(msgs.len(), 2);
    }

    // ---- FakeMailExecutor tests ----

    #[tokio::test]
    async fn executor_list_recent_returns_summaries() {
        let repo = FakeMailRepository::new().with_summaries(vec![test_summary(
            "msg-1",
            "Important Email",
            false,
        )]);
        let executor = FakeMailExecutor::new(repo, ts());

        let result = executor
            .execute(&action_request(
                mail_list_recent_action_kind(),
                serde_json::json!({"account_id": "account-1", "max_results": 10}),
            ))
            .await
            .unwrap();

        assert_eq!(result.status, ActionStatus::Completed);
        let ActionResultPayload::Json(value) = result.payload else {
            panic!("expected json payload");
        };
        let summaries: Vec<MailMessageSummary> = serde_json::from_value(value).unwrap();
        assert_eq!(summaries.len(), 1);
        assert_eq!(summaries[0].subject, "Important Email");
    }

    #[tokio::test]
    async fn executor_get_message_returns_full_message() {
        let repo = FakeMailRepository::new().with_messages(vec![test_message(
            "msg-1",
            "Test",
            "a@example.com",
        )]);
        let executor = FakeMailExecutor::new(repo, ts());

        let result = executor
            .execute(&action_request(
                mail_get_message_action_kind(),
                serde_json::json!({"message_id": "msg-1"}),
            ))
            .await
            .unwrap();

        assert_eq!(result.status, ActionStatus::Completed);
        let ActionResultPayload::Json(value) = result.payload else {
            panic!("expected json payload");
        };
        let msg: Option<MailMessage> = serde_json::from_value(value).unwrap();
        assert!(msg.is_some());
        assert_eq!(msg.unwrap().subject, "Test");
    }

    #[tokio::test]
    async fn executor_summarize_thread_returns_text() {
        let repo = FakeMailRepository::new().with_messages(vec![test_message(
            "msg-1",
            "Thread Start",
            "a@example.com",
        )]);
        let executor = FakeMailExecutor::new(repo, ts());

        let result = executor
            .execute(&action_request(
                mail_summarize_thread_action_kind(),
                serde_json::json!({"thread_id": "thread-1"}),
            ))
            .await
            .unwrap();

        assert_eq!(result.status, ActionStatus::Completed);
        if let ActionResultPayload::Text(text) = &result.payload {
            assert!(text.contains("1 message"));
        } else {
            panic!("expected text payload");
        }
    }

    #[tokio::test]
    async fn executor_create_draft_reply_builds_draft() {
        let repo = FakeMailRepository::new().with_messages(vec![test_message(
            "msg-1",
            "Original",
            "sender@example.com",
        )]);
        let executor = FakeMailExecutor::new(repo, ts());

        let result = executor
            .execute(&action_request(
                mail_create_draft_reply_action_kind(),
                serde_json::json!({"message_id": "msg-1", "body_text": "My reply"}),
            ))
            .await
            .unwrap();

        assert_eq!(result.status, ActionStatus::Completed);
        let ActionResultPayload::Json(value) = result.payload else {
            panic!("expected json payload");
        };
        let draft: MailDraft = serde_json::from_value(value).unwrap();
        assert_eq!(draft.subject, "Re: Original");
        assert_eq!(draft.body_text, "My reply");
        assert_eq!(draft.to[0].email, "sender@example.com");
    }

    #[tokio::test]
    async fn executor_send_returns_sent_status() {
        let repo = FakeMailRepository::new();
        let executor = FakeMailExecutor::new(repo, ts());

        let draft = MailDraft {
            reply_to: MailMessageId::from("msg-1"),
            to: vec![MailAddress::new("to@example.com")],
            subject: "Test".to_string(),
            body_text: "Body".to_string(),
            created_at: ts(),
        };

        let result = executor
            .execute(&action_request(
                mail_send_action_kind(),
                serde_json::to_value(MailSendActionInput { draft }).unwrap(),
            ))
            .await
            .unwrap();

        assert_eq!(result.status, ActionStatus::Completed);
        let ActionResultPayload::Json(value) = result.payload else {
            panic!("expected json payload");
        };
        assert_eq!(value["status"], "sent");
    }

    #[tokio::test]
    async fn executor_rejects_unknown_action_kind() {
        let executor = FakeMailExecutor::new(FakeMailRepository::new(), ts());
        let result = executor
            .execute(&action_request(
                ActionKind::from("mail.unknown"),
                serde_json::json!({}),
            ))
            .await;
        assert!(matches!(
            result.unwrap_err(),
            ActionExecutorError::NotSupported(_)
        ));
    }
}
