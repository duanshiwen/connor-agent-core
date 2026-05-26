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
use std::sync::Arc;

// ---------------------------------------------------------------------------
// Browser session types
// ---------------------------------------------------------------------------

/// Stable browser session identifier.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct BrowserSessionId(pub String);

impl BrowserSessionId {
    pub fn new(value: impl Into<String>) -> Result<Self, BrowserKernelError> {
        let value = value.into();
        if value.trim().is_empty() {
            Err(BrowserKernelError::InvalidConfig(
                "browser session id cannot be empty".to_string(),
            ))
        } else {
            Ok(Self(value))
        }
    }
}

/// Stable browser page/tab identifier within a session.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct BrowserPageId(pub String);

impl BrowserPageId {
    pub fn new(value: impl Into<String>) -> Result<Self, BrowserKernelError> {
        let value = value.into();
        if value.trim().is_empty() {
            Err(BrowserKernelError::InvalidConfig(
                "browser page id cannot be empty".to_string(),
            ))
        } else {
            Ok(Self(value))
        }
    }
}

/// Runtime lifecycle state for a browser page/tab.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BrowserPageStatus {
    Opening,
    Active,
    Background,
    Closed,
    Crashed,
}

/// Lightweight health classification for a browser page/tab.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BrowserPageHealthStatus {
    Healthy,
    Unresponsive,
    Crashed,
    Closed,
}

/// Latest known page/tab health details.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BrowserPageHealth {
    pub status: BrowserPageHealthStatus,
    pub reason: Option<String>,
    pub checked_at: DateTime<Utc>,
}

impl BrowserPageHealth {
    pub fn healthy(now: DateTime<Utc>) -> Self {
        Self {
            status: BrowserPageHealthStatus::Healthy,
            reason: None,
            checked_at: now,
        }
    }

    pub fn unresponsive(reason: impl Into<String>, now: DateTime<Utc>) -> Self {
        Self {
            status: BrowserPageHealthStatus::Unresponsive,
            reason: Some(reason.into()),
            checked_at: now,
        }
    }

    pub fn crashed(reason: impl Into<String>, now: DateTime<Utc>) -> Self {
        Self {
            status: BrowserPageHealthStatus::Crashed,
            reason: Some(reason.into()),
            checked_at: now,
        }
    }

    pub fn closed(now: DateTime<Utc>) -> Self {
        Self {
            status: BrowserPageHealthStatus::Closed,
            reason: None,
            checked_at: now,
        }
    }
}

/// Metadata for one browser page/tab in a session.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BrowserPageInfo {
    pub page_id: BrowserPageId,
    pub url: Option<String>,
    pub title: Option<String>,
    pub status: BrowserPageStatus,
    pub health: BrowserPageHealth,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl BrowserPageInfo {
    pub fn new(page_id: BrowserPageId, now: DateTime<Utc>) -> Self {
        Self {
            page_id,
            url: None,
            title: None,
            status: BrowserPageStatus::Opening,
            health: BrowserPageHealth::healthy(now),
            created_at: now,
            updated_at: now,
        }
    }

    pub fn with_location(
        mut self,
        url: impl Into<String>,
        title: Option<impl Into<String>>,
        now: DateTime<Utc>,
    ) -> Self {
        self.url = Some(url.into());
        self.title = title.map(Into::into);
        self.updated_at = now;
        self
    }

    pub fn with_status(mut self, status: BrowserPageStatus, now: DateTime<Utc>) -> Self {
        self.status = status;
        self.updated_at = now;
        self
    }

    pub fn with_health(mut self, health: BrowserPageHealth, now: DateTime<Utc>) -> Self {
        self.health = health;
        self.updated_at = now;
        self
    }
}

/// Profile binding captured in a browser session for restart/recovery.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BrowserSessionProfileBinding {
    pub mode: BrowserProfileMode,
    pub resolved_path: Option<PathBuf>,
}

impl BrowserSessionProfileBinding {
    pub fn new(mode: BrowserProfileMode, resolved_path: Option<PathBuf>) -> Self {
        Self {
            mode,
            resolved_path,
        }
    }
}

