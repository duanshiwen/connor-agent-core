//! # Browser Entity
//!
//! Domain types, action schemas, and fake executor for AgentOS Browser Entity.
//!
//! The Browser Entity enables the assistant to help users read web pages, extract
//! content, summarize, and compare pages. It is accessed through the ActionRuntime
//! as a linked entity, not as a foreground participant.
//!
//! This crate provides:
//! - Core domain types (`BrowsingSessionId`, `WebPageUrl`, `WebPageSnapshot`, etc.)
//! - Browser action schemas registered with `ActionRegistry`
//! - `FakeBrowserExecutor` for testing and early runtime flows
//!
//! Future work:
//! - `StaticHtmlBrowserExecutor` (HTTP fetch + HTML parse)
//! - Headless browser integration
//! - Real UI surface rendering

use action_core::{
    ActionExecutor, ActionExecutorError, ActionKind, ActionRegistry, ActionRegistryError,
    ActionRequest, ActionResult, ActionResultPayload, ActionSchema, ActionStatus, SideEffectKind,
};
use artifact_core::ArtifactId;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fmt;

// ---------------------------------------------------------------------------
// Action kind constants
// ---------------------------------------------------------------------------

pub const BROWSER_OPEN_URL_ACTION_KIND: &str = "browser.open_url";
pub const BROWSER_EXTRACT_CONTENT_ACTION_KIND: &str = "browser.extract_content";
pub const BROWSER_SUMMARIZE_PAGE_ACTION_KIND: &str = "browser.summarize_page";
pub const BROWSER_COMPARE_PAGES_ACTION_KIND: &str = "browser.compare_pages";
pub const BROWSER_CAPTURE_SNAPSHOT_ACTION_KIND: &str = "browser.capture_snapshot";

pub fn browser_open_url_action_kind() -> ActionKind {
    ActionKind::from(BROWSER_OPEN_URL_ACTION_KIND)
}

pub fn browser_extract_content_action_kind() -> ActionKind {
    ActionKind::from(BROWSER_EXTRACT_CONTENT_ACTION_KIND)
}

pub fn browser_summarize_page_action_kind() -> ActionKind {
    ActionKind::from(BROWSER_SUMMARIZE_PAGE_ACTION_KIND)
}

pub fn browser_compare_pages_action_kind() -> ActionKind {
    ActionKind::from(BROWSER_COMPARE_PAGES_ACTION_KIND)
}

pub fn browser_capture_snapshot_action_kind() -> ActionKind {
    ActionKind::from(BROWSER_CAPTURE_SNAPSHOT_ACTION_KIND)
}

// ---------------------------------------------------------------------------
// Core types
// ---------------------------------------------------------------------------

/// Unique identifier for a browsing session.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct BrowsingSessionId(pub String);

impl fmt::Display for BrowsingSessionId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl From<String> for BrowsingSessionId {
    fn from(s: String) -> Self {
        Self(s)
    }
}

impl From<&str> for BrowsingSessionId {
    fn from(s: &str) -> Self {
        Self(s.to_string())
    }
}

/// Validated URL for a web page.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct WebPageUrl(pub String);

impl WebPageUrl {
    /// Create a new URL, validating it is non-empty.
    pub fn new(url: impl Into<String>) -> Result<Self, BrowserValidationError> {
        let url = url.into();
        if url.trim().is_empty() {
            return Err(BrowserValidationError::EmptyUrl);
        }
        Ok(Self(url))
    }

    /// The URL string.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for WebPageUrl {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Content extracted from a web page.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WebPageContent {
    /// The page title (from <title> or og:title).
    pub title: String,
    /// Main text content extracted from the page.
    pub text: String,
    /// Optional description (from meta description or og:description).
    pub description: Option<String>,
    /// Optional image URL (from og:image).
    pub image_url: Option<String>,
    /// When the content was extracted.
    pub extracted_at: DateTime<Utc>,
}

