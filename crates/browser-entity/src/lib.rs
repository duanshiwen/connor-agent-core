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
pub const BROWSER_CLICK_ELEMENT_ACTION_KIND: &str = "browser.click_element";
pub const BROWSER_TYPE_TEXT_ACTION_KIND: &str = "browser.type_text";
pub const BROWSER_FILL_FORM_ACTION_KIND: &str = "browser.fill_form";
pub const BROWSER_SELECT_OPTION_ACTION_KIND: &str = "browser.select_option";
pub const BROWSER_SCROLL_PAGE_ACTION_KIND: &str = "browser.scroll_page";
pub const BROWSER_PRESS_KEY_ACTION_KIND: &str = "browser.press_key";
pub const BROWSER_EXECUTE_JS_ACTION_KIND: &str = "browser.execute_js";
pub const BROWSER_WAIT_FOR_ELEMENT_ACTION_KIND: &str = "browser.wait_for_element";
pub const BROWSER_GET_PAGE_SCREENSHOT_ACTION_KIND: &str = "browser.get_page_screenshot";

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

pub fn browser_click_element_action_kind() -> ActionKind {
    ActionKind::from(BROWSER_CLICK_ELEMENT_ACTION_KIND)
}

pub fn browser_type_text_action_kind() -> ActionKind {
    ActionKind::from(BROWSER_TYPE_TEXT_ACTION_KIND)
}

pub fn browser_fill_form_action_kind() -> ActionKind {
    ActionKind::from(BROWSER_FILL_FORM_ACTION_KIND)
}

pub fn browser_select_option_action_kind() -> ActionKind {
    ActionKind::from(BROWSER_SELECT_OPTION_ACTION_KIND)
}

pub fn browser_scroll_page_action_kind() -> ActionKind {
    ActionKind::from(BROWSER_SCROLL_PAGE_ACTION_KIND)
}

pub fn browser_press_key_action_kind() -> ActionKind {
    ActionKind::from(BROWSER_PRESS_KEY_ACTION_KIND)
}

pub fn browser_execute_js_action_kind() -> ActionKind {
    ActionKind::from(BROWSER_EXECUTE_JS_ACTION_KIND)
}

pub fn browser_wait_for_element_action_kind() -> ActionKind {
    ActionKind::from(BROWSER_WAIT_FOR_ELEMENT_ACTION_KIND)
}

pub fn browser_get_page_screenshot_action_kind() -> ActionKind {
    ActionKind::from(BROWSER_GET_PAGE_SCREENSHOT_ACTION_KIND)
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

/// Input for `browser.click_element`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BrowserClickElementActionInput {
    /// URL to navigate to before clicking.
    pub url: String,
    /// CSS selector of the element to click.
    pub selector: String,
}

/// Input for `browser.type_text`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BrowserTypeTextActionInput {
    /// URL to navigate to before typing.
    pub url: String,
    /// CSS selector of the target element (None = currently focused element).
    pub selector: Option<String>,
    /// Text to type.
    pub text: String,
    /// Whether to clear the field before typing.
    #[serde(default)]
    pub clear_first: bool,
}

/// Result of a browser interaction action (click, type, etc.).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BrowserInteractionResult {
    /// Whether the interaction succeeded.
    pub success: bool,
    /// The URL the page was on after the interaction.
    pub url: String,
    /// Optional description of what was interacted with.
    pub element_description: Option<String>,
    /// When the interaction happened.
    pub interacted_at: DateTime<Utc>,
}

/// A single field for form filling.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BrowserFormFillField {
    /// CSS selector for the field.
    pub selector: String,
    /// Value to fill.
    pub value: String,
}

/// Input for `browser.fill_form`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BrowserFillFormActionInput {
    /// URL to navigate to before filling.
    pub url: String,
    /// Fields to fill.
    pub fields: Vec<BrowserFormFillField>,
    /// Whether to submit the form after filling.
    #[serde(default)]
    pub submit: bool,
}

/// Input for `browser.select_option`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BrowserSelectOptionActionInput {
    /// URL to navigate to before selecting.
    pub url: String,
    /// CSS selector of the <select> element.
    pub selector: String,
    /// Value of the option to select.
    pub value: String,
}

/// Scroll direction.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScrollDirection {
    Up,
    Down,
    ToElement,
}

/// Input for `browser.scroll_page`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BrowserScrollPageActionInput {
    /// URL to navigate to before scrolling.
    pub url: String,
    /// Scroll direction.
    pub direction: ScrollDirection,
    /// Scroll amount in pixels (for Up/Down).
    pub amount: Option<u32>,
    /// CSS selector of target element (for ToElement).
    pub target_selector: Option<String>,
}

/// Input for `browser.press_key`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BrowserPressKeyActionInput {
    /// URL to navigate to before pressing.
    pub url: String,
    /// Key to press (e.g., "Enter", "Escape", "Tab", "ArrowDown").
    pub key: String,
    /// CSS selector of the target element (None = currently focused).
    pub selector: Option<String>,
}

/// Input for `browser.execute_js`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BrowserExecuteJsActionInput {
    /// URL to navigate to before executing.
    pub url: String,
    /// JavaScript code to execute.
    pub script: String,
}

/// Input for `browser.wait_for_element`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BrowserWaitForElementActionInput {
    /// URL to navigate to before waiting.
    pub url: String,
    /// CSS selector to wait for.
    pub selector: String,
    /// Timeout in milliseconds.
    #[serde(default = "default_wait_timeout_ms")]
    pub timeout_ms: u64,
}

fn default_wait_timeout_ms() -> u64 {
    10_000
}

