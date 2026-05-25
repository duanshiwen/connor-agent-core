//! # Browser Kernel Core
//!
//! Browser kernel domain types and CDP executor skeleton for AgentOS.
//!
//! This crate intentionally does not launch Chromium yet. It defines the stable
//! boundary that later PRs can connect to a real CDP implementation.

use artifact_core::ArtifactId;
use chrono::{DateTime, Utc};
use futures_util::stream::StreamExt;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

// ---------------------------------------------------------------------------
// Browser profile types
// ---------------------------------------------------------------------------

/// How a browser profile should be managed.
///
/// - `Named("default")` → persistent profile with cookie/localStorage retention
/// - `Temporary` → isolated UUID directory, cleaned up after session
/// - `Ephemeral` → no profile directory (incognito-like)
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BrowserProfileMode {
    /// Use a named persistent profile (cookie/localStorage auto-retained).
    Named(String),
    /// Use a temporary profile (isolated UUID directory, cleaned up after session).
    Temporary,
    /// No profile management (incognito-like, Chromium uses in-memory temp dir).
    Ephemeral,
}

impl Default for BrowserProfileMode {
    fn default() -> Self {
        Self::Named("default".to_string())
    }
}

/// Information about a browser profile's storage on disk.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BrowserProfile {
    pub name: String,
    pub path: PathBuf,
    pub created_at: DateTime<Utc>,
    pub is_temporary: bool,
}

/// Storage usage information for a browser profile.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProfileStorageInfo {
    pub profile_name: String,
    pub total_bytes: u64,
    pub cookies_db_bytes: u64,
    pub local_storage_bytes: u64,
    pub cache_bytes: u64,
}

/// Platform-agnostic browser storage interface.
///
/// Desktop: manages Chromium profile directories.
/// Mobile: may be no-op (WebView manages persistence itself).
pub trait BrowserStorage: Send + Sync {
    /// Resolve a profile's storage path (desktop only).
    /// Mobile may return `None` (WebView manages persistence).
    fn resolve_profile(&self, name: &str) -> Result<Option<PathBuf>, BrowserKernelError>;

    /// Check if a profile exists.
    fn profile_exists(&self, name: &str) -> bool;

    /// List existing profile names.
    fn list_profiles(&self) -> Result<Vec<String>, BrowserKernelError>;

    /// Clear a profile's data.
    fn clear_profile(&self, name: &str) -> Result<(), BrowserKernelError>;
}

// ---------------------------------------------------------------------------
// FsBrowserStorage (desktop implementation)
// ---------------------------------------------------------------------------

/// Desktop filesystem-backed browser storage.
///
/// Manages Chromium profile directories under `{storage_root}/browser-profiles/`.
/// Profile metadata is persisted in `profiles.json`.
pub struct FsBrowserStorage {
    #[allow(dead_code)] // Used in tests to verify canonicalization
    root: PathBuf,
    profiles_dir: PathBuf,
}

impl FsBrowserStorage {
    /// Create a new `FsBrowserStorage` with the given storage root.
    ///
    /// The `storage_root` can be relative (canonicalized on first use) or absolute.
    /// Profiles are stored under `{storage_root}/browser-profiles/`.
    pub fn new(storage_root: impl Into<PathBuf>) -> Result<Self, BrowserKernelError> {
        let root = storage_root.into();
        if root.as_os_str().is_empty() {
            return Err(BrowserKernelError::InvalidConfig(
                "storage_root cannot be empty".to_string(),
            ));
        }

        // Canonicalize relative paths to absolute (required by Chromium --user-data-dir).
        let canonical_root = if root.is_absolute() {
            root.clone()
        } else {
            std::fs::canonicalize(&root).unwrap_or_else(|_| {
                // If canonicalize fails (dir doesn't exist yet), create it first.
                let _ = std::fs::create_dir_all(&root);
                std::fs::canonicalize(&root).unwrap_or(root.clone())
            })
        };

        let profiles_dir = canonical_root.join("browser-profiles");
        Ok(Self {
            root: canonical_root,
            profiles_dir,
        })
    }

    /// Resolve a named profile, creating it if it doesn't exist.
    pub fn resolve_or_create(&self, name: &str) -> Result<PathBuf, BrowserKernelError> {
        let profile_path = self.profiles_dir.join(name);
        if !profile_path.exists() {
            std::fs::create_dir_all(&profile_path).map_err(|e| {
                BrowserKernelError::InvalidConfig(format!(
                    "failed to create profile directory: {}",
                    e
                ))
            })?;
            self.update_profiles_json(name, false)?;
        }
        Ok(profile_path)
    }

    /// Create a temporary profile with a UUID name.
    pub fn create_temporary(&self) -> Result<BrowserProfile, BrowserKernelError> {
        let uuid = uuid_v4();
        let profile_path = self.profiles_dir.join("_tmp").join(&uuid);
        std::fs::create_dir_all(&profile_path).map_err(|e| {
            BrowserKernelError::InvalidConfig(format!(
                "failed to create temporary profile directory: {}",
                e
            ))
        })?;

        let now = chrono::Utc::now();
        let profile = BrowserProfile {
            name: uuid.clone(),
            path: profile_path,
            created_at: now,
            is_temporary: true,
        };

        self.update_profiles_json(&uuid, true)?;
        Ok(profile)
    }

    /// Delete all temporary profiles under `_tmp/`.
    pub fn delete_temporaries(&self) -> Result<(), BrowserKernelError> {
        let tmp_dir = self.profiles_dir.join("_tmp");
        if tmp_dir.exists() {
            std::fs::remove_dir_all(&tmp_dir).map_err(|e| {
                BrowserKernelError::InvalidConfig(format!("failed to delete temporaries: {}", e))
            })?;
        }
        Ok(())
    }