/// A snapshot of a web page at a point in time.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WebPageSnapshot {
    /// The URL that was snapshotted.
    pub url: WebPageUrl,
    /// The page title.
    pub title: String,
    /// Extracted text content.
    pub content: String,
    /// Optional HTML source (may be omitted for large pages).
    pub html_source: Option<String>,
    /// When the snapshot was taken.
    pub captured_at: DateTime<Utc>,
    /// Optional associated artifact id.
    pub artifact_id: Option<ArtifactId>,
}

/// Structured extraction result from one or more pages.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WebExtractedContent {
    /// Source URL.
    pub source_url: WebPageUrl,
    /// Extracted text content.
    pub text: String,
    /// Extracted links (absolute URLs found on the page).
    pub links: Vec<String>,
    /// Extracted images (absolute URLs).
    pub images: Vec<String>,
    /// When the extraction happened.
    pub extracted_at: DateTime<Utc>,
}

// ---------------------------------------------------------------------------
// Action inputs
// ---------------------------------------------------------------------------

/// Input for `browser.open_url`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BrowserOpenUrlActionInput {
    pub url: String,
    pub take_snapshot: bool,
}

/// Input for `browser.extract_content`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BrowserExtractContentActionInput {
    pub url: String,
}

/// Input for `browser.summarize_page`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BrowserSummarizePageActionInput {
    pub url: String,
    pub max_length: Option<usize>,
}

/// Input for `browser.compare_pages`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BrowserComparePagesActionInput {
    pub url_a: String,
    pub url_b: String,
    pub aspect: Option<String>,
}

/// Input for `browser.capture_snapshot`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BrowserCaptureSnapshotActionInput {
    pub url: String,
    pub include_html: bool,
}

// ---------------------------------------------------------------------------
// Validation errors
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum BrowserValidationError {
    #[error("url cannot be empty")]
    EmptyUrl,
}

// ---------------------------------------------------------------------------
// Action schema registration
// ---------------------------------------------------------------------------

/// Register all browser action schemas with the given registry.
pub fn register_browser_action_schemas(
    registry: &mut ActionRegistry,
) -> Result<(), ActionRegistryError> {
    registry.register(ActionSchema {
        kind: browser_open_url_action_kind(),
        display_name: "Open URL".to_string(),
        description: "Open a web page URL and optionally take a snapshot.".to_string(),
        side_effect: SideEffectKind::NetworkAccess,
        input_schema: None,
        output_schema: None,
    })?;
    registry.register(ActionSchema {
        kind: browser_extract_content_action_kind(),
        display_name: "Extract Content".to_string(),
        description: "Extract text content from a web page.".to_string(),
        side_effect: SideEffectKind::ReadOnly,
        input_schema: None,
        output_schema: None,
    })?;
    registry.register(ActionSchema {
        kind: browser_summarize_page_action_kind(),
        display_name: "Summarize Page".to_string(),
        description: "Summarize the content of a web page.".to_string(),
        side_effect: SideEffectKind::ReadOnly,
        input_schema: None,
        output_schema: None,
    })?;
    registry.register(ActionSchema {
        kind: browser_compare_pages_action_kind(),
        display_name: "Compare Pages".to_string(),
        description: "Compare the content of two web pages.".to_string(),
        side_effect: SideEffectKind::ReadOnly,
        input_schema: None,
        output_schema: None,
    })?;
    registry.register(ActionSchema {
        kind: browser_capture_snapshot_action_kind(),
        display_name: "Capture Snapshot".to_string(),
        description: "Capture a snapshot of a web page.".to_string(),
        side_effect: SideEffectKind::NetworkAccess,
        input_schema: None,
        output_schema: None,
    })?;
    Ok(())
}

// ---------------------------------------------------------------------------
// FakeBrowserExecutor
// ---------------------------------------------------------------------------

/// Deterministic fake browser executor for testing and early runtime flows.
///
/// Returns static fake content for all browser actions.
pub struct FakeBrowserExecutor {
    now: DateTime<Utc>,
}

impl FakeBrowserExecutor {
    pub fn new(now: DateTime<Utc>) -> Self {
        Self { now }
    }

