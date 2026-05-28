//! # Notification Core
//!
//! Domain types, sinks, and storage for AgentOS Notifications.
//!
//! Notifications are emitted by the scheduler when reminders become due,
//! or by other subsystems for system alerts and informational messages.

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt;
use std::sync::Mutex;

// ---------------------------------------------------------------------------
// Core types
// ---------------------------------------------------------------------------

/// Unique identifier for a notification.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct NotificationId(pub String);

impl fmt::Display for NotificationId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl From<String> for NotificationId {
    fn from(s: String) -> Self {
        Self(s)
    }
}

impl From<&str> for NotificationId {
    fn from(s: &str) -> Self {
        Self(s.to_string())
    }
}

/// Type of notification.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum NotificationType {
    /// Emitted when a reminder becomes due.
    ReminderDue,
    /// System alert (errors, warnings).
    SystemAlert,
    /// Informational message.
    Info,
}

impl fmt::Display for NotificationType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            NotificationType::ReminderDue => write!(f, "ReminderDue"),
            NotificationType::SystemAlert => write!(f, "SystemAlert"),
            NotificationType::Info => write!(f, "Info"),
        }
    }
}

/// A notification entity.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Notification {
    pub id: NotificationId,
    pub title: String,
    pub body: String,
    pub notification_type: NotificationType,
    pub source_reminder_id: Option<String>,
    pub created_at: DateTime<Utc>,
    pub read_at: Option<DateTime<Utc>>,
}

// ---------------------------------------------------------------------------
// NotificationSink trait
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum NotificationError {
    #[error("notification not found: {0}")]
    NotFound(String),
    #[error("internal error: {0}")]
    Internal(String),
}

/// Trait for emitting notifications.
#[async_trait]
pub trait NotificationSink: Send + Sync {
    async fn emit(&self, notification: &Notification) -> Result<(), NotificationError>;
}

// ---------------------------------------------------------------------------
// FakeNotificationSink — collects notifications for testing
// ---------------------------------------------------------------------------

pub struct FakeNotificationSink {
    emitted: Mutex<Vec<Notification>>,
}

impl FakeNotificationSink {
    pub fn new() -> Self {
        Self {
            emitted: Mutex::new(Vec::new()),
        }
    }

    /// Get all emitted notifications.
    pub fn emitted(&self) -> Vec<Notification> {
        self.emitted.lock().unwrap().clone()
    }

    /// Count of emitted notifications.
    pub fn count(&self) -> usize {
        self.emitted.lock().unwrap().len()
    }
}

impl Default for FakeNotificationSink {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl NotificationSink for FakeNotificationSink {
    async fn emit(&self, notification: &Notification) -> Result<(), NotificationError> {
        self.emitted.lock().unwrap().push(notification.clone());
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// MemoryNotificationStore
// ---------------------------------------------------------------------------

/// Trait for storing and querying notifications.
#[async_trait]
pub trait NotificationStore: Send + Sync {
    async fn save(&self, notification: &Notification) -> Result<(), NotificationError>;
    async fn get(&self, id: &NotificationId) -> Result<Notification, NotificationError>;
    async fn list(&self) -> Result<Vec<Notification>, NotificationError>;
    async fn list_unread(&self) -> Result<Vec<Notification>, NotificationError>;
    async fn mark_read(&self, id: &NotificationId) -> Result<(), NotificationError>;
}

pub struct MemoryNotificationStore {
    inner: Mutex<HashMap<String, Notification>>,
}

impl MemoryNotificationStore {
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(HashMap::new()),
        }
    }
}

impl Default for MemoryNotificationStore {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl NotificationStore for MemoryNotificationStore {
    async fn save(&self, notification: &Notification) -> Result<(), NotificationError> {
        let mut store = self.inner.lock().unwrap();
        store.insert(notification.id.0.clone(), notification.clone());
        Ok(())
    }

    async fn get(&self, id: &NotificationId) -> Result<Notification, NotificationError> {
        let store = self.inner.lock().unwrap();
        store
            .get(&id.0)
            .cloned()
            .ok_or_else(|| NotificationError::NotFound(id.0.clone()))
    }

    async fn list(&self) -> Result<Vec<Notification>, NotificationError> {
        let store = self.inner.lock().unwrap();
        Ok(store.values().cloned().collect())
    }

    async fn list_unread(&self) -> Result<Vec<Notification>, NotificationError> {
        let store = self.inner.lock().unwrap();
        Ok(store
            .values()
            .filter(|n| n.read_at.is_none())
            .cloned()
            .collect())
    }

