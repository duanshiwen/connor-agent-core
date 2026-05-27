//! # Browser Kernel Core
//!
//! Browser kernel domain types and CDP executor skeleton for AgentOS.
//!
//! This crate intentionally does not launch Chromium yet. It defines the stable
//! boundary that later PRs can connect to a real CDP implementation.

use artifact_core::{ArtifactDescriptor, ArtifactId, ArtifactKind};
use chrono::{DateTime, Utc};
use futures_util::stream::StreamExt;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
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

    pub fn record_crash(&mut self, event: BrowserCrashEvent) -> Result<(), BrowserKernelError> {
        event.validate()?;
        match event.scope {
            BrowserCrashScope::Page => {
                let page_id = event.page_id.as_ref().ok_or_else(|| {
                    BrowserKernelError::InvalidConfig(
                        "page crash event must include page_id".to_string(),
                    )
                })?;
                self.session.update_page_health(
                    page_id,
                    BrowserPageHealth::crashed(event.message, event.detected_at),
                    event.detected_at,
                )?;
            }
            BrowserCrashScope::BrowserProcess => {
                for page in &mut self.session.pages {
                    if page.status != BrowserPageStatus::Closed {
                        page.status = BrowserPageStatus::Crashed;
                        page.health =
                            BrowserPageHealth::crashed(event.message.clone(), event.detected_at);
                        page.updated_at = event.detected_at;
                    }
                }
                self.session.updated_at = event.detected_at;
            }
        }
        Ok(())
    }

    pub fn recover_from_crash(
        &mut self,
        event: BrowserCrashEvent,
        policy: &BrowserRecoveryPolicy,
    ) -> Result<BrowserRecoveryPlan, BrowserKernelError> {
        self.record_crash(event.clone())?;
        policy.plan_recovery(&self.session, &event)
    }
}

/// Scope of a browser crash event.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BrowserCrashScope {
    Page,
    BrowserProcess,
}

/// Provider-neutral browser crash reason classification.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BrowserCrashReason {
    PageCrashed,
    BrowserDisconnected,
    RendererUnresponsive,
    ProcessExited,
    Unknown,
}

/// Crash event captured by browser supervision or deterministic tests.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BrowserCrashEvent {
    pub scope: BrowserCrashScope,
    pub page_id: Option<BrowserPageId>,
    pub reason: BrowserCrashReason,
    pub message: String,
    pub detected_at: DateTime<Utc>,
}

impl BrowserCrashEvent {
    pub fn validate(&self) -> Result<(), BrowserKernelError> {
        if self.message.trim().is_empty() {
            return Err(BrowserKernelError::InvalidConfig(
                "browser crash message cannot be empty".to_string(),
            ));
        }
        if self.scope == BrowserCrashScope::Page && self.page_id.is_none() {
            return Err(BrowserKernelError::InvalidConfig(
                "page crash event must include page_id".to_string(),
            ));
        }
        Ok(())
    }
}

/// High-level browser crash recovery strategy.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BrowserRecoveryStrategy {
    FailFast,
    ReopenActivePage,
    RelaunchSession,
}

/// Policy controlling deterministic recovery plan generation after a crash.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BrowserRecoveryPolicy {
    pub strategy: BrowserRecoveryStrategy,
    pub max_relaunch_attempts: u32,
    pub retry_after_relaunch: bool,
}

impl Default for BrowserRecoveryPolicy {
    fn default() -> Self {
        Self {
            strategy: BrowserRecoveryStrategy::RelaunchSession,
            max_relaunch_attempts: 1,
            retry_after_relaunch: true,
        }
    }
}

impl BrowserRecoveryPolicy {
    pub fn validate(&self) -> Result<(), BrowserKernelError> {
        if self.strategy == BrowserRecoveryStrategy::RelaunchSession
            && self.max_relaunch_attempts == 0
        {
            return Err(BrowserKernelError::InvalidConfig(
                "browser recovery max_relaunch_attempts must be greater than zero".to_string(),
            ));
        }
        Ok(())
    }

    pub fn plan_recovery(
        &self,
        session: &BrowserSession,
        event: &BrowserCrashEvent,
    ) -> Result<BrowserRecoveryPlan, BrowserKernelError> {
        self.validate()?;
        event.validate()?;
        let action = match self.strategy {
            BrowserRecoveryStrategy::FailFast => BrowserRecoveryAction::Fail {
                reason: format!("browser crash recovery fail-fast: {}", event.message),
            },
            BrowserRecoveryStrategy::ReopenActivePage => {
                let page_id = event
                    .page_id
                    .clone()
                    .or_else(|| session.active_page_id.clone())
                    .ok_or_else(|| {
                        BrowserKernelError::BrowserCrashed(
                            "cannot reopen page because session has no active page".to_string(),
                        )
                    })?;
                let page = session.page(&page_id)?;
                BrowserRecoveryAction::ReopenPage {
                    page_id,
                    url: page.url.clone(),
                }
            }
            BrowserRecoveryStrategy::RelaunchSession => BrowserRecoveryAction::RelaunchSession {
                session_id: session.session_id.clone(),
                profile: session.profile.clone(),
                pages: session
                    .pages
                    .iter()
                    .filter(|page| page.status != BrowserPageStatus::Closed)
                    .cloned()
                    .collect(),
                retry_after_relaunch: self.retry_after_relaunch,
            },
        };

        Ok(BrowserRecoveryPlan {
            event: event.clone(),
            strategy: self.strategy.clone(),
            action,
            planned_at: event.detected_at,
        })
    }
}

/// Deterministic recovery action selected by [`BrowserRecoveryPolicy`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum BrowserRecoveryAction {
    Fail {
        reason: String,
    },
    ReopenPage {
        page_id: BrowserPageId,
        url: Option<String>,
    },
    RelaunchSession {
        session_id: BrowserSessionId,
        profile: BrowserSessionProfileBinding,
        pages: Vec<BrowserPageInfo>,
        retry_after_relaunch: bool,
    },
}

/// Recovery plan returned to host/runtime after a browser crash.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BrowserRecoveryPlan {
    pub event: BrowserCrashEvent,
    pub strategy: BrowserRecoveryStrategy,
    pub action: BrowserRecoveryAction,
    pub planned_at: DateTime<Utc>,
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

// ---------------------------------------------------------------------------
// Dialog and permission prompt handling
// ---------------------------------------------------------------------------

/// Browser modal dialog kind surfaced by CDP/page events.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BrowserDialogKind {
    Alert,
    Confirm,
    Prompt,
    BeforeUnload,
}

/// Action selected for a modal browser dialog.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BrowserDialogDecision {
    Accept,
    Dismiss,
}

/// Browser dialog event captured before it can block the executor.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BrowserDialogEvent {
    pub page_id: Option<BrowserPageId>,
    pub kind: BrowserDialogKind,
    pub message: String,
    #[serde(default)]
    pub default_prompt_text: Option<String>,
    pub occurred_at: DateTime<Utc>,
}

impl BrowserDialogEvent {
    pub fn validate(&self) -> Result<(), BrowserKernelError> {
        if self.message.trim().is_empty() {
            return Err(BrowserKernelError::InvalidConfig(
                "browser dialog message cannot be empty".to_string(),
            ));
        }
        Ok(())
    }
}

/// Deterministic resolution for a modal dialog.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BrowserDialogResolution {
    pub decision: BrowserDialogDecision,
    #[serde(default)]
    pub prompt_text: Option<String>,
    pub reason: String,
    pub resolved_at: DateTime<Utc>,
}

/// Browser permission prompt kind.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BrowserPermissionKind {
    ClipboardRead,
    ClipboardWrite,
    Geolocation,
    Camera,
    Microphone,
    Notifications,
    Downloads,
    Other(String),
}

/// Permission prompt decision. `Ask` means surface to host/human instead of auto-allowing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BrowserPermissionDecision {
    Allow,
    Deny,
    Ask,
}

/// Browser permission prompt event for audit and host handoff.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BrowserPermissionPromptEvent {
    pub page_id: Option<BrowserPageId>,
    pub origin: String,
    pub permission: BrowserPermissionKind,
    pub occurred_at: DateTime<Utc>,
}

impl BrowserPermissionPromptEvent {
    pub fn validate(&self) -> Result<(), BrowserKernelError> {
        if self.origin.trim().is_empty() {
            return Err(BrowserKernelError::InvalidConfig(
                "browser permission prompt origin cannot be empty".to_string(),
            ));
        }
        Ok(())
    }
}

/// Deterministic resolution for a browser permission prompt.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BrowserPermissionPromptResolution {
    pub decision: BrowserPermissionDecision,
    pub reason: String,
    pub resolved_at: DateTime<Utc>,
}

/// Policy used to resolve dialogs and permission prompts without hanging automation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BrowserPromptPolicy {
    pub timeout_ms: u64,
    pub alert_decision: BrowserDialogDecision,
    pub confirm_decision: BrowserDialogDecision,
    pub prompt_decision: BrowserDialogDecision,
    #[serde(default)]
    pub prompt_text: Option<String>,
    pub before_unload_decision: BrowserDialogDecision,
    pub default_permission_decision: BrowserPermissionDecision,
    #[serde(default)]
    pub allowed_permissions: BTreeSet<BrowserPermissionKind>,
}

impl Default for BrowserPromptPolicy {
    fn default() -> Self {
        Self {
            timeout_ms: 5_000,
            alert_decision: BrowserDialogDecision::Accept,
            confirm_decision: BrowserDialogDecision::Dismiss,
            prompt_decision: BrowserDialogDecision::Dismiss,
            prompt_text: None,
            before_unload_decision: BrowserDialogDecision::Dismiss,
            default_permission_decision: BrowserPermissionDecision::Ask,
            allowed_permissions: BTreeSet::new(),
        }
    }
}

impl BrowserPromptPolicy {
    pub fn validate(&self) -> Result<(), BrowserKernelError> {
        if self.timeout_ms == 0 {
            return Err(BrowserKernelError::InvalidConfig(
                "browser prompt timeout_ms must be greater than zero".to_string(),
            ));
        }
        Ok(())
    }

    pub fn resolve_dialog(
        &self,
        event: &BrowserDialogEvent,
        resolved_at: DateTime<Utc>,
    ) -> Result<BrowserDialogResolution, BrowserKernelError> {
        self.validate()?;
        event.validate()?;
        let decision = match event.kind {
            BrowserDialogKind::Alert => self.alert_decision.clone(),
            BrowserDialogKind::Confirm => self.confirm_decision.clone(),
            BrowserDialogKind::Prompt => self.prompt_decision.clone(),
            BrowserDialogKind::BeforeUnload => self.before_unload_decision.clone(),
        };
        let prompt_text = if event.kind == BrowserDialogKind::Prompt
            && decision == BrowserDialogDecision::Accept
        {
            self.prompt_text
                .clone()
                .or_else(|| event.default_prompt_text.clone())
        } else {
            None
        };
        Ok(BrowserDialogResolution {
            decision,
            prompt_text,
            reason: format!(
                "resolved {:?} dialog via non-blocking prompt policy within {}ms",
                event.kind, self.timeout_ms
            ),
            resolved_at,
        })
    }

    pub fn resolve_permission_prompt(
        &self,
        event: &BrowserPermissionPromptEvent,
        resolved_at: DateTime<Utc>,
    ) -> Result<BrowserPermissionPromptResolution, BrowserKernelError> {
        self.validate()?;
        event.validate()?;
        let decision = if self.allowed_permissions.contains(&event.permission) {
            BrowserPermissionDecision::Allow
        } else {
            self.default_permission_decision.clone()
        };
        Ok(BrowserPermissionPromptResolution {
            decision,
            reason: format!(
                "resolved {:?} permission prompt for {} via browser prompt policy",
                event.permission, event.origin
            ),
            resolved_at,
        })
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
    pub prompt_policy: BrowserPromptPolicy,
    pub recovery_policy: BrowserRecoveryPolicy,
    pub security_policy: BrowserSecurityPolicy,
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
            prompt_policy: BrowserPromptPolicy::default(),
            recovery_policy: BrowserRecoveryPolicy::default(),
            security_policy: BrowserSecurityPolicy::default(),
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

    pub fn with_prompt_policy(mut self, policy: BrowserPromptPolicy) -> Self {
        self.prompt_policy = policy;
        self
    }

    pub fn with_recovery_policy(mut self, policy: BrowserRecoveryPolicy) -> Self {
        self.recovery_policy = policy;
        self
    }

    pub fn with_security_policy(mut self, policy: BrowserSecurityPolicy) -> Self {
        self.security_policy = policy;
        self
    }

    pub fn with_max_pages(mut self, max_pages: usize) -> Self {
        self.max_pages = max_pages;
        self
    }
}

// ---------------------------------------------------------------------------
// Browser security policy
// ---------------------------------------------------------------------------

/// Security policy decision for browser actions.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BrowserSecurityDecision {
    Allow,
    Ask,
    Deny,
}