    fn fake_snapshot(&self, url: &str, include_html: bool) -> WebPageSnapshot {
        WebPageSnapshot {
            url: WebPageUrl::new(url).unwrap_or_else(|_| WebPageUrl("about:blank".to_string())),
            title: format!("Fake Page: {}", url),
            content: format!("This is fake extracted content from {}.", url),
            html_source: if include_html {
                Some(format!(
                    "<html><head><title>Fake Page: {}</title></head><body>Fake content.</body></html>",
                    url
                ))
            } else {
                None
            },
            captured_at: self.now,
            artifact_id: None,
        }
    }

    fn fake_extracted_content(&self, url: &str) -> WebExtractedContent {
        WebExtractedContent {
            source_url: WebPageUrl::new(url)
                .unwrap_or_else(|_| WebPageUrl("about:blank".to_string())),
            text: format!("This is fake extracted content from {}.", url),
            links: vec![format!("{}/link1", url), format!("{}/link2", url)],
            images: vec![format!("{}/image.png", url)],
            extracted_at: self.now,
        }
    }

    fn fake_summary(&self, url: &str) -> String {
        format!(
            "This is a fake summary of {}. The page discusses important topics related to the URL.",
            url
        )
    }

    fn fake_comparison(&self, url_a: &str, url_b: &str) -> String {
        format!(
            "Comparison between {} and {}:\n\
             - Both pages discuss similar topics.\n\
             - Page A focuses on the content of {}.\n\
             - Page B focuses on the content of {}.",
            url_a, url_b, url_a, url_b
        )
    }
}

