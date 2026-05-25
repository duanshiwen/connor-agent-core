//! # Browser Kernel Core
//!
//! Browser kernel domain types and CDP executor skeleton for AgentOS.
//!
//! This crate intentionally does not launch Chromium yet. It defines the stable
//! boundary that later PRs can connect to a real CDP implementation.

use artifact_core::ArtifactId;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Configuration types
// ---------------------------------------------------------------------------

/// Browser viewport configuration used by CDP sessions.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BrowserViewport {
    pub width: u32,
    pub height: u32,
    pub device_scale_factor: f64,
}

impl Default for BrowserViewport {
    fn default() -> Self {
        Self {
            width: 1280,
            height: 720,
            device_scale_factor: 1.0,
        }
    }
}

/// Timeout configuration for browser operations.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BrowserTimeouts {
    pub navigation_timeout_ms: u64,
    pub action_timeout_ms: u64,
    pub idle_shutdown_ms: u64,
}

impl Default for BrowserTimeouts {
    fn default() -> Self {
        Self {
            navigation_timeout_ms: 30_000,
            action_timeout_ms: 10_000,
            idle_shutdown_ms: 300_000,
        }
    }
}

/// How Chromium should be launched or connected.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChromiumLaunchMode {
    Headless,
    Headful,
    Connect { websocket_url: String },
}

/// Configuration for a CDP-backed browser session.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CdpBrowserConfig {
    pub launch_mode: ChromiumLaunchMode,
    pub viewport: BrowserViewport,
    pub timeouts: BrowserTimeouts,
    pub user_data_dir: Option<String>,
    pub max_pages: usize,
}

impl Default for CdpBrowserConfig {
    fn default() -> Self {
        Self {
            launch_mode: ChromiumLaunchMode::Headless,
            viewport: BrowserViewport::default(),
            timeouts: BrowserTimeouts::default(),
            user_data_dir: None,
            max_pages: 5,
        }
    }
}

impl CdpBrowserConfig {
    pub fn with_launch_mode(mut self, launch_mode: ChromiumLaunchMode) -> Self {
        self.launch_mode = launch_mode;
        self
    }

    pub fn with_viewport(mut self, viewport: BrowserViewport) -> Self {
        self.viewport = viewport;
        self
    }

    pub fn with_user_data_dir(mut self, user_data_dir: impl Into<String>) -> Self {
        self.user_data_dir = Some(user_data_dir.into());
        self
    }

    pub fn with_max_pages(mut self, max_pages: usize) -> Self {
        self.max_pages = max_pages;
        self
    }
}

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

/// Errors produced by browser-kernel-core validation and skeleton runtime paths.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum BrowserKernelError {
    #[error("element selector cannot be empty")]
    EmptySelector,
    #[error("form fill spec must contain at least one field")]
    EmptyForm,
    #[error("invalid browser config: {0}")]
    InvalidConfig(String),
    #[error("chromium browser is not available in skeleton executor")]
    ChromiumNotAvailable,
    #[error("unsupported browser action: {0}")]
    UnsupportedAction(String),
}

// ---------------------------------------------------------------------------
// Element selector
// ---------------------------------------------------------------------------

/// Selector for locating an element on an interactive page.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind", content = "value")]
pub enum ElementSelector {
    Css(String),
    Text(String),
    XPath(String),
}

impl ElementSelector {
    pub fn css(selector: impl Into<String>) -> Self {
        Self::Css(selector.into())
    }

    pub fn text(text: impl Into<String>) -> Self {
        Self::Text(text.into())
    }

    pub fn xpath(xpath: impl Into<String>) -> Self {
        Self::XPath(xpath.into())
    }

    pub fn validate(&self) -> Result<(), BrowserKernelError> {
        let value = match self {
            Self::Css(value) | Self::Text(value) | Self::XPath(value) => value,
        };

        if value.trim().is_empty() {
            Err(BrowserKernelError::EmptySelector)
        } else {
            Ok(())
        }
    }
}

// ---------------------------------------------------------------------------
// Form fill spec
// ---------------------------------------------------------------------------

/// One form field to fill on an interactive page.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FormFillField {
    pub selector: ElementSelector,
    pub value: String,
}

/// A batch form fill request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FormFillSpec {
    pub fields: Vec<FormFillField>,
    pub submit: bool,
}

impl FormFillSpec {
    pub fn new(fields: Vec<FormFillField>) -> Self {
        Self {
            fields,
            submit: false,
        }
    }