    async fn mark_read(&self, id: &NotificationId) -> Result<(), NotificationError> {
        let mut store = self.inner.lock().unwrap();
        if let Some(notification) = store.get_mut(&id.0) {
            notification.read_at = Some(Utc::now());
            Ok(())
        } else {
            Err(NotificationError::NotFound(id.0.clone()))
        }
    }
}

// ===========================================================================
// macOS Notification Sink (Feature Gated)
// ===========================================================================

/// Configuration for macOS notifications.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MacOsNotificationConfig {
    pub app_name: String,
    pub sound: Option<String>,
}

impl Default for MacOsNotificationConfig {
    fn default() -> Self {
        Self {
            app_name: "AgentOS".to_string(),
            sound: Some("default".to_string()),
        }
    }
}

/// Mock macOS notification sink for testing.
/// Records all emitted notifications without actually sending to macOS.
pub struct MacOsMockNotificationSink {
    config: MacOsNotificationConfig,
    emitted: Mutex<Vec<Notification>>,
}

impl MacOsMockNotificationSink {
    pub fn new() -> Self {
        Self {
            config: MacOsNotificationConfig::default(),
            emitted: Mutex::new(Vec::new()),
        }
    }

    pub fn with_config(config: MacOsNotificationConfig) -> Self {
        Self {
            config,
            emitted: Mutex::new(Vec::new()),
        }
    }

    /// Get all emitted notifications.
    pub fn all(&self) -> Vec<Notification> {
        self.emitted.lock().unwrap().clone()
    }

    /// Get the last emitted notification.
    pub fn last(&self) -> Option<Notification> {
        self.emitted.lock().unwrap().last().cloned()
    }

    /// Count of emitted notifications.
    pub fn count(&self) -> usize {
        self.emitted.lock().unwrap().len()
    }

    /// Clear all recorded notifications.
    pub fn clear(&self) {
        self.emitted.lock().unwrap().clear();
    }

    /// Get the configuration.
    pub fn config(&self) -> &MacOsNotificationConfig {
        &self.config
    }
}

impl Default for MacOsMockNotificationSink {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl NotificationSink for MacOsMockNotificationSink {
    async fn emit(&self, notification: &Notification) -> Result<(), NotificationError> {
        self.emitted.lock().unwrap().push(notification.clone());
        Ok(())
    }
}

// ===========================================================================
// Real macOS Notification Sink (Feature Gated)
// ===========================================================================

/// macOS notification sink enabled by the `macos` feature.
///
/// The current core crate keeps platform integration behind this sink boundary so
/// callers can use the same `NotificationSink` interface in tests, CLI tools, and
/// host applications.
#[cfg(feature = "macos")]
pub struct MacOsNotificationSink {
    config: MacOsNotificationConfig,
}

#[cfg(feature = "macos")]
impl MacOsNotificationSink {
    pub fn new(config: MacOsNotificationConfig) -> Self {
        Self { config }
    }
}

#[cfg(feature = "macos")]
#[async_trait]
impl NotificationSink for MacOsNotificationSink {
    async fn emit(&self, notification: &Notification) -> Result<(), NotificationError> {
        // Platform hosts can replace this boundary with a native notification
        // adapter while preserving the core crate's dependency-light API.
        println!(
            "[{}] {}: {}",
            self.config.app_name, notification.title, notification.body
        );
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn ts() -> DateTime<Utc> {
        "2026-05-24T12:00:00Z".parse().unwrap()
    }

    fn sample_notification(id: &str) -> Notification {
        Notification {
            id: NotificationId::from(id),
            title: "Reminder Due".to_string(),
            body: "Time to send the email!".to_string(),
            notification_type: NotificationType::ReminderDue,
            source_reminder_id: Some("reminder-1".to_string()),
            created_at: ts(),
            read_at: None,
        }
    }

    // ---- Type roundtrip tests ----

    #[test]
    fn notification_id_roundtrips() {
        let id = NotificationId::from("notif-1");
        assert_eq!(id.to_string(), "notif-1");
        let json = serde_json::to_string(&id).unwrap();
        let decoded: NotificationId = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded, id);
    }

    #[test]
    fn notification_roundtrips() {
        let notification = sample_notification("notif-1");
        let json = serde_json::to_string_pretty(&notification).unwrap();
        let decoded: Notification = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded, notification);
    }

    #[test]
    fn notification_type_roundtrips() {
        let types = vec![
            NotificationType::ReminderDue,
            NotificationType::SystemAlert,
            NotificationType::Info,
        ];
        for t in types {
            let json = serde_json::to_string(&t).unwrap();
            let decoded: NotificationType = serde_json::from_str(&json).unwrap();
            assert_eq!(decoded, t);
        }
    }

    // ---- FakeNotificationSink tests ----

    #[tokio::test]
    async fn fake_sink_collects_emitted_notifications() {
        let sink = FakeNotificationSink::new();
        let n1 = sample_notification("n-1");
        let n2 = sample_notification("n-2");
        sink.emit(&n1).await.unwrap();
        sink.emit(&n2).await.unwrap();
        assert_eq!(sink.count(), 2);
        assert_eq!(sink.emitted()[0].id, NotificationId::from("n-1"));
        assert_eq!(sink.emitted()[1].id, NotificationId::from("n-2"));
    }