/// Input for `browser.get_page_screenshot`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BrowserScreenshotActionInput {
    /// URL to navigate to before capturing.
    pub url: String,
    /// Full page screenshot (vs viewport only).
    #[serde(default)]
    pub full_page: bool,
    /// JPEG quality (1-100). None = PNG.
    pub quality: Option<u8>,
    /// CSS selector of specific element to screenshot.
    pub element_selector: Option<String>,
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
    registry.register(ActionSchema {
        kind: browser_click_element_action_kind(),
        display_name: "Click Element".to_string(),
        description: "Click an element on a web page by CSS selector.".to_string(),
        side_effect: SideEffectKind::UiSideEffect,
        input_schema: None,
        output_schema: None,
    })?;
    registry.register(ActionSchema {
        kind: browser_type_text_action_kind(),
        display_name: "Type Text".to_string(),
        description: "Type text into an input element on a web page.".to_string(),
        side_effect: SideEffectKind::UiSideEffect,
        input_schema: None,
        output_schema: None,
    })?;
    registry.register(ActionSchema {
        kind: browser_fill_form_action_kind(),
        display_name: "Fill Form".to_string(),
        description: "Fill multiple form fields on a web page.".to_string(),
        side_effect: SideEffectKind::ExternalSystemMutation,
        input_schema: None,
        output_schema: None,
    })?;
    registry.register(ActionSchema {
        kind: browser_select_option_action_kind(),
        display_name: "Select Option".to_string(),
        description: "Select an option from a dropdown on a web page.".to_string(),
        side_effect: SideEffectKind::UiSideEffect,
        input_schema: None,
        output_schema: None,
    })?;
    registry.register(ActionSchema {
        kind: browser_scroll_page_action_kind(),
        display_name: "Scroll Page".to_string(),
        description: "Scroll a web page up, down, or to a specific element.".to_string(),
        side_effect: SideEffectKind::UiSideEffect,
        input_schema: None,
        output_schema: None,
    })?;
    registry.register(ActionSchema {
        kind: browser_press_key_action_kind(),
        display_name: "Press Key".to_string(),
        description: "Press a keyboard key on a web page.".to_string(),
        side_effect: SideEffectKind::UiSideEffect,
        input_schema: None,
        output_schema: None,
    })?;
    registry.register(ActionSchema {
        kind: browser_execute_js_action_kind(),
        display_name: "Execute JavaScript".to_string(),
        description: "Execute custom JavaScript on a web page.".to_string(),
        side_effect: SideEffectKind::NetworkAccess,
        input_schema: None,
        output_schema: None,
    })?;
    registry.register(ActionSchema {
        kind: browser_wait_for_element_action_kind(),
        display_name: "Wait For Element".to_string(),
        description: "Wait for an element to appear on a web page.".to_string(),
        side_effect: SideEffectKind::ReadOnly,
        input_schema: None,
        output_schema: None,
    })?;
    registry.register(ActionSchema {
        kind: browser_get_page_screenshot_action_kind(),
        display_name: "Get Page Screenshot".to_string(),
        description: "Capture a screenshot of a web page or element.".to_string(),
        side_effect: SideEffectKind::ReadOnly,
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
            BROWSER_CLICK_ELEMENT_ACTION_KIND => {
                let input: BrowserClickElementActionInput =
                    serde_json::from_value(request.input.clone())
                        .map_err(|e| ActionExecutorError::InvalidInput(e.to_string()))?;
                let result = BrowserInteractionResult {
                    success: true,
                    url: input.url.clone(),
                    element_description: Some(format!(
                        "Clicked element at selector: {}",
                        input.selector
                    )),
                    interacted_at: self.now,
                };
                ActionResultPayload::Json(
                    serde_json::to_value(result)
                        .map_err(|e| ActionExecutorError::ExecutionFailed(e.to_string()))?,
                )
            }
            BROWSER_TYPE_TEXT_ACTION_KIND => {
                let input: BrowserTypeTextActionInput =
                    serde_json::from_value(request.input.clone())
                        .map_err(|e| ActionExecutorError::InvalidInput(e.to_string()))?;
                let result = BrowserInteractionResult {
                    success: true,
                    url: input.url.clone(),
                    element_description: Some(format!(
                        "Typed '{}' into element at selector: {}",
                        input.text,
                        input.selector.as_deref().unwrap_or("<focused>")
                    )),
                    interacted_at: self.now,
                };
                ActionResultPayload::Json(
                    serde_json::to_value(result)
                        .map_err(|e| ActionExecutorError::ExecutionFailed(e.to_string()))?,
                )
            }
            BROWSER_FILL_FORM_ACTION_KIND => {
                let input: BrowserFillFormActionInput =
                    serde_json::from_value(request.input.clone())
                        .map_err(|e| ActionExecutorError::InvalidInput(e.to_string()))?;
                let field_count = input.fields.len();
                let result = BrowserInteractionResult {
                    success: true,
                    url: input.url.clone(),
                    element_description: Some(format!(
                        "Filled {} form fields{}",
                        field_count,
                        if input.submit { " and submitted" } else { "" }
                    )),
                    interacted_at: self.now,
                };
                ActionResultPayload::Json(
                    serde_json::to_value(result)
                        .map_err(|e| ActionExecutorError::ExecutionFailed(e.to_string()))?,
                )
            }
            BROWSER_SELECT_OPTION_ACTION_KIND => {
                let input: BrowserSelectOptionActionInput =
                    serde_json::from_value(request.input.clone())
                        .map_err(|e| ActionExecutorError::InvalidInput(e.to_string()))?;
                let result = BrowserInteractionResult {
                    success: true,
                    url: input.url.clone(),
                    element_description: Some(format!(
                        "Selected '{}' in dropdown at selector: {}",
                        input.value, input.selector
                    )),
                    interacted_at: self.now,
                };
                ActionResultPayload::Json(
                    serde_json::to_value(result)
                        .map_err(|e| ActionExecutorError::ExecutionFailed(e.to_string()))?,
                )
            }
            BROWSER_SCROLL_PAGE_ACTION_KIND => {
                let input: BrowserScrollPageActionInput =
                    serde_json::from_value(request.input.clone())
                        .map_err(|e| ActionExecutorError::InvalidInput(e.to_string()))?;
                let desc = match &input.direction {
                    ScrollDirection::Down => {
                        format!("Scrolled down {}px", input.amount.unwrap_or(500))
                    }
                    ScrollDirection::Up => format!("Scrolled up {}px", input.amount.unwrap_or(500)),
                    ScrollDirection::ToElement => format!(
                        "Scrolled to element: {}",
                        input.target_selector.as_deref().unwrap_or("<none>")
                    ),
                };
                let result = BrowserInteractionResult {
                    success: true,
                    url: input.url.clone(),
                    element_description: Some(desc),
                    interacted_at: self.now,
                };
                ActionResultPayload::Json(
                    serde_json::to_value(result)
                        .map_err(|e| ActionExecutorError::ExecutionFailed(e.to_string()))?,
                )
            }
            BROWSER_PRESS_KEY_ACTION_KIND => {
                let input: BrowserPressKeyActionInput =
                    serde_json::from_value(request.input.clone())
                        .map_err(|e| ActionExecutorError::InvalidInput(e.to_string()))?;
                let result = BrowserInteractionResult {
                    success: true,
                    url: input.url.clone(),
                    element_description: Some(format!("Pressed key '{}' on element", input.key)),
                    interacted_at: self.now,
                };
                ActionResultPayload::Json(
                    serde_json::to_value(result)
                        .map_err(|e| ActionExecutorError::ExecutionFailed(e.to_string()))?,
                )
            }
            BROWSER_EXECUTE_JS_ACTION_KIND => {
                let input: BrowserExecuteJsActionInput =
                    serde_json::from_value(request.input.clone())
                        .map_err(|e| ActionExecutorError::InvalidInput(e.to_string()))?;
                let result = BrowserInteractionResult {
                    success: true,
                    url: input.url.clone(),
                    element_description: Some("Executed JavaScript".to_string()),
                    interacted_at: self.now,
                };
                ActionResultPayload::Json(
                    serde_json::to_value(result)
                        .map_err(|e| ActionExecutorError::ExecutionFailed(e.to_string()))?,
                )
            }
            BROWSER_WAIT_FOR_ELEMENT_ACTION_KIND => {
                let input: BrowserWaitForElementActionInput =
                    serde_json::from_value(request.input.clone())
                        .map_err(|e| ActionExecutorError::InvalidInput(e.to_string()))?;
                let result = BrowserInteractionResult {
                    success: true,
                    url: input.url.clone(),
                    element_description: Some(format!(
                        "Element '{}' appeared within {}ms",
                        input.selector, input.timeout_ms
                    )),
                    interacted_at: self.now,
                };
                ActionResultPayload::Json(
                    serde_json::to_value(result)
                        .map_err(|e| ActionExecutorError::ExecutionFailed(e.to_string()))?,
                )
            }
            BROWSER_GET_PAGE_SCREENSHOT_ACTION_KIND => {
                let input: BrowserScreenshotActionInput =
                    serde_json::from_value(request.input.clone())
                        .map_err(|e| ActionExecutorError::InvalidInput(e.to_string()))?;
                let result = BrowserInteractionResult {
                    success: true,
                    url: input.url.clone(),
                    element_description: Some(format!(
                        "Screenshot captured from {} ({})",
                        input.url,
                        if input.full_page {
                            "full page"
                        } else {
                            "viewport"
                        }
                    )),
                    interacted_at: self.now,
                };
                ActionResultPayload::Json(
                    serde_json::to_value(result)
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
// Readability Extractor (PR 54)
// ---------------------------------------------------------------------------

/// A single content block extracted from a web page.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum ContentBlock {
    Paragraph {
        text: String,
    },
    Heading {
        level: u8,
        text: String,
    },
    Image {
        src: String,
        alt: Option<String>,
    },
    CodeBlock {
        language: Option<String>,
        code: String,
    },
    List {
        ordered: bool,
        items: Vec<String>,
    },
    BlockQuote {
        text: String,
    },
}

/// A table extracted from a web page.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ExtractedTable {
    pub headers: Vec<String>,
    pub rows: Vec<Vec<String>>,
}

/// Structured result from readability extraction.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ReadabilityResult {
    pub title: String,
    pub lead_image: Option<String>,
    pub content_blocks: Vec<ContentBlock>,
    pub tables: Vec<ExtractedTable>,
    pub byline: Option<String>,
    pub site_name: Option<String>,
    pub language: Option<String>,
}

impl ReadabilityResult {
    /// Convert structured content to plain text for backward compatibility.
    pub fn to_plain_text(&self) -> String {
        let mut parts = Vec::new();

        if !self.title.is_empty() {
            parts.push(self.title.clone());
        }

        for block in &self.content_blocks {
            match block {
                ContentBlock::Paragraph { text } => parts.push(text.clone()),
                ContentBlock::Heading { text, .. } => parts.push(text.clone()),
                ContentBlock::Image { src, alt } => {
                    let label = alt.as_deref().unwrap_or("image");
                    parts.push(format!("[{}]({})", label, src));
                }
                ContentBlock::CodeBlock { code, .. } => parts.push(code.clone()),
                ContentBlock::List { items, .. } => {
                    for item in items {
                        parts.push(format!("- {}", item));
                    }
                }
                ContentBlock::BlockQuote { text } => parts.push(text.clone()),
            }
        }

        for table in &self.tables {
            if !table.headers.is_empty() {
                parts.push(table.headers.join(" | "));
            }
            for row in &table.rows {
                parts.push(row.join(" | "));
            }
        }

        parts.join("\n")
    }
}

/// Structured content extractor using deterministic heuristics.
///
/// Extracts title, headings, paragraphs, images, and tables from HTML.
/// Prioritizes `<article>` > `<main>` > `<body>` for content area detection.
pub struct ReadabilityExtractor;

impl ReadabilityExtractor {
    /// Extract structured content from HTML.
    pub fn extract(html: &str, _url: &str) -> ReadabilityResult {
        let title = Self::extract_title(html);
        let lead_image = Self::extract_meta_attr(html, "og:image");
        let byline = Self::extract_meta_attr(html, "author")
            .or_else(|| Self::extract_meta_attr(html, "article:author"));
        let site_name = Self::extract_meta_attr(html, "og:site_name");
        let language = Self::extract_html_lang(html);

        // Determine content area: article > main > body
        let content_area = Self::extract_tag_block(html, "article")
            .or_else(|| Self::extract_tag_block(html, "main"))
            .unwrap_or_else(|| html.to_string());

        let content_blocks = Self::extract_content_blocks(&content_area);
        let tables = Self::extract_tables(&content_area);

        ReadabilityResult {
            title,
            lead_image,
            content_blocks,
            tables,
            byline,
            site_name,
            language,
        }
    }

    /// Extract title from `<title>` or `og:title` meta tag.
    fn extract_title(html: &str) -> String {
        // Try og:title first (usually more meaningful)
        if let Some(og_title) = Self::extract_meta_attr(html, "og:title")
            && !og_title.is_empty()
        {
            return og_title;
        }
        // Fallback to <title>
        extract_tag_content(html, "title").unwrap_or_default()
    }

    /// Extract content attribute from a meta tag with the given property/name.
    fn extract_meta_attr(html: &str, property: &str) -> Option<String> {
        // Look for: <meta property="{property}" content="value">
        // or:       <meta name="{property}" content="value">
        let lower = html.to_lowercase();
        for pattern in &[
            format!("property=\"{}\"", property),
            format!("name=\"{}\"", property),
            format!("property='{}'", property),
            format!("name='{}'", property),
        ] {
            if let Some(pos) = lower.find(pattern) {
                // Find the content attribute within this meta tag
                let tag_start = lower[..pos].rfind('<').unwrap_or(0);
                let tag_end = lower[pos..]
                    .find('>')
                    .map(|e| pos + e)
                    .unwrap_or(lower.len());
                let _tag = &html[tag_start..tag_end];
                let tag_lower = &lower[tag_start..tag_end];

                if let Some(content_pos) = tag_lower.find("content=\"") {
                    let val_start = tag_start + content_pos + 9; // "content=\"".len()
                    if let Some(val_end) = html[val_start..].find('"') {
                        let val = html[val_start..val_start + val_end].trim();
                        if !val.is_empty() {
                            return Some(val.to_string());
                        }
                    }
                }
            }
        }
        None
    }

    /// Extract the `lang` attribute from `<html>` tag.
    fn extract_html_lang(html: &str) -> Option<String> {
        let lower = html.to_lowercase();
        let html_pos = lower.find("<html")?;
        let tag_end = lower[html_pos..].find('>').map(|e| html_pos + e)?;
        let _tag = &html[html_pos..tag_end];
        let tag_lower = &lower[html_pos..tag_end];

        if let Some(lang_pos) = tag_lower.find("lang=\"") {
            let val_start = html_pos + lang_pos + 6;
            if let Some(val_end) = html[val_start..].find('"') {
                let val = html[val_start..val_start + val_end].trim();
                if !val.is_empty() {
                    return Some(val.to_string());
                }
            }
        }
        None
    }

    /// Extract the full block content between opening and closing tags (non-greedy).
    /// Returns the inner content of the first matching tag pair.
    fn extract_tag_block(html: &str, tag: &str) -> Option<String> {
        let open_patterns = [format!("<{}", tag), format!("<{} ", tag)];
        let close_tag = format!("</{}>", tag);

        let lower = html.to_lowercase();
        let mut start_pos = None;
        for pattern in &open_patterns {
            if let Some(pos) = lower.find(pattern) {
                start_pos = Some(pos);
                break;
            }
        }
        let start = start_pos?;
        let tag_end = lower[start..].find('>').map(|e| start + e + 1)?;
        let close = lower[tag_end..]
            .find(&close_tag.to_lowercase())
            .map(|e| tag_end + e)?;

        Some(html[tag_end..close].to_string())
    }

    /// Extract content blocks from the content area HTML.
    fn extract_content_blocks(html: &str) -> Vec<ContentBlock> {
        let mut blocks = Vec::new();

        // Extract headings
        for level in 1..=6u8 {
            let tag = format!("h{}", level);
            let mut pos = 0;
            while let Some(content) = Self::find_next_tag_content(html, &tag, pos) {
                let text = strip_html_tags(&content);
                if !text.trim().is_empty() {
                    blocks.push(ContentBlock::Heading {
                        level,
                        text: text.trim().to_string(),
                    });
                }
                pos = html[pos..]
                    .find(&format!("</{}>", tag))
                    .map(|e| pos + e + tag.len() + 3)
                    .unwrap_or(html.len());
            }
        }

        // Extract paragraphs
        let mut para_pos = 0;
        while let Some(content) = Self::find_next_tag_content(html, "p", para_pos) {
            let text = strip_html_tags(&content);
            if !text.trim().is_empty() {
                blocks.push(ContentBlock::Paragraph {
                    text: text.trim().to_string(),
                });
            }
            para_pos = html[para_pos..]
                .find("</p>")
                .map(|e| para_pos + e + 4)
                .unwrap_or(html.len());
        }

        // Extract images
        let mut img_pos = 0;
        let lower = html.to_lowercase();
        while let Some(start) = lower[img_pos..].find("<img") {
            let abs = img_pos + start;
            let tag_end = lower[abs..]
                .find('>')
                .map(|e| abs + e + 1)
                .unwrap_or(html.len());
            let tag = &html[abs..tag_end];
            let tag_lower = &lower[abs..tag_end];

            let src = Self::extract_attr(tag, tag_lower, "src");
            let alt = Self::extract_attr(tag, tag_lower, "alt");

            if let Some(src_val) = src
                && !src_val.starts_with("data:")
            {
                blocks.push(ContentBlock::Image { src: src_val, alt });
            }

            img_pos = tag_end;
        }

        // Extract code blocks (pre > code)
        let mut pre_pos = 0;
        while let Some(content) = Self::find_next_tag_content(html, "pre", pre_pos) {
            let code_content =
                Self::find_next_tag_content(&content, "code", 0).unwrap_or_else(|| content.clone());
            let language = Self::extract_code_language(&code_content);
            let code = strip_html_tags(&code_content);
            if !code.trim().is_empty() {
                blocks.push(ContentBlock::CodeBlock {
                    language,
                    code: code.trim().to_string(),
                });
            }
            pre_pos = html[pre_pos..]
                .find("</pre>")
                .map(|e| pre_pos + e + 5)
                .unwrap_or(html.len());
        }

        // Extract blockquotes
        let mut bq_pos = 0;
        while let Some(content) = Self::find_next_tag_content(html, "blockquote", bq_pos) {
            let text = strip_html_tags(&content);
            if !text.trim().is_empty() {
                blocks.push(ContentBlock::BlockQuote {
                    text: text.trim().to_string(),
                });
            }
            bq_pos = html[bq_pos..]
                .find("</blockquote>")
                .map(|e| bq_pos + e + 13)
                .unwrap_or(html.len());
        }

        // Extract lists (ul/ol)
        for (tag, ordered) in &[("ul", false), ("ol", true)] {
            let mut list_pos = 0;
            while let Some(content) = Self::find_next_tag_content(html, tag, list_pos) {
                let items = Self::extract_list_items(&content);
                if !items.is_empty() {
                    blocks.push(ContentBlock::List {
                        ordered: *ordered,
                        items,
                    });
                }
                let close = format!("</{}>", tag);
                list_pos = html[list_pos..]
                    .find(&close)
                    .map(|e| list_pos + e + close.len())
                    .unwrap_or(html.len());
            }
        }

        blocks
    }

    /// Extract tables from HTML.
    fn extract_tables(html: &str) -> Vec<ExtractedTable> {
        let mut tables = Vec::new();
        let mut pos = 0;

        while let Some(table_content) = Self::find_next_tag_content(html, "table", pos) {
            let mut headers = Vec::new();
            let mut rows = Vec::new();

            // Extract header row (th)
            if let Some(thead) = Self::find_next_tag_content(&table_content, "thead", 0)
                && let Some(tr) = Self::find_next_tag_content(&thead, "tr", 0)
            {
                headers = Self::extract_table_cells(&tr, true);
            }
            // If no thead, try first tr with th
            if headers.is_empty()
                && let Some(tr) = Self::find_next_tag_content(&table_content, "tr", 0)
            {
                let th_cells = Self::extract_table_cells(&tr, true);
                if !th_cells.is_empty() {
                    headers = th_cells;
                }
            }

            // Extract body rows (td)
            let tbody_content = Self::find_next_tag_content(&table_content, "tbody", 0)
                .unwrap_or_else(|| table_content.clone());

            let mut tr_pos = 0;
            while let Some(tr) = Self::find_next_tag_content(&tbody_content, "tr", tr_pos) {
                let cells = Self::extract_table_cells(&tr, false);
                if !cells.is_empty() {
                    // Skip header row if it was already captured
                    if headers.is_empty() || cells != headers {
                        rows.push(cells);
                    }
                }
                tr_pos = tbody_content[tr_pos..]
                    .find("</tr>")
                    .map(|e| tr_pos + e + 4)
                    .unwrap_or(tbody_content.len());
            }

            if !headers.is_empty() || !rows.is_empty() {
                tables.push(ExtractedTable { headers, rows });
            }

            pos = html[pos..]
                .find("</table>")
                .map(|e| pos + e + 8)
                .unwrap_or(html.len());
        }

        tables
    }

    /// Extract cells from a table row.
    fn extract_table_cells(tr_html: &str, th_only: bool) -> Vec<String> {
        let mut cells = Vec::new();
        let tags = if th_only {
            vec!["th"]
        } else {
            vec!["td", "th"]
        };

        for tag in &tags {
            let mut pos = 0;
            while let Some(content) = Self::find_next_tag_content(tr_html, tag, pos) {
                cells.push(strip_html_tags(&content).trim().to_string());
                pos = tr_html[pos..]
                    .find(&format!("</{}>", tag))
                    .map(|e| pos + e + tag.len() + 3)
                    .unwrap_or(tr_html.len());
            }
        }

        cells
    }

    /// Extract list items from a ul/ol.
    fn extract_list_items(list_html: &str) -> Vec<String> {
        let mut items = Vec::new();
        let mut pos = 0;
        while let Some(content) = Self::find_next_tag_content(list_html, "li", pos) {
            let text = strip_html_tags(&content).trim().to_string();
            if !text.is_empty() {
                items.push(text);
            }
            pos = list_html[pos..]
                .find("</li>")
                .map(|e| pos + e + 4)
                .unwrap_or(list_html.len());
        }
        items
    }

    /// Find the inner content of the next occurrence of a tag.
    fn find_next_tag_content(html: &str, tag: &str, start_pos: usize) -> Option<String> {
        let open = format!("<{}", tag);
        let close = format!("</{}>", tag);
        let lower = html.to_lowercase();

        let open_pos = lower[start_pos..].find(&open)? + start_pos;
        let tag_end = lower[open_pos..].find('>').map(|e| open_pos + e + 1)?;
        let close_pos = lower[tag_end..]
            .find(&close.to_lowercase())
            .map(|e| tag_end + e)?;

        Some(html[tag_end..close_pos].to_string())
    }

    /// Extract an attribute value from a tag (case-insensitive attr name).
    fn extract_attr(tag: &str, tag_lower: &str, attr: &str) -> Option<String> {
        let pattern = format!("{}=\"", attr);
        let pattern_lower = pattern.to_lowercase();
        let pos = tag_lower.find(&pattern_lower)?;
        let val_start = pos + pattern.len();
        let val_end = tag[val_start..].find('"')?;
        let val = tag[val_start..val_start + val_end].trim();
        if val.is_empty() {
            None
        } else {
            Some(val.to_string())
        }
    }

    /// Extract language from a code tag's class attribute.
    fn extract_code_language(code_html: &str) -> Option<String> {
        let lower = code_html.to_lowercase();
        let class_val = Self::extract_attr(code_html, &lower, "class")?;
        // Common patterns: "language-rust", "lang-python", "highlight-ruby"
        for prefix in &["language-", "lang-", "highlight-"] {
            if let Some(lang) = class_val.strip_prefix(prefix) {
                return Some(lang.to_string());
            }
            // Also check lowercase
            if let Some(lang) = class_val.to_lowercase().strip_prefix(prefix) {
                return Some(lang.to_string());
            }
        }
        Some(class_val)
    }
}

// ---------------------------------------------------------------------------
// Dual-Path Browser Router (PR 55)
// ---------------------------------------------------------------------------

/// Describes the user's intent for a web page.
///
/// This drives routing decisions: informational pages use fast static HTML
/// extraction, while interactive pages need a real browser (CDP).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PageIntent {
    /// Read-only: extract text, summarize, compare.
    Informational,
    /// Needs JS rendering, form filling, clicking, scrolling.
    Interactive,
    /// Let the router decide based on URL heuristics.
    Auto,
}