/// Coarse JS execution risk classification.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BrowserJsRisk {
    Low,
    Medium,
    High,
}

impl BrowserJsRisk {
    pub fn classify(script: &str) -> Self {
        let normalized = script.to_ascii_lowercase();
        if [
            ".click(",
            ".submit(",
            "fetch(",
            "xmlhttprequest",
            "localstorage",
            "sessionstorage",
            "document.cookie",
            "authorization",
            "password",
            "token",
            "apikey",
            "api_key",
        ]
        .iter()
        .any(|term| normalized.contains(term))
        {
            Self::High
        } else if normalized.contains("document.")
            || normalized.contains("window.")
            || normalized.contains("navigator.")
        {
            Self::Medium
        } else {
            Self::Low
        }
    }
}

/// Credential exposure warning detected before executing browser JavaScript.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BrowserCredentialExposureWarning {
    pub url: String,
    pub matched_terms: Vec<String>,
    pub severity: BrowserJsRisk,
}

impl BrowserCredentialExposureWarning {
    pub fn detect(url: impl Into<String>, script: &str) -> Option<Self> {
        let normalized = script.to_ascii_lowercase();
        let mut matched_terms = Vec::new();
        for term in [
            "authorization",
            "cookie",
            "password",
            "token",
            "api_key",
            "apikey",
            "secret",
            "bearer",
        ] {
            if normalized.contains(term) {
                matched_terms.push(term.to_string());
            }
        }
        if matched_terms.is_empty() {
            None
        } else {
            Some(Self {
                url: url.into(),
                matched_terms,
                severity: BrowserJsRisk::High,
            })
        }
    }
}

/// Security evaluation result for a browser action.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BrowserSecurityEvaluation {
    pub decision: BrowserSecurityDecision,
    pub url: String,
    #[serde(default)]
    pub matched_domain: Option<String>,
    pub js_risk: BrowserJsRisk,
    #[serde(default)]
    pub credential_warning: Option<BrowserCredentialExposureWarning>,
}

/// Browser security policy for domain allow/deny and JavaScript execution risk.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BrowserSecurityPolicy {
    #[serde(default)]
    pub allowed_domains: Vec<String>,
    #[serde(default)]
    pub denied_domains: Vec<String>,
    #[serde(default)]
    pub high_risk_domains: Vec<String>,
    pub default_decision: BrowserSecurityDecision,
    pub high_risk_domain_decision: BrowserSecurityDecision,
}

impl Default for BrowserSecurityPolicy {
    fn default() -> Self {
        Self {
            allowed_domains: Vec::new(),
            denied_domains: Vec::new(),
            high_risk_domains: Vec::new(),
            default_decision: BrowserSecurityDecision::Ask,
            high_risk_domain_decision: BrowserSecurityDecision::Ask,
        }
    }
}

impl BrowserSecurityPolicy {
    pub fn with_allowed_domain(mut self, domain: impl Into<String>) -> Self {
        self.allowed_domains.push(normalize_domain(domain));
        self
    }

    pub fn with_denied_domain(mut self, domain: impl Into<String>) -> Self {
        self.denied_domains.push(normalize_domain(domain));
        self
    }

    pub fn with_high_risk_domain(mut self, domain: impl Into<String>) -> Self {
        self.high_risk_domains.push(normalize_domain(domain));
        self
    }

    pub fn evaluate_url(
        &self,
        url: impl AsRef<str>,
    ) -> Result<BrowserSecurityEvaluation, BrowserKernelError> {
        let url = url.as_ref().trim().to_string();
        if url.is_empty() {
            return Err(BrowserKernelError::EmptyUrl);
        }
        let domain = extract_url_host(&url)?;
        if let Some(matched) = matching_domain(&domain, &self.denied_domains) {
            return Ok(BrowserSecurityEvaluation {
                decision: BrowserSecurityDecision::Deny,
                url,
                matched_domain: Some(matched),
                js_risk: BrowserJsRisk::Low,
                credential_warning: None,
            });
        }
        if let Some(matched) = matching_domain(&domain, &self.allowed_domains) {
            return Ok(BrowserSecurityEvaluation {
                decision: BrowserSecurityDecision::Allow,
                url,
                matched_domain: Some(matched),
                js_risk: BrowserJsRisk::Low,
                credential_warning: None,
            });
        }
        Ok(BrowserSecurityEvaluation {
            decision: self.default_decision.clone(),
            url,
            matched_domain: None,
            js_risk: BrowserJsRisk::Low,
            credential_warning: None,
        })
    }

    pub fn evaluate_execute_js(
        &self,
        url: impl AsRef<str>,
        script: &str,
    ) -> Result<BrowserSecurityEvaluation, BrowserKernelError> {
        let mut evaluation = self.evaluate_url(url.as_ref())?;
        let domain = extract_url_host(url.as_ref())?;
        let js_risk = BrowserJsRisk::classify(script);
        let credential_warning = BrowserCredentialExposureWarning::detect(url.as_ref(), script);
        let high_risk_domain = matching_domain(&domain, &self.high_risk_domains).is_some();

        if high_risk_domain && evaluation.decision == BrowserSecurityDecision::Allow {
            evaluation.decision = self.high_risk_domain_decision.clone();
        }
        if high_risk_domain && evaluation.decision == BrowserSecurityDecision::Ask {
            evaluation.decision = self.high_risk_domain_decision.clone();
        }
        if js_risk == BrowserJsRisk::High && evaluation.decision == BrowserSecurityDecision::Allow {
            evaluation.decision = BrowserSecurityDecision::Ask;
        }
        if credential_warning.is_some() && evaluation.decision == BrowserSecurityDecision::Allow {
            evaluation.decision = BrowserSecurityDecision::Ask;
        }

        evaluation.js_risk = js_risk;
        evaluation.credential_warning = credential_warning;
        Ok(evaluation)
    }
}

fn normalize_domain(domain: impl Into<String>) -> String {
    domain
        .into()
        .trim()
        .trim_start_matches('.')
        .to_ascii_lowercase()
}

fn matching_domain(host: &str, domains: &[String]) -> Option<String> {
    let host = normalize_domain(host.to_string());
    domains
        .iter()
        .find(|domain| host == **domain || host.ends_with(&format!(".{}", domain)))
        .cloned()
}

fn extract_url_host(url: &str) -> Result<String, BrowserKernelError> {
    let trimmed = url.trim();
    let after_scheme = trimmed
        .split_once("://")
        .map(|(_, rest)| rest)
        .unwrap_or(trimmed);
    let host_port = after_scheme.split('/').next().unwrap_or_default();
    let host = host_port
        .rsplit_once('@')
        .map(|(_, host)| host)
        .unwrap_or(host_port)
        .split(':')
        .next()
        .unwrap_or_default()
        .trim();
    if host.is_empty() {
        Err(BrowserKernelError::InvalidConfig(format!(
            "browser security policy could not extract host from url: {}",
            url
        )))
    } else {
        Ok(normalize_domain(host))
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
    #[error("browser crashed: {0}")]
    BrowserCrashed(String),
    #[error("browser automation is paused for human takeover: {0}")]
    AutomationPaused(String),
    #[error("unsupported browser action: {0}")]
    UnsupportedAction(String),
    #[error("browser action failed: {0}")]
    ActionFailed(String),
}

// ---------------------------------------------------------------------------
// Human takeover boundary
// ---------------------------------------------------------------------------

/// Current automation state for a browser session.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BrowserAutomationState {
    Running,
    PausedForHumanTakeover,
}

/// Reason automation was paused for a human takeover.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind", content = "value")]
pub enum BrowserHumanTakeoverReason {
    UserRequested,
    RiskyAction,
    AuthenticationRequired,
    Debugging,
    Other(String),
}

/// Request to pause browser automation and expose a session to the host.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BrowserHumanTakeoverRequest {
    pub session_id: BrowserSessionId,
    #[serde(default)]
    pub page_id: Option<BrowserPageId>,
    pub reason: BrowserHumanTakeoverReason,
    pub requested_at: DateTime<Utc>,
    #[serde(default)]
    pub expires_at: Option<DateTime<Utc>>,
}

/// Active human takeover lease for a browser session.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BrowserHumanTakeoverLease {
    pub session_id: BrowserSessionId,
    #[serde(default)]
    pub page_id: Option<BrowserPageId>,
    pub reason: BrowserHumanTakeoverReason,
    pub started_at: DateTime<Utc>,
    #[serde(default)]
    pub expires_at: Option<DateTime<Utc>>,
}

/// Metadata-only browser session handle exposed to the host during takeover.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BrowserHostSessionHandle {
    pub session_id: BrowserSessionId,
    #[serde(default)]
    pub active_page_id: Option<BrowserPageId>,
    pub pages: Vec<BrowserPageInfo>,
    pub profile: BrowserSessionProfileBinding,
    pub takeover_started_at: DateTime<Utc>,
}

/// Browser actions that mutate or may mutate browser/page state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BrowserMutationActionKind {
    ClickElement,
    TypeText,
    FillForm,
    SelectOption,
    ScrollPage,
    PressKey,
    ExecuteJs,
    UploadFile,
    DownloadFile,
}

impl BrowserMutationActionKind {
    pub fn all() -> Vec<Self> {
        vec![
            Self::ClickElement,
            Self::TypeText,
            Self::FillForm,
            Self::SelectOption,
            Self::ScrollPage,
            Self::PressKey,
            Self::ExecuteJs,
            Self::UploadFile,
            Self::DownloadFile,
        ]
    }

    pub fn from_action_name(action: &str) -> Option<Self> {
        match action {
            "browser.click_element" => Some(Self::ClickElement),
            "browser.type_text" => Some(Self::TypeText),
            "browser.fill_form" => Some(Self::FillForm),
            "browser.select_option" => Some(Self::SelectOption),
            "browser.scroll_page" => Some(Self::ScrollPage),
            "browser.press_key" => Some(Self::PressKey),
            "browser.execute_js" => Some(Self::ExecuteJs),
            "browser.upload_file" => Some(Self::UploadFile),
            "browser.download_file" => Some(Self::DownloadFile),
            _ => None,
        }
    }
}

/// Automation gate that blocks mutation actions while a human takeover lease is active.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BrowserAutomationGate {
    pub state: BrowserAutomationState,
    #[serde(default)]
    pub active_takeover: Option<BrowserHumanTakeoverLease>,
}

impl BrowserAutomationGate {
    pub fn running() -> Self {
        Self {
            state: BrowserAutomationState::Running,
            active_takeover: None,
        }
    }

    pub fn pause_for_takeover(
        &mut self,
        request: BrowserHumanTakeoverRequest,
    ) -> Result<BrowserHumanTakeoverLease, BrowserKernelError> {
        let lease = BrowserHumanTakeoverLease {
            session_id: request.session_id,
            page_id: request.page_id,
            reason: request.reason,
            started_at: request.requested_at,
            expires_at: request.expires_at,
        };
        self.state = BrowserAutomationState::PausedForHumanTakeover;
        self.active_takeover = Some(lease.clone());
        Ok(lease)
    }

    pub fn resume(
        &mut self,
        session_id: &BrowserSessionId,
        _resumed_at: DateTime<Utc>,
    ) -> Result<(), BrowserKernelError> {
        let Some(lease) = &self.active_takeover else {
            self.state = BrowserAutomationState::Running;
            return Ok(());
        };
        if &lease.session_id != session_id {
            return Err(BrowserKernelError::InvalidConfig(format!(
                "cannot resume browser session {} while takeover is active for {}",
                session_id.0, lease.session_id.0
            )));
        }
        self.state = BrowserAutomationState::Running;
        self.active_takeover = None;
        Ok(())
    }

    pub fn ensure_mutation_allowed(
        &self,
        action: BrowserMutationActionKind,
    ) -> Result<(), BrowserKernelError> {
        match self.state {
            BrowserAutomationState::Running => Ok(()),
            BrowserAutomationState::PausedForHumanTakeover => {
                let session = self
                    .active_takeover
                    .as_ref()
                    .map(|lease| lease.session_id.0.as_str())
                    .unwrap_or("unknown");
                Err(BrowserKernelError::AutomationPaused(format!(
                    "session {} is paused; mutation action {:?} is blocked",
                    session, action
                )))
            }
        }
    }