    #[tokio::test]
    async fn fake_sink_starts_empty() {
        let sink = FakeNotificationSink::new();
        assert_eq!(sink.count(), 0);
        assert!(sink.emitted().is_empty());
    }

    // ---- MemoryNotificationStore tests ----

    #[tokio::test]
    async fn store_save_and_get() {
        let store = MemoryNotificationStore::new();
        let notification = sample_notification("n-1");
        store.save(&notification).await.unwrap();
        let fetched = store.get(&NotificationId::from("n-1")).await.unwrap();
        assert_eq!(fetched, notification);
    }

    #[tokio::test]
    async fn store_list_unread() {
        let store = MemoryNotificationStore::new();
        let n1 = sample_notification("n-1");
        store.save(&n1).await.unwrap();
        let n2 = sample_notification("n-2");
        store.save(&n2).await.unwrap();
        // Mark n1 as read
        store.mark_read(&NotificationId::from("n-1")).await.unwrap();
        let unread = store.list_unread().await.unwrap();
        assert_eq!(unread.len(), 1);
        assert_eq!(unread[0].id, NotificationId::from("n-2"));
    }

    #[tokio::test]
    async fn store_mark_read_sets_read_at() {
        let store = MemoryNotificationStore::new();
        let notification = sample_notification("n-1");
        store.save(&notification).await.unwrap();
        store.mark_read(&NotificationId::from("n-1")).await.unwrap();
        let fetched = store.get(&NotificationId::from("n-1")).await.unwrap();
        assert!(fetched.read_at.is_some());
    }

    #[tokio::test]
    async fn store_get_not_found() {
        let store = MemoryNotificationStore::new();
        let result = store.get(&NotificationId::from("nonexistent")).await;
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            NotificationError::NotFound(_)
        ));
    }

    // -----------------------------------------------------------------------
    // PR 150: macOS notification sink
    // -----------------------------------------------------------------------

    #[test]
    fn macos_notification_sink_config_default() {
        let config = MacOsNotificationConfig::default();
        assert_eq!(config.app_name, "AgentOS");
        assert!(config.sound.is_some());
    }

    #[test]
    fn macos_notification_sink_config_custom() {
        let config = MacOsNotificationConfig {
            app_name: "MyApp".to_string(),
            sound: Some("Ping".to_string()),
        };
        assert_eq!(config.app_name, "MyApp");
        assert_eq!(config.sound.as_deref(), Some("Ping"));
    }

    #[tokio::test]
    async fn macos_mock_sink_records_notifications() {
        let sink = MacOsMockNotificationSink::new();
        let notification = Notification {
            id: NotificationId::from("n-1"),
            title: "Test".to_string(),
            body: "Body".to_string(),
            notification_type: NotificationType::ReminderDue,
            source_reminder_id: None,
            created_at: Utc::now(),
            read_at: None,
        };

        sink.emit(&notification).await.unwrap();
        assert_eq!(sink.count(), 1);
        assert_eq!(sink.last().unwrap().id.0, "n-1");
    }

    #[tokio::test]
    async fn macos_mock_sink_records_multiple() {
        let sink = MacOsMockNotificationSink::new();

        for i in 0..3 {
            let notification = Notification {
                id: NotificationId::from(format!("n-{}", i)),
                title: format!("Title {}", i),
                body: format!("Body {}", i),
                notification_type: NotificationType::Info,
                source_reminder_id: None,
                created_at: Utc::now(),
                read_at: None,
            };
            sink.emit(&notification).await.unwrap();
        }

        assert_eq!(sink.count(), 3);
        let all = sink.all();
        assert_eq!(all[0].id.0, "n-0");
        assert_eq!(all[1].id.0, "n-1");
        assert_eq!(all[2].id.0, "n-2");
    }

    #[tokio::test]
    async fn macos_mock_sink_clear() {
        let sink = MacOsMockNotificationSink::new();
        let notification = Notification {
            id: NotificationId::from("n-1"),
            title: "Test".to_string(),
            body: "Body".to_string(),
            notification_type: NotificationType::ReminderDue,
            source_reminder_id: None,
            created_at: Utc::now(),
            read_at: None,
        };

        sink.emit(&notification).await.unwrap();
        assert_eq!(sink.count(), 1);

        sink.clear();
        assert_eq!(sink.count(), 0);
    }

    #[test]
    fn macos_mock_sink_default_is_empty() {
        let sink = MacOsMockNotificationSink::new();
        assert_eq!(sink.count(), 0);
        assert!(sink.last().is_none());
        assert!(sink.all().is_empty());
    }
}