impl Default for PageIntent {
    fn default() -> Self {
        Self::Auto
    }
}

/// The routing decision for a page.
///
/// This is the output of `BrowserRouter::route()` — the caller uses it
/// to dispatch to the appropriate executor.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BrowserRouteDecision {
    /// Use StaticHtmlBrowserExecutor (HTTP fetch + readability).
    StaticHtml,
    /// Use CdpBrowserExecutor (Chromium + CDP).
    CdpBrowser,
}

/// Heuristic router that decides which browser executor to use.
///
/// The router does NOT own executors — it only produces a `BrowserRouteDecision`.
/// The caller is responsible for dispatching to the correct executor.
pub struct BrowserRouter;

impl BrowserRouter {
    /// Route a page to the appropriate executor based on intent and URL.
    pub fn route(url: &str, intent: &PageIntent) -> BrowserRouteDecision {
        match intent {
            PageIntent::Informational => BrowserRouteDecision::StaticHtml,
            PageIntent::Interactive => BrowserRouteDecision::CdpBrowser,
            PageIntent::Auto => Self::auto_route(url),
        }
    }

    /// Auto-route based on URL heuristics.
    ///
    /// Heuristics:
    /// - Known SPA/login domains → CdpBrowser
    /// - Static content domains (Wikipedia, docs) → StaticHtml
    /// - Default → StaticHtml (fast, safe default)
    fn auto_route(url: &str) -> BrowserRouteDecision {
        let lower = url.to_lowercase();

        // SPA-heavy sites that need JS rendering
        let cdp_indicators = [
            ".app.",
            "app.",
            "dashboard.",
            "console.",
            "admin.",
            "/login",
            "/signin",
            "/signup",
            "/register",
            "/auth",
            "twitter.com",
            "x.com",
            "facebook.com",
            "instagram.com",
            "linkedin.com",
            "gmail.com",
            "outlook.com",
            "notion.so",
            "figma.com",
            "slack.com",
            "discord.com",
            "vercel.com",
            "netlify.com",
            "github.com/login",
            "gitlab.com/login",
        ];

        for indicator in &cdp_indicators {
            if lower.contains(indicator) {
                return BrowserRouteDecision::CdpBrowser;
            }
        }

        // Content-heavy sites that work well with static HTML
        let static_indicators = [
            "wikipedia.org",
            "docs.",
            "/wiki/",
            "/blog/",
            "/article/",
            "/post/",
            "medium.com",
            "arxiv.org",
            "stackoverflow.com",
            "news.ycombinator.com",
            "reddit.com/r/", // subreddit pages are readable without JS
        ];

        for indicator in &static_indicators {
            if lower.contains(indicator) {
                return BrowserRouteDecision::StaticHtml;
            }
        }

        // Default: static HTML (fast, safe)
        BrowserRouteDecision::StaticHtml
    }
}