    pub fn with_submit(mut self, submit: bool) -> Self {
        self.submit = submit;
        self
    }

    pub fn validate(&self) -> Result<(), BrowserKernelError> {
        if self.fields.is_empty() {
            return Err(BrowserKernelError::EmptyForm);
        }

        for field in &self.fields {
            field.selector.validate()?;
        }

        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Interactive snapshot
// ---------------------------------------------------------------------------

/// The kind of interactive element detected on a page.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InteractiveElementKind {
    Link,
    Button,
    Input,
    Select,
    TextArea,
    Checkbox,
    Radio,
    Other,
}

/// Bounding box of an element on the rendered page.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ElementBoundingBox {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

/// A single interactive element detected on a rendered page.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct InteractiveElement {
    pub id: String,
    pub kind: InteractiveElementKind,
    pub selector: Option<ElementSelector>,
    pub text: Option<String>,
    pub aria_label: Option<String>,
    pub bounding_box: Option<ElementBoundingBox>,
}

/// A full interactive snapshot of a rendered page including element index.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct InteractiveSnapshot {
    pub url: String,
    pub title: String,
    pub elements: Vec<InteractiveElement>,
    pub screenshot_artifact_id: Option<ArtifactId>,
    pub captured_at: DateTime<Utc>,
}

/// Result of a browser action execution.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BrowserActionResult {
    pub success: bool,
    pub message: String,
    pub navigation_changed: bool,
    pub snapshot: Option<InteractiveSnapshot>,
    pub artifact_id: Option<ArtifactId>,
}

// ---------------------------------------------------------------------------
// Chromium lifecycle manager skeleton
// ---------------------------------------------------------------------------

/// Skeleton manager for Chromium process lifecycle.
///
/// In this skeleton, Chromium is never actually launched.
/// The manager records the intended config and reports unavailability.
#[derive(Debug, Clone)]
pub struct ChromiumLifecycleManager {
    config: CdpBrowserConfig,
}

impl ChromiumLifecycleManager {
    pub fn new(config: CdpBrowserConfig) -> Self {
        Self { config }
    }

    pub fn config(&self) -> &CdpBrowserConfig {
        &self.config
    }

    /// Returns `true` when a real Chromium instance is available.
    /// In the skeleton implementation this always returns `false`.
    pub fn is_available(&self) -> bool {
        false
    }
}

// ---------------------------------------------------------------------------
// CDP browser executor skeleton
// ---------------------------------------------------------------------------

/// Skeleton CDP browser executor.
///
/// This executor implements [`ActionExecutor`] but always returns
/// [`ActionExecutorError::ExecutionFailed`] for known browser actions
/// because no real Chromium process is connected. Unknown action kinds
/// return [`ActionExecutorError::NotSupported`].
#[derive(Debug)]
pub struct CdpBrowserExecutor {
    lifecycle: ChromiumLifecycleManager,
}

impl CdpBrowserExecutor {
    pub fn new(lifecycle: ChromiumLifecycleManager, _now: DateTime<Utc>) -> Self {
        Self { lifecycle }
    }

    pub fn lifecycle(&self) -> &ChromiumLifecycleManager {
        &self.lifecycle
    }
}

const KNOWN_BROWSER_ACTIONS: &[&str] = &[
    "browser.open_url",
    "browser.extract_content",
    "browser.capture_snapshot",
    "browser.summarize_page",
    "browser.compare_pages",
    "browser.click_element",
    "browser.type_text",
    "browser.fill_form",
    "browser.scroll_page",
    "browser.select_option",
    "browser.wait_for_element",
    "browser.press_key",
    "browser.execute_js",
    "browser.get_page_screenshot",
    "browser.get_interactive_snapshot",
];