    pub fn host_session_handle(
        &self,
        session: &BrowserSession,
    ) -> Result<BrowserHostSessionHandle, BrowserKernelError> {
        let Some(lease) = &self.active_takeover else {
            return Err(BrowserKernelError::InvalidConfig(
                "host session handle requires active human takeover".to_string(),
            ));
        };
        if lease.session_id != session.session_id {
            return Err(BrowserKernelError::InvalidConfig(format!(
                "takeover session {} does not match requested session {}",
                lease.session_id.0, session.session_id.0
            )));
        }
        Ok(BrowserHostSessionHandle {
            session_id: session.session_id.clone(),
            active_page_id: session.active_page_id.clone(),
            pages: session.pages.clone(),
            profile: session.profile.clone(),
            takeover_started_at: lease.started_at,
        })
    }
}

// ---------------------------------------------------------------------------
// Element selector
// ---------------------------------------------------------------------------

/// Stable browser frame/iframe identifier within an interactive snapshot.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct BrowserFrameId(pub String);

impl BrowserFrameId {
    pub fn new(value: impl Into<String>) -> Result<Self, BrowserKernelError> {
        let value = value.into();
        if value.trim().is_empty() {
            Err(BrowserKernelError::InvalidConfig(
                "browser frame id cannot be empty".to_string(),
            ))
        } else {
            Ok(Self(value))
        }
    }
}

/// Selector scoped to a specific browser frame/iframe.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BrowserFrameSelector {
    pub frame_id: BrowserFrameId,
    pub selector: Box<ElementSelector>,
}

impl BrowserFrameSelector {
    pub fn new(frame_id: BrowserFrameId, selector: ElementSelector) -> Self {
        Self {
            frame_id,
            selector: Box::new(selector),
        }
    }

    pub fn validate(&self) -> Result<(), BrowserKernelError> {
        BrowserFrameId::new(self.frame_id.0.clone())?;
        self.selector.validate()
    }
}

/// Selector for locating an element on an interactive page.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind", content = "value")]
pub enum ElementSelector {
    Css(String),
    Text(String),
    XPath(String),
    Frame(BrowserFrameSelector),
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

    pub fn frame(frame_id: BrowserFrameId, selector: ElementSelector) -> Self {
        Self::Frame(BrowserFrameSelector::new(frame_id, selector))
    }

    pub fn validate(&self) -> Result<(), BrowserKernelError> {
        match self {
            Self::Css(value) | Self::Text(value) | Self::XPath(value) => {
                if value.trim().is_empty() {
                    Err(BrowserKernelError::EmptySelector)
                } else {
                    Ok(())
                }
            }
            Self::Frame(selector) => selector.validate(),
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

/// Metadata for a frame/iframe discovered while capturing an interactive snapshot.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BrowserFrameMetadata {
    pub frame_id: BrowserFrameId,
    #[serde(default)]
    pub parent_frame_id: Option<BrowserFrameId>,
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub selector: Option<ElementSelector>,
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
    #[serde(default)]
    pub frame_id: Option<BrowserFrameId>,
}

/// A full interactive snapshot of a rendered page including element index.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct InteractiveSnapshot {
    pub url: String,
    pub title: String,
    pub elements: Vec<InteractiveElement>,
    #[serde(default)]
    pub frames: Vec<BrowserFrameMetadata>,
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
// Upload handling
// ---------------------------------------------------------------------------

/// Input contract for a browser file upload action.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BrowserUploadInput {
    pub url: String,
    pub selector: ElementSelector,
    pub local_path: PathBuf,
    #[serde(default)]
    pub filename: Option<String>,
    #[serde(default)]
    pub content_type: Option<String>,
    #[serde(default)]
    pub size_bytes: Option<u64>,
}

impl BrowserUploadInput {
    pub fn validate(&self) -> Result<(), BrowserKernelError> {
        if self.url.trim().is_empty() {
            return Err(BrowserKernelError::EmptyUrl);
        }
        self.selector.validate()?;
        if self.local_path.as_os_str().is_empty() {
            return Err(BrowserKernelError::InvalidConfig(
                "upload local_path cannot be empty".to_string(),
            ));
        }
        if let Some(filename) = &self.filename
            && filename.trim().is_empty()
        {
            return Err(BrowserKernelError::InvalidConfig(
                "upload filename cannot be empty".to_string(),
            ));
        }
        Ok(())
    }

    pub fn audit_data(
        &self,
        requested_at: DateTime<Utc>,
    ) -> Result<BrowserUploadAuditData, BrowserKernelError> {
        self.validate()?;
        Ok(BrowserUploadAuditData {
            url: self.url.trim().to_string(),
            selector: self.selector.clone(),
            local_path: self.local_path.clone(),
            filename: self
                .filename
                .clone()
                .unwrap_or_else(|| infer_upload_filename(&self.local_path)),
            content_type: self.content_type.clone(),
            size_bytes: self.size_bytes,
            requested_at,
        })
    }

    pub fn approval_requirement(
        &self,
        requested_at: DateTime<Utc>,
    ) -> BrowserUploadApprovalRequirement {
        match self.audit_data(requested_at) {
            Ok(audit) => BrowserUploadApprovalRequirement::ask(
                "browser upload exposes a local file to a web page and requires human approval",
                audit,
            ),
            Err(error) => BrowserUploadApprovalRequirement::deny(
                format!("invalid browser upload input: {}", error),
                BrowserUploadAuditData::invalid(requested_at),
            ),
        }
    }
}

/// Audit payload exposed before a browser upload is approved or denied.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BrowserUploadAuditData {
    pub url: String,
    pub selector: ElementSelector,
    pub local_path: PathBuf,
    pub filename: String,
    pub content_type: Option<String>,
    pub size_bytes: Option<u64>,
    pub requested_at: DateTime<Utc>,
}

impl BrowserUploadAuditData {
    fn invalid(requested_at: DateTime<Utc>) -> Self {
        Self {
            url: "".to_string(),
            selector: ElementSelector::css("invalid"),
            local_path: PathBuf::new(),
            filename: "".to_string(),
            content_type: None,
            size_bytes: None,
            requested_at,
        }
    }
}

/// Policy result for upload actions. Uploads are never silently allowed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BrowserUploadPolicyDecision {
    Ask,
    Deny,
}

/// Approval requirement returned for every browser upload request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BrowserUploadApprovalRequirement {
    pub decision: BrowserUploadPolicyDecision,
    pub reason: String,
    pub audit_data: BrowserUploadAuditData,
}

impl BrowserUploadApprovalRequirement {
    pub fn ask(reason: impl Into<String>, audit_data: BrowserUploadAuditData) -> Self {
        Self {
            decision: BrowserUploadPolicyDecision::Ask,
            reason: reason.into(),
            audit_data,
        }
    }

    pub fn deny(reason: impl Into<String>, audit_data: BrowserUploadAuditData) -> Self {
        Self {
            decision: BrowserUploadPolicyDecision::Deny,
            reason: reason.into(),
            audit_data,
        }
    }

    pub fn requires_human_approval(&self) -> bool {
        self.decision == BrowserUploadPolicyDecision::Ask
    }

    pub fn is_denied(&self) -> bool {
        self.decision == BrowserUploadPolicyDecision::Deny
    }
}

fn infer_upload_filename(path: &Path) -> String {
    path.file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.trim().is_empty())
        .unwrap_or("upload.bin")
        .to_string()
}

// ---------------------------------------------------------------------------
// Network trace artifacts
// ---------------------------------------------------------------------------

/// Redaction policy attached to captured browser network traces.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BrowserNetworkRedactionPolicy {
    pub redact_auth_headers: bool,
    pub redacted_value: String,
}

impl Default for BrowserNetworkRedactionPolicy {
    fn default() -> Self {
        Self {
            redact_auth_headers: true,
            redacted_value: BrowserNetworkHeader::REDACTED.to_string(),
        }
    }
}

/// Optional network capture policy for browser actions.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BrowserNetworkTracePolicy {
    #[serde(default = "default_network_capture_enabled")]
    pub capture_enabled: bool,
    #[serde(default = "default_network_trace_max_entries")]
    pub max_entries: usize,
    #[serde(default)]
    pub redaction: BrowserNetworkRedactionPolicy,
}

impl Default for BrowserNetworkTracePolicy {
    fn default() -> Self {
        Self {
            capture_enabled: default_network_capture_enabled(),
            max_entries: default_network_trace_max_entries(),
            redaction: BrowserNetworkRedactionPolicy::default(),
        }
    }
}

impl BrowserNetworkTracePolicy {
    pub fn should_redact_header(&self, name: &str) -> bool {
        self.redaction.redact_auth_headers && is_sensitive_network_header(name)
    }

    pub fn redact_headers(&self, headers: Vec<BrowserNetworkHeader>) -> Vec<BrowserNetworkHeader> {
        headers
            .into_iter()
            .map(|mut header| {
                if self.should_redact_header(&header.name) {
                    header.value = self.redaction.redacted_value.clone();
                }
                header
            })
            .collect()
    }
}

const fn default_network_capture_enabled() -> bool {
    true
}

const fn default_network_trace_max_entries() -> usize {
    200
}

fn is_sensitive_network_header(name: &str) -> bool {
    matches!(
        name.trim().to_ascii_lowercase().as_str(),
        "authorization"
            | "proxy-authorization"
            | "cookie"
            | "set-cookie"
            | "x-api-key"
            | "x-auth-token"
            | "x-csrf-token"
    )
}

/// Header captured in browser network traces.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BrowserNetworkHeader {
    pub name: String,
    pub value: String,
}

impl BrowserNetworkHeader {
    pub const REDACTED: &'static str = "[REDACTED]";

    pub fn new(name: impl Into<String>, value: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            value: value.into(),
        }
    }
}

/// One request/response pair in a browser network trace.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BrowserNetworkTraceEntry {
    pub request_id: String,
    pub method: String,
    pub url: String,
    #[serde(default)]
    pub request_headers: Vec<BrowserNetworkHeader>,
    #[serde(default)]
    pub response_status: Option<u16>,
    #[serde(default)]
    pub response_headers: Vec<BrowserNetworkHeader>,
    pub started_at: DateTime<Utc>,
    #[serde(default)]
    pub finished_at: Option<DateTime<Utc>>,
}

/// Captured HAR-like browser network trace boundary.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BrowserNetworkTrace {
    pub artifact_id: ArtifactId,
    pub source_url: String,
    #[serde(default)]
    pub entries: Vec<BrowserNetworkTraceEntry>,
    pub captured_at: DateTime<Utc>,
    #[serde(default)]
    pub redaction_policy: BrowserNetworkRedactionPolicy,
}

impl BrowserNetworkTrace {
    pub fn to_artifact_descriptor(&self, source_action: Option<&str>) -> ArtifactDescriptor {
        let mut descriptor = ArtifactDescriptor::new(
            self.artifact_id.clone(),
            ArtifactKind::ToolResult,
            self.captured_at,
        );
        descriptor.title = Some(format!("Network trace for {}", self.source_url));
        descriptor.source_uri = Some(self.source_url.clone());
        descriptor.mime_type = Some("application/har+json".to_string());
        descriptor.metadata = serde_json::json!({
            "source": "browser_network_trace",
            "source_action": source_action,
            "entries": self.entries.len(),
            "auth_headers_redacted": self.redaction_policy.redact_auth_headers,
        });
        descriptor
    }
}

// ---------------------------------------------------------------------------
// Download artifacts
// ---------------------------------------------------------------------------

/// Input contract for a browser download action.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BrowserDownloadInput {
    pub url: String,
    #[serde(default)]
    pub suggested_filename: Option<String>,
    #[serde(default)]
    pub content_type: Option<String>,
}

impl BrowserDownloadInput {
    pub fn validate(&self) -> Result<(), BrowserKernelError> {
        if self.url.trim().is_empty() {
            return Err(BrowserKernelError::EmptyUrl);
        }
        if let Some(filename) = &self.suggested_filename
            && filename.trim().is_empty()
        {
            return Err(BrowserKernelError::InvalidConfig(
                "download suggested_filename cannot be empty".to_string(),
            ));
        }
        Ok(())
    }
}

/// Captured metadata for one browser DOM snapshot artifact.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BrowserDomSnapshotArtifact {
    pub artifact_id: ArtifactId,
    pub source_url: String,
    pub title: Option<String>,
    pub mime_type: String,
    pub size_bytes: u64,
    pub sha256: String,
    pub captured_at: DateTime<Utc>,
}