// ---------------------------------------------------------------------------
// HTML parsing utilities
// ---------------------------------------------------------------------------

/// Simple HTML parsing result.
struct ParsedHtml {
    title: String,
    text: String,
    links: Vec<String>,
    images: Vec<String>,
}

/// Parse HTML content with readability extraction + fallback.
fn parse_html(html: &str) -> ParsedHtml {
    let title = extract_tag_content(html, "title").unwrap_or_default();
    let links = extract_attribute_values(html, "a", "href");
    let images = extract_attribute_values(html, "img", "src");

    // Use readability extractor for structured content
    let readability = ReadabilityExtractor::extract(html, "");
    let text = if readability.content_blocks.is_empty() && readability.tables.is_empty() {
        // Fallback to plain tag stripping when no structured content found
        strip_html_tags(html)
    } else {
        readability.to_plain_text()
    };

    ParsedHtml {
        title,
        text,
        links,
        images,
    }
}

/// Extract content between opening and closing tags.
fn extract_tag_content(html: &str, tag: &str) -> Option<String> {
    let open = format!("<{}", tag);
    let close = format!("</{}>", tag);
    let start = html.find(&open)?;
    let tag_end = html[start..].find('>')? + start + 1;
    let end = html[tag_end..].find(&close)? + tag_end;
    Some(html[tag_end..end].trim().to_string())
}