#[async_trait]
impl ActionExecutor for FakeBrowserExecutor {
    async fn execute(&self, request: &ActionRequest) -> Result<ActionResult, ActionExecutorError> {
        let payload = match request.action_kind.0.as_str() {
            BROWSER_OPEN_URL_ACTION_KIND => {
                let input: BrowserOpenUrlActionInput =
                    serde_json::from_value(request.input.clone())
                        .map_err(|e| ActionExecutorError::InvalidInput(e.to_string()))?;
                let snapshot = self.fake_snapshot(&input.url, false);
                ActionResultPayload::Json(
                    serde_json::to_value(snapshot)
                        .map_err(|e| ActionExecutorError::ExecutionFailed(e.to_string()))?,
                )
            }
            BROWSER_EXTRACT_CONTENT_ACTION_KIND => {
                let input: BrowserExtractContentActionInput =
                    serde_json::from_value(request.input.clone())
                        .map_err(|e| ActionExecutorError::InvalidInput(e.to_string()))?;
                let content = self.fake_extracted_content(&input.url);
                ActionResultPayload::Json(
                    serde_json::to_value(content)
                        .map_err(|e| ActionExecutorError::ExecutionFailed(e.to_string()))?,
                )
            }
            BROWSER_SUMMARIZE_PAGE_ACTION_KIND => {
                let input: BrowserSummarizePageActionInput =
                    serde_json::from_value(request.input.clone())
                        .map_err(|e| ActionExecutorError::InvalidInput(e.to_string()))?;
                let summary = self.fake_summary(&input.url);
                ActionResultPayload::Text(summary)
            }
            BROWSER_COMPARE_PAGES_ACTION_KIND => {
                let input: BrowserComparePagesActionInput =
                    serde_json::from_value(request.input.clone())
                        .map_err(|e| ActionExecutorError::InvalidInput(e.to_string()))?;
                let comparison = self.fake_comparison(&input.url_a, &input.url_b);
                ActionResultPayload::Text(comparison)
            }
            BROWSER_CAPTURE_SNAPSHOT_ACTION_KIND => {
                let input: BrowserCaptureSnapshotActionInput =
                    serde_json::from_value(request.input.clone())
                        .map_err(|e| ActionExecutorError::InvalidInput(e.to_string()))?;
                let snapshot = self.fake_snapshot(&input.url, input.include_html);
                ActionResultPayload::Json(
                    serde_json::to_value(snapshot)
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
            completed_at: self.now,
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
    use capability_policy::{CapabilityPolicy, PolicyDecision};

    fn ts() -> DateTime<Utc> {
        "2026-05-24T12:00:00Z".parse().unwrap()
    }

    fn action_request(kind: ActionKind, input: serde_json::Value) -> ActionRequest {
        ActionRequest {
            action_id: ActionId::from("action-browser-1"),
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
    fn browsing_session_id_roundtrips() {
        let id = BrowsingSessionId::from("session-1");
        assert_eq!(id.to_string(), "session-1");

        let json = serde_json::to_string(&id).unwrap();
        let decoded: BrowsingSessionId = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded, id);
    }

    #[test]
    fn web_page_url_validates_non_empty() {
        assert!(WebPageUrl::new("https://example.com").is_ok());
        assert!(WebPageUrl::new("").is_err());
        assert!(WebPageUrl::new("   ").is_err());
    }

    #[test]
    fn web_page_url_rejects_empty() {
        let result = WebPageUrl::new("");
        assert!(matches!(result, Err(BrowserValidationError::EmptyUrl)));
    }

    #[test]
    fn web_page_snapshot_roundtrips() {
        let snapshot = WebPageSnapshot {
            url: WebPageUrl::new("https://example.com").unwrap(),
            title: "Example".to_string(),
            content: "Page content".to_string(),
            html_source: None,
            captured_at: ts(),
            artifact_id: Some(ArtifactId::from("artifact-1")),
        };

        let json = serde_json::to_string_pretty(&snapshot).unwrap();
        let decoded: WebPageSnapshot = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded, snapshot);
    }

    #[test]
    fn web_extracted_content_roundtrips() {
        let content = WebExtractedContent {
            source_url: WebPageUrl::new("https://example.com").unwrap(),
            text: "Extracted text".to_string(),
            links: vec!["https://example.com/a".to_string()],
            images: vec!["https://example.com/img.png".to_string()],
            extracted_at: ts(),
        };

        let json = serde_json::to_string_pretty(&content).unwrap();
        let decoded: WebExtractedContent = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded, content);
    }

    #[test]
    fn web_page_content_roundtrips() {
        let content = WebPageContent {
            title: "Test Page".to_string(),
            text: "Page text".to_string(),
            description: Some("A test page".to_string()),
            image_url: Some("https://example.com/og.png".to_string()),
            extracted_at: ts(),
        };

        let json = serde_json::to_string_pretty(&content).unwrap();
        let decoded: WebPageContent = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded, content);
    }

    // ---- Schema registration tests ----

    #[test]
    fn register_browser_action_schemas_adds_expected_actions() {
        let mut registry = ActionRegistry::new();
        register_browser_action_schemas(&mut registry).unwrap();

        assert!(registry.get(&browser_open_url_action_kind()).is_some());
        assert!(
            registry
                .get(&browser_extract_content_action_kind())
                .is_some()
        );
        assert!(
            registry
                .get(&browser_summarize_page_action_kind())
                .is_some()
        );
        assert!(registry.get(&browser_compare_pages_action_kind()).is_some());
        assert!(
            registry
                .get(&browser_capture_snapshot_action_kind())
                .is_some()
        );
    }

    #[test]
    fn browser_action_schemas_side_effects_match_policy_contract() {
        let mut registry = ActionRegistry::new();
        register_browser_action_schemas(&mut registry).unwrap();

        assert_eq!(
            registry
                .get(&browser_open_url_action_kind())
                .unwrap()
                .side_effect,
            SideEffectKind::NetworkAccess
        );
        assert_eq!(
            registry
                .get(&browser_extract_content_action_kind())
                .unwrap()
                .side_effect,
            SideEffectKind::ReadOnly
        );
        assert_eq!(
            registry
                .get(&browser_summarize_page_action_kind())
                .unwrap()
                .side_effect,
            SideEffectKind::ReadOnly
        );
        assert_eq!(
            registry
                .get(&browser_compare_pages_action_kind())
                .unwrap()
                .side_effect,
            SideEffectKind::ReadOnly
        );
        assert_eq!(
            registry
                .get(&browser_capture_snapshot_action_kind())
                .unwrap()
                .side_effect,
            SideEffectKind::NetworkAccess
        );
    }

    // ---- Policy tests ----

    #[test]
    fn browser_extract_content_is_allowed_by_default_safe_policy() {
        let policy = CapabilityPolicy::default_safe();
        let req = action_request(
            browser_extract_content_action_kind(),
            serde_json::json!({"url": "https://example.com"}),
        );
        assert_eq!(
            policy.evaluate(&req, &SideEffectKind::ReadOnly),
            PolicyDecision::Allow
        );
    }

    #[test]
    fn browser_open_url_requires_approval_by_default_safe_policy() {
        let policy = CapabilityPolicy::default_safe();
        let req = action_request(
            browser_open_url_action_kind(),
            serde_json::json!({"url": "https://example.com", "take_snapshot": true}),
        );
        assert!(matches!(
            policy.evaluate(&req, &SideEffectKind::NetworkAccess),
            PolicyDecision::Ask { .. }
        ));
    }

    // ---- FakeBrowserExecutor tests ----

    #[test]
    fn fake_browser_executor_returns_snapshot_for_open_url() {
        let executor = FakeBrowserExecutor::new(ts());
        let rt = tokio::runtime::Runtime::new().unwrap();
        let result = rt.block_on(async {
            executor
                .execute(&action_request(
                    browser_open_url_action_kind(),
                    serde_json::json!({"url": "https://example.com", "take_snapshot": true}),
                ))
                .await
                .unwrap()
        });

        assert_eq!(result.status, ActionStatus::Completed);
        let ActionResultPayload::Json(value) = result.payload else {
            panic!("expected json payload");
        };
        let snapshot: WebPageSnapshot = serde_json::from_value(value).unwrap();
        assert_eq!(snapshot.url.as_str(), "https://example.com");
        assert!(snapshot.title.contains("Fake Page"));
    }

    #[test]
    fn fake_browser_executor_returns_extracted_content() {
        let executor = FakeBrowserExecutor::new(ts());
        let rt = tokio::runtime::Runtime::new().unwrap();
        let result = rt.block_on(async {
            executor
                .execute(&action_request(
                    browser_extract_content_action_kind(),
                    serde_json::json!({"url": "https://example.com/article"}),
                ))
                .await
                .unwrap()
        });

        assert_eq!(result.status, ActionStatus::Completed);
        let ActionResultPayload::Json(value) = result.payload else {
            panic!("expected json payload");
        };
        let content: WebExtractedContent = serde_json::from_value(value).unwrap();
        assert_eq!(content.source_url.as_str(), "https://example.com/article");
        assert!(!content.links.is_empty());
    }

    #[test]
    fn fake_browser_executor_returns_text_summary() {
        let executor = FakeBrowserExecutor::new(ts());
        let rt = tokio::runtime::Runtime::new().unwrap();
        let result = rt.block_on(async {
            executor
                .execute(&action_request(
                    browser_summarize_page_action_kind(),
                    serde_json::json!({"url": "https://example.com"}),
                ))
                .await
                .unwrap()
        });

        assert_eq!(result.status, ActionStatus::Completed);
        if let ActionResultPayload::Text(text) = &result.payload {
            assert!(text.contains("fake summary"));
        } else {
            panic!("expected text payload");
        }
    }

    #[test]
    fn fake_browser_executor_returns_text_comparison() {
        let executor = FakeBrowserExecutor::new(ts());
        let rt = tokio::runtime::Runtime::new().unwrap();
        let result = rt.block_on(async {
            executor
                .execute(&action_request(
                    browser_compare_pages_action_kind(),
                    serde_json::json!({"url_a": "https://a.com", "url_b": "https://b.com"}),
                ))
                .await
                .unwrap()
        });

        assert_eq!(result.status, ActionStatus::Completed);
        if let ActionResultPayload::Text(text) = &result.payload {
            assert!(text.contains("Comparison"));
            assert!(text.contains("a.com"));
            assert!(text.contains("b.com"));
        } else {
            panic!("expected text payload");
        }
    }

    #[test]
    fn fake_browser_executor_rejects_unknown_action_kind() {
        let executor = FakeBrowserExecutor::new(ts());
        let rt = tokio::runtime::Runtime::new().unwrap();
        let result = rt.block_on(async {
            executor
                .execute(&action_request(
                    ActionKind::from("browser.unknown"),
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
}