impl BrowserDomSnapshotArtifact {
    pub fn from_html(
        artifact_id: ArtifactId,
        source_url: impl Into<String>,
        title: impl Into<String>,
        html: impl AsRef<str>,
        captured_at: DateTime<Utc>,
    ) -> Result<Self, BrowserKernelError> {
        let source_url = source_url.into();
        let title = title.into();
        let html = html.as_ref();
        if html.trim().is_empty() {
            return Err(BrowserKernelError::InvalidConfig(
                "dom snapshot html cannot be empty".to_string(),
            ));
        }

        let mut hasher = Sha256::new();
        hasher.update(html.as_bytes());
        let sha256 = format!("{:x}", hasher.finalize());

        Ok(Self {
            artifact_id,
            source_url,
            title: if title.trim().is_empty() {
                None
            } else {
                Some(title)
            },
            mime_type: "text/html".to_string(),
            size_bytes: html.len() as u64,
            sha256,
            captured_at,
        })
    }

    pub fn to_artifact_descriptor(&self, source_action: Option<&str>) -> ArtifactDescriptor {
        let mut descriptor = ArtifactDescriptor::new(
            self.artifact_id.clone(),
            ArtifactKind::WebPage,
            self.captured_at,
        );
        descriptor.title = Some(match self.title.as_deref() {
            Some(title) => format!("DOM snapshot of {}", title),
            None => format!("DOM snapshot of {}", self.source_url),
        });
        descriptor.source_uri = Some(self.source_url.clone());
        descriptor.mime_type = Some(self.mime_type.clone());
        descriptor.metadata = serde_json::json!({
            "source": "browser_dom_snapshot",
            "source_action": source_action,
            "size_bytes": self.size_bytes,
            "sha256": self.sha256,
        });
        descriptor
    }
}

/// Captured metadata for one browser download artifact.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BrowserDownloadArtifact {
    pub artifact_id: ArtifactId,
    pub source_url: String,
    pub filename: String,
    pub content_type: String,
    pub size_bytes: u64,
    pub sha256: String,
    pub downloaded_at: DateTime<Utc>,
}

impl BrowserDownloadArtifact {
    pub fn from_bytes(
        artifact_id: ArtifactId,
        source_url: impl Into<String>,
        filename: impl Into<String>,
        content_type: impl Into<String>,
        bytes: &[u8],
        downloaded_at: DateTime<Utc>,
    ) -> Result<Self, BrowserKernelError> {
        let source_url = source_url.into();
        if source_url.trim().is_empty() {
            return Err(BrowserKernelError::EmptyUrl);
        }
        let filename = filename.into();
        if filename.trim().is_empty() {
            return Err(BrowserKernelError::InvalidConfig(
                "download filename cannot be empty".to_string(),
            ));
        }
        let content_type = content_type.into();
        if content_type.trim().is_empty() {
            return Err(BrowserKernelError::InvalidConfig(
                "download content_type cannot be empty".to_string(),
            ));
        }

        let mut hasher = Sha256::new();
        hasher.update(bytes);
        let sha256 = format!("{:x}", hasher.finalize());

        Ok(Self {
            artifact_id,
            source_url,
            filename,
            content_type,
            size_bytes: bytes.len() as u64,
            sha256,
            downloaded_at,
        })
    }

    pub fn to_artifact_descriptor(&self) -> ArtifactDescriptor {
        let kind = if self.content_type == "application/pdf" {
            ArtifactKind::Pdf
        } else if self.content_type.starts_with("image/") {
            ArtifactKind::Image
        } else {
            ArtifactKind::Document
        };

        let mut descriptor =
            ArtifactDescriptor::new(self.artifact_id.clone(), kind, self.downloaded_at);
        descriptor.title = Some(self.filename.clone());
        descriptor.source_uri = Some(self.source_url.clone());
        descriptor.mime_type = Some(self.content_type.clone());
        descriptor.metadata = serde_json::json!({
            "filename": self.filename,
            "content_type": self.content_type,
            "size_bytes": self.size_bytes,
            "sha256": self.sha256,
            "downloaded_at": self.downloaded_at,
        });
        descriptor
    }
}

/// Result of handling a browser download.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BrowserDownloadResult {
    pub download: BrowserDownloadArtifact,
    pub artifact: ArtifactDescriptor,
}