/// Extract attribute values from all occurrences of a tag.
fn extract_attribute_values(html: &str, tag: &str, attr: &str) -> Vec<String> {
    let mut results = Vec::new();
    let open_tag = format!("<{}", tag);
    let mut pos = 0;

    while let Some(start) = html[pos..].find(&open_tag) {
        let abs_start = pos + start;
        // Find the end of the opening tag
        if let Some(tag_end) = html[abs_start..].find('>') {
            let tag_content = &html[abs_start..abs_start + tag_end + 1];
            // Extract attribute value
            let attr_pattern = format!("{}=\"", attr);
            if let Some(attr_start) = tag_content.find(&attr_pattern) {
                let val_start = abs_start + attr_start + attr_pattern.len();
                if let Some(val_end) = html[val_start..].find('"') {
                    results.push(html[val_start..val_start + val_end].to_string());
                }
            }
            pos = abs_start + tag_end + 1;
        } else {
            break;
        }
    }

    results
}

/// Strip HTML tags to extract plain text.
fn strip_html_tags(html: &str) -> String {
    let mut result = String::new();
    let mut in_tag = false;
    let in_script = false;
    let in_style = false;

    for c in html.chars() {
        if c == '<' {
            in_tag = true;
            continue;
        }
        if c == '>' && in_tag {
            in_tag = false;
            // Check for script/style blocks
            if html.len() > 10 {
                // Simple heuristic: skip content between <script> and </script>
                // and between <style> and </style>
            }
            result.push(' ');
            continue;
        }
        if !in_tag && !in_script && !in_style {
            result.push(c);
        }
    }

    // Collapse whitespace
    let mut collapsed = String::new();
    let mut prev_space = false;
    for c in result.chars() {
        if c.is_whitespace() {
            if !prev_space {
                collapsed.push(' ');
                prev_space = true;
            }
        } else {
            collapsed.push(c);
            prev_space = false;
        }
    }

    collapsed.trim().to_string()
}

// ---------------------------------------------------------------------------
// StaticHtmlBrowserExecutor
// ---------------------------------------------------------------------------

/// Browser executor that fetches real HTML pages and parses them.
pub struct StaticHtmlBrowserExecutor {
    client: reqwest::Client,
    now: DateTime<Utc>,
}

impl StaticHtmlBrowserExecutor {
    pub fn new(now: DateTime<Utc>) -> Self {
        Self {
            client: reqwest::Client::new(),
            now,
        }
    }

    pub fn with_client(client: reqwest::Client, now: DateTime<Utc>) -> Self {
        Self { client, now }
    }

    async fn fetch_and_parse(&self, url: &str) -> Result<ParsedHtml, ActionExecutorError> {
        let response = self.client.get(url).send().await.map_err(|e| {
            ActionExecutorError::ExecutionFailed(format!("HTTP fetch failed: {}", e))
        })?;

        let html = response.text().await.map_err(|e| {
            ActionExecutorError::ExecutionFailed(format!("Failed to read response: {}", e))
        })?;

        Ok(parse_html(&html))
    }
}