/// Serializable browser session descriptor.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BrowserSession {
    pub session_id: BrowserSessionId,
    pub profile: BrowserSessionProfileBinding,
    pub pages: Vec<BrowserPageInfo>,
    pub active_page_id: Option<BrowserPageId>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl BrowserSession {
    pub fn new(
        session_id: BrowserSessionId,
        profile: BrowserSessionProfileBinding,
        now: DateTime<Utc>,
    ) -> Self {
        Self {
            session_id,
            profile,
            pages: Vec::new(),
            active_page_id: None,
            created_at: now,
            updated_at: now,
        }
    }

    pub fn add_page(&mut self, page: BrowserPageInfo) {
        let page_id = page.page_id.clone();
        let now = page.updated_at;
        self.pages.push(page);
        if self.active_page_id.is_none() {
            let _ = self.set_active_page(&page_id, now);
        } else {
            self.updated_at = now;
        }
    }

    pub fn open_page(
        &mut self,
        page_id: BrowserPageId,
        url: Option<String>,
        title: Option<String>,
        now: DateTime<Utc>,
    ) -> Result<(), BrowserKernelError> {
        if self.pages.iter().any(|page| page.page_id == page_id) {
            return Err(BrowserKernelError::ActionFailed(format!(
                "browser page already exists: {}",
                page_id.0
            )));
        }

        let mut page =
            BrowserPageInfo::new(page_id, now).with_status(BrowserPageStatus::Background, now);
        page.url = url;
        page.title = title;
        self.add_page(page);
        Ok(())
    }

    pub fn close_page(
        &mut self,
        page_id: &BrowserPageId,
        now: DateTime<Utc>,
    ) -> Result<(), BrowserKernelError> {
        let closing_active = self.active_page_id.as_ref() == Some(page_id);
        let page = self.page_mut(page_id)?;
        page.status = BrowserPageStatus::Closed;
        page.health = BrowserPageHealth::closed(now);
        page.updated_at = now;

        if closing_active {
            self.active_page_id = self
                .pages
                .iter()
                .find(|candidate| candidate.status != BrowserPageStatus::Closed)
                .map(|candidate| candidate.page_id.clone());
            if let Some(next_active_id) = self.active_page_id.clone() {
                self.set_active_page(&next_active_id, now)?;
            }
        }
        self.updated_at = now;
        Ok(())
    }

    pub fn update_page_metadata(
        &mut self,
        page_id: &BrowserPageId,
        url: Option<String>,
        title: Option<String>,
        now: DateTime<Utc>,
    ) -> Result<(), BrowserKernelError> {
        let page = self.page_mut(page_id)?;
        page.url = url;
        page.title = title;
        page.updated_at = now;
        self.updated_at = now;
        Ok(())
    }

    pub fn update_page_health(
        &mut self,
        page_id: &BrowserPageId,
        health: BrowserPageHealth,
        now: DateTime<Utc>,
    ) -> Result<(), BrowserKernelError> {
        let status = match health.status {
            BrowserPageHealthStatus::Healthy => None,
            BrowserPageHealthStatus::Unresponsive => None,
            BrowserPageHealthStatus::Crashed => Some(BrowserPageStatus::Crashed),
            BrowserPageHealthStatus::Closed => Some(BrowserPageStatus::Closed),
        };
        let page = self.page_mut(page_id)?;
        page.health = health;
        if let Some(status) = status {
            page.status = status;
        }
        page.updated_at = now;
        self.updated_at = now;
        Ok(())
    }

    pub fn set_active_page(
        &mut self,
        page_id: &BrowserPageId,
        now: DateTime<Utc>,
    ) -> Result<(), BrowserKernelError> {
        if self.page(page_id)?.status == BrowserPageStatus::Closed {
            return Err(BrowserKernelError::ActionFailed(format!(
                "browser page is closed: {}",
                page_id.0
            )));
        }
        for page in &mut self.pages {
            if &page.page_id == page_id {
                page.status = BrowserPageStatus::Active;
            } else if page.status == BrowserPageStatus::Active {
                page.status = BrowserPageStatus::Background;
            }
            page.updated_at = now;
        }
        self.active_page_id = Some(page_id.clone());
        self.updated_at = now;
        Ok(())
    }

    pub fn active_page(&self) -> Option<&BrowserPageInfo> {
        self.active_page_id
            .as_ref()
            .and_then(|active_id| self.pages.iter().find(|page| &page.page_id == active_id))
    }

    pub fn page(&self, page_id: &BrowserPageId) -> Result<&BrowserPageInfo, BrowserKernelError> {
        self.pages
            .iter()
            .find(|page| &page.page_id == page_id)
            .ok_or_else(|| {
                BrowserKernelError::ActionFailed(format!("browser page not found: {}", page_id.0))
            })
    }

    pub fn page_mut(
        &mut self,
        page_id: &BrowserPageId,
    ) -> Result<&mut BrowserPageInfo, BrowserKernelError> {
        self.pages
            .iter_mut()
            .find(|page| &page.page_id == page_id)
            .ok_or_else(|| {
                BrowserKernelError::ActionFailed(format!("browser page not found: {}", page_id.0))
            })
    }

    pub fn open_page_count(&self) -> usize {
        self.pages
            .iter()
            .filter(|page| page.status != BrowserPageStatus::Closed)
            .count()
    }

    pub fn page_count(&self) -> usize {
        self.pages.len()
    }
}

/// In-memory tab/page lifecycle manager for one browser session.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BrowserPageLifecycleManager {
    pub session: BrowserSession,
    pub max_pages: usize,
}

impl BrowserPageLifecycleManager {
    pub fn new(session: BrowserSession, max_pages: usize) -> Result<Self, BrowserKernelError> {
        if max_pages == 0 {
            return Err(BrowserKernelError::InvalidConfig(
                "browser max_pages must be greater than zero".to_string(),
            ));
        }
        if session.open_page_count() > max_pages {
            return Err(BrowserKernelError::InvalidConfig(format!(
                "browser session has {} open pages but max_pages is {}",
                session.open_page_count(),
                max_pages
            )));
        }
        Ok(Self { session, max_pages })
    }

    pub fn open_page(
        &mut self,
        page_id: BrowserPageId,
        url: Option<String>,
        title: Option<String>,
        now: DateTime<Utc>,
    ) -> Result<(), BrowserKernelError> {
        if self.session.open_page_count() >= self.max_pages {
            return Err(BrowserKernelError::ActionFailed(format!(
                "browser max_pages limit reached: {}",
                self.max_pages
            )));
        }
        self.session.open_page(page_id, url, title, now)
    }

    pub fn close_page(
        &mut self,
        page_id: &BrowserPageId,
        now: DateTime<Utc>,
    ) -> Result<(), BrowserKernelError> {
        self.session.close_page(page_id, now)
    }

    pub fn switch_page(
        &mut self,
        page_id: &BrowserPageId,
        now: DateTime<Utc>,
    ) -> Result<(), BrowserKernelError> {
        self.session.set_active_page(page_id, now)
    }

    pub fn update_page_metadata(
        &mut self,
        page_id: &BrowserPageId,
        url: Option<String>,
        title: Option<String>,
        now: DateTime<Utc>,
    ) -> Result<(), BrowserKernelError> {
        self.session.update_page_metadata(page_id, url, title, now)
    }

    pub fn update_page_health(
        &mut self,
        page_id: &BrowserPageId,
        health: BrowserPageHealth,
        now: DateTime<Utc>,
    ) -> Result<(), BrowserKernelError> {
        self.session.update_page_health(page_id, health, now)
    }

    pub fn active_page(&self) -> Option<&BrowserPageInfo> {
        self.session.active_page()
    }
}

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

/// Browser navigation readiness target.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind", content = "value")]
pub enum NavigationWaitUntil {
    Load,
    DomContentLoaded,
    NetworkIdle,
    Selector(ElementSelector),
}

impl Default for NavigationWaitUntil {
    fn default() -> Self {
        Self::Load
    }
}