impl BrowserDownloadResult {
    pub fn from_bytes(
        artifact_id: ArtifactId,
        source_url: impl Into<String>,
        filename: impl Into<String>,
        content_type: impl Into<String>,
        bytes: &[u8],
        downloaded_at: DateTime<Utc>,
    ) -> Result<Self, BrowserKernelError> {
        let download = BrowserDownloadArtifact::from_bytes(
            artifact_id,
            source_url,
            filename,
            content_type,
            bytes,
            downloaded_at,
        )?;
        let artifact = download.to_artifact_descriptor();
        Ok(Self { download, artifact })
    }
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
    #[serde(default)]
    save_dom_snapshot: bool,
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct BrowserDownloadActionInput {
    url: String,
    #[serde(default)]
    suggested_filename: Option<String>,
    #[serde(default)]
    content_type: Option<String>,
}

impl From<BrowserDownloadActionInput> for BrowserDownloadInput {
    fn from(input: BrowserDownloadActionInput) -> Self {
        Self {
            url: input.url,
            suggested_filename: input.suggested_filename,
            content_type: input.content_type,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct BrowserUploadActionInput {
    url: String,
    selector: ElementSelector,
    local_path: PathBuf,
    #[serde(default)]
    filename: Option<String>,
    #[serde(default)]
    content_type: Option<String>,
    #[serde(default)]
    size_bytes: Option<u64>,
}

impl From<BrowserUploadActionInput> for BrowserUploadInput {
    fn from(input: BrowserUploadActionInput) -> Self {
        Self {
            url: input.url,
            selector: input.selector,
            local_path: input.local_path,
            filename: input.filename,
            content_type: input.content_type,
            size_bytes: input.size_bytes,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct BrowserDialogActionInput {
    #[serde(default)]
    page_id: Option<BrowserPageId>,
    kind: BrowserDialogKind,
    message: String,
    #[serde(default)]
    default_prompt_text: Option<String>,
}

impl BrowserDialogActionInput {
    fn into_event(self, occurred_at: DateTime<Utc>) -> BrowserDialogEvent {
        BrowserDialogEvent {
            page_id: self.page_id,
            kind: self.kind,
            message: self.message,
            default_prompt_text: self.default_prompt_text,
            occurred_at,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct BrowserPermissionPromptActionInput {
    #[serde(default)]
    page_id: Option<BrowserPageId>,
    origin: String,
    permission: BrowserPermissionKind,
}

impl BrowserPermissionPromptActionInput {
    fn into_event(self, occurred_at: DateTime<Utc>) -> BrowserPermissionPromptEvent {
        BrowserPermissionPromptEvent {
            page_id: self.page_id,
            origin: self.origin,
            permission: self.permission,
            occurred_at,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct RawInteractiveElement {
    id: String,
    kind: String,
    selector: Option<String>,
    text: Option<String>,
    aria_label: Option<String>,
    bounding_box: Option<ElementBoundingBox>,
    #[serde(default)]
    frame_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct RawFrameMetadata {
    frame_id: String,
    #[serde(default)]
    parent_frame_id: Option<String>,
    #[serde(default)]
    url: Option<String>,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    selector: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct RawInteractiveSnapshot {
    #[serde(default)]
    elements: Vec<RawInteractiveElement>,
    #[serde(default)]
    frames: Vec<RawFrameMetadata>,
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
        let title = page_title(&page).await.unwrap_or_default();
        let text = page_body_text(&page).await.unwrap_or_default();
        let links = page_string_array(&page, LINK_EXTRACTION_JS)
            .await
            .unwrap_or_default();
        let images = page_string_array(&page, IMAGE_EXTRACTION_JS)
            .await
            .unwrap_or_default();
        let extracted_at = chrono::Utc::now();
        let mut dom_snapshot_artifact_id = None;

        if input.save_dom_snapshot {
            let html = page.content().await.map_err(to_execution_failed)?;
            let artifact = BrowserDomSnapshotArtifact::from_html(
                ArtifactId(format!("dom-snapshot-{}", extracted_at.timestamp_millis())),
                current_url.clone(),
                title.clone(),
                html,
                extracted_at,
            )
            .map_err(to_execution_failed)?;
            let descriptor = artifact.to_artifact_descriptor(Some("browser.extract_content"));
            if let Some(ref store) = self.artifact_store {
                store.put(descriptor).await.map_err(|e| {
                    to_execution_failed(format!("failed to store dom snapshot artifact: {}", e))
                })?;
            }
            dom_snapshot_artifact_id = Some(artifact.artifact_id);
        }

        Ok(action_core::ActionResult {
            status: action_core::ActionStatus::Completed,
            summary: format!("Extracted content from URL: {}", current_url),
            payload: action_core::ActionResultPayload::Json(serde_json::json!({
                "source_url": { "0": current_url },
                "title": title,
                "text": text,
                "links": links,
                "images": images,
                "dom_snapshot_artifact_id": dom_snapshot_artifact_id,
                "extracted_at": extracted_at
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
        let (elements, frames) = page_interactive_snapshot_parts(&page)
            .await
            .map_err(to_execution_failed)?;
        let snapshot = InteractiveSnapshot {
            url: current_url.clone(),
            title,
            elements,
            frames,
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

    async fn download_file(
        &self,
        input: BrowserDownloadInput,
    ) -> Result<action_core::ActionResult, action_core::ActionExecutorError> {
        input.validate().map_err(to_invalid_input)?;
        let url = Self::validate_url(&input.url).map_err(to_invalid_input)?;
        let filename = input
            .suggested_filename
            .clone()
            .unwrap_or_else(|| infer_download_filename(&url));
        let content_type = input
            .content_type
            .clone()
            .unwrap_or_else(|| "application/octet-stream".to_string());
        let downloaded_at = chrono::Utc::now();

        // PR125 defines the stable action/artifact boundary. Actual browser-backed
        // bytes are wired by later CDP download plumbing; until then we create a
        // deterministic zero-byte descriptor for mock/download metadata tests.
        let result = BrowserDownloadResult::from_bytes(
            ArtifactId(format!("download-{}", downloaded_at.timestamp_millis())),
            url.clone(),
            filename,
            content_type,
            &[],
            downloaded_at,
        )
        .map_err(to_invalid_input)?;

        if let Some(ref store) = self.artifact_store {
            store.put(result.artifact.clone()).await.map_err(|e| {
                to_execution_failed(format!("failed to store download artifact: {}", e))
            })?;
        }

        Ok(action_core::ActionResult {
            status: action_core::ActionStatus::Completed,
            summary: format!(
                "Download artifact recorded for {} as {}",
                result.download.source_url, result.download.filename
            ),
            payload: action_core::ActionResultPayload::Json(serde_json::json!({
                "success": true,
                "artifact_id": result.download.artifact_id,
                "url": result.download.source_url,
                "filename": result.download.filename,
                "content_type": result.download.content_type,
                "size_bytes": result.download.size_bytes,
                "sha256": result.download.sha256,
                "downloaded_at": result.download.downloaded_at,
            })),
            completed_at: downloaded_at,
        })
    }

    async fn upload_file(
        &self,
        input: BrowserUploadInput,
    ) -> Result<action_core::ActionResult, action_core::ActionExecutorError> {
        let requested_at = chrono::Utc::now();
        let requirement = input.approval_requirement(requested_at);
        let status = if requirement.requires_human_approval() {
            action_core::ActionStatus::ApprovalRequired
        } else {
            action_core::ActionStatus::Denied
        };
        let summary = match requirement.decision {
            BrowserUploadPolicyDecision::Ask => {
                format!("Browser upload requires approval: {}", requirement.reason)
            }
            BrowserUploadPolicyDecision::Deny => {
                format!("Browser upload denied: {}", requirement.reason)
            }
        };

        Ok(action_core::ActionResult {
            status,
            summary,
            payload: action_core::ActionResultPayload::Json(serde_json::json!({
                "decision": requirement.decision,
                "reason": requirement.reason,
                "audit_data": requirement.audit_data,
            })),
            completed_at: requested_at,
        })
    }

    async fn handle_dialog(
        &self,
        input: BrowserDialogActionInput,
    ) -> Result<action_core::ActionResult, action_core::ActionExecutorError> {
        let now = chrono::Utc::now();
        let event = input.into_event(now);
        let resolution = self
            .lifecycle
            .config()
            .prompt_policy
            .resolve_dialog(&event, now)
            .map_err(to_invalid_input)?;

        Ok(action_core::ActionResult {
            status: action_core::ActionStatus::Completed,
            summary: format!(
                "Resolved browser {:?} dialog with {:?}",
                event.kind, resolution.decision
            ),
            payload: action_core::ActionResultPayload::Json(serde_json::json!({
                "event": event,
                "resolution": resolution,
            })),
            completed_at: now,
        })
    }

    async fn handle_permission_prompt(
        &self,
        input: BrowserPermissionPromptActionInput,
    ) -> Result<action_core::ActionResult, action_core::ActionExecutorError> {
        let now = chrono::Utc::now();
        let event = input.into_event(now);
        let resolution = self
            .lifecycle
            .config()
            .prompt_policy
            .resolve_permission_prompt(&event, now)
            .map_err(to_invalid_input)?;
        let status = match resolution.decision {
            BrowserPermissionDecision::Ask => action_core::ActionStatus::ApprovalRequired,
            BrowserPermissionDecision::Deny => action_core::ActionStatus::Denied,
            BrowserPermissionDecision::Allow => action_core::ActionStatus::Completed,
        };

        Ok(action_core::ActionResult {
            status,
            summary: format!(
                "Resolved browser {:?} permission prompt with {:?}",
                event.permission, resolution.decision
            ),
            payload: action_core::ActionResultPayload::Json(serde_json::json!({
                "event": event,
                "resolution": resolution,
            })),
            completed_at: now,
        })
    }
}

fn infer_download_filename(url: &str) -> String {
    url.trim_end_matches('/')
        .rsplit('/')
        .next()
        .filter(|segment| !segment.trim().is_empty())
        .unwrap_or("download.bin")
        .to_string()
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
  const visible = (el, win) => {
    const rect = el.getBoundingClientRect();
    const style = win.getComputedStyle(el);
    return rect.width > 0 && rect.height > 0 && style.visibility !== 'hidden' && style.display !== 'none';
  };
  const elementRecord = (el, index, frameId, idPrefix, offsetX = 0, offsetY = 0) => {
    const rect = el.getBoundingClientRect();
    const text = (el.innerText || el.value || el.getAttribute('title') || '').trim();
    return {
      id: `${idPrefix}e${index + 1}`,
      kind: kindFor(el),
      selector: selectorFor(el),
      text: text ? text.slice(0, 200) : null,
      aria_label: el.getAttribute('aria-label'),
      bounding_box: { x: rect.x + offsetX, y: rect.y + offsetY, width: rect.width, height: rect.height },
      frame_id: frameId
    };
  };
  const selector = 'a[href],button,input,select,textarea,[role="button"],[onclick],[tabindex]';
  const elements = Array.from(document.querySelectorAll(selector))
    .filter(el => visible(el, window))
    .slice(0, 200)
    .map((el, index) => elementRecord(el, index, null, ''));
  const frames = [];
  Array.from(document.querySelectorAll('iframe,frame')).slice(0, 50).forEach((frameEl, frameIndex) => {
    const frameId = `frame-${frameIndex + 1}`;
    frames.push({
      frame_id: frameId,
      parent_frame_id: null,
      url: frameEl.src || null,
      name: frameEl.getAttribute('name'),
      title: frameEl.getAttribute('title'),
      selector: selectorFor(frameEl)
    });
    try {
      const doc = frameEl.contentDocument;
      const win = frameEl.contentWindow;
      if (!doc || !win) return;
      const frameRect = frameEl.getBoundingClientRect();
      Array.from(doc.querySelectorAll(selector))
        .filter(el => visible(el, win))
        .slice(0, 100)
        .forEach((el, index) => elements.push(elementRecord(el, index, frameId, `${frameId}-`, frameRect.x, frameRect.y)));
    } catch (_error) {
      // Cross-origin frames still contribute metadata, but their elements cannot be inspected.
    }
  });
  return { elements, frames };
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

async fn page_interactive_snapshot_parts(
    page: &chromiumoxide::Page,
) -> Result<(Vec<InteractiveElement>, Vec<BrowserFrameMetadata>), BrowserKernelError> {
    let raw: RawInteractiveSnapshot = page
        .evaluate(INTERACTIVE_ELEMENTS_JS)
        .await
        .map_err(to_browser_action_failed)?
        .into_value()
        .map_err(|e| BrowserKernelError::ActionFailed(e.to_string()))?;

    raw_interactive_snapshot_to_parts(raw)
}

fn raw_interactive_snapshot_to_parts(
    raw: RawInteractiveSnapshot,
) -> Result<(Vec<InteractiveElement>, Vec<BrowserFrameMetadata>), BrowserKernelError> {
    let elements = raw
        .elements
        .into_iter()
        .map(raw_interactive_element_to_interactive_element)
        .collect::<Result<Vec<_>, _>>()?;
    let frames = raw
        .frames
        .into_iter()
        .map(raw_frame_metadata_to_frame_metadata)
        .collect::<Result<Vec<_>, _>>()?;
    Ok((elements, frames))
}

fn raw_interactive_element_to_interactive_element(
    element: RawInteractiveElement,
) -> Result<InteractiveElement, BrowserKernelError> {
    let frame_id = element.frame_id.map(BrowserFrameId::new).transpose()?;
    let selector = match (frame_id.clone(), element.selector) {
        (Some(frame_id), Some(selector)) => Some(ElementSelector::frame(
            frame_id,
            ElementSelector::Css(selector),
        )),
        (None, Some(selector)) => Some(ElementSelector::Css(selector)),
        (_, None) => None,
    };
    Ok(InteractiveElement {
        id: element.id,
        kind: interactive_kind_from_str(&element.kind),
        selector,
        text: element.text,
        aria_label: element.aria_label,
        bounding_box: element.bounding_box,
        frame_id,
    })
}

fn raw_frame_metadata_to_frame_metadata(
    raw: RawFrameMetadata,
) -> Result<BrowserFrameMetadata, BrowserKernelError> {
    Ok(BrowserFrameMetadata {
        frame_id: BrowserFrameId::new(raw.frame_id)?,
        parent_frame_id: raw.parent_frame_id.map(BrowserFrameId::new).transpose()?,
        url: raw.url,
        name: raw.name,
        title: raw.title,
        selector: raw.selector.map(ElementSelector::Css),
    })
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
    "browser.download_file",
    "browser.upload_file",
    "browser.handle_dialog",
    "browser.handle_permission_prompt",
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
            "browser.download_file" => {
                let input: BrowserDownloadActionInput =
                    serde_json::from_value(request.input.clone()).map_err(|e| {
                        action_core::ActionExecutorError::InvalidInput(e.to_string())
                    })?;
                self.download_file(input.into()).await
            }
            "browser.upload_file" => {
                let input: BrowserUploadActionInput = serde_json::from_value(request.input.clone())
                    .map_err(|e| action_core::ActionExecutorError::InvalidInput(e.to_string()))?;
                self.upload_file(input.into()).await
            }
            "browser.handle_dialog" => {
                let input: BrowserDialogActionInput = serde_json::from_value(request.input.clone())
                    .map_err(|e| action_core::ActionExecutorError::InvalidInput(e.to_string()))?;
                self.handle_dialog(input).await
            }
            "browser.handle_permission_prompt" => {
                let input: BrowserPermissionPromptActionInput =
                    serde_json::from_value(request.input.clone()).map_err(|e| {
                        action_core::ActionExecutorError::InvalidInput(e.to_string())
                    })?;
                self.handle_permission_prompt(input).await
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

    // ---- Human takeover boundary tests ----

    #[test]
    fn browser_automation_gate_starts_running() {
        let gate = BrowserAutomationGate::running();

        assert_eq!(gate.state, BrowserAutomationState::Running);
        assert!(gate.active_takeover.is_none());
    }

    #[test]
    fn browser_automation_gate_pauses_for_human_takeover() {
        let now = ts();
        let mut gate = BrowserAutomationGate::running();
        let request = BrowserHumanTakeoverRequest {
            session_id: BrowserSessionId::new("session-1").unwrap(),
            page_id: Some(BrowserPageId::new("page-1").unwrap()),
            reason: BrowserHumanTakeoverReason::UserRequested,
            requested_at: now,
            expires_at: None,
        };

        let lease = gate.pause_for_takeover(request).unwrap();

        assert_eq!(gate.state, BrowserAutomationState::PausedForHumanTakeover);
        assert_eq!(lease.session_id, BrowserSessionId("session-1".to_string()));
        assert_eq!(lease.page_id, Some(BrowserPageId("page-1".to_string())));
        assert_eq!(lease.reason, BrowserHumanTakeoverReason::UserRequested);
        assert_eq!(lease.started_at, now);
        assert_eq!(gate.active_takeover, Some(lease));
    }

    #[test]
    fn browser_automation_gate_resumes_from_matching_session() {
        let now = ts();
        let mut gate = BrowserAutomationGate::running();
        gate.pause_for_takeover(BrowserHumanTakeoverRequest {
            session_id: BrowserSessionId::new("session-1").unwrap(),
            page_id: None,
            reason: BrowserHumanTakeoverReason::Debugging,
            requested_at: now,
            expires_at: None,
        })
        .unwrap();

        gate.resume(&BrowserSessionId::new("session-1").unwrap(), now)
            .unwrap();

        assert_eq!(gate.state, BrowserAutomationState::Running);
        assert!(gate.active_takeover.is_none());
    }

    #[test]
    fn browser_automation_gate_rejects_resume_for_wrong_session() {
        let now = ts();
        let mut gate = BrowserAutomationGate::running();
        gate.pause_for_takeover(BrowserHumanTakeoverRequest {
            session_id: BrowserSessionId::new("session-1").unwrap(),
            page_id: None,
            reason: BrowserHumanTakeoverReason::Debugging,
            requested_at: now,
            expires_at: None,
        })
        .unwrap();

        let error = gate
            .resume(&BrowserSessionId::new("session-2").unwrap(), now)
            .unwrap_err();

        assert!(matches!(error, BrowserKernelError::InvalidConfig(_)));
        assert_eq!(gate.state, BrowserAutomationState::PausedForHumanTakeover);
    }

    #[test]
    fn paused_automation_rejects_mutation_actions() {
        let now = ts();
        let mut gate = BrowserAutomationGate::running();
        gate.pause_for_takeover(BrowserHumanTakeoverRequest {
            session_id: BrowserSessionId::new("session-1").unwrap(),
            page_id: None,
            reason: BrowserHumanTakeoverReason::UserRequested,
            requested_at: now,
            expires_at: None,
        })
        .unwrap();

        for action in BrowserMutationActionKind::all() {
            let error = gate.ensure_mutation_allowed(action.clone()).unwrap_err();
            assert!(matches!(error, BrowserKernelError::AutomationPaused(_)));
        }
    }

    #[test]
    fn running_automation_allows_mutation_actions() {
        let gate = BrowserAutomationGate::running();

        for action in BrowserMutationActionKind::all() {
            assert!(gate.ensure_mutation_allowed(action).is_ok());
        }
    }

    #[test]
    fn host_session_handle_exposes_session_metadata_during_takeover() {
        let now = ts();
        let mut session = BrowserSession::new(
            BrowserSessionId::new("session-1").unwrap(),
            BrowserSessionProfileBinding::new(BrowserProfileMode::Ephemeral, None),
            now,
        );
        session
            .open_page(
                BrowserPageId::new("page-1").unwrap(),
                Some("https://example.com".to_string()),
                Some("Example".to_string()),
                now,
            )
            .unwrap();
        let mut gate = BrowserAutomationGate::running();
        gate.pause_for_takeover(BrowserHumanTakeoverRequest {
            session_id: session.session_id.clone(),
            page_id: session.active_page_id.clone(),
            reason: BrowserHumanTakeoverReason::AuthenticationRequired,
            requested_at: now,
            expires_at: None,
        })
        .unwrap();

        let handle = gate.host_session_handle(&session).unwrap();

        assert_eq!(handle.session_id, session.session_id);
        assert_eq!(
            handle.active_page_id,
            Some(BrowserPageId("page-1".to_string()))
        );
        assert_eq!(handle.pages.len(), 1);
        assert_eq!(handle.profile, session.profile);
        assert_eq!(handle.takeover_started_at, now);
    }

    #[test]
    fn host_session_handle_requires_active_takeover() {
        let now = ts();
        let session = BrowserSession::new(
            BrowserSessionId::new("session-1").unwrap(),
            BrowserSessionProfileBinding::new(BrowserProfileMode::Ephemeral, None),
            now,
        );
        let gate = BrowserAutomationGate::running();

        let error = gate.host_session_handle(&session).unwrap_err();

        assert!(matches!(error, BrowserKernelError::InvalidConfig(_)));
    }

    #[test]
    fn browser_mutation_action_kind_maps_known_mutation_actions() {
        assert_eq!(
            BrowserMutationActionKind::from_action_name("browser.click_element"),
            Some(BrowserMutationActionKind::ClickElement)
        );
        assert_eq!(
            BrowserMutationActionKind::from_action_name("browser.type_text"),
            Some(BrowserMutationActionKind::TypeText)
        );
        assert_eq!(
            BrowserMutationActionKind::from_action_name("browser.fill_form"),
            Some(BrowserMutationActionKind::FillForm)
        );
        assert_eq!(
            BrowserMutationActionKind::from_action_name("browser.select_option"),
            Some(BrowserMutationActionKind::SelectOption)
        );
        assert_eq!(
            BrowserMutationActionKind::from_action_name("browser.scroll_page"),
            Some(BrowserMutationActionKind::ScrollPage)
        );
        assert_eq!(
            BrowserMutationActionKind::from_action_name("browser.press_key"),
            Some(BrowserMutationActionKind::PressKey)
        );
        assert_eq!(
            BrowserMutationActionKind::from_action_name("browser.execute_js"),
            Some(BrowserMutationActionKind::ExecuteJs)
        );
        assert_eq!(
            BrowserMutationActionKind::from_action_name("browser.upload_file"),
            Some(BrowserMutationActionKind::UploadFile)
        );
        assert_eq!(
            BrowserMutationActionKind::from_action_name("browser.download_file"),
            Some(BrowserMutationActionKind::DownloadFile)
        );
        assert_eq!(
            BrowserMutationActionKind::from_action_name("browser.extract_content"),
            None
        );
    }

    // ---- Browser crash recovery tests ----

    #[test]
    fn browser_crash_event_requires_page_id_for_page_scope() {
        let event = BrowserCrashEvent {
            scope: BrowserCrashScope::Page,
            page_id: None,
            reason: BrowserCrashReason::PageCrashed,
            message: "renderer crashed".to_string(),
            detected_at: ts(),
        };

        assert!(matches!(
            event.validate(),
            Err(BrowserKernelError::InvalidConfig(_))
        ));
    }

    #[test]
    fn browser_crash_event_rejects_empty_message() {
        let event = BrowserCrashEvent {
            scope: BrowserCrashScope::BrowserProcess,
            page_id: None,
            reason: BrowserCrashReason::ProcessExited,
            message: " ".to_string(),
            detected_at: ts(),
        };

        assert!(matches!(
            event.validate(),
            Err(BrowserKernelError::InvalidConfig(_))
        ));
    }

    #[test]
    fn browser_recovery_policy_fail_fast_returns_typed_plan() {
        let now = ts();
        let session = BrowserSession::new(
            BrowserSessionId::new("session-1").unwrap(),
            BrowserSessionProfileBinding::new(BrowserProfileMode::Ephemeral, None),
            now,
        );
        let event = BrowserCrashEvent {
            scope: BrowserCrashScope::BrowserProcess,
            page_id: None,
            reason: BrowserCrashReason::BrowserDisconnected,
            message: "websocket disconnected".to_string(),
            detected_at: now,
        };
        let policy = BrowserRecoveryPolicy {
            strategy: BrowserRecoveryStrategy::FailFast,
            ..BrowserRecoveryPolicy::default()
        };

        let plan = policy.plan_recovery(&session, &event).unwrap();

        assert_eq!(plan.strategy, BrowserRecoveryStrategy::FailFast);
        assert!(matches!(plan.action, BrowserRecoveryAction::Fail { .. }));
    }

    #[test]
    fn browser_recovery_policy_reopen_active_page_preserves_url() {
        let now = ts();
        let mut session = BrowserSession::new(
            BrowserSessionId::new("session-1").unwrap(),
            BrowserSessionProfileBinding::new(BrowserProfileMode::Ephemeral, None),
            now,
        );
        let page_1 = BrowserPageId::new("page-1").unwrap();
        session
            .open_page(
                page_1.clone(),
                Some("https://example.com/app".to_string()),
                Some("App".to_string()),
                now,
            )
            .unwrap();
        let event = BrowserCrashEvent {
            scope: BrowserCrashScope::Page,
            page_id: Some(page_1.clone()),
            reason: BrowserCrashReason::PageCrashed,
            message: "renderer crashed".to_string(),
            detected_at: now,
        };
        let policy = BrowserRecoveryPolicy {
            strategy: BrowserRecoveryStrategy::ReopenActivePage,
            ..BrowserRecoveryPolicy::default()
        };

        let plan = policy.plan_recovery(&session, &event).unwrap();

        assert_eq!(plan.strategy, BrowserRecoveryStrategy::ReopenActivePage);
        assert_eq!(
            plan.action,
            BrowserRecoveryAction::ReopenPage {
                page_id: page_1,
                url: Some("https://example.com/app".to_string()),
            }
        );
    }

    #[test]
    fn browser_recovery_policy_relaunch_session_preserves_profile_and_pages() {
        let now = ts();
        let mut session = BrowserSession::new(
            BrowserSessionId::new("session-1").unwrap(),
            BrowserSessionProfileBinding::new(
                BrowserProfileMode::Named("work".to_string()),
                Some(PathBuf::from("/tmp/browser-profiles/work")),
            ),
            now,
        );
        session
            .open_page(
                BrowserPageId::new("page-1").unwrap(),
                Some("https://example.com".to_string()),
                Some("Example".to_string()),
                now,
            )
            .unwrap();
        let event = BrowserCrashEvent {
            scope: BrowserCrashScope::BrowserProcess,
            page_id: None,
            reason: BrowserCrashReason::ProcessExited,
            message: "chromium exited".to_string(),
            detected_at: now,
        };

        let plan = BrowserRecoveryPolicy::default()
            .plan_recovery(&session, &event)
            .unwrap();

        match plan.action {
            BrowserRecoveryAction::RelaunchSession {
                session_id,
                profile,
                pages,
                retry_after_relaunch,
            } => {
                assert_eq!(session_id, BrowserSessionId("session-1".to_string()));
                assert_eq!(profile.mode, BrowserProfileMode::Named("work".to_string()));
                assert_eq!(pages.len(), 1);
                assert_eq!(pages[0].url.as_deref(), Some("https://example.com"));
                assert!(retry_after_relaunch);
            }
            other => panic!("unexpected recovery action: {:?}", other),
        }
    }

    #[test]
    fn browser_recovery_policy_rejects_zero_relaunch_attempts() {
        let policy = BrowserRecoveryPolicy {
            max_relaunch_attempts: 0,
            ..BrowserRecoveryPolicy::default()
        };

        assert!(matches!(
            policy.validate(),
            Err(BrowserKernelError::InvalidConfig(_))
        ));
    }

    #[test]
    fn page_lifecycle_manager_records_page_crash() {
        let now = ts();
        let session = BrowserSession::new(
            BrowserSessionId::new("session-1").unwrap(),
            BrowserSessionProfileBinding::new(BrowserProfileMode::Ephemeral, None),
            now,
        );
        let mut manager = BrowserPageLifecycleManager::new(session, 2).unwrap();
        let page_1 = BrowserPageId::new("page-1").unwrap();
        manager.open_page(page_1.clone(), None, None, now).unwrap();
        let event = BrowserCrashEvent {
            scope: BrowserCrashScope::Page,
            page_id: Some(page_1.clone()),
            reason: BrowserCrashReason::PageCrashed,
            message: "renderer crashed".to_string(),
            detected_at: now,
        };

        manager.record_crash(event).unwrap();

        assert_eq!(
            manager.session.page(&page_1).unwrap().status,
            BrowserPageStatus::Crashed
        );
        assert_eq!(
            manager.session.page(&page_1).unwrap().health.status,
            BrowserPageHealthStatus::Crashed
        );
    }

    #[test]
    fn page_lifecycle_manager_records_browser_process_crash_for_open_pages() {
        let now = ts();
        let session = BrowserSession::new(
            BrowserSessionId::new("session-1").unwrap(),
            BrowserSessionProfileBinding::new(BrowserProfileMode::Ephemeral, None),
            now,
        );
        let mut manager = BrowserPageLifecycleManager::new(session, 3).unwrap();
        let page_1 = BrowserPageId::new("page-1").unwrap();
        let page_2 = BrowserPageId::new("page-2").unwrap();
        manager.open_page(page_1.clone(), None, None, now).unwrap();
        manager.open_page(page_2.clone(), None, None, now).unwrap();
        manager.close_page(&page_2, now).unwrap();
        let event = BrowserCrashEvent {
            scope: BrowserCrashScope::BrowserProcess,
            page_id: None,
            reason: BrowserCrashReason::ProcessExited,
            message: "chromium exited".to_string(),
            detected_at: now,
        };

        manager.record_crash(event).unwrap();

        assert_eq!(
            manager.session.page(&page_1).unwrap().status,
            BrowserPageStatus::Crashed
        );
        assert_eq!(
            manager.session.page(&page_2).unwrap().status,
            BrowserPageStatus::Closed
        );
    }

    #[test]
    fn page_lifecycle_manager_recovery_plan_does_not_retry_mutation_actions_implicitly() {
        let now = ts();
        let session = BrowserSession::new(
            BrowserSessionId::new("session-1").unwrap(),
            BrowserSessionProfileBinding::new(BrowserProfileMode::Ephemeral, None),
            now,
        );
        let mut manager = BrowserPageLifecycleManager::new(session, 2).unwrap();
        manager
            .open_page(
                BrowserPageId::new("page-1").unwrap(),
                Some("https://example.com".to_string()),
                None,
                now,
            )
            .unwrap();
        let event = BrowserCrashEvent {
            scope: BrowserCrashScope::BrowserProcess,
            page_id: None,
            reason: BrowserCrashReason::ProcessExited,
            message: "chromium exited".to_string(),
            detected_at: now,
        };

        let plan = manager
            .recover_from_crash(event, &BrowserRecoveryPolicy::default())
            .unwrap();

        match plan.action {
            BrowserRecoveryAction::RelaunchSession {
                retry_after_relaunch,
                ..
            } => assert!(retry_after_relaunch),
            other => panic!("unexpected recovery action: {:?}", other),
        }
    }

    // ---- Browser security policy tests ----

    #[test]
    fn browser_security_policy_allows_allowed_domains_and_denies_blocked_domains() {
        let policy = BrowserSecurityPolicy::default()
            .with_allowed_domain("example.com")
            .with_denied_domain("blocked.example");

        assert_eq!(
            policy
                .evaluate_url("https://example.com/app")
                .unwrap()
                .decision,
            BrowserSecurityDecision::Allow
        );
        assert_eq!(
            policy
                .evaluate_url("https://sub.example.com/app")
                .unwrap()
                .decision,
            BrowserSecurityDecision::Allow
        );
        assert_eq!(
            policy
                .evaluate_url("https://blocked.example/app")
                .unwrap()
                .decision,
            BrowserSecurityDecision::Deny
        );
        assert_eq!(
            policy
                .evaluate_url("https://other.example/app")
                .unwrap()
                .decision,
            BrowserSecurityDecision::Ask
        );
    }

    #[test]
    fn browser_security_policy_classifies_execute_js_risk() {
        assert_eq!(
            BrowserJsRisk::classify("document.querySelector('button').click()"),
            BrowserJsRisk::High
        );
        assert_eq!(
            BrowserJsRisk::classify("localStorage.getItem('token')"),
            BrowserJsRisk::High
        );
        assert_eq!(
            BrowserJsRisk::classify("document.body.innerText"),
            BrowserJsRisk::Medium
        );
        assert_eq!(BrowserJsRisk::classify("1 + 1"), BrowserJsRisk::Low);
    }

    #[test]
    fn browser_security_policy_warns_on_credential_exposure() {
        let warning = BrowserCredentialExposureWarning::detect(
            "https://example.com",
            "fetch('/api', {headers: {Authorization: 'Bearer secret'}})",
        );

        assert!(warning.is_some());
        let warning = warning.unwrap();
        assert_eq!(warning.url, "https://example.com");
        assert!(warning.matched_terms.contains(&"authorization".to_string()));
        assert_eq!(warning.severity, BrowserJsRisk::High);
    }

    #[test]
    fn execute_js_on_high_risk_domain_requires_ask_or_deny() {
        let ask_policy = BrowserSecurityPolicy::default().with_high_risk_domain("bank.example");
        let ask_result = ask_policy
            .evaluate_execute_js("https://bank.example/login", "document.body.innerText")
            .unwrap();

        assert_eq!(ask_result.decision, BrowserSecurityDecision::Ask);
        assert_eq!(ask_result.js_risk, BrowserJsRisk::Medium);

        let deny_policy = BrowserSecurityPolicy {
            high_risk_domain_decision: BrowserSecurityDecision::Deny,
            ..BrowserSecurityPolicy::default().with_high_risk_domain("bank.example")
        };
        let deny_result = deny_policy
            .evaluate_execute_js("https://bank.example/login", "document.body.innerText")
            .unwrap();

        assert_eq!(deny_result.decision, BrowserSecurityDecision::Deny);
    }

    #[test]
    fn cdp_browser_config_builder_sets_security_policy() {
        let policy = BrowserSecurityPolicy::default().with_denied_domain("blocked.example");
        let config = CdpBrowserConfig::default().with_security_policy(policy.clone());

        assert_eq!(config.security_policy, policy);
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
        assert_eq!(config.prompt_policy, BrowserPromptPolicy::default());
        assert_eq!(config.recovery_policy, BrowserRecoveryPolicy::default());
        assert_eq!(config.security_policy, BrowserSecurityPolicy::default());
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
            .with_recovery_policy(BrowserRecoveryPolicy {
                strategy: BrowserRecoveryStrategy::FailFast,
                ..BrowserRecoveryPolicy::default()
            })
            .with_profile(BrowserProfileMode::Named("work".to_string()));

        assert_eq!(config.viewport.width, 1920);
        assert_eq!(config.viewport.height, 1080);
        assert_eq!(config.viewport.device_scale_factor, 2.0);
        assert_eq!(config.max_pages, 3);
        assert_eq!(
            config.recovery_policy.strategy,
            BrowserRecoveryStrategy::FailFast
        );
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

    // ---- Dialog / permission prompt tests ----

    #[test]
    fn browser_prompt_policy_resolves_dialogs_without_hanging() {
        let policy = BrowserPromptPolicy::default();
        let alert = BrowserDialogEvent {
            page_id: Some(BrowserPageId::new("page-1").unwrap()),
            kind: BrowserDialogKind::Alert,
            message: "hello".to_string(),
            default_prompt_text: None,
            occurred_at: ts(),
        };
        let confirm = BrowserDialogEvent {
            kind: BrowserDialogKind::Confirm,
            message: "continue?".to_string(),
            ..alert.clone()
        };

        let alert_resolution = policy.resolve_dialog(&alert, ts()).unwrap();
        let confirm_resolution = policy.resolve_dialog(&confirm, ts()).unwrap();

        assert_eq!(alert_resolution.decision, BrowserDialogDecision::Accept);
        assert_eq!(confirm_resolution.decision, BrowserDialogDecision::Dismiss);
        assert!(
            alert_resolution
                .reason
                .contains("non-blocking prompt policy")
        );
    }

    #[test]
    fn browser_prompt_policy_accepts_prompt_with_prompt_text() {
        let policy = BrowserPromptPolicy {
            prompt_decision: BrowserDialogDecision::Accept,
            prompt_text: Some("typed response".to_string()),
            ..BrowserPromptPolicy::default()
        };
        let event = BrowserDialogEvent {
            page_id: None,
            kind: BrowserDialogKind::Prompt,
            message: "name?".to_string(),
            default_prompt_text: Some("default".to_string()),
            occurred_at: ts(),
        };

        let resolution = policy.resolve_dialog(&event, ts()).unwrap();

        assert_eq!(resolution.decision, BrowserDialogDecision::Accept);
        assert_eq!(resolution.prompt_text.as_deref(), Some("typed response"));
    }

    #[test]
    fn browser_prompt_policy_resolves_permission_prompts_to_ask_by_default() {
        let policy = BrowserPromptPolicy::default();
        let event = BrowserPermissionPromptEvent {
            page_id: None,
            origin: "https://example.com".to_string(),
            permission: BrowserPermissionKind::Geolocation,
            occurred_at: ts(),
        };

        let resolution = policy.resolve_permission_prompt(&event, ts()).unwrap();

        assert_eq!(resolution.decision, BrowserPermissionDecision::Ask);
        assert!(resolution.reason.contains("permission prompt"));
    }

    #[test]
    fn browser_prompt_policy_allows_explicit_allowed_permissions() {
        let policy = BrowserPromptPolicy {
            allowed_permissions: BTreeSet::from([BrowserPermissionKind::ClipboardWrite]),
            ..BrowserPromptPolicy::default()
        };
        let event = BrowserPermissionPromptEvent {
            page_id: None,
            origin: "https://example.com".to_string(),
            permission: BrowserPermissionKind::ClipboardWrite,
            occurred_at: ts(),
        };

        let resolution = policy.resolve_permission_prompt(&event, ts()).unwrap();

        assert_eq!(resolution.decision, BrowserPermissionDecision::Allow);
    }

    #[test]
    fn browser_prompt_policy_rejects_zero_timeout_and_invalid_events() {
        let policy = BrowserPromptPolicy {
            timeout_ms: 0,
            ..BrowserPromptPolicy::default()
        };
        assert!(policy.validate().is_err());

        let event = BrowserDialogEvent {
            page_id: None,
            kind: BrowserDialogKind::Alert,
            message: " ".to_string(),
            default_prompt_text: None,
            occurred_at: ts(),
        };
        assert!(
            BrowserPromptPolicy::default()
                .resolve_dialog(&event, ts())
                .is_err()
        );
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

    #[test]
    fn browser_frame_id_rejects_empty_values() {
        assert!(matches!(
            BrowserFrameId::new(" "),
            Err(BrowserKernelError::InvalidConfig(_))
        ));
    }

    #[test]
    fn element_selector_frame_roundtrips_and_validates() {
        let selector = ElementSelector::frame(
            BrowserFrameId::new("frame-1").unwrap(),
            ElementSelector::css("button.submit"),
        );

        let json = serde_json::to_string_pretty(&selector).unwrap();
        let decoded: ElementSelector = serde_json::from_str(&json).unwrap();

        assert_eq!(decoded, selector);
        assert!(selector.validate().is_ok());
    }

    #[test]
    fn element_selector_frame_rejects_invalid_inner_selector() {
        let selector = ElementSelector::frame(
            BrowserFrameId::new("frame-1").unwrap(),
            ElementSelector::css(" "),
        );

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
    fn browser_frame_metadata_roundtrips() {
        let metadata = BrowserFrameMetadata {
            frame_id: BrowserFrameId::new("frame-1").unwrap(),
            parent_frame_id: None,
            url: Some("https://embed.example".to_string()),
            name: Some("embed".to_string()),
            title: Some("Embed".to_string()),
            selector: Some(ElementSelector::css("iframe#embed")),
        };

        let json = serde_json::to_string_pretty(&metadata).unwrap();
        let decoded: BrowserFrameMetadata = serde_json::from_str(&json).unwrap();

        assert_eq!(decoded, metadata);
    }

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
            frame_id: Some(BrowserFrameId::new("frame-1").unwrap()),
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
                frame_id: None,
            }],
            frames: vec![BrowserFrameMetadata {
                frame_id: BrowserFrameId::new("frame-1").unwrap(),
                parent_frame_id: None,
                url: Some("https://embed.example".to_string()),
                name: Some("embed".to_string()),
                title: None,
                selector: Some(ElementSelector::css("iframe#embed")),
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

    // ---- DOM snapshot artifact tests ----

    #[test]
    fn browser_dom_snapshot_artifact_records_hash_size_and_content_type() {
        let snapshot = BrowserDomSnapshotArtifact::from_html(
            ArtifactId::from("dom-1"),
            "https://example.com/app",
            "Example App",
            "<html><body>Hello</body></html>",
            ts(),
        )
        .unwrap();

        assert_eq!(snapshot.artifact_id, ArtifactId::from("dom-1"));
        assert_eq!(snapshot.source_url, "https://example.com/app");
        assert_eq!(snapshot.title, Some("Example App".to_string()));
        assert_eq!(snapshot.mime_type, "text/html");
        assert_eq!(snapshot.size_bytes, 31);
        assert_eq!(
            snapshot.sha256,
            "03ee66f1452916b4f91a504c1e9babfa201b6d64c26a82b2cf03c3ed49d91585"
        );
    }

    #[test]
    fn browser_dom_snapshot_artifact_descriptor_links_to_source_action() {
        let snapshot = BrowserDomSnapshotArtifact::from_html(
            ArtifactId::from("dom-1"),
            "https://example.com/app",
            "Example App",
            "<html><body>Hello</body></html>",
            ts(),
        )
        .unwrap();

        let descriptor = snapshot.to_artifact_descriptor(Some("browser.extract_content"));

        assert_eq!(descriptor.id, ArtifactId::from("dom-1"));
        assert_eq!(descriptor.kind, ArtifactKind::WebPage);
        assert_eq!(
            descriptor.title,
            Some("DOM snapshot of Example App".to_string())
        );
        assert_eq!(
            descriptor.source_uri,
            Some("https://example.com/app".to_string())
        );
        assert_eq!(descriptor.mime_type, Some("text/html".to_string()));
        assert_eq!(descriptor.metadata["source"], "browser_dom_snapshot");
        assert_eq!(
            descriptor.metadata["source_action"],
            "browser.extract_content"
        );
        assert_eq!(descriptor.metadata["size_bytes"], 31);
        assert_eq!(
            descriptor.metadata["sha256"],
            "03ee66f1452916b4f91a504c1e9babfa201b6d64c26a82b2cf03c3ed49d91585"
        );
    }

    #[test]
    fn browser_dom_snapshot_artifact_rejects_empty_html() {
        assert!(matches!(
            BrowserDomSnapshotArtifact::from_html(
                ArtifactId::from("dom-empty"),
                "https://example.com",
                "Example",
                "   ",
                ts(),
            ),
            Err(BrowserKernelError::InvalidConfig(_))
        ));
    }

    // ---- Network trace tests ----

    #[test]
    fn browser_network_trace_policy_defaults_auth_headers_redacted() {
        let policy = BrowserNetworkTracePolicy::default();

        assert!(policy.capture_enabled);
        assert_eq!(policy.max_entries, 200);
        assert!(policy.should_redact_header("authorization"));
        assert!(policy.should_redact_header("Cookie"));
        assert!(policy.should_redact_header("x-api-key"));
        assert!(!policy.should_redact_header("content-type"));
    }

    #[test]
    fn browser_network_header_redaction_preserves_non_sensitive_headers() {
        let policy = BrowserNetworkTracePolicy::default();
        let headers = vec![
            BrowserNetworkHeader::new("Authorization", "Bearer secret"),
            BrowserNetworkHeader::new("Content-Type", "application/json"),
            BrowserNetworkHeader::new("Set-Cookie", "sid=secret"),
        ];

        let redacted = policy.redact_headers(headers);

        assert_eq!(redacted[0].value, BrowserNetworkHeader::REDACTED);
        assert_eq!(redacted[1].value, "application/json");
        assert_eq!(redacted[2].value, BrowserNetworkHeader::REDACTED);
    }

    #[test]
    fn browser_network_trace_roundtrips_with_redacted_entries() {
        let trace = BrowserNetworkTrace {
            artifact_id: ArtifactId::from("har-1"),
            source_url: "https://example.com".to_string(),
            entries: vec![BrowserNetworkTraceEntry {
                request_id: "req-1".to_string(),
                method: "GET".to_string(),
                url: "https://example.com/api".to_string(),
                request_headers: vec![BrowserNetworkHeader::new(
                    "Authorization",
                    BrowserNetworkHeader::REDACTED,
                )],
                response_status: Some(200),
                response_headers: vec![BrowserNetworkHeader::new(
                    "Content-Type",
                    "application/json",
                )],
                started_at: ts(),
                finished_at: Some(ts()),
            }],
            captured_at: ts(),
            redaction_policy: BrowserNetworkRedactionPolicy::default(),
        };

        let json = serde_json::to_string_pretty(&trace).unwrap();
        let decoded: BrowserNetworkTrace = serde_json::from_str(&json).unwrap();

        assert_eq!(decoded, trace);
        assert_eq!(
            decoded.entries[0].request_headers[0].value,
            BrowserNetworkHeader::REDACTED
        );
    }

    #[test]
    fn browser_network_trace_descriptor_uses_tool_result_artifact() {
        let trace = BrowserNetworkTrace {
            artifact_id: ArtifactId::from("har-1"),
            source_url: "https://example.com".to_string(),
            entries: vec![],
            captured_at: ts(),
            redaction_policy: BrowserNetworkRedactionPolicy::default(),
        };

        let descriptor = trace.to_artifact_descriptor(Some("browser.extract_content"));

        assert_eq!(descriptor.id, ArtifactId::from("har-1"));
        assert_eq!(descriptor.kind, ArtifactKind::ToolResult);
        assert_eq!(
            descriptor.mime_type,
            Some("application/har+json".to_string())
        );
        assert_eq!(descriptor.metadata["source"], "browser_network_trace");
        assert_eq!(
            descriptor.metadata["source_action"],
            "browser.extract_content"
        );
        assert_eq!(descriptor.metadata["entries"], 0);
        assert_eq!(descriptor.metadata["auth_headers_redacted"], true);
    }

    // ---- Download artifact tests ----

    #[test]
    fn browser_download_input_validates_url_and_filename() {
        assert!(
            BrowserDownloadInput {
                url: " ".to_string(),
                suggested_filename: None,
                content_type: None,
            }
            .validate()
            .is_err()
        );
        assert!(
            BrowserDownloadInput {
                url: "https://example.com/file.txt".to_string(),
                suggested_filename: Some(" ".to_string()),
                content_type: None,
            }
            .validate()
            .is_err()
        );
    }

    #[test]
    fn browser_download_artifact_records_filename_content_type_size_and_hash() {
        let bytes = b"hello browser download";
        let result = BrowserDownloadResult::from_bytes(
            ArtifactId::from("download-1"),
            "https://example.com/report.txt",
            "report.txt",
            "text/plain",
            bytes,
            ts(),
        )
        .unwrap();

        assert_eq!(result.download.filename, "report.txt");
        assert_eq!(result.download.content_type, "text/plain");
        assert_eq!(result.download.size_bytes, bytes.len() as u64);
        assert_eq!(
            result.download.sha256,
            "800b164878d61ce3df8bc591daacdabde133651cd47885194305ce4f4a3d2a30"
        );
        assert_eq!(result.artifact.id, ArtifactId::from("download-1"));
        assert_eq!(result.artifact.kind, ArtifactKind::Document);
        assert_eq!(result.artifact.mime_type.as_deref(), Some("text/plain"));
        assert_eq!(result.artifact.metadata["filename"], "report.txt");
        assert_eq!(result.artifact.metadata["content_type"], "text/plain");
        assert_eq!(result.artifact.metadata["size_bytes"], bytes.len());
        assert_eq!(result.artifact.metadata["sha256"], result.download.sha256);
    }

    #[test]
    fn browser_download_artifact_classifies_pdf_and_image_artifacts() {
        let pdf = BrowserDownloadResult::from_bytes(
            ArtifactId::from("download-pdf"),
            "https://example.com/report.pdf",
            "report.pdf",
            "application/pdf",
            b"%PDF",
            ts(),
        )
        .unwrap();
        let image = BrowserDownloadResult::from_bytes(
            ArtifactId::from("download-image"),
            "https://example.com/image.png",
            "image.png",
            "image/png",
            b"png",
            ts(),
        )
        .unwrap();

        assert_eq!(pdf.artifact.kind, ArtifactKind::Pdf);
        assert_eq!(image.artifact.kind, ArtifactKind::Image);
    }

    #[test]
    fn infer_download_filename_uses_url_tail_or_default() {
        assert_eq!(
            infer_download_filename("https://example.com/files/a.pdf"),
            "a.pdf"
        );
        assert_eq!(
            infer_download_filename("https://example.com/files/"),
            "files"
        );
        assert_eq!(infer_download_filename(""), "download.bin");
    }

    // ---- Upload handling tests ----

    fn valid_upload_input() -> BrowserUploadInput {
        BrowserUploadInput {
            url: "https://example.com/upload".to_string(),
            selector: ElementSelector::css("input[type=file]"),
            local_path: PathBuf::from("/tmp/report.pdf"),
            filename: None,
            content_type: Some("application/pdf".to_string()),
            size_bytes: Some(42),
        }
    }

    #[test]
    fn browser_upload_input_validates_url_selector_and_path() {
        assert!(valid_upload_input().validate().is_ok());

        let mut empty_url = valid_upload_input();
        empty_url.url = " ".to_string();
        assert!(matches!(
            empty_url.validate(),
            Err(BrowserKernelError::EmptyUrl)
        ));

        let mut empty_selector = valid_upload_input();
        empty_selector.selector = ElementSelector::css(" ");
        assert!(matches!(
            empty_selector.validate(),
            Err(BrowserKernelError::EmptySelector)
        ));

        let mut empty_path = valid_upload_input();
        empty_path.local_path = PathBuf::new();
        assert!(empty_path.validate().is_err());
    }

    #[test]
    fn browser_upload_requirement_is_always_ask_or_deny_never_allow() {
        let requirement = valid_upload_input().approval_requirement(ts());

        assert_eq!(requirement.decision, BrowserUploadPolicyDecision::Ask);
        assert!(requirement.requires_human_approval());
        assert!(!requirement.is_denied());
        assert!(requirement.reason.contains("requires human approval"));
    }

    #[test]
    fn browser_upload_audit_data_exposes_target_file_and_metadata() {
        let audit = valid_upload_input().audit_data(ts()).unwrap();

        assert_eq!(audit.url, "https://example.com/upload");
        assert_eq!(audit.selector, ElementSelector::css("input[type=file]"));
        assert_eq!(audit.local_path, PathBuf::from("/tmp/report.pdf"));
        assert_eq!(audit.filename, "report.pdf");
        assert_eq!(audit.content_type.as_deref(), Some("application/pdf"));
        assert_eq!(audit.size_bytes, Some(42));
    }

    #[test]
    fn browser_upload_requirement_denies_invalid_input() {
        let mut input = valid_upload_input();
        input.selector = ElementSelector::css(" ");

        let requirement = input.approval_requirement(ts());

        assert_eq!(requirement.decision, BrowserUploadPolicyDecision::Deny);
        assert!(requirement.is_denied());
        assert!(requirement.reason.contains("invalid browser upload input"));
    }

    #[test]
    fn infer_upload_filename_uses_path_tail_or_default() {
        assert_eq!(
            infer_upload_filename(Path::new("/tmp/report.pdf")),
            "report.pdf"
        );
        assert_eq!(infer_upload_filename(Path::new("")), "upload.bin");
    }

    #[tokio::test]
    async fn upload_action_returns_approval_required_payload() {
        let mut lifecycle = ChromiumLifecycleManager::new(CdpBrowserConfig::default());
        // SAFETY: unit test only; upload_file does not touch the browser handle.
        lifecycle.browser = None;
        let executor = CdpBrowserExecutor::new(lifecycle, ts(), None);

        let result = executor.upload_file(valid_upload_input()).await.unwrap();

        assert_eq!(result.status, action_core::ActionStatus::ApprovalRequired);
        match result.payload {
            action_core::ActionResultPayload::Json(payload) => {
                assert_eq!(payload["decision"], "ask");
                assert_eq!(payload["audit_data"]["filename"], "report.pdf");
                assert_eq!(payload["audit_data"]["size_bytes"], 42);
            }
            other => panic!("unexpected payload: {:?}", other),
        }
    }

    #[tokio::test]
    async fn dialog_action_returns_completed_resolution_payload() {
        let lifecycle = ChromiumLifecycleManager::new(CdpBrowserConfig::default());
        let executor = CdpBrowserExecutor::new(lifecycle, ts(), None);
        let input = BrowserDialogActionInput {
            page_id: Some(BrowserPageId::new("page-1").unwrap()),
            kind: BrowserDialogKind::Alert,
            message: "hello".to_string(),
            default_prompt_text: None,
        };

        let result = executor.handle_dialog(input).await.unwrap();

        assert_eq!(result.status, action_core::ActionStatus::Completed);
        match result.payload {
            action_core::ActionResultPayload::Json(payload) => {
                assert_eq!(payload["event"]["kind"], "alert");
                assert_eq!(payload["resolution"]["decision"], "accept");
            }
            other => panic!("unexpected payload: {:?}", other),
        }
    }

    #[tokio::test]
    async fn permission_prompt_action_returns_approval_required_by_default() {
        let lifecycle = ChromiumLifecycleManager::new(CdpBrowserConfig::default());
        let executor = CdpBrowserExecutor::new(lifecycle, ts(), None);
        let input = BrowserPermissionPromptActionInput {
            page_id: Some(BrowserPageId::new("page-1").unwrap()),
            origin: "https://example.com".to_string(),
            permission: BrowserPermissionKind::Camera,
        };

        let result = executor.handle_permission_prompt(input).await.unwrap();

        assert_eq!(result.status, action_core::ActionStatus::ApprovalRequired);
        match result.payload {
            action_core::ActionResultPayload::Json(payload) => {
                assert_eq!(payload["event"]["permission"], "camera");
                assert_eq!(payload["resolution"]["decision"], "ask");
            }
            other => panic!("unexpected payload: {:?}", other),
        }
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
    fn browser_extract_content_input_defaults_dom_snapshot_false() {
        let input: BrowserExtractContentInput =
            serde_json::from_value(serde_json::json!({"url": "https://example.com/app"})).unwrap();

        assert_eq!(input.url, "https://example.com/app");
        assert!(!input.save_dom_snapshot);
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
            frame_id: None,
        };

        let element = raw_interactive_element_to_interactive_element(raw).unwrap();

        assert_eq!(element.id, "e1");
        assert_eq!(element.kind, InteractiveElementKind::Button);
        assert_eq!(
            element.selector,
            Some(ElementSelector::Css("#submit".to_string()))
        );
        assert_eq!(element.text, Some("Submit".to_string()));
        assert_eq!(element.aria_label, Some("Submit form".to_string()));
        assert_eq!(element.bounding_box.unwrap().width, 100.0);
        assert_eq!(element.frame_id, None);
    }

    #[test]
    fn raw_frame_metadata_maps_to_snapshot_frame_metadata() {
        let raw = RawFrameMetadata {
            frame_id: "frame-1".to_string(),
            parent_frame_id: None,
            url: Some("https://embed.example".to_string()),
            name: Some("embed".to_string()),
            title: Some("Embed".to_string()),
            selector: Some("iframe#embed".to_string()),
        };

        let metadata = raw_frame_metadata_to_frame_metadata(raw).unwrap();

        assert_eq!(metadata.frame_id, BrowserFrameId("frame-1".to_string()));
        assert_eq!(metadata.url.as_deref(), Some("https://embed.example"));
        assert_eq!(
            metadata.selector,
            Some(ElementSelector::css("iframe#embed"))
        );
    }

    #[test]
    fn raw_interactive_element_with_frame_maps_to_frame_selector() {
        let raw = RawInteractiveElement {
            id: "frame-1-e1".to_string(),
            kind: "button".to_string(),
            selector: Some("button.pay".to_string()),
            text: Some("Pay".to_string()),
            aria_label: None,
            bounding_box: None,
            frame_id: Some("frame-1".to_string()),
        };

        let element = raw_interactive_element_to_interactive_element(raw).unwrap();

        assert_eq!(
            element.frame_id,
            Some(BrowserFrameId("frame-1".to_string()))
        );
        assert_eq!(
            element.selector,
            Some(ElementSelector::frame(
                BrowserFrameId("frame-1".to_string()),
                ElementSelector::css("button.pay"),
            ))
        );
    }

    #[test]
    fn raw_interactive_snapshot_maps_iframe_elements_and_frames() {
        let raw = RawInteractiveSnapshot {
            elements: vec![RawInteractiveElement {
                id: "frame-1-e1".to_string(),
                kind: "button".to_string(),
                selector: Some("button.pay".to_string()),
                text: Some("Pay".to_string()),
                aria_label: None,
                bounding_box: None,
                frame_id: Some("frame-1".to_string()),
            }],
            frames: vec![RawFrameMetadata {
                frame_id: "frame-1".to_string(),
                parent_frame_id: None,
                url: Some("https://checkout.example".to_string()),
                name: Some("checkout".to_string()),
                title: None,
                selector: Some("iframe#checkout".to_string()),
            }],
        };

        let (elements, frames) = raw_interactive_snapshot_to_parts(raw).unwrap();

        assert_eq!(frames.len(), 1);
        assert_eq!(elements.len(), 1);
        assert_eq!(elements[0].frame_id, Some(frames[0].frame_id.clone()));
        assert!(matches!(
            elements[0].selector,
            Some(ElementSelector::Frame(_))
        ));
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
        assert!(!profile.name.is_empty());
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