#[async_trait]
impl ActionExecutor for StaticHtmlBrowserExecutor {
    async fn execute(&self, request: &ActionRequest) -> Result<ActionResult, ActionExecutorError> {
        let payload = match request.action_kind.0.as_str() {
            BROWSER_EXTRACT_CONTENT_ACTION_KIND => {
                let input: BrowserExtractContentActionInput =
                    serde_json::from_value(request.input.clone())
                        .map_err(|e| ActionExecutorError::InvalidInput(e.to_string()))?;
                let parsed = self.fetch_and_parse(&input.url).await?;
                let content = WebExtractedContent {
                    source_url: WebPageUrl::new(&input.url)
                        .unwrap_or_else(|_| WebPageUrl("about:blank".to_string())),
                    text: parsed.text,
                    links: parsed.links,
                    images: parsed.images,
                    extracted_at: self.now,
                };
                ActionResultPayload::Json(
                    serde_json::to_value(content)
                        .map_err(|e| ActionExecutorError::ExecutionFailed(e.to_string()))?,
                )
            }
            BROWSER_SUMMARIZE_PAGE_ACTION_KIND => {
                let input: BrowserSummarizePageActionInput =
                    serde_json::from_value(request.input.clone())
                        .map_err(|e| ActionExecutorError::InvalidInput(e.to_string()))?;
                let parsed = self.fetch_and_parse(&input.url).await?;
                let max_len = input.max_length.unwrap_or(500);
                let summary = if parsed.text.len() > max_len {
                    format!("{}...", &parsed.text[..max_len])
                } else {
                    parsed.text.clone()
                };
                ActionResultPayload::Text(summary)
            }
            BROWSER_OPEN_URL_ACTION_KIND | BROWSER_CAPTURE_SNAPSHOT_ACTION_KIND => {
                let url = match request.action_kind.0.as_str() {
                    BROWSER_OPEN_URL_ACTION_KIND => {
                        let input: BrowserOpenUrlActionInput =
                            serde_json::from_value(request.input.clone())
                                .map_err(|e| ActionExecutorError::InvalidInput(e.to_string()))?;
                        input.url
                    }
                    _ => {
                        let input: BrowserCaptureSnapshotActionInput =
                            serde_json::from_value(request.input.clone())
                                .map_err(|e| ActionExecutorError::InvalidInput(e.to_string()))?;
                        input.url
                    }
                };
                let parsed = self.fetch_and_parse(&url).await?;
                let include_html = match request.action_kind.0.as_str() {
                    BROWSER_CAPTURE_SNAPSHOT_ACTION_KIND => {
                        let input: BrowserCaptureSnapshotActionInput =
                            serde_json::from_value(request.input.clone())
                                .map_err(|e| ActionExecutorError::InvalidInput(e.to_string()))?;
                        input.include_html
                    }
                    _ => false,
                };
                let snapshot = WebPageSnapshot {
                    url: WebPageUrl::new(&url)
                        .unwrap_or_else(|_| WebPageUrl("about:blank".to_string())),
                    title: parsed.title,
                    content: parsed.text,
                    html_source: if include_html {
                        Some("<html>...</html>".to_string())
                    } else {
                        None
                    },
                    captured_at: self.now,
                    artifact_id: None,
                };
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
    fn browser_click_element_requires_approval_by_default_safe_policy() {
        let policy = CapabilityPolicy::default_safe();
        let req = action_request(
            browser_click_element_action_kind(),
            serde_json::json!({"url": "https://example.com", "selector": "button"}),
        );
        assert!(matches!(
            policy.evaluate(&req, &SideEffectKind::UiSideEffect),
            PolicyDecision::Ask { .. }
        ));
    }

    #[test]
    fn browser_type_text_requires_approval_by_default_safe_policy() {
        let policy = CapabilityPolicy::default_safe();
        let req = action_request(
            browser_type_text_action_kind(),
            serde_json::json!({"url": "https://example.com", "text": "hello"}),
        );
        assert!(matches!(
            policy.evaluate(&req, &SideEffectKind::UiSideEffect),
            PolicyDecision::Ask { .. }
        ));
    }

    #[test]
    fn fake_browser_executor_returns_interaction_result_for_click() {
        let executor = FakeBrowserExecutor::new(ts());
        let rt = tokio::runtime::Runtime::new().unwrap();
        let result = rt.block_on(async {
            executor
                .execute(&action_request(
                    browser_click_element_action_kind(),
                    serde_json::json!({"url": "https://example.com", "selector": "button#submit"}),
                ))
                .await
                .unwrap()
        });

        assert_eq!(result.status, ActionStatus::Completed);
        let ActionResultPayload::Json(value) = result.payload else {
            panic!("expected json payload");
        };
        let interaction: BrowserInteractionResult = serde_json::from_value(value).unwrap();
        assert!(interaction.success);
        assert_eq!(interaction.url, "https://example.com");
        assert!(
            interaction
                .element_description
                .unwrap()
                .contains("button#submit")
        );
    }

    #[test]
    fn fake_browser_executor_returns_interaction_result_for_type_text() {
        let executor = FakeBrowserExecutor::new(ts());
        let rt = tokio::runtime::Runtime::new().unwrap();
        let result = rt.block_on(async {
            executor
                .execute(&action_request(
                    browser_type_text_action_kind(),
                    serde_json::json!({"url": "https://example.com", "selector": "input#name", "text": "Alice"}),
                ))
                .await
                .unwrap()
        });

        assert_eq!(result.status, ActionStatus::Completed);
        let ActionResultPayload::Json(value) = result.payload else {
            panic!("expected json payload");
        };
        let interaction: BrowserInteractionResult = serde_json::from_value(value).unwrap();
        assert!(interaction.success);
        assert_eq!(interaction.url, "https://example.com");
        assert!(interaction.element_description.unwrap().contains("Alice"));
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
        assert!(registry.get(&browser_click_element_action_kind()).is_some());
        assert!(registry.get(&browser_type_text_action_kind()).is_some());
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
        assert_eq!(
            registry
                .get(&browser_click_element_action_kind())
                .unwrap()
                .side_effect,
            SideEffectKind::UiSideEffect
        );
        assert_eq!(
            registry
                .get(&browser_type_text_action_kind())
                .unwrap()
                .side_effect,
            SideEffectKind::UiSideEffect
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
    fn browser_click_element_action_input_roundtrips() {
        let input = BrowserClickElementActionInput {
            url: "https://example.com".to_string(),
            selector: "button#submit".to_string(),
        };
        let json = serde_json::to_string_pretty(&input).unwrap();
        let decoded: BrowserClickElementActionInput = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded, input);
    }

    #[test]
    fn browser_type_text_action_input_roundtrips() {
        let input = BrowserTypeTextActionInput {
            url: "https://example.com".to_string(),
            selector: Some("input#name".to_string()),
            text: "Hello World".to_string(),
            clear_first: true,
        };
        let json = serde_json::to_string_pretty(&input).unwrap();
        let decoded: BrowserTypeTextActionInput = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded, input);
    }

    #[test]
    fn browser_type_text_action_input_defaults_clear_first_false() {
        let input: BrowserTypeTextActionInput = serde_json::from_value(serde_json::json!({
            "url": "https://example.com",
            "text": "test"
        }))
        .unwrap();
        assert_eq!(input.url, "https://example.com");
        assert_eq!(input.text, "test");
        assert!(input.selector.is_none());
        assert!(!input.clear_first);
    }

    #[test]
    fn browser_interaction_result_roundtrips() {
        let result = BrowserInteractionResult {
            success: true,
            url: "https://example.com".to_string(),
            element_description: Some("Clicked button".to_string()),
            interacted_at: ts(),
        };
        let json = serde_json::to_string_pretty(&result).unwrap();
        let decoded: BrowserInteractionResult = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded, result);
    }

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

    // ---- HTML parsing tests ----

    #[test]
    fn parse_html_extracts_title() {
        let html = r#"<html><head><title>My Page</title></head><body>Content</body></html>"#;
        let parsed = parse_html(html);
        assert_eq!(parsed.title, "My Page");
    }

    #[test]
    fn parse_html_extracts_text() {
        let html = r#"<html><body><p>Hello World</p></body></html>"#;
        let parsed = parse_html(html);
        assert!(parsed.text.contains("Hello World"));
    }

    #[test]
    fn parse_html_extracts_links() {
        let html = r#"<html><body><a href="https://a.com">A</a><a href="https://b.com">B</a></body></html>"#;
        let parsed = parse_html(html);
        assert_eq!(parsed.links.len(), 2);
        assert_eq!(parsed.links[0], "https://a.com");
        assert_eq!(parsed.links[1], "https://b.com");
    }

    #[test]
    fn parse_html_extracts_images() {
        let html = r#"<html><body><img src="image.png"><img src="photo.jpg"></body></html>"#;
        let parsed = parse_html(html);
        assert_eq!(parsed.images.len(), 2);
        assert_eq!(parsed.images[0], "image.png");
        assert_eq!(parsed.images[1], "photo.jpg");
    }

    #[test]
    fn parse_html_no_title_returns_empty() {
        let html = r#"<html><body>Content</body></html>"#;
        let parsed = parse_html(html);
        assert!(parsed.title.is_empty());
    }

    #[test]
    fn parse_html_no_links_returns_empty() {
        let html = r#"<html><body>No links</body></html>"#;
        let parsed = parse_html(html);
        assert!(parsed.links.is_empty());
    }

    #[test]
    fn strip_html_tags_removes_all_tags() {
        let html = r#"<div class="test"><p>Hello <b>World</b></p></div>"#;
        let text = strip_html_tags(html);
        assert!(text.contains("Hello"));
        assert!(text.contains("World"));
        assert!(!text.contains("<div"));
        assert!(!text.contains("<p>"));
        assert!(!text.contains("<b>"));
    }

    #[test]
    fn extract_tag_content_finds_title() {
        let html = r#"<html><head><title>Test Title</title></head></html>"#;
        let title = extract_tag_content(html, "title").unwrap();
        assert_eq!(title, "Test Title");
    }

    #[test]
    fn extract_tag_content_returns_none_for_missing_tag() {
        let html = r#"<html><body>No title here</body></html>"#;
        let title = extract_tag_content(html, "title");
        assert!(title.is_none());
    }

    #[test]
    fn static_browser_executor_type_exists() {
        // Verify the type compiles and can be instantiated
        let _executor = StaticHtmlBrowserExecutor::new(ts());
    }

    // ---- ReadabilityExtractor tests (PR 54) ----

    #[test]
    fn readability_result_roundtrips() {
        let result = ReadabilityResult {
            title: "Test Article".to_string(),
            lead_image: Some("https://example.com/img.png".to_string()),
            content_blocks: vec![
                ContentBlock::Heading {
                    level: 1,
                    text: "Main Title".to_string(),
                },
                ContentBlock::Paragraph {
                    text: "Some body text.".to_string(),
                },
            ],
            tables: vec![],
            byline: Some("Author Name".to_string()),
            site_name: Some("Example Site".to_string()),
            language: Some("en".to_string()),
        };

        let json = serde_json::to_string_pretty(&result).unwrap();
        let decoded: ReadabilityResult = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded, result);
    }

    #[test]
    fn content_block_variants_roundtrip() {
        let blocks = vec![
            ContentBlock::Paragraph {
                text: "para".to_string(),
            },
            ContentBlock::Heading {
                level: 2,
                text: "heading".to_string(),
            },
            ContentBlock::Image {
                src: "img.png".to_string(),
                alt: Some("alt".to_string()),
            },
            ContentBlock::CodeBlock {
                language: Some("rust".to_string()),
                code: "fn main() {}".to_string(),
            },
            ContentBlock::List {
                ordered: true,
                items: vec!["a".to_string(), "b".to_string()],
            },
            ContentBlock::BlockQuote {
                text: "quoted".to_string(),
            },
        ];

        let json = serde_json::to_string_pretty(&blocks).unwrap();
        let decoded: Vec<ContentBlock> = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded, blocks);
    }