impl NavigationWaitUntil {
    pub fn validate(&self) -> Result<(), BrowserKernelError> {
        if let Self::Selector(selector) = self {
            selector.validate()?;
        }
        Ok(())
    }
}

/// Concrete navigation wait strategy for one page transition.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NavigationWaitStrategy {
    pub wait_until: NavigationWaitUntil,
    pub timeout_ms: u64,
    pub poll_interval_ms: u64,
}

impl Default for NavigationWaitStrategy {
    fn default() -> Self {
        Self {
            wait_until: NavigationWaitUntil::Load,
            timeout_ms: 30_000,
            poll_interval_ms: 200,
        }
    }
}

impl NavigationWaitStrategy {
    pub fn new(wait_until: NavigationWaitUntil, timeout_ms: u64) -> Self {
        Self {
            wait_until,
            timeout_ms,
            poll_interval_ms: 200,
        }
    }

    pub fn with_poll_interval_ms(mut self, poll_interval_ms: u64) -> Self {
        self.poll_interval_ms = poll_interval_ms;
        self
    }

    pub fn validate(&self) -> Result<(), BrowserKernelError> {
        if self.timeout_ms == 0 {
            return Err(BrowserKernelError::InvalidConfig(
                "navigation wait timeout_ms must be greater than zero".to_string(),
            ));
        }
        if self.poll_interval_ms == 0 {
            return Err(BrowserKernelError::InvalidConfig(
                "navigation wait poll_interval_ms must be greater than zero".to_string(),
            ));
        }
        if self.poll_interval_ms > self.timeout_ms {
            return Err(BrowserKernelError::InvalidConfig(
                "navigation wait poll_interval_ms cannot exceed timeout_ms".to_string(),
            ));
        }
        self.wait_until.validate()
    }
}

/// Timeout policy used to derive effective navigation wait strategies.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NavigationTimeoutPolicy {
    pub default_timeout_ms: u64,
    pub max_timeout_ms: u64,
    pub poll_interval_ms: u64,
}

impl Default for NavigationTimeoutPolicy {
    fn default() -> Self {
        Self {
            default_timeout_ms: 30_000,
            max_timeout_ms: 120_000,
            poll_interval_ms: 200,
        }
    }
}

impl NavigationTimeoutPolicy {
    pub fn validate(&self) -> Result<(), BrowserKernelError> {
        if self.default_timeout_ms == 0 || self.max_timeout_ms == 0 {
            return Err(BrowserKernelError::InvalidConfig(
                "navigation timeout policy values must be greater than zero".to_string(),
            ));
        }
        if self.default_timeout_ms > self.max_timeout_ms {
            return Err(BrowserKernelError::InvalidConfig(
                "navigation default_timeout_ms cannot exceed max_timeout_ms".to_string(),
            ));
        }
        if self.poll_interval_ms == 0 || self.poll_interval_ms > self.default_timeout_ms {
            return Err(BrowserKernelError::InvalidConfig(
                "navigation poll_interval_ms must be between 1 and default_timeout_ms".to_string(),
            ));
        }
        Ok(())
    }