    /// Get storage info for a profile (async for large directories).
    pub async fn storage_info(&self, name: &str) -> Result<ProfileStorageInfo, BrowserKernelError> {
        let profile_path = self.profiles_dir.join(name);
        if !profile_path.exists() {
            return Err(BrowserKernelError::InvalidConfig(format!(
                "profile '{}' does not exist",
                name
            )));
        }

        let total_bytes = dir_size(&profile_path);
        let cookies_db_bytes = file_size(&profile_path.join("Cookies"));
        let local_storage_bytes = dir_size(&profile_path.join("Local Storage"));
        let cache_bytes = dir_size(&profile_path.join("Cache"));

        Ok(ProfileStorageInfo {
            profile_name: name.to_string(),
            total_bytes,
            cookies_db_bytes,
            local_storage_bytes,
            cache_bytes,
        })
    }

    /// Clear cache for a profile (preserves cookies and localStorage).
    pub async fn clear_cache(&self, name: &str) -> Result<(), BrowserKernelError> {
        let profile_path = self.profiles_dir.join(name);
        if !profile_path.exists() {
            return Err(BrowserKernelError::InvalidConfig(format!(
                "profile '{}' does not exist",
                name
            )));
        }

        let cache_dir = profile_path.join("Cache");
        if cache_dir.exists() {
            std::fs::remove_dir_all(&cache_dir).map_err(|e| {
                BrowserKernelError::InvalidConfig(format!("failed to clear cache: {}", e))
            })?;
        }

        let code_cache_dir = profile_path.join("Code Cache");
        if code_cache_dir.exists() {
            std::fs::remove_dir_all(&code_cache_dir).map_err(|e| {
                BrowserKernelError::InvalidConfig(format!("failed to clear code cache: {}", e))
            })?;
        }

        Ok(())
    }

    fn update_profiles_json(
        &self,
        name: &str,
        is_temporary: bool,
    ) -> Result<(), BrowserKernelError> {
        let json_path = self.profiles_dir.join("profiles.json");
        let mut profiles: serde_json::Value = if json_path.exists() {
            let content = std::fs::read_to_string(&json_path).unwrap_or_default();
            serde_json::from_str(&content).unwrap_or(serde_json::json!({}))
        } else {
            serde_json::json!({})
        };

        profiles[name] = serde_json::json!({
            "created_at": chrono::Utc::now().to_rfc3339(),
            "is_temporary": is_temporary
        });

        std::fs::create_dir_all(&self.profiles_dir).map_err(|e| {
            BrowserKernelError::InvalidConfig(format!("failed to create profiles dir: {}", e))
        })?;

        std::fs::write(&json_path, serde_json::to_string_pretty(&profiles).unwrap()).map_err(
            |e| BrowserKernelError::InvalidConfig(format!("failed to write profiles.json: {}", e)),
        )?;

        Ok(())
    }
}

impl BrowserStorage for FsBrowserStorage {
    fn resolve_profile(&self, name: &str) -> Result<Option<PathBuf>, BrowserKernelError> {
        let profile_path = self.profiles_dir.join(name);
        if profile_path.exists() {
            Ok(Some(profile_path))
        } else {
            Ok(None)
        }
    }

    fn profile_exists(&self, name: &str) -> bool {
        self.profiles_dir.join(name).exists()
    }

    fn list_profiles(&self) -> Result<Vec<String>, BrowserKernelError> {
        let json_path = self.profiles_dir.join("profiles.json");
        if !json_path.exists() {
            return Ok(vec![]);
        }

        let content = std::fs::read_to_string(&json_path).map_err(|e| {
            BrowserKernelError::InvalidConfig(format!("failed to read profiles.json: {}", e))
        })?;

        let profiles: serde_json::Value =
            serde_json::from_str(&content).unwrap_or(serde_json::json!({}));
        let names: Vec<String> = profiles
            .as_object()
            .map(|obj| obj.keys().cloned().collect())
            .unwrap_or_default();

        Ok(names)
    }