    #[test]
    fn extracted_table_roundtrips() {
        let table = ExtractedTable {
            headers: vec!["Name".to_string(), "Age".to_string()],
            rows: vec![
                vec!["Alice".to_string(), "30".to_string()],
                vec!["Bob".to_string(), "25".to_string()],
            ],
        };

        let json = serde_json::to_string_pretty(&table).unwrap();
        let decoded: ExtractedTable = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded, table);
    }

    #[test]
    fn extract_from_article_html() {
        let html = r#"<html lang="en">
            <head>
                <title>Test Article</title>
                <meta property="og:title" content="The Real Title">
                <meta property="og:image" content="https://example.com/hero.jpg">
            </head>
            <body>
                <article>
                    <h1>The Real Title</h1>
                    <p>First paragraph of the article.</p>
                    <h2>Section One</h2>
                    <p>Second paragraph with <b>bold</b> text.</p>
                    <img src="photo.jpg" alt="A photo">
                </article>
                <footer>Footer content</footer>
            </body>
        </html>"#;

        let result = ReadabilityExtractor::extract(html, "https://example.com/article");

        assert_eq!(result.title, "The Real Title");
        assert_eq!(
            result.lead_image,
            Some("https://example.com/hero.jpg".to_string())
        );
        assert_eq!(result.language, Some("en".to_string()));

        // Should have heading + paragraphs + image from article
        let has_h1 = result.content_blocks.iter().any(
            |b| matches!(b, ContentBlock::Heading { level: 1, text } if text == "The Real Title"),
        );
        let has_para1 = result.content_blocks.iter().any(
            |b| matches!(b, ContentBlock::Paragraph { text } if text.contains("First paragraph")),
        );
        let has_para2 = result.content_blocks.iter().any(
            |b| matches!(b, ContentBlock::Paragraph { text } if text.contains("Second paragraph")),
        );
        let has_img = result.content_blocks.iter().any(|b| matches!(b, ContentBlock::Image { src, alt } if src == "photo.jpg" && alt.as_deref() == Some("A photo")));

        assert!(has_h1, "should extract h1");
        assert!(has_para1, "should extract first paragraph");
        assert!(has_para2, "should extract second paragraph");
        assert!(has_img, "should extract image");

        // Footer should NOT be in content blocks (article takes priority)
        let has_footer = result.content_blocks.iter().any(|b| match b {
            ContentBlock::Paragraph { text } => text.contains("Footer"),
            _ => false,
        });
        assert!(!has_footer, "footer should not be in article content");
    }

    #[test]
    fn extract_from_blog_html() {
        let html = r#"<html>
            <head><title>My Blog Post</title></head>
            <body>
                <main>
                    <h1>Blog Title</h1>
                    <p>Introduction paragraph.</p>
                    <ul>
                        <li>Item one</li>
                        <li>Item two</li>
                    </ul>
                    <blockquote>A wise quote.</blockquote>
                </main>
                <nav>Sidebar navigation</nav>
            </body>
        </html>"#;

        let result = ReadabilityExtractor::extract(html, "https://blog.example.com/post");

        assert_eq!(result.title, "My Blog Post");

        let has_list = result
            .content_blocks
            .iter()
            .any(|b| matches!(b, ContentBlock::List { ordered: false, items } if items.len() == 2));
        let has_quote = result
            .content_blocks
            .iter()
            .any(|b| matches!(b, ContentBlock::BlockQuote { text } if text == "A wise quote."));

        assert!(has_list, "should extract unordered list");
        assert!(has_quote, "should extract blockquote");
    }

    #[test]
    fn extract_fallback_to_plain_text() {
        // HTML with no article/main/body structure worth extracting
        let html = r#"<html><head><title>Empty</title></head><body><div>Just some text</div></body></html>"#;

        let result = ReadabilityExtractor::extract(html, "");

        assert_eq!(result.title, "Empty");
        // Should still produce content blocks from body
        // div content is not directly extracted as p/h, so content_blocks may be empty
        // but to_plain_text should still work
        let plain = result.to_plain_text();
        assert!(plain.contains("Empty"));
    }

    #[test]
    fn extract_handles_empty_html() {
        let result = ReadabilityExtractor::extract("", "");

        assert!(result.title.is_empty());
        assert!(result.content_blocks.is_empty());
        assert!(result.tables.is_empty());
        assert!(result.to_plain_text().is_empty());
    }

    #[test]
    fn extract_preserves_image_urls() {
        let html = r#"<html><head><title>T</title></head>
            <body>
                <article>
                    <p>Text</p>
                    <img src="https://cdn.example.com/photo.jpg" alt="Photo">
                    <img src="data:image/png;base64,abc" alt="inline">
                </article>
            </body>
        </html>"#;

        let result = ReadabilityExtractor::extract(html, "");

        let real_img = result.content_blocks.iter().find(
            |b| matches!(b, ContentBlock::Image { src, .. } if src.contains("cdn.example.com")),
        );
        assert!(real_img.is_some(), "should extract real image URL");

        let data_img = result
            .content_blocks
            .iter()
            .find(|b| matches!(b, ContentBlock::Image { src, .. } if src.starts_with("data:")));
        assert!(data_img.is_none(), "should skip data: URIs");
    }

    #[test]
    fn extract_preserves_table_structure() {
        let html = r#"<html><head><title>T</title></head>
            <body>
                <article>
                    <table>
                        <thead><tr><th>Name</th><th>Score</th></tr></thead>
                        <tbody>
                            <tr><td>Alice</td><td>95</td></tr>
                            <tr><td>Bob</td><td>87</td></tr>
                        </tbody>
                    </table>
                </article>
            </body>
        </html>"#;

        let result = ReadabilityExtractor::extract(html, "");

        assert_eq!(result.tables.len(), 1, "should extract one table");
        let table = &result.tables[0];
        assert_eq!(table.headers, vec!["Name", "Score"]);
        assert_eq!(table.rows.len(), 2);
        assert_eq!(table.rows[0], vec!["Alice", "95"]);
        assert_eq!(table.rows[1], vec!["Bob", "87"]);
    }

    #[test]
    fn extract_table_in_plain_text() {
        let table = ExtractedTable {
            headers: vec!["A".to_string(), "B".to_string()],
            rows: vec![vec!["1".to_string(), "2".to_string()]],
        };
        let result = ReadabilityResult {
            title: "T".to_string(),
            lead_image: None,
            content_blocks: vec![],
            tables: vec![table],
            byline: None,
            site_name: None,
            language: None,
        };

        let plain = result.to_plain_text();
        assert!(plain.contains("A | B"));
        assert!(plain.contains("1 | 2"));
    }

    #[test]
    fn static_html_executor_uses_readability_text() {
        // Verify that StaticHtmlBrowserExecutor's parse_html uses ReadabilityExtractor
        let html = r#"<html><head><title>Test</title></head>
            <body>
                <article>
                    <h1>Title</h1>
                    <p>Article content.</p>
                </article>
                <nav>Navigation noise</nav>
            </body>
        </html>"#;

        let parsed = parse_html(html);

        assert_eq!(parsed.title, "Test");
        assert!(
            parsed.text.contains("Title"),
            "should contain article heading"
        );
        assert!(
            parsed.text.contains("Article content"),
            "should contain article paragraph"
        );
        assert!(
            !parsed.text.contains("Navigation noise"),
            "should not contain nav content"
        );
    }

    // ---- Dual-Path Browser Router tests (PR 55) ----

    #[test]
    fn page_intent_default_is_auto() {
        let intent = PageIntent::default();
        assert_eq!(intent, PageIntent::Auto);
    }

    #[test]
    fn page_intent_serializes_correctly() {
        let cases = vec![
            (PageIntent::Informational, "\"informational\""),
            (PageIntent::Interactive, "\"interactive\""),
            (PageIntent::Auto, "\"auto\""),
        ];
        for (intent, expected) in cases {
            let json = serde_json::to_string(&intent).unwrap();
            assert_eq!(json, expected);
            let decoded: PageIntent = serde_json::from_str(&json).unwrap();
            assert_eq!(decoded, intent);
        }
    }

    #[test]
    fn browser_route_decision_serializes() {
        let cases = vec![
            BrowserRouteDecision::StaticHtml,
            BrowserRouteDecision::CdpBrowser,
        ];
        for decision in cases {
            let json = serde_json::to_string(&decision).unwrap();
            let decoded: BrowserRouteDecision = serde_json::from_str(&json).unwrap();
            assert_eq!(decoded, decision);
        }
    }

    #[test]
    fn router_informational_always_static() {
        let urls = vec![
            "https://example.com",
            "https://twitter.com/home",
            "https://gmail.com",
        ];
        for url in urls {
            let decision = BrowserRouter::route(url, &PageIntent::Informational);
            assert_eq!(
                decision,
                BrowserRouteDecision::StaticHtml,
                "Informational should always route to StaticHtml for {}",
                url
            );
        }
    }

    #[test]
    fn router_interactive_always_cdp() {
        let urls = vec![
            "https://example.com",
            "https://wikipedia.org/wiki/Rust",
            "https://docs.rs/some-crate",
        ];
        for url in urls {
            let decision = BrowserRouter::route(url, &PageIntent::Interactive);
            assert_eq!(
                decision,
                BrowserRouteDecision::CdpBrowser,
                "Interactive should always route to CdpBrowser for {}",
                url
            );
        }
    }

    #[test]
    fn router_auto_spa_sites_route_to_cdp() {
        let spa_urls = vec![
            "https://twitter.com/home",
            "https://gmail.com/inbox",
            "https://app.example.com/dashboard",
            "https://notion.so/my-page",
            "https://figma.com/design/abc",
            "https://example.com/login",
            "https://example.com/auth/callback",
        ];
        for url in spa_urls {
            let decision = BrowserRouter::route(url, &PageIntent::Auto);
            assert_eq!(
                decision,
                BrowserRouteDecision::CdpBrowser,
                "SPA site should route to CdpBrowser: {}",
                url
            );
        }
    }

    #[test]
    fn router_auto_content_sites_route_to_static() {
        let static_urls = vec![
            "https://en.wikipedia.org/wiki/Rust_(programming_language)",
            "https://docs.rs/tokio/latest/tokio",
            "https://medium.com/@user/my-article",
            "https://arxiv.org/abs/2301.00001",
            "https://stackoverflow.com/questions/12345",
            "https://example.com/blog/my-post",
            "https://example.com/article/tech-news",
            "https://news.ycombinator.com/item?id=12345",
            "https://reddit.com/r/rust/comments/abc",
        ];
        for url in static_urls {
            let decision = BrowserRouter::route(url, &PageIntent::Auto);
            assert_eq!(
                decision,
                BrowserRouteDecision::StaticHtml,
                "Content site should route to StaticHtml: {}",
                url
            );
        }
    }

    #[test]
    fn router_auto_unknown_url_defaults_to_static() {
        let decision = BrowserRouter::route("https://some-random-blog.com/post", &PageIntent::Auto);
        assert_eq!(decision, BrowserRouteDecision::StaticHtml);
    }
}