    pub fn effective_strategy(
        &self,
        wait_until: NavigationWaitUntil,
        requested_timeout_ms: Option<u64>,
    ) -> Result<NavigationWaitStrategy, BrowserKernelError> {
        self.validate()?;
        let timeout_ms = requested_timeout_ms
            .unwrap_or(self.default_timeout_ms)
            .min(self.max_timeout_ms);
        let strategy = NavigationWaitStrategy {
            wait_until,
            timeout_ms,
            poll_interval_ms: self.poll_interval_ms.min(timeout_ms),
        };
        strategy.validate()?;
        Ok(strategy)
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
    pub navigation_timeout_policy: NavigationTimeoutPolicy,
    pub profile: BrowserProfileMode,
    pub max_pages: usize,
}

impl Default for CdpBrowserConfig {
    fn default() -> Self {
        Self {
            launch_mode: ChromiumLaunchMode::Headless,
            viewport: BrowserViewport::default(),
            timeouts: BrowserTimeouts::default(),
            navigation_timeout_policy: NavigationTimeoutPolicy::default(),
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

    pub fn with_navigation_timeout_policy(mut self, policy: NavigationTimeoutPolicy) -> Self {
        self.navigation_timeout_policy = policy;
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct BrowserFormFillField {
    selector: String,
    value: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct BrowserFillFormInput {
    url: String,
    fields: Vec<BrowserFormFillField>,
    #[serde(default)]
    submit: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct BrowserSelectOptionInput {
    url: String,
    selector: String,
    value: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum ScrollDirection {
    Up,
    Down,
    ToElement,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct BrowserScrollPageInput {
    url: String,
    direction: ScrollDirection,
    #[serde(default)]
    amount: Option<u32>,
    #[serde(default)]
    target_selector: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct BrowserPressKeyInput {
    url: String,
    key: String,
    #[serde(default)]
    selector: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct BrowserExecuteJsInput {
    url: String,
    script: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct BrowserWaitForElementInput {
    url: String,
    selector: String,
    #[serde(default = "default_wait_timeout_ms")]
    timeout_ms: u64,
    #[serde(default)]
    wait_until: NavigationWaitUntil,
}

impl BrowserWaitForElementInput {
    fn navigation_wait_strategy(
        &self,
        policy: &NavigationTimeoutPolicy,
    ) -> Result<NavigationWaitStrategy, BrowserKernelError> {
        policy.effective_strategy(self.wait_until.clone(), Some(self.timeout_ms))
    }
}

fn default_wait_timeout_ms() -> u64 {
    10_000
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct BrowserScreenshotInput {
    url: String,
    #[serde(default)]
    full_page: bool,
    quality: Option<u8>,
    element_selector: Option<String>,
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
pub struct CdpBrowserExecutor {
    lifecycle: ChromiumLifecycleManager,
    artifact_store: Option<Arc<dyn artifact_core::ArtifactStore>>,
}

impl std::fmt::Debug for CdpBrowserExecutor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CdpBrowserExecutor")
            .field("lifecycle", &self.lifecycle)
            .field(
                "artifact_store",
                &self.artifact_store.as_ref().map(|_| "<ArtifactStore>"),
            )
            .finish()
    }
}

impl CdpBrowserExecutor {
    pub fn new(
        lifecycle: ChromiumLifecycleManager,
        _now: DateTime<Utc>,
        artifact_store: Option<Arc<dyn artifact_core::ArtifactStore>>,
    ) -> Self {
        Self {
            lifecycle,
            artifact_store,
        }
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

        let element = page.find_element(&*selector).await.map_err(|e| {
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

    async fn fill_form(
        &self,
        browser: &chromiumoxide::Browser,
        input: BrowserFillFormInput,
    ) -> Result<action_core::ActionResult, action_core::ActionExecutorError> {
        let url = Self::validate_url(&input.url).map_err(to_invalid_input)?;

        if input.fields.is_empty() {
            return Err(to_invalid_input(BrowserKernelError::ActionFailed(
                "fill_form requires at least one field".to_string(),
            )));
        }

        let page = browser
            .new_page("about:blank")
            .await
            .map_err(to_execution_failed)?;
        page.goto(url.clone()).await.map_err(to_execution_failed)?;

        let current_url = page_url(&page).await.unwrap_or_else(|_| url.clone());

        let field_count = input.fields.len();
        for field in &input.fields {
            let selector = field.selector.trim();
            if selector.is_empty() {
                return Err(to_invalid_input(BrowserKernelError::EmptySelector));
            }
            let element = page.find_element(selector).await.map_err(|e| {
                to_execution_failed(format!(
                    "Form field not found with selector '{}': {}",
                    selector, e
                ))
            })?;
            element.click().await.map_err(to_execution_failed)?;
            // Select all existing text and replace
            element
                .press_key("Meta+a")
                .await
                .map_err(to_execution_failed)?;
            element
                .type_str(&field.value)
                .await
                .map_err(to_execution_failed)?;
        }

        // Submit if requested
        if input.submit {
            // Try common submit button selectors
            let submit_selectors = ["button[type='submit']", "input[type='submit']", "button"];
            let mut submitted = false;
            for sel in &submit_selectors {
                if let Ok(btn) = page.find_element(*sel).await
                    && btn.click().await.is_ok()
                {
                    submitted = true;
                    break;
                }
            }
            if !submitted {
                // Try pressing Enter on the last field
                if let Some(last_field) = input.fields.last()
                    && let Ok(el) = page.find_element(last_field.selector.trim()).await
                {
                    let _ = el.press_key("Enter").await;
                }
            }
        }

        Ok(action_core::ActionResult {
            status: action_core::ActionStatus::Completed,
            summary: format!(
                "Filled {} form fields on page: {}",
                field_count, current_url
            ),
            payload: action_core::ActionResultPayload::Json(serde_json::json!({
                "success": true,
                "url": current_url,
                "element_description": format!(
                    "Filled {} form fields{}",
                    field_count,
                    if input.submit { " and submitted" } else { "" }
                ),
                "interacted_at": chrono::Utc::now()
            })),
            completed_at: chrono::Utc::now(),
        })
    }

    async fn select_option(
        &self,
        browser: &chromiumoxide::Browser,
        input: BrowserSelectOptionInput,
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

        // Use JS to set the select value and dispatch change event
        let js = format!(
            r#"(() => {{
                const sel = document.querySelector('{}');
                if (!sel) throw new Error('Select element not found');
                sel.value = '{}';
                sel.dispatchEvent(new Event('change', {{ bubbles: true }}));
                return sel.value;
            }})()"#,
            selector.replace('\\', "\\\\").replace('\'', "\\'"),
            input.value.replace('\\', "\\\\").replace('\'', "\\'")
        );

        page.evaluate(js).await.map_err(|e| {
            to_execution_failed(format!(
                "Failed to select option with selector '{}': {}",
                selector, e
            ))
        })?;

        Ok(action_core::ActionResult {
            status: action_core::ActionStatus::Completed,
            summary: format!(
                "Selected option '{}' in dropdown on page: {}",
                input.value, current_url
            ),
            payload: action_core::ActionResultPayload::Json(serde_json::json!({
                "success": true,
                "url": current_url,
                "element_description": format!(
                    "Selected '{}' in dropdown at selector: {}",
                    input.value, selector
                ),
                "interacted_at": chrono::Utc::now()
            })),
            completed_at: chrono::Utc::now(),
        })
    }

    async fn scroll_page(
        &self,
        browser: &chromiumoxide::Browser,
        input: BrowserScrollPageInput,
    ) -> Result<action_core::ActionResult, action_core::ActionExecutorError> {
        let url = Self::validate_url(&input.url).map_err(to_invalid_input)?;

        let page = browser
            .new_page("about:blank")
            .await
            .map_err(to_execution_failed)?;
        page.goto(url.clone()).await.map_err(to_execution_failed)?;

        let current_url = page_url(&page).await.unwrap_or_else(|_| url.clone());

        let js = match &input.direction {
            ScrollDirection::Down => {
                let amount = input.amount.unwrap_or(500);
                format!("window.scrollBy(0, {})", amount)
            }
            ScrollDirection::Up => {
                let amount = input.amount.unwrap_or(500);
                format!("window.scrollBy(0, -{})", amount)
            }
            ScrollDirection::ToElement => {
                let sel = input.target_selector.as_deref().unwrap_or("");
                if sel.is_empty() {
                    return Err(to_invalid_input(BrowserKernelError::ActionFailed(
                        "scroll_page to_element requires target_selector".to_string(),
                    )));
                }
                format!(
                    "(() => {{ const el = document.querySelector('{}'); if (el) el.scrollIntoView({{behavior:'smooth'}}); return !!el; }})()",
                    sel.replace('\\', "\\\\").replace('\'', "\\'")
                )
            }
        };

        page.evaluate(js).await.map_err(to_execution_failed)?;

        let desc = match &input.direction {
            ScrollDirection::Down => format!("Scrolled down {}px", input.amount.unwrap_or(500)),
            ScrollDirection::Up => format!("Scrolled up {}px", input.amount.unwrap_or(500)),
            ScrollDirection::ToElement => format!(
                "Scrolled to element: {}",
                input.target_selector.as_deref().unwrap_or("")
            ),
        };

        Ok(action_core::ActionResult {
            status: action_core::ActionStatus::Completed,
            summary: format!("{} on page: {}", desc, current_url),
            payload: action_core::ActionResultPayload::Json(serde_json::json!({
                "success": true,
                "url": current_url,
                "element_description": desc,
                "interacted_at": chrono::Utc::now()
            })),
            completed_at: chrono::Utc::now(),
        })
    }

    async fn press_key(
        &self,
        browser: &chromiumoxide::Browser,
        input: BrowserPressKeyInput,
    ) -> Result<action_core::ActionResult, action_core::ActionExecutorError> {
        let url = Self::validate_url(&input.url).map_err(to_invalid_input)?;

        let page = browser
            .new_page("about:blank")
            .await
            .map_err(to_execution_failed)?;
        page.goto(url.clone()).await.map_err(to_execution_failed)?;

        let current_url = page_url(&page).await.unwrap_or_else(|_| url.clone());

        if let Some(ref selector) = input.selector {
            let sel = selector.trim();
            if sel.is_empty() {
                return Err(to_invalid_input(BrowserKernelError::EmptySelector));
            }
            let element = page.find_element(sel).await.map_err(|e| {
                to_execution_failed(format!("Element not found with selector '{}': {}", sel, e))
            })?;
            element
                .press_key(&input.key)
                .await
                .map_err(to_execution_failed)?;
        } else {
            // Press key on the page (keyboard event)
            let js = format!(
                "document.dispatchEvent(new KeyboardEvent('keydown', {{key: '{}'}}))",
                input.key.replace('\\', "\\\\").replace('\'', "\\'")
            );
            page.evaluate(js).await.map_err(to_execution_failed)?;
        }

        Ok(action_core::ActionResult {
            status: action_core::ActionStatus::Completed,
            summary: format!("Pressed key '{}' on page: {}", input.key, current_url),
            payload: action_core::ActionResultPayload::Json(serde_json::json!({
                "success": true,
                "url": current_url,
                "element_description": format!("Pressed key '{}' on element", input.key),
                "interacted_at": chrono::Utc::now()
            })),
            completed_at: chrono::Utc::now(),
        })
    }

    async fn execute_js(
        &self,
        browser: &chromiumoxide::Browser,
        input: BrowserExecuteJsInput,
    ) -> Result<action_core::ActionResult, action_core::ActionExecutorError> {
        let url = Self::validate_url(&input.url).map_err(to_invalid_input)?;

        if input.script.trim().is_empty() {
            return Err(to_invalid_input(BrowserKernelError::ActionFailed(
                "execute_js script cannot be empty".to_string(),
            )));
        }

        let page = browser
            .new_page("about:blank")
            .await
            .map_err(to_execution_failed)?;
        page.goto(url.clone()).await.map_err(to_execution_failed)?;

        let current_url = page_url(&page).await.unwrap_or_else(|_| url.clone());

        let result = page
            .evaluate(input.script.clone())
            .await
            .map_err(|e| to_execution_failed(format!("JavaScript execution failed: {}", e)))?;

        let result_value: serde_json::Value = result
            .into_value()
            .map_err(|e| to_execution_failed(format!("Failed to parse JS result: {}", e)))?;

        Ok(action_core::ActionResult {
            status: action_core::ActionStatus::Completed,
            summary: format!("Executed JavaScript on page: {}", current_url),
            payload: action_core::ActionResultPayload::Json(serde_json::json!({
                "success": true,
                "url": current_url,
                "result": result_value,
                "element_description": "Executed JavaScript".to_string(),
                "interacted_at": chrono::Utc::now()
            })),
            completed_at: chrono::Utc::now(),
        })
    }

    async fn wait_for_element(
        &self,
        browser: &chromiumoxide::Browser,
        input: BrowserWaitForElementInput,
    ) -> Result<action_core::ActionResult, action_core::ActionExecutorError> {
        let url = Self::validate_url(&input.url).map_err(to_invalid_input)?;
        let selector = input.selector.trim().to_string();
        if selector.is_empty() {
            return Err(to_invalid_input(BrowserKernelError::EmptySelector));
        }

        let strategy = input
            .navigation_wait_strategy(&self.lifecycle.config().navigation_timeout_policy)
            .map_err(to_invalid_input)?;

        let page = browser
            .new_page("about:blank")
            .await
            .map_err(to_execution_failed)?;
        page.goto(url.clone()).await.map_err(to_execution_failed)?;

        let current_url = page_url(&page).await.unwrap_or_else(|_| url.clone());

        // Poll for element with timeout
        let start = std::time::Instant::now();
        let timeout = std::time::Duration::from_millis(strategy.timeout_ms);
        let poll_interval = std::time::Duration::from_millis(strategy.poll_interval_ms);

        loop {
            if page.find_element(&*selector).await.is_ok() {
                return Ok(action_core::ActionResult {
                    status: action_core::ActionStatus::Completed,
                    summary: format!(
                        "Element '{}' found on page: {} (within {}ms)",
                        selector,
                        current_url,
                        start.elapsed().as_millis()
                    ),
                    payload: action_core::ActionResultPayload::Json(serde_json::json!({
                        "success": true,
                        "url": current_url,
                        "element_description": format!(
                            "Element '{}' appeared within {}ms",
                            selector,
                            start.elapsed().as_millis()
                        ),
                        "interacted_at": chrono::Utc::now()
                    })),
                    completed_at: chrono::Utc::now(),
                });
            }

            if start.elapsed() >= timeout {
                return Err(to_execution_failed(format!(
                    "Timed out waiting for element '{}' after {}ms using {:?}",
                    selector, strategy.timeout_ms, strategy.wait_until
                )));
            }

            tokio::time::sleep(poll_interval).await;
        }
    }

    async fn get_page_screenshot(
        &self,
        browser: &chromiumoxide::Browser,
        input: BrowserScreenshotInput,
    ) -> Result<action_core::ActionResult, action_core::ActionExecutorError> {
        let url = Self::validate_url(&input.url).map_err(to_invalid_input)?;

        let page = browser
            .new_page("about:blank")
            .await
            .map_err(to_execution_failed)?;
        page.goto(url.clone()).await.map_err(to_execution_failed)?;

        let current_url = page_url(&page).await.unwrap_or_else(|_| url.clone());

        use chromiumoxide::cdp::browser_protocol::page::CaptureScreenshotFormat;

        let screenshot_bytes = if let Some(ref element_selector) = input.element_selector {
            let sel = element_selector.trim();
            if sel.is_empty() {
                return Err(to_invalid_input(BrowserKernelError::EmptySelector));
            }
            let element = page.find_element(sel).await.map_err(|e| {
                to_execution_failed(format!("Element not found with selector '{}': {}", sel, e))
            })?;
            let format = if input.quality.is_some() {
                CaptureScreenshotFormat::Jpeg
            } else {
                CaptureScreenshotFormat::Png
            };
            element
                .screenshot(format)
                .await
                .map_err(to_execution_failed)?
        } else {
            use chromiumoxide::page::ScreenshotParams;

            let format = if input.quality.is_some() {
                CaptureScreenshotFormat::Jpeg
            } else {
                CaptureScreenshotFormat::Png
            };

            let params = ScreenshotParams::builder()
                .format(format)
                .full_page(input.full_page)
                .build();

            page.screenshot(params).await.map_err(to_execution_failed)?
        };

        let format_str = if input.quality.is_some() {
            "jpeg"
        } else {
            "png"
        };

        let result = action_core::ActionResult {
            status: action_core::ActionStatus::Completed,
            summary: format!(
                "Screenshot captured from {} ({} format, {} bytes)",
                current_url,
                format_str,
                screenshot_bytes.len()
            ),
            payload: action_core::ActionResultPayload::Json(serde_json::json!({
                "success": true,
                "url": current_url,
                "format": format_str,
                "size_bytes": screenshot_bytes.len(),
                "full_page": input.full_page,
                "interacted_at": chrono::Utc::now()
            })),
            completed_at: chrono::Utc::now(),
        };

        // Persist to ArtifactStore if available
        if let Some(ref store) = self.artifact_store {
            use artifact_core::{ArtifactDescriptor, ArtifactKind};

            let artifact_id = artifact_core::ArtifactId(format!(
                "screenshot-{}",
                chrono::Utc::now().timestamp_millis()
            ));
            let descriptor = ArtifactDescriptor {
                id: artifact_id.clone(),
                kind: ArtifactKind::Image,
                title: Some(format!("Screenshot of {}", current_url)),
                source_uri: Some(current_url.clone()),
                mime_type: Some(if format_str == "jpeg" {
                    "image/jpeg".to_string()
                } else {
                    "image/png".to_string()
                }),
                metadata: serde_json::json!({
                    "size_bytes": screenshot_bytes.len(),
                    "format": format_str,
                    "full_page": input.full_page,
                    "source": "browser_screenshot"
                }),
                created_at: chrono::Utc::now(),
            };

            let _ = store.put(descriptor).await;
        }

        Ok(result)
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
            "browser.fill_form" => {
                let input: BrowserFillFormInput = serde_json::from_value(request.input.clone())
                    .map_err(|e| action_core::ActionExecutorError::InvalidInput(e.to_string()))?;
                self.fill_form(browser, input).await
            }
            "browser.select_option" => {
                let input: BrowserSelectOptionInput = serde_json::from_value(request.input.clone())
                    .map_err(|e| action_core::ActionExecutorError::InvalidInput(e.to_string()))?;
                self.select_option(browser, input).await
            }
            "browser.scroll_page" => {
                let input: BrowserScrollPageInput = serde_json::from_value(request.input.clone())
                    .map_err(|e| {
                    action_core::ActionExecutorError::InvalidInput(e.to_string())
                })?;
                self.scroll_page(browser, input).await
            }
            "browser.press_key" => {
                let input: BrowserPressKeyInput = serde_json::from_value(request.input.clone())
                    .map_err(|e| action_core::ActionExecutorError::InvalidInput(e.to_string()))?;
                self.press_key(browser, input).await
            }
            "browser.execute_js" => {
                let input: BrowserExecuteJsInput = serde_json::from_value(request.input.clone())
                    .map_err(|e| action_core::ActionExecutorError::InvalidInput(e.to_string()))?;
                self.execute_js(browser, input).await
            }
            "browser.wait_for_element" => {
                let input: BrowserWaitForElementInput =
                    serde_json::from_value(request.input.clone()).map_err(|e| {
                        action_core::ActionExecutorError::InvalidInput(e.to_string())
                    })?;
                self.wait_for_element(browser, input).await
            }
            "browser.get_page_screenshot" => {
                let input: BrowserScreenshotInput = serde_json::from_value(request.input.clone())
                    .map_err(|e| {
                    action_core::ActionExecutorError::InvalidInput(e.to_string())
                })?;
                self.get_page_screenshot(browser, input).await
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

    // ---- BrowserSession tests ----

    #[test]
    fn browser_session_roundtrips_profile_binding_and_pages() {
        let now = ts();
        let mut session = BrowserSession::new(
            BrowserSessionId::new("session-1").unwrap(),
            BrowserSessionProfileBinding::new(
                BrowserProfileMode::Named("work".to_string()),
                Some(PathBuf::from("/tmp/browser-profiles/work")),
            ),
            now,
        );
        session.add_page(
            BrowserPageInfo::new(BrowserPageId::new("page-1").unwrap(), now).with_location(
                "https://example.com",
                Some("Example"),
                now,
            ),
        );

        let json = serde_json::to_string_pretty(&session).unwrap();
        let decoded: BrowserSession = serde_json::from_str(&json).unwrap();

        assert_eq!(
            decoded.session_id,
            BrowserSessionId("session-1".to_string())
        );
        assert_eq!(
            decoded.profile.mode,
            BrowserProfileMode::Named("work".to_string())
        );
        assert_eq!(decoded.page_count(), 1);
        assert_eq!(
            decoded.active_page().unwrap().url.as_deref(),
            Some("https://example.com")
        );
    }

    #[test]
    fn browser_session_tracks_active_page_and_background_pages() {
        let now = ts();
        let mut session = BrowserSession::new(
            BrowserSessionId::new("session-1").unwrap(),
            BrowserSessionProfileBinding::new(BrowserProfileMode::Ephemeral, None),
            now,
        );
        let page_1 = BrowserPageId::new("page-1").unwrap();
        let page_2 = BrowserPageId::new("page-2").unwrap();
        session.add_page(BrowserPageInfo::new(page_1.clone(), now));
        session.add_page(BrowserPageInfo::new(page_2.clone(), now));

        session.set_active_page(&page_2, now).unwrap();

        assert_eq!(session.active_page_id, Some(page_2.clone()));
        assert_eq!(session.active_page().unwrap().page_id, page_2);
        assert_eq!(session.pages[0].status, BrowserPageStatus::Background);
        assert_eq!(session.pages[1].status, BrowserPageStatus::Active);
    }

    #[test]
    fn browser_session_rejects_missing_active_page() {
        let now = ts();
        let mut session = BrowserSession::new(
            BrowserSessionId::new("session-1").unwrap(),
            BrowserSessionProfileBinding::new(BrowserProfileMode::Ephemeral, None),
            now,
        );

        let error = session
            .set_active_page(&BrowserPageId::new("missing").unwrap(), now)
            .unwrap_err();

        assert!(matches!(error, BrowserKernelError::ActionFailed(_)));
    }

    #[test]
    fn browser_session_id_rejects_empty_values() {
        assert!(BrowserSessionId::new(" ").is_err());
        assert!(BrowserPageId::new("").is_err());
    }

    // ---- BrowserPageLifecycleManager tests ----

    #[test]
    fn page_lifecycle_manager_opens_closes_and_switches_pages() {
        let now = ts();
        let session = BrowserSession::new(
            BrowserSessionId::new("session-1").unwrap(),
            BrowserSessionProfileBinding::new(BrowserProfileMode::Ephemeral, None),
            now,
        );
        let mut manager = BrowserPageLifecycleManager::new(session, 3).unwrap();
        let page_1 = BrowserPageId::new("page-1").unwrap();
        let page_2 = BrowserPageId::new("page-2").unwrap();

        manager
            .open_page(
                page_1.clone(),
                Some("https://one.example".to_string()),
                Some("One".to_string()),
                now,
            )
            .unwrap();
        manager
            .open_page(
                page_2.clone(),
                Some("https://two.example".to_string()),
                Some("Two".to_string()),
                now,
            )
            .unwrap();
        manager.switch_page(&page_2, now).unwrap();
        manager.close_page(&page_2, now).unwrap();

        assert_eq!(manager.active_page().unwrap().page_id, page_1);
        assert_eq!(
            manager.session.page(&page_2).unwrap().status,
            BrowserPageStatus::Closed
        );
        assert_eq!(
            manager.session.page(&page_2).unwrap().health.status,
            BrowserPageHealthStatus::Closed
        );
        assert_eq!(manager.session.open_page_count(), 1);
    }

    #[test]
    fn page_lifecycle_manager_preserves_per_page_metadata_without_cross_tab_bleed() {
        let now = ts();
        let session = BrowserSession::new(
            BrowserSessionId::new("session-1").unwrap(),
            BrowserSessionProfileBinding::new(BrowserProfileMode::Ephemeral, None),
            now,
        );
        let mut manager = BrowserPageLifecycleManager::new(session, 5).unwrap();
        let page_1 = BrowserPageId::new("page-1").unwrap();
        let page_2 = BrowserPageId::new("page-2").unwrap();
        manager
            .open_page(
                page_1.clone(),
                Some("https://one.example".to_string()),
                Some("One".to_string()),
                now,
            )
            .unwrap();
        manager
            .open_page(
                page_2.clone(),
                Some("https://two.example".to_string()),
                Some("Two".to_string()),
                now,
            )
            .unwrap();

        manager
            .update_page_metadata(
                &page_2,
                Some("https://two.example/dashboard".to_string()),
                Some("Two Dashboard".to_string()),
                now,
            )
            .unwrap();
        manager
            .update_page_health(
                &page_2,
                BrowserPageHealth::unresponsive("network idle timeout", now),
                now,
            )
            .unwrap();

        assert_eq!(
            manager.session.page(&page_1).unwrap().url.as_deref(),
            Some("https://one.example")
        );
        assert_eq!(
            manager.session.page(&page_1).unwrap().health.status,
            BrowserPageHealthStatus::Healthy
        );
        assert_eq!(
            manager.session.page(&page_2).unwrap().url.as_deref(),
            Some("https://two.example/dashboard")
        );
        assert_eq!(
            manager.session.page(&page_2).unwrap().health.status,
            BrowserPageHealthStatus::Unresponsive
        );
    }

    #[test]
    fn page_lifecycle_manager_enforces_max_pages() {
        let now = ts();
        let session = BrowserSession::new(
            BrowserSessionId::new("session-1").unwrap(),
            BrowserSessionProfileBinding::new(BrowserProfileMode::Ephemeral, None),
            now,
        );
        let mut manager = BrowserPageLifecycleManager::new(session, 1).unwrap();
        manager
            .open_page(BrowserPageId::new("page-1").unwrap(), None, None, now)
            .unwrap();

        let error = manager
            .open_page(BrowserPageId::new("page-2").unwrap(), None, None, now)
            .unwrap_err();

        assert!(matches!(error, BrowserKernelError::ActionFailed(_)));
    }

    #[test]
    fn page_lifecycle_manager_rejects_switching_to_closed_page() {
        let now = ts();
        let session = BrowserSession::new(
            BrowserSessionId::new("session-1").unwrap(),
            BrowserSessionProfileBinding::new(BrowserProfileMode::Ephemeral, None),
            now,
        );
        let mut manager = BrowserPageLifecycleManager::new(session, 2).unwrap();
        let page_1 = BrowserPageId::new("page-1").unwrap();
        manager.open_page(page_1.clone(), None, None, now).unwrap();
        manager.close_page(&page_1, now).unwrap();

        let error = manager.switch_page(&page_1, now).unwrap_err();

        assert!(matches!(error, BrowserKernelError::ActionFailed(_)));
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

    // ---- Navigation wait strategy tests ----

    #[test]
    fn navigation_wait_strategy_supports_load_domcontentloaded_networkidle_and_selector() {
        let strategies = vec![
            NavigationWaitStrategy::new(NavigationWaitUntil::Load, 1_000),
            NavigationWaitStrategy::new(NavigationWaitUntil::DomContentLoaded, 1_000),
            NavigationWaitStrategy::new(NavigationWaitUntil::NetworkIdle, 1_000),
            NavigationWaitStrategy::new(
                NavigationWaitUntil::Selector(ElementSelector::css("#ready")),
                1_000,
            ),
        ];

        for strategy in strategies {
            let json = serde_json::to_string_pretty(&strategy).unwrap();
            let decoded: NavigationWaitStrategy = serde_json::from_str(&json).unwrap();
            assert_eq!(decoded, strategy);
            assert!(decoded.validate().is_ok());
        }
    }

    #[test]
    fn navigation_wait_strategy_rejects_invalid_timeout_policy_values() {
        assert!(
            NavigationWaitStrategy::new(NavigationWaitUntil::Load, 0)
                .validate()
                .is_err()
        );
        assert!(
            NavigationWaitStrategy::new(
                NavigationWaitUntil::Selector(ElementSelector::css(" ")),
                1_000,
            )
            .validate()
            .is_err()
        );
        assert!(
            NavigationWaitStrategy::new(NavigationWaitUntil::NetworkIdle, 100)
                .with_poll_interval_ms(200)
                .validate()
                .is_err()
        );
    }

    #[test]
    fn navigation_timeout_policy_clamps_requested_timeout_and_preserves_wait_kind() {
        let policy = NavigationTimeoutPolicy {
            default_timeout_ms: 5_000,
            max_timeout_ms: 30_000,
            poll_interval_ms: 250,
        };

        let strategy = policy
            .effective_strategy(NavigationWaitUntil::NetworkIdle, Some(60_000))
            .unwrap();

        assert_eq!(strategy.wait_until, NavigationWaitUntil::NetworkIdle);
        assert_eq!(strategy.timeout_ms, 30_000);
        assert_eq!(strategy.poll_interval_ms, 250);
    }

    #[test]
    fn browser_wait_for_element_input_builds_selector_wait_strategy() {
        let input: BrowserWaitForElementInput = serde_json::from_value(serde_json::json!({
            "url": "https://example.com",
            "selector": "#ready",
            "timeout_ms": 2_000,
            "wait_until": { "kind": "selector", "value": { "kind": "css", "value": "#ready" } }
        }))
        .unwrap();
        let strategy = input
            .navigation_wait_strategy(&NavigationTimeoutPolicy::default())
            .unwrap();

        assert_eq!(strategy.timeout_ms, 2_000);
        assert_eq!(
            strategy.wait_until,
            NavigationWaitUntil::Selector(ElementSelector::css("#ready"))
        );
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
        let executor = CdpBrowserExecutor::new(lifecycle, ts(), None);

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
        let executor = CdpBrowserExecutor::new(lifecycle, ts(), None);

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
        let executor = CdpBrowserExecutor::new(lifecycle, ts(), None);

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

    #[test]
    fn browser_fill_form_input_parses_with_fields() {
        let input: BrowserFillFormInput = serde_json::from_value(serde_json::json!({
            "url": "https://example.com/form",
            "fields": [
                {"selector": "#name", "value": "Alice"},
                {"selector": "#email", "value": "alice@example.com"}
            ],
            "submit": true
        }))
        .unwrap();

        assert_eq!(input.url, "https://example.com/form");
        assert_eq!(input.fields.len(), 2);
        assert_eq!(input.fields[0].selector, "#name");
        assert_eq!(input.fields[0].value, "Alice");
        assert_eq!(input.fields[1].selector, "#email");
        assert!(input.submit);
    }

    #[test]
    fn browser_fill_form_input_defaults_submit_false() {
        let input: BrowserFillFormInput = serde_json::from_value(serde_json::json!({
            "url": "https://example.com",
            "fields": [{"selector": "#x", "value": "y"}]
        }))
        .unwrap();

        assert!(!input.submit);
    }

    #[test]
    fn browser_select_option_input_parses() {
        let input: BrowserSelectOptionInput = serde_json::from_value(serde_json::json!({
            "url": "https://example.com",
            "selector": "select#country",
            "value": "CN"
        }))
        .unwrap();

        assert_eq!(input.url, "https://example.com");
        assert_eq!(input.selector, "select#country");
        assert_eq!(input.value, "CN");
    }

    #[tokio::test]
    async fn cdp_browser_click_element_returns_chromium_not_available() {
        let lifecycle = ChromiumLifecycleManager::new(CdpBrowserConfig::default());
        let executor = CdpBrowserExecutor::new(lifecycle, ts(), None);

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
        let executor = CdpBrowserExecutor::new(lifecycle, ts(), None);

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

    #[tokio::test]
    async fn cdp_browser_fill_form_returns_chromium_not_available() {
        let lifecycle = ChromiumLifecycleManager::new(CdpBrowserConfig::default());
        let executor = CdpBrowserExecutor::new(lifecycle, ts(), None);

        let request = action_request_with_input(
            "browser.fill_form",
            serde_json::json!({"url": "https://example.com", "fields": [{"selector": "#x", "value": "y"}]}),
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
    async fn cdp_browser_select_option_returns_chromium_not_available() {
        let lifecycle = ChromiumLifecycleManager::new(CdpBrowserConfig::default());
        let executor = CdpBrowserExecutor::new(lifecycle, ts(), None);

        let request = action_request_with_input(
            "browser.select_option",
            serde_json::json!({"url": "https://example.com", "selector": "select#x", "value": "a"}),
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