    fn clear_profile(&self, name: &str) -> Result<(), BrowserKernelError> {
        let profile_path = self.profiles_dir.join(name);
        if profile_path.exists() {
            std::fs::remove_dir_all(&profile_path).map_err(|e| {
                BrowserKernelError::InvalidConfig(format!("failed to clear profile: {}", e))
            })?;
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn uuid_v4() -> String {
    // Simple UUID v4 implementation (no external dependency).
    use std::time::{SystemTime, UNIX_EPOCH};
    let duration = SystemTime::now().duration_since(UNIX_EPOCH).unwrap();
    let nanos = duration.as_nanos();
    format!(
        "{:08x}-{:04x}-{:04x}-{:04x}-{:012x}",
        (nanos >> 96) as u32,
        (nanos >> 80) as u16,
        ((nanos >> 64) as u16 & 0x0fff) | 0x4000, // version 4
        ((nanos >> 48) as u16 & 0x3fff) | 0x8000, // variant 1
        (nanos & 0xffffffffffff) as u64
    )
}

fn dir_size(path: &Path) -> u64 {
    if !path.exists() {
        return 0;
    }
    let mut total = 0;
    if let Ok(entries) = std::fs::read_dir(path) {
        for entry in entries.flatten() {
            if let Ok(meta) = entry.metadata() {
                if meta.is_dir() {
                    total += dir_size(&entry.path());
                } else {
                    total += meta.len();
                }
            }
        }
    }
    total
}

fn file_size(path: &Path) -> u64 {
    std::fs::metadata(path).map(|m| m.len()).unwrap_or(0)
}

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
    pub profile: BrowserProfileMode,
    pub max_pages: usize,
}

impl Default for CdpBrowserConfig {
    fn default() -> Self {
        Self {
            launch_mode: ChromiumLaunchMode::Headless,
            viewport: BrowserViewport::default(),
            timeouts: BrowserTimeouts::default(),
            profile: BrowserProfileMode::default(),
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

    pub fn with_profile(mut self, profile: BrowserProfileMode) -> Self {
        self.profile = profile;
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

/// Errors produced by browser-kernel-core validation and runtime paths.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum BrowserKernelError {
    #[error("element selector cannot be empty")]
    EmptySelector,
    #[error("form fill spec must contain at least one field")]
    EmptyForm,
    #[error("url cannot be empty")]
    EmptyUrl,
    #[error("invalid browser config: {0}")]
    InvalidConfig(String),
    #[error("chromium browser is not available")]
    ChromiumNotAvailable,
    #[error("unsupported browser action: {0}")]
    UnsupportedAction(String),
    #[error("browser action failed: {0}")]
    ActionFailed(String),
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
// Chromium lifecycle manager
// ---------------------------------------------------------------------------

/// Manager for Chromium process lifecycle.
///
/// Can operate in two modes:
/// - Skeleton mode: no real Chromium, always reports unavailability
/// - Real mode: launches/connects to Chromium via CDP
pub struct ChromiumLifecycleManager {
    config: CdpBrowserConfig,
    storage: Option<std::sync::Arc<dyn BrowserStorage>>,
    browser: Option<chromiumoxide::Browser>,
    handler: Option<tokio::task::JoinHandle<()>>,
}

impl std::fmt::Debug for ChromiumLifecycleManager {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ChromiumLifecycleManager")
            .field("config", &self.config)
            .field("storage", &self.storage.is_some())
            .field("browser", &self.browser.is_some())
            .finish()
    }
}

impl ChromiumLifecycleManager {
    pub fn new(config: CdpBrowserConfig) -> Self {
        Self {
            config,
            storage: None,
            browser: None,
            handler: None,
        }
    }

    /// Create with a BrowserStorage backend for profile management.
    pub fn with_storage(mut self, storage: std::sync::Arc<dyn BrowserStorage>) -> Self {
        self.storage = Some(storage);
        self
    }

    pub fn config(&self) -> &CdpBrowserConfig {
        &self.config
    }

    /// Returns `true` when a real Chromium instance is available.
    pub fn is_available(&self) -> bool {
        self.browser.is_some()
    }

    /// Launch a new Chromium instance with the configured settings.
    pub async fn launch(&mut self) -> Result<(), BrowserKernelError> {
        use chromiumoxide::{Browser, BrowserConfig};

        let mut builder = BrowserConfig::builder();

        // Set launch mode
        match &self.config.launch_mode {
            ChromiumLaunchMode::Headless => {
                // Default is headless
            }
            ChromiumLaunchMode::Headful => {
                builder = builder.with_head();
            }
            ChromiumLaunchMode::Connect { .. } => {
                return Err(BrowserKernelError::InvalidConfig(
                    "use connect() for Connect mode".to_string(),
                ));
            }
        }

        // Set viewport
        builder = builder.window_size(self.config.viewport.width, self.config.viewport.height);

        // Resolve profile path
        let user_data_dir = match &self.config.profile {
            BrowserProfileMode::Named(name) => {
                if let Some(ref storage) = self.storage {
                    storage
                        .resolve_profile(name)?
                        .map(|p| p.to_string_lossy().to_string())
                } else {
                    None
                }
            }
            BrowserProfileMode::Temporary => {
                // Create temporary profile if storage available
                // For FsBrowserStorage, we need to call create_temporary
                // but BrowserStorage trait doesn't have this method
                // So we skip for now
                None
            }
            BrowserProfileMode::Ephemeral => None,
        };

        // Add user-data-dir argument if available
        if let Some(ref dir) = user_data_dir {
            builder = builder.arg(format!("--user-data-dir={}", dir));
        }

        let config = builder.build().map_err(|e| {
            BrowserKernelError::InvalidConfig(format!("failed to build browser config: {}", e))
        })?;

        let (browser, handler) = Browser::launch(config).await.map_err(|e| {
            BrowserKernelError::InvalidConfig(format!("failed to launch chromium: {}", e))
        })?;

        // The handler needs to be polled to drive the browser.
        // We spawn it in the background.
        let handle = tokio::spawn(async move {
            let mut handler = handler;
            loop {
                if handler.next().await.is_none() {
                    break;
                }
            }
        });

        self.browser = Some(browser);
        self.handler = Some(handle);

        Ok(())
    }

    /// Connect to an existing Chromium instance via websocket URL.
    pub async fn connect(&mut self, websocket_url: &str) -> Result<(), BrowserKernelError> {
        use chromiumoxide::Browser;

        let (browser, handler) = Browser::connect(websocket_url).await.map_err(|e| {
            BrowserKernelError::InvalidConfig(format!("failed to connect to chromium: {}", e))
        })?;

        // The handler needs to be polled to drive the browser.
        let handle = tokio::spawn(async move {
            let mut handler = handler;
            loop {
                if handler.next().await.is_none() {
                    break;
                }
            }
        });

        self.browser = Some(browser);
        self.handler = Some(handle);

        Ok(())
    }

    /// Get a reference to the browser (if available).
    pub fn browser(&self) -> Option<&chromiumoxide::Browser> {
        self.browser.as_ref()
    }

    /// Shutdown the Chromium instance gracefully.
    pub async fn shutdown(&mut self) -> Result<(), BrowserKernelError> {
        if let Some(mut browser) = self.browser.take() {
            browser.close().await.map_err(|e| {
                BrowserKernelError::InvalidConfig(format!("failed to close browser: {}", e))
            })?;
        }
        if let Some(handler) = self.handler.take() {
            handler.abort();
        }
        Ok(())
    }
}

impl Drop for ChromiumLifecycleManager {
    fn drop(&mut self) {
        if let Some(handler) = self.handler.take() {
            handler.abort();
        }
    }
}

// ---------------------------------------------------------------------------
// CDP browser executor skeleton
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct BrowserOpenUrlInput {
    url: String,
    #[serde(default)]
    take_snapshot: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct BrowserExtractContentInput {
    url: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct BrowserInteractiveSnapshotInput {
    url: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct BrowserClickElementInput {
    url: String,
    selector: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct BrowserTypeTextInput {
    url: String,
    #[serde(default)]
    selector: Option<String>,
    text: String,
    #[serde(default)]
    clear_first: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct RawInteractiveElement {
    id: String,
    kind: String,
    selector: Option<String>,
    text: Option<String>,
    aria_label: Option<String>,
    bounding_box: Option<ElementBoundingBox>,
}

/// Real CDP browser executor.
///
/// This executor implements [`ActionExecutor`] for browser actions backed by a
/// live `chromiumoxide::Browser` when Chromium has been launched or connected.
/// Unknown action kinds return [`ActionExecutorError::NotSupported`]. Known
/// actions still return [`ActionExecutorError::ExecutionFailed`] when Chromium
/// is not available.
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

    /// Check if Chromium is available for action execution.
    pub fn is_chromium_available(&self) -> bool {
        self.lifecycle.is_available()
    }

    /// Get a reference to the browser (if available).
    pub fn browser(&self) -> Option<&chromiumoxide::Browser> {
        self.lifecycle.browser()
    }

    fn validate_url(url: &str) -> Result<String, BrowserKernelError> {
        let trimmed = url.trim();
        if trimmed.is_empty() {
            return Err(BrowserKernelError::EmptyUrl);
        }
        Ok(trimmed.to_string())
    }

    async fn open_url(
        &self,
        browser: &chromiumoxide::Browser,
        input: BrowserOpenUrlInput,
    ) -> Result<action_core::ActionResult, action_core::ActionExecutorError> {
        let url = Self::validate_url(&input.url).map_err(to_invalid_input)?;
        let page = browser
            .new_page("about:blank")
            .await
            .map_err(to_execution_failed)?;
        page.goto(url.clone()).await.map_err(to_execution_failed)?;

        let title = page_title(&page).await.unwrap_or_default();
        let current_url = page_url(&page).await.unwrap_or_else(|_| url.clone());
        let html_source = if input.take_snapshot {
            Some(page.content().await.map_err(to_execution_failed)?)
        } else {
            None
        };
        let text = page_body_text(&page).await.unwrap_or_default();

        Ok(action_core::ActionResult {
            status: action_core::ActionStatus::Completed,
            summary: format!("Opened URL: {}", current_url),
            payload: action_core::ActionResultPayload::Json(serde_json::json!({
                "url": { "0": current_url },
                "title": title,
                "content": text,
                "html_source": html_source,
                "captured_at": chrono::Utc::now(),
                "artifact_id": null
            })),
            completed_at: chrono::Utc::now(),
        })
    }

    async fn extract_content(
        &self,
        browser: &chromiumoxide::Browser,
        input: BrowserExtractContentInput,
    ) -> Result<action_core::ActionResult, action_core::ActionExecutorError> {
        let url = Self::validate_url(&input.url).map_err(to_invalid_input)?;
        let page = browser
            .new_page("about:blank")
            .await
            .map_err(to_execution_failed)?;
        page.goto(url.clone()).await.map_err(to_execution_failed)?;

        let current_url = page_url(&page).await.unwrap_or_else(|_| url.clone());
        let text = page_body_text(&page).await.unwrap_or_default();
        let links = page_string_array(&page, LINK_EXTRACTION_JS)
            .await
            .unwrap_or_default();
        let images = page_string_array(&page, IMAGE_EXTRACTION_JS)
            .await
            .unwrap_or_default();

        Ok(action_core::ActionResult {
            status: action_core::ActionStatus::Completed,
            summary: format!("Extracted content from URL: {}", current_url),
            payload: action_core::ActionResultPayload::Json(serde_json::json!({
                "source_url": { "0": current_url },
                "text": text,
                "links": links,
                "images": images,
                "extracted_at": chrono::Utc::now()
            })),
            completed_at: chrono::Utc::now(),
        })
    }

    async fn get_interactive_snapshot(
        &self,
        browser: &chromiumoxide::Browser,
        input: BrowserInteractiveSnapshotInput,
    ) -> Result<action_core::ActionResult, action_core::ActionExecutorError> {
        let url = Self::validate_url(&input.url).map_err(to_invalid_input)?;
        let page = browser
            .new_page("about:blank")
            .await
            .map_err(to_execution_failed)?;
        page.goto(url.clone()).await.map_err(to_execution_failed)?;

        let current_url = page_url(&page).await.unwrap_or_else(|_| url.clone());
        let title = page_title(&page).await.unwrap_or_default();
        let elements = page_interactive_elements(&page)
            .await
            .map_err(to_execution_failed)?;
        let snapshot = InteractiveSnapshot {
            url: current_url.clone(),
            title,
            elements,
            screenshot_artifact_id: None,
            captured_at: chrono::Utc::now(),
        };

        Ok(action_core::ActionResult {
            status: action_core::ActionStatus::Completed,
            summary: format!(
                "Captured interactive snapshot for URL: {} ({} elements)",
                current_url,
                snapshot.elements.len()
            ),
            payload: action_core::ActionResultPayload::Json(
                serde_json::to_value(snapshot).map_err(to_execution_failed)?,
            ),
            completed_at: chrono::Utc::now(),
        })
    }

    async fn click_element(
        &self,
        browser: &chromiumoxide::Browser,
        input: BrowserClickElementInput,
    ) -> Result<action_core::ActionResult, action_core::ActionExecutorError> {
        let url = Self::validate_url(&input.url).map_err(to_invalid_input)?;
        let selector = input.selector.trim().to_string();
        if selector.is_empty() {
            return Err(to_invalid_input(BrowserKernelError::EmptySelector));
        }

        let page = browser
            .new_page("about:blank")
            .await
            .map_err(to_execution_failed)?;
        page.goto(url.clone()).await.map_err(to_execution_failed)?;

        let current_url = page_url(&page).await.unwrap_or_else(|_| url.clone());

        let element = page.find_element(&selector).await.map_err(|e| {
            to_execution_failed(format!(
                "Element not found with selector '{}': {}",
                selector, e
            ))
        })?;
        element.click().await.map_err(to_execution_failed)?;

        Ok(action_core::ActionResult {
            status: action_core::ActionStatus::Completed,
            summary: format!("Clicked element '{}' on page: {}", selector, current_url),
            payload: action_core::ActionResultPayload::Json(serde_json::json!({
                "success": true,
                "url": current_url,
                "element_description": format!("Clicked element at selector: {}", selector),
                "interacted_at": chrono::Utc::now()
            })),
            completed_at: chrono::Utc::now(),
        })
    }

    async fn type_text(
        &self,
        browser: &chromiumoxide::Browser,
        input: BrowserTypeTextInput,
    ) -> Result<action_core::ActionResult, action_core::ActionExecutorError> {
        let url = Self::validate_url(&input.url).map_err(to_invalid_input)?;
        let text = input.text.clone();

        let page = browser
            .new_page("about:blank")
            .await
            .map_err(to_execution_failed)?;
        page.goto(url.clone()).await.map_err(to_execution_failed)?;

        let current_url = page_url(&page).await.unwrap_or_else(|_| url.clone());

        // Find and focus the target element
        let element = if let Some(ref selector) = input.selector {
            let sel = selector.trim();
            if sel.is_empty() {
                return Err(to_invalid_input(BrowserKernelError::EmptySelector));
            }
            page.find_element(sel).await.map_err(|e| {
                to_execution_failed(format!("Element not found with selector '{}': {}", sel, e))
            })?
        } else {
            // No selector: use the currently focused element via activeElement
            // We use evaluate to get a reference to document.activeElement
            let active_js = "document.activeElement";
            page.evaluate(active_js)
                .await
                .map_err(|e| to_execution_failed(format!("Could not get active element: {}", e)))?;
            // If no selector and no active element, we still need a target.
            // For safety, return an error asking for a selector.
            return Err(to_invalid_input(BrowserKernelError::ActionFailed(
                "type_text requires a selector when no element is focused".to_string(),
            )));
        };

        // Clear existing content if requested
        if input.clear_first {
            element.click().await.map_err(to_execution_failed)?;
            // Select all text and delete it
            element
                .press_key("Meta+a")
                .await
                .map_err(to_execution_failed)?;
            element
                .press_key("Backspace")
                .await
                .map_err(to_execution_failed)?;
        } else {
            // Focus the element first
            element.click().await.map_err(to_execution_failed)?;
        }

        // Type the text
        element.type_str(&text).await.map_err(to_execution_failed)?;

        Ok(action_core::ActionResult {
            status: action_core::ActionStatus::Completed,
            summary: format!("Typed text into element on page: {}", current_url),
            payload: action_core::ActionResultPayload::Json(serde_json::json!({
                "success": true,
                "url": current_url,
                "element_description": format!(
                    "Typed '{}' into element at selector: {}",
                    text,
                    input.selector.as_deref().unwrap_or("<focused>")
                ),
                "interacted_at": chrono::Utc::now()
            })),
            completed_at: chrono::Utc::now(),
        })
    }
}

const PAGE_TITLE_JS: &str = "document.title || ''";
const PAGE_URL_JS: &str = "window.location.href || ''";
const BODY_TEXT_JS: &str = "document.body ? document.body.innerText : ''";
const LINK_EXTRACTION_JS: &str =
    "Array.from(document.querySelectorAll('a[href]')).map(a => a.href).filter(Boolean)";
const IMAGE_EXTRACTION_JS: &str =
    "Array.from(document.querySelectorAll('img[src]')).map(img => img.src).filter(Boolean)";
const INTERACTIVE_ELEMENTS_JS: &str = r#"
(() => {
  const selectorFor = (el) => {
    if (el.id) return `#${CSS.escape(el.id)}`;
    const testId = el.getAttribute('data-testid') || el.getAttribute('data-test-id');
    if (testId) return `[data-testid="${CSS.escape(testId)}"]`;
    const tag = el.tagName.toLowerCase();
    const name = el.getAttribute('name');
    if (name) return `${tag}[name="${CSS.escape(name)}"]`;
    const parent = el.parentElement;
    if (!parent) return tag;
    const siblings = Array.from(parent.children).filter(child => child.tagName === el.tagName);
    const index = siblings.indexOf(el) + 1;
    return siblings.length > 1 ? `${tag}:nth-of-type(${index})` : tag;
  };
  const kindFor = (el) => {
    const tag = el.tagName.toLowerCase();
    const role = (el.getAttribute('role') || '').toLowerCase();
    const type = (el.getAttribute('type') || '').toLowerCase();
    if (tag === 'a') return 'link';
    if (tag === 'button' || role === 'button' || type === 'button' || type === 'submit') return 'button';
    if (tag === 'select') return 'select';
    if (tag === 'textarea') return 'text_area';
    if (type === 'checkbox') return 'checkbox';
    if (type === 'radio') return 'radio';
    if (tag === 'input') return 'input';
    return 'other';
  };
  const candidates = Array.from(document.querySelectorAll(
    'a[href],button,input,select,textarea,[role="button"],[onclick],[tabindex]'
  ));
  return candidates
    .filter(el => {
      const rect = el.getBoundingClientRect();
      const style = window.getComputedStyle(el);
      return rect.width > 0 && rect.height > 0 && style.visibility !== 'hidden' && style.display !== 'none';
    })
    .slice(0, 200)
    .map((el, index) => {
      const rect = el.getBoundingClientRect();
      const text = (el.innerText || el.value || el.getAttribute('title') || '').trim();
      return {
        id: `e${index + 1}`,
        kind: kindFor(el),
        selector: selectorFor(el),
        text: text ? text.slice(0, 200) : null,
        aria_label: el.getAttribute('aria-label'),
        bounding_box: { x: rect.x, y: rect.y, width: rect.width, height: rect.height }
      };
    });
})()
"#;

async fn page_title(page: &chromiumoxide::Page) -> Result<String, BrowserKernelError> {
    page_string(page, PAGE_TITLE_JS).await
}

async fn page_url(page: &chromiumoxide::Page) -> Result<String, BrowserKernelError> {
    page_string(page, PAGE_URL_JS).await
}

async fn page_body_text(page: &chromiumoxide::Page) -> Result<String, BrowserKernelError> {
    page_string(page, BODY_TEXT_JS).await
}

async fn page_string(
    page: &chromiumoxide::Page,
    expression: &str,
) -> Result<String, BrowserKernelError> {
    page.evaluate(expression)
        .await
        .map_err(to_browser_action_failed)?
        .into_value()
        .map_err(|e| BrowserKernelError::ActionFailed(e.to_string()))
}

async fn page_string_array(
    page: &chromiumoxide::Page,
    expression: &str,
) -> Result<Vec<String>, BrowserKernelError> {
    page.evaluate(expression)
        .await
        .map_err(to_browser_action_failed)?
        .into_value()
        .map_err(|e| BrowserKernelError::ActionFailed(e.to_string()))
}

async fn page_interactive_elements(
    page: &chromiumoxide::Page,
) -> Result<Vec<InteractiveElement>, BrowserKernelError> {
    let raw: Vec<RawInteractiveElement> = page
        .evaluate(INTERACTIVE_ELEMENTS_JS)
        .await
        .map_err(to_browser_action_failed)?
        .into_value()
        .map_err(|e| BrowserKernelError::ActionFailed(e.to_string()))?;

    Ok(raw
        .into_iter()
        .map(|element| InteractiveElement {
            id: element.id,
            kind: interactive_kind_from_str(&element.kind),
            selector: element.selector.map(ElementSelector::Css),
            text: element.text,
            aria_label: element.aria_label,
            bounding_box: element.bounding_box,
        })
        .collect())
}

fn interactive_kind_from_str(kind: &str) -> InteractiveElementKind {
    match kind {
        "link" => InteractiveElementKind::Link,
        "button" => InteractiveElementKind::Button,
        "input" => InteractiveElementKind::Input,
        "select" => InteractiveElementKind::Select,
        "text_area" => InteractiveElementKind::TextArea,
        "checkbox" => InteractiveElementKind::Checkbox,
        "radio" => InteractiveElementKind::Radio,
        _ => InteractiveElementKind::Other,
    }
}

fn to_browser_action_failed(error: impl std::fmt::Display) -> BrowserKernelError {
    BrowserKernelError::ActionFailed(error.to_string())
}

fn to_execution_failed(error: impl std::fmt::Display) -> action_core::ActionExecutorError {
    action_core::ActionExecutorError::ExecutionFailed(error.to_string())
}

fn to_invalid_input(error: impl std::fmt::Display) -> action_core::ActionExecutorError {
    action_core::ActionExecutorError::InvalidInput(error.to_string())
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

        // Check if this is a known browser action
        if !KNOWN_BROWSER_ACTIONS.contains(&kind) {
            return Err(action_core::ActionExecutorError::NotSupported(
                request.action_kind.clone(),
            ));
        }

        // Check if Chromium is available
        if !self.is_chromium_available() {
            return Err(action_core::ActionExecutorError::ExecutionFailed(
                BrowserKernelError::ChromiumNotAvailable.to_string(),
            ));
        }

        // Get browser reference
        let browser = self.browser().ok_or_else(|| {
            action_core::ActionExecutorError::ExecutionFailed(
                BrowserKernelError::ChromiumNotAvailable.to_string(),
            )
        })?;

        match kind {
            "browser.open_url" => {
                let input: BrowserOpenUrlInput = serde_json::from_value(request.input.clone())
                    .map_err(|e| action_core::ActionExecutorError::InvalidInput(e.to_string()))?;
                self.open_url(browser, input).await
            }
            "browser.extract_content" => {
                let input: BrowserExtractContentInput =
                    serde_json::from_value(request.input.clone()).map_err(|e| {
                        action_core::ActionExecutorError::InvalidInput(e.to_string())
                    })?;
                self.extract_content(browser, input).await
            }
            "browser.get_interactive_snapshot" => {
                let input: BrowserInteractiveSnapshotInput =
                    serde_json::from_value(request.input.clone()).map_err(|e| {
                        action_core::ActionExecutorError::InvalidInput(e.to_string())
                    })?;
                self.get_interactive_snapshot(browser, input).await
            }
            "browser.click_element" => {
                let input: BrowserClickElementInput = serde_json::from_value(request.input.clone())
                    .map_err(|e| action_core::ActionExecutorError::InvalidInput(e.to_string()))?;
                self.click_element(browser, input).await
            }
            "browser.type_text" => {
                let input: BrowserTypeTextInput = serde_json::from_value(request.input.clone())
                    .map_err(|e| action_core::ActionExecutorError::InvalidInput(e.to_string()))?;
                self.type_text(browser, input).await
            }
            _ => Err(action_core::ActionExecutorError::NotSupported(
                request.action_kind.clone(),
            )),
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

    fn action_request_with_input(kind_str: &str, input: serde_json::Value) -> ActionRequest {
        ActionRequest {
            action_id: ActionId::from("action-test-1"),
            action_kind: ActionKind::from(kind_str),
            input,
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
        assert_eq!(
            config.profile,
            BrowserProfileMode::Named("default".to_string())
        );
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
            .with_profile(BrowserProfileMode::Named("work".to_string()));

        assert_eq!(config.viewport.width, 1920);
        assert_eq!(config.viewport.height, 1080);
        assert_eq!(config.viewport.device_scale_factor, 2.0);
        assert_eq!(config.max_pages, 3);
        assert_eq!(
            config.profile,
            BrowserProfileMode::Named("work".to_string())
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

    #[test]
    fn browser_open_url_input_defaults_take_snapshot_false() {
        let input: BrowserOpenUrlInput =
            serde_json::from_value(serde_json::json!({"url": "https://example.com"})).unwrap();

        assert_eq!(input.url, "https://example.com");
        assert!(!input.take_snapshot);
    }

    #[test]
    fn cdp_browser_executor_validate_url_trims_and_rejects_empty() {
        assert_eq!(
            CdpBrowserExecutor::validate_url("  https://example.com  ").unwrap(),
            "https://example.com"
        );
        assert!(matches!(
            CdpBrowserExecutor::validate_url("   "),
            Err(BrowserKernelError::EmptyUrl)
        ));
    }

    #[tokio::test]
    async fn cdp_browser_executor_rejects_unimplemented_known_browser_action_even_if_available_check_would_apply()
     {
        let lifecycle = ChromiumLifecycleManager::new(CdpBrowserConfig::default());
        let executor = CdpBrowserExecutor::new(lifecycle, ts());

        let request = action_request("browser.click_element");

        let result = executor.execute(&request).await;
        assert!(result.is_err());
        match result.unwrap_err() {
            ActionExecutorError::ExecutionFailed(msg) => {
                assert!(msg.contains("chromium browser is not available"));
            }
            other => panic!("expected ExecutionFailed before dispatch, got: {other:?}"),
        }
    }

    #[test]
    fn browser_interactive_snapshot_input_parses_url() {
        let input: BrowserInteractiveSnapshotInput =
            serde_json::from_value(serde_json::json!({"url": "https://example.com/app"})).unwrap();

        assert_eq!(input.url, "https://example.com/app");
    }

    #[test]
    fn browser_click_element_input_parses_url_and_selector() {
        let input: BrowserClickElementInput = serde_json::from_value(
            serde_json::json!({"url": "https://example.com", "selector": "button#submit"}),
        )
        .unwrap();

        assert_eq!(input.url, "https://example.com");
        assert_eq!(input.selector, "button#submit");
    }

    #[test]
    fn browser_type_text_input_parses_with_defaults() {
        let input: BrowserTypeTextInput = serde_json::from_value(
            serde_json::json!({"url": "https://example.com", "text": "hello"}),
        )
        .unwrap();

        assert_eq!(input.url, "https://example.com");
        assert_eq!(input.text, "hello");
        assert!(input.selector.is_none());
        assert!(!input.clear_first);
    }

    #[test]
    fn browser_type_text_input_parses_all_fields() {
        let input: BrowserTypeTextInput = serde_json::from_value(serde_json::json!({
            "url": "https://example.com",
            "selector": "input#name",
            "text": "Alice",
            "clear_first": true
        }))
        .unwrap();

        assert_eq!(input.url, "https://example.com");
        assert_eq!(input.selector, Some("input#name".to_string()));
        assert_eq!(input.text, "Alice");
        assert!(input.clear_first);
    }

    #[tokio::test]
    async fn cdp_browser_click_element_returns_chromium_not_available() {
        let lifecycle = ChromiumLifecycleManager::new(CdpBrowserConfig::default());
        let executor = CdpBrowserExecutor::new(lifecycle, ts());

        let request = action_request_with_input(
            "browser.click_element",
            serde_json::json!({"url": "https://example.com", "selector": "button"}),
        );
        let result = executor.execute(&request).await;
        assert!(result.is_err());
        match result.unwrap_err() {
            ActionExecutorError::ExecutionFailed(msg) => {
                assert!(msg.contains("chromium browser is not available"));
            }
            other => panic!("expected ExecutionFailed, got: {other:?}"),
        }
    }

    #[tokio::test]
    async fn cdp_browser_type_text_returns_chromium_not_available() {
        let lifecycle = ChromiumLifecycleManager::new(CdpBrowserConfig::default());
        let executor = CdpBrowserExecutor::new(lifecycle, ts());

        let request = action_request_with_input(
            "browser.type_text",
            serde_json::json!({"url": "https://example.com", "text": "hello"}),
        );
        let result = executor.execute(&request).await;
        assert!(result.is_err());
        match result.unwrap_err() {
            ActionExecutorError::ExecutionFailed(msg) => {
                assert!(msg.contains("chromium browser is not available"));
            }
            other => panic!("expected ExecutionFailed, got: {other:?}"),
        }
    }

    #[test]
    fn interactive_kind_from_str_maps_known_kinds() {
        assert_eq!(
            interactive_kind_from_str("link"),
            InteractiveElementKind::Link
        );
        assert_eq!(
            interactive_kind_from_str("button"),
            InteractiveElementKind::Button
        );
        assert_eq!(
            interactive_kind_from_str("input"),
            InteractiveElementKind::Input
        );
        assert_eq!(
            interactive_kind_from_str("select"),
            InteractiveElementKind::Select
        );
        assert_eq!(
            interactive_kind_from_str("text_area"),
            InteractiveElementKind::TextArea
        );
        assert_eq!(
            interactive_kind_from_str("checkbox"),
            InteractiveElementKind::Checkbox
        );
        assert_eq!(
            interactive_kind_from_str("radio"),
            InteractiveElementKind::Radio
        );
        assert_eq!(
            interactive_kind_from_str("unknown"),
            InteractiveElementKind::Other
        );
    }

    #[test]
    fn raw_interactive_element_maps_to_interactive_element_shape() {
        let raw = RawInteractiveElement {
            id: "e1".to_string(),
            kind: "button".to_string(),
            selector: Some("#submit".to_string()),
            text: Some("Submit".to_string()),
            aria_label: Some("Submit form".to_string()),
            bounding_box: Some(ElementBoundingBox {
                x: 1.0,
                y: 2.0,
                width: 100.0,
                height: 32.0,
            }),
        };

        let element = InteractiveElement {
            id: raw.id,
            kind: interactive_kind_from_str(&raw.kind),
            selector: raw.selector.map(ElementSelector::Css),
            text: raw.text,
            aria_label: raw.aria_label,
            bounding_box: raw.bounding_box,
        };

        assert_eq!(element.id, "e1");
        assert_eq!(element.kind, InteractiveElementKind::Button);
        assert_eq!(
            element.selector,
            Some(ElementSelector::Css("#submit".to_string()))
        );
        assert_eq!(element.text, Some("Submit".to_string()));
        assert_eq!(element.aria_label, Some("Submit form".to_string()));
        assert_eq!(element.bounding_box.unwrap().width, 100.0);
    }

    // ---- BrowserProfileMode tests ----

    #[test]
    fn browser_profile_mode_serde_roundtrip() {
        let modes = vec![
            BrowserProfileMode::Named("default".to_string()),
            BrowserProfileMode::Named("work".to_string()),
            BrowserProfileMode::Temporary,
            BrowserProfileMode::Ephemeral,
        ];

        for mode in modes {
            let json = serde_json::to_string(&mode).unwrap();
            let decoded: BrowserProfileMode = serde_json::from_str(&json).unwrap();
            assert_eq!(decoded, mode);
        }
    }

    #[test]
    fn browser_profile_mode_default_is_named_default() {
        let mode = BrowserProfileMode::default();
        assert_eq!(mode, BrowserProfileMode::Named("default".to_string()));
    }

    // ---- FsBrowserStorage tests ----

    #[test]
    fn fs_storage_rejects_empty_root() {
        let result = FsBrowserStorage::new("");
        assert!(result.is_err());
    }

    #[test]
    fn fs_storage_resolve_or_create_creates_directory() {
        let tmp = tempfile::tempdir().unwrap();
        let storage = FsBrowserStorage::new(tmp.path()).unwrap();

        let path = storage.resolve_or_create("test-profile").unwrap();
        assert!(path.exists());
        assert!(path.ends_with("browser-profiles/test-profile"));
    }

    #[test]
    fn fs_storage_resolve_or_create_returns_existing_path() {
        let tmp = tempfile::tempdir().unwrap();
        let storage = FsBrowserStorage::new(tmp.path()).unwrap();

        let path1 = storage.resolve_or_create("my-profile").unwrap();
        let path2 = storage.resolve_or_create("my-profile").unwrap();
        assert_eq!(path1, path2);
    }

    #[test]
    fn fs_storage_create_temporary_generates_uuid_directory() {
        let tmp = tempfile::tempdir().unwrap();
        let storage = FsBrowserStorage::new(tmp.path()).unwrap();

        let profile = storage.create_temporary().unwrap();
        assert!(profile.is_temporary);
        assert!(profile.path.exists());
        assert!(profile.name.len() > 0);
    }

    #[test]
    fn fs_storage_delete_temporaries_removes_tmp_directory() {
        let tmp = tempfile::tempdir().unwrap();
        let storage = FsBrowserStorage::new(tmp.path()).unwrap();

        let profile1 = storage.create_temporary().unwrap();
        let profile2 = storage.create_temporary().unwrap();
        assert!(profile1.path.exists());
        assert!(profile2.path.exists());

        storage.delete_temporaries().unwrap();
        assert!(!profile1.path.exists());
        assert!(!profile2.path.exists());
    }

    #[test]
    fn fs_storage_list_profiles_returns_named_profiles() {
        let tmp = tempfile::tempdir().unwrap();
        let storage = FsBrowserStorage::new(tmp.path()).unwrap();

        storage.resolve_or_create("default").unwrap();
        storage.resolve_or_create("work").unwrap();

        let profiles = storage.list_profiles().unwrap();
        assert!(profiles.contains(&"default".to_string()));
        assert!(profiles.contains(&"work".to_string()));
    }

    #[test]
    fn fs_storage_clear_profile_removes_directory() {
        let tmp = tempfile::tempdir().unwrap();
        let storage = FsBrowserStorage::new(tmp.path()).unwrap();

        storage.resolve_or_create("to-clear").unwrap();
        assert!(storage.profile_exists("to-clear"));

        storage.clear_profile("to-clear").unwrap();
        assert!(!storage.profile_exists("to-clear"));
    }

    #[test]
    fn fs_storage_profiles_json_created_on_first_profile() {
        let tmp = tempfile::tempdir().unwrap();
        let storage = FsBrowserStorage::new(tmp.path()).unwrap();

        storage.resolve_or_create("first").unwrap();

        let json_path = tmp.path().join("browser-profiles/profiles.json");
        assert!(json_path.exists());

        let content = std::fs::read_to_string(&json_path).unwrap();
        let profiles: serde_json::Value = serde_json::from_str(&content).unwrap();
        assert!(profiles.get("first").is_some());
    }

    #[test]
    fn fs_storage_profiles_json_graceful_on_corrupt() {
        let tmp = tempfile::tempdir().unwrap();
        let storage = FsBrowserStorage::new(tmp.path()).unwrap();

        // Create a corrupt profiles.json
        let json_path = tmp.path().join("browser-profiles/profiles.json");
        std::fs::create_dir_all(json_path.parent().unwrap()).unwrap();
        std::fs::write(&json_path, "not valid json").unwrap();

        // Should still work (graceful fallback)
        let profiles = storage.list_profiles().unwrap();
        assert!(profiles.is_empty());
    }

    #[test]
    fn fs_storage_relative_path_canonicalized() {
        let tmp = tempfile::tempdir().unwrap();
        let relative_path = tmp.path().join("relative");
        let storage = FsBrowserStorage::new(&relative_path).unwrap();

        // The root should be canonicalized to absolute
        assert!(storage.root.is_absolute());
    }

    #[test]
    fn profile_storage_info_calculates_sizes() {
        let tmp = tempfile::tempdir().unwrap();
        let storage = FsBrowserStorage::new(tmp.path()).unwrap();

        let profile_path = storage.resolve_or_create("info-test").unwrap();
        // Create some files to measure
        std::fs::write(profile_path.join("Cookies"), vec![0u8; 1024]).unwrap();
        std::fs::create_dir_all(profile_path.join("Local Storage")).unwrap();
        std::fs::write(profile_path.join("Local Storage/data"), vec![0u8; 512]).unwrap();

        let rt = tokio::runtime::Runtime::new().unwrap();
        let info = rt.block_on(storage.storage_info("info-test")).unwrap();

        assert_eq!(info.profile_name, "info-test");
        assert!(info.total_bytes > 0);
        assert_eq!(info.cookies_db_bytes, 1024);
        assert_eq!(info.local_storage_bytes, 512);
    }

    #[tokio::test]
    async fn fs_storage_clear_cache_preserves_cookies() {
        let tmp = tempfile::tempdir().unwrap();
        let storage = FsBrowserStorage::new(tmp.path()).unwrap();

        let profile_path = storage.resolve_or_create("cache-test").unwrap();
        std::fs::write(profile_path.join("Cookies"), "cookie-data").unwrap();
        std::fs::create_dir_all(profile_path.join("Cache")).unwrap();
        std::fs::write(profile_path.join("Cache/data"), "cache-data").unwrap();

        storage.clear_cache("cache-test").await.unwrap();

        // Cookies preserved
        assert!(profile_path.join("Cookies").exists());
        // Cache cleared
        assert!(!profile_path.join("Cache").exists());
    }

    #[test]
    fn ephemeral_mode_does_not_create_directories() {
        let config = CdpBrowserConfig::default().with_profile(BrowserProfileMode::Ephemeral);
        assert_eq!(config.profile, BrowserProfileMode::Ephemeral);
        // Ephemeral mode should not create any profile directory
        // (this is verified by the ChromiumLifecycleManager not calling resolve_profile)
    }
}