#[async_trait::async_trait]
impl action_core::ActionExecutor for CdpBrowserExecutor {
    async fn execute(
        &self,
        request: &action_core::ActionRequest,
    ) -> Result<action_core::ActionResult, action_core::ActionExecutorError> {
        let kind = request.action_kind.0.as_str();

        if KNOWN_BROWSER_ACTIONS.contains(&kind) {
            Err(action_core::ActionExecutorError::ExecutionFailed(
                BrowserKernelError::ChromiumNotAvailable.to_string(),
            ))
        } else {
            Err(action_core::ActionExecutorError::NotSupported(
                request.action_kind.clone(),
            ))
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use action_core::{ActionExecutor, ActionExecutorError, ActionId, ActionKind, ActionRequest};

    fn ts() -> DateTime<Utc> {
        "2026-05-25T12:00:00Z".parse().unwrap()
    }

    fn action_request(kind_str: &str) -> ActionRequest {
        ActionRequest {
            action_id: ActionId::from("action-test-1"),
            action_kind: ActionKind::from(kind_str),
            input: serde_json::json!({}),
            requested_by: "user-1".to_string(),
            conversation_id: Some("conv-1".to_string()),
            message_id: Some("msg-1".to_string()),
            requested_at: ts(),
        }
    }

    // ---- Config tests ----

    #[test]
    fn cdp_browser_config_defaults_are_stable() {
        let config = CdpBrowserConfig::default();

        assert_eq!(config.launch_mode, ChromiumLaunchMode::Headless);
        assert_eq!(config.viewport.width, 1280);
        assert_eq!(config.viewport.height, 720);
        assert_eq!(config.viewport.device_scale_factor, 1.0);
        assert_eq!(config.timeouts.navigation_timeout_ms, 30_000);
        assert_eq!(config.timeouts.action_timeout_ms, 10_000);
        assert_eq!(config.timeouts.idle_shutdown_ms, 300_000);
        assert_eq!(config.user_data_dir, None);
        assert_eq!(config.max_pages, 5);
    }

    #[test]
    fn cdp_browser_config_builder_sets_viewport() {
        let config = CdpBrowserConfig::default()
            .with_viewport(BrowserViewport {
                width: 1920,
                height: 1080,
                device_scale_factor: 2.0,
            })
            .with_max_pages(3)
            .with_user_data_dir("/tmp/agentos-browser-profile");

        assert_eq!(config.viewport.width, 1920);
        assert_eq!(config.viewport.height, 1080);
        assert_eq!(config.viewport.device_scale_factor, 2.0);
        assert_eq!(config.max_pages, 3);
        assert_eq!(
            config.user_data_dir,
            Some("/tmp/agentos-browser-profile".to_string())
        );
    }

    #[test]
    fn cdp_browser_config_connect_mode_roundtrips() {
        let config = CdpBrowserConfig::default().with_launch_mode(ChromiumLaunchMode::Connect {
            websocket_url: "ws://localhost:9222/devtools/browser/test".to_string(),
        });

        let json = serde_json::to_string_pretty(&config).unwrap();
        let decoded: CdpBrowserConfig = serde_json::from_str(&json).unwrap();

        assert_eq!(decoded, config);
    }

    // ---- Selector tests ----

    #[test]
    fn element_selector_css_roundtrips() {
        let selector = ElementSelector::css("button.primary");

        let json = serde_json::to_string_pretty(&selector).unwrap();
        let decoded: ElementSelector = serde_json::from_str(&json).unwrap();

        assert_eq!(decoded, selector);
        assert!(selector.validate().is_ok());
    }

    #[test]
    fn element_selector_text_roundtrips() {
        let selector = ElementSelector::text("Submit");

        let json = serde_json::to_string_pretty(&selector).unwrap();
        let decoded: ElementSelector = serde_json::from_str(&json).unwrap();

        assert_eq!(decoded, selector);
        assert!(selector.validate().is_ok());
    }

    #[test]
    fn element_selector_xpath_roundtrips() {
        let selector = ElementSelector::xpath("//button[@type='submit']");

        let json = serde_json::to_string_pretty(&selector).unwrap();
        let decoded: ElementSelector = serde_json::from_str(&json).unwrap();

        assert_eq!(decoded, selector);
        assert!(selector.validate().is_ok());
    }

    #[test]
    fn element_selector_rejects_empty_css() {
        let selector = ElementSelector::css("   ");

        assert!(matches!(
            selector.validate(),
            Err(BrowserKernelError::EmptySelector)
        ));
    }

    // ---- Form fill tests ----

    #[test]
    fn form_fill_spec_roundtrips() {
        let spec = FormFillSpec::new(vec![FormFillField {
            selector: ElementSelector::css("#email"),
            value: "user@example.com".to_string(),
        }])
        .with_submit(true);

        let json = serde_json::to_string_pretty(&spec).unwrap();
        let decoded: FormFillSpec = serde_json::from_str(&json).unwrap();

        assert_eq!(decoded, spec);
        assert!(spec.validate().is_ok());
    }

    #[test]
    fn form_fill_spec_rejects_empty_fields() {
        let spec = FormFillSpec::new(vec![]);

        assert!(matches!(
            spec.validate(),
            Err(BrowserKernelError::EmptyForm)
        ));
    }

    #[test]
    fn form_fill_spec_rejects_invalid_selector() {
        let spec = FormFillSpec::new(vec![FormFillField {
            selector: ElementSelector::text(" "),
            value: "value".to_string(),
        }]);

        assert!(matches!(
            spec.validate(),
            Err(BrowserKernelError::EmptySelector)
        ));
    }

    // ---- Snapshot tests ----

    #[test]
    fn interactive_element_roundtrips() {
        let element = InteractiveElement {
            id: "btn-1".to_string(),
            kind: InteractiveElementKind::Button,
            selector: Some(ElementSelector::css("button#btn-1")),
            text: Some("Submit".to_string()),
            aria_label: Some("Submit form".to_string()),
            bounding_box: Some(ElementBoundingBox {
                x: 100.0,
                y: 200.0,
                width: 120.0,
                height: 40.0,
            }),
        };

        let json = serde_json::to_string_pretty(&element).unwrap();
        let decoded: InteractiveElement = serde_json::from_str(&json).unwrap();

        assert_eq!(decoded, element);
    }

    #[test]
    fn interactive_snapshot_roundtrips() {
        let snapshot = InteractiveSnapshot {
            url: "https://example.com".to_string(),
            title: "Example".to_string(),
            elements: vec![InteractiveElement {
                id: "link-1".to_string(),
                kind: InteractiveElementKind::Link,
                selector: Some(ElementSelector::css("a.link-1")),
                text: Some("Click me".to_string()),
                aria_label: None,
                bounding_box: None,
            }],
            screenshot_artifact_id: Some(ArtifactId::from("art-1")),
            captured_at: ts(),
        };

        let json = serde_json::to_string_pretty(&snapshot).unwrap();
        let decoded: InteractiveSnapshot = serde_json::from_str(&json).unwrap();

        assert_eq!(decoded, snapshot);
    }

    #[test]
    fn browser_action_result_roundtrips() {
        let result = BrowserActionResult {
            success: true,
            message: "Element clicked".to_string(),
            navigation_changed: false,
            snapshot: None,
            artifact_id: Some(ArtifactId::from("art-2")),
        };

        let json = serde_json::to_string_pretty(&result).unwrap();
        let decoded: BrowserActionResult = serde_json::from_str(&json).unwrap();

        assert_eq!(decoded, result);
    }

    // ---- Lifecycle manager tests ----

    #[test]
    fn chromium_lifecycle_manager_keeps_config() {
        let config = CdpBrowserConfig::default().with_max_pages(10);
        let manager = ChromiumLifecycleManager::new(config.clone());

        assert_eq!(*manager.config(), config);
    }

    #[test]
    fn chromium_lifecycle_manager_reports_unavailable_in_skeleton() {
        let manager = ChromiumLifecycleManager::new(CdpBrowserConfig::default());

        assert!(!manager.is_available());
    }

    // ---- Executor tests ----

    #[tokio::test]
    async fn cdp_browser_executor_rejects_unknown_action_kind() {
        let lifecycle = ChromiumLifecycleManager::new(CdpBrowserConfig::default());
        let executor = CdpBrowserExecutor::new(lifecycle, ts());

        let request = action_request("system.shutdown");

        let result = executor.execute(&request).await;
        assert!(result.is_err());
        match result.unwrap_err() {
            ActionExecutorError::NotSupported(kind) => {
                assert_eq!(kind.0, "system.shutdown");
            }
            other => panic!("expected NotSupported, got: {other:?}"),
        }
    }

    #[tokio::test]
    async fn cdp_browser_executor_returns_chromium_not_available_for_known_action() {
        let lifecycle = ChromiumLifecycleManager::new(CdpBrowserConfig::default());
        let executor = CdpBrowserExecutor::new(lifecycle, ts());

        let request = action_request("browser.open_url");

        let result = executor.execute(&request).await;
        assert!(result.is_err());
        match result.unwrap_err() {
            ActionExecutorError::ExecutionFailed(msg) => {
                assert!(msg.contains("chromium browser is not available"));
            }
            other => panic!("expected ExecutionFailed, got: {other:?}"),
        }
    }
}
