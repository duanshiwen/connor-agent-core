//! # Scheduler
//!
//! In-process scheduler for AgentOS — manages scheduled jobs and polls due items.
//!
//! The scheduler does not run as a background daemon. It provides a `poll_due_jobs()`
//! method that the caller invokes at appropriate intervals. This makes it fully
//! testable with fake clocks.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt;
use std::sync::Mutex;

// ---------------------------------------------------------------------------
// Core types
// ---------------------------------------------------------------------------

/// Unique identifier for a scheduled job.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ScheduledJobId(pub String);

impl fmt::Display for ScheduledJobId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl From<String> for ScheduledJobId {
    fn from(s: String) -> Self {
        Self(s)
    }
}

impl From<&str> for ScheduledJobId {
    fn from(s: &str) -> Self {
        Self(s.to_string())
    }
}

/// Status of a scheduled job.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ScheduledJobStatus {
    /// Waiting to fire.
    Pending,
    /// Has been fired (due time reached).
    Fired,
    /// Cancelled before firing.
    Cancelled,
}

impl fmt::Display for ScheduledJobStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ScheduledJobStatus::Pending => write!(f, "Pending"),
            ScheduledJobStatus::Fired => write!(f, "Fired"),
            ScheduledJobStatus::Cancelled => write!(f, "Cancelled"),
        }
    }
}

/// A scheduled job — linked to a reminder, fires at due_at.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ScheduledJob {
    pub id: ScheduledJobId,
    pub reminder_id: String,
    pub due_at: DateTime<Utc>,
    pub status: ScheduledJobStatus,
    pub created_at: DateTime<Utc>,
}

// ---------------------------------------------------------------------------
// Clock trait
// ---------------------------------------------------------------------------

/// Trait for providing the current time. Enables deterministic testing.
pub trait Clock: Send + Sync {
    fn now(&self) -> DateTime<Utc>;
}

/// Real clock using Utc::now().
pub struct RealClock;

impl Clock for RealClock {
    fn now(&self) -> DateTime<Utc> {
        Utc::now()
    }
}

/// Fake clock with manually controllable time.
pub struct FakeClock {
    now: Mutex<DateTime<Utc>>,
}

impl FakeClock {
    pub fn new(initial: DateTime<Utc>) -> Self {
        Self {
            now: Mutex::new(initial),
        }
    }

    /// Advance the clock by the given duration.
    pub fn advance(&self, duration: chrono::Duration) {
        let mut now = self.now.lock().unwrap();
        *now += duration;
    }

    /// Set the clock to a specific time.
    pub fn set(&self, time: DateTime<Utc>) {
        let mut now = self.now.lock().unwrap();
        *now = time;
    }
}

// ---------------------------------------------------------------------------
// Persistent Filesystem Scheduler Store (JSONL)
// ---------------------------------------------------------------------------

/// Persistent scheduler store using JSONL file format.
///
/// Each line in the file is a JSON object representing a ScheduledJob.
/// Jobs are loaded on startup and appended on save.
pub struct FsJsonlSchedulerStore {
    path: std::path::PathBuf,
    inner: Mutex<HashMap<String, ScheduledJob>>,
}

impl FsJsonlSchedulerStore {
    /// Create a new FsJsonlSchedulerStore.
    /// Loads existing jobs from the file if it exists.
    pub fn new(path: impl Into<std::path::PathBuf>) -> Result<Self, SchedulerError> {
        let path = path.into();
        let inner = if path.exists() {
            Self::load_from_file(&path)?
        } else {
            HashMap::new()
        };
        Ok(Self {
            path,
            inner: Mutex::new(inner),
        })
    }

    fn load_from_file(
        path: &std::path::Path,
    ) -> Result<HashMap<String, ScheduledJob>, SchedulerError> {
        let content = std::fs::read_to_string(path)
            .map_err(|e| SchedulerError::Internal(format!("failed to read store file: {}", e)))?;
        let mut jobs = HashMap::new();
        for line in content.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            let job: ScheduledJob = serde_json::from_str(line)
                .map_err(|e| SchedulerError::Internal(format!("failed to parse job: {}", e)))?;
            jobs.insert(job.id.0.clone(), job);
        }
        Ok(jobs)
    }

    fn persist_all(&self) -> Result<(), SchedulerError> {
        let store = self.inner.lock().unwrap();
        let mut content = String::new();
        for job in store.values() {
            let line = serde_json::to_string(job)
                .map_err(|e| SchedulerError::Internal(format!("failed to serialize job: {}", e)))?;
            content.push_str(&line);
            content.push('\n');
        }
        std::fs::write(&self.path, content)
            .map_err(|e| SchedulerError::Internal(format!("failed to write store file: {}", e)))?;
        Ok(())
    }

    /// Save or update a job and persist to disk.
    pub fn save(&self, job: &ScheduledJob) -> Result<(), SchedulerError> {
        let mut store = self.inner.lock().unwrap();
        store.insert(job.id.0.clone(), job.clone());
        drop(store);
        self.persist_all()
    }

    /// Get a job by ID.
    pub fn get(&self, id: &ScheduledJobId) -> Result<ScheduledJob, SchedulerError> {
        let store = self.inner.lock().unwrap();
        store
            .get(&id.0)
            .cloned()
            .ok_or_else(|| SchedulerError::NotFound(id.0.clone()))
    }

    /// List all jobs.
    pub fn list(&self) -> Vec<ScheduledJob> {
        let store = self.inner.lock().unwrap();
        store.values().cloned().collect()
    }

    /// Update a job's status and persist to disk.
    pub fn update_status(
        &self,
        id: &ScheduledJobId,
        status: ScheduledJobStatus,
    ) -> Result<(), SchedulerError> {
        let mut store = self.inner.lock().unwrap();
        if let Some(job) = store.get_mut(&id.0) {
            job.status = status;
            drop(store);
            self.persist_all()
        } else {
            Err(SchedulerError::NotFound(id.0.clone()))
        }
    }

    /// Delete a job and persist to disk.
    pub fn delete(&self, id: &ScheduledJobId) -> Result<(), SchedulerError> {
        let mut store = self.inner.lock().unwrap();
        store.remove(&id.0);
        drop(store);
        self.persist_all()
    }

    /// Get the file path.
    pub fn path(&self) -> &std::path::Path {
        &self.path
    }
}

impl Clock for FakeClock {
    fn now(&self) -> DateTime<Utc> {
        *self.now.lock().unwrap()
    }
}

// ---------------------------------------------------------------------------
// Scheduler errors
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SchedulerError {
    #[error("job not found: {0}")]
    NotFound(String),
    #[error("internal error: {0}")]
    Internal(String),
}

// ---------------------------------------------------------------------------
// MemorySchedulerStore
// ---------------------------------------------------------------------------

/// In-memory store for scheduled jobs.
pub struct MemorySchedulerStore {
    inner: Mutex<HashMap<String, ScheduledJob>>,
}

impl MemorySchedulerStore {
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(HashMap::new()),
        }
    }

    /// Save or update a job.
    pub fn save(&self, job: &ScheduledJob) -> Result<(), SchedulerError> {
        let mut store = self.inner.lock().unwrap();
        store.insert(job.id.0.clone(), job.clone());
        Ok(())
    }

    /// Get a job by ID.
    pub fn get(&self, id: &ScheduledJobId) -> Result<ScheduledJob, SchedulerError> {
        let store = self.inner.lock().unwrap();
        store
            .get(&id.0)
            .cloned()
            .ok_or_else(|| SchedulerError::NotFound(id.0.clone()))
    }

    /// List all jobs.
    pub fn list(&self) -> Vec<ScheduledJob> {
        let store = self.inner.lock().unwrap();
        store.values().cloned().collect()
    }

    /// Update a job's status.
    pub fn update_status(
        &self,
        id: &ScheduledJobId,
        status: ScheduledJobStatus,
    ) -> Result<(), SchedulerError> {
        let mut store = self.inner.lock().unwrap();
        if let Some(job) = store.get_mut(&id.0) {
            job.status = status;
            Ok(())
        } else {
            Err(SchedulerError::NotFound(id.0.clone()))
        }
    }
}

impl Default for MemorySchedulerStore {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Scheduler
// ---------------------------------------------------------------------------

/// In-process scheduler that polls for due jobs.
pub struct Scheduler {
    store: MemorySchedulerStore,
    clock: std::sync::Arc<dyn Clock>,
}

impl Scheduler {
    pub fn new(clock: std::sync::Arc<dyn Clock>) -> Self {
        Self {
            store: MemorySchedulerStore::new(),
            clock,
        }
    }

    /// Schedule a new job.
    pub fn schedule(
        &self,
        id: ScheduledJobId,
        reminder_id: String,
        due_at: DateTime<Utc>,
        created_at: DateTime<Utc>,
    ) -> Result<(), SchedulerError> {
        let job = ScheduledJob {
            id,
            reminder_id,
            due_at,
            status: ScheduledJobStatus::Pending,
            created_at,
        };
        self.store.save(&job)
    }

    /// Poll for due jobs — returns jobs where due_at ≤ now and status is Pending.
    /// Marks returned jobs as Fired.
    pub fn poll_due_jobs(&self) -> Vec<ScheduledJob> {
        let now = self.clock.now();
        let store = &self.store;
        let all_jobs = store.list();
        let mut due_jobs = Vec::new();

        for job in all_jobs {
            if job.status == ScheduledJobStatus::Pending && job.due_at <= now {
                store
                    .update_status(&job.id, ScheduledJobStatus::Fired)
                    .unwrap();
                let mut fired_job = job;
                fired_job.status = ScheduledJobStatus::Fired;
                due_jobs.push(fired_job);
            }
        }

        due_jobs
    }

    /// Cancel a scheduled job.
    pub fn cancel(&self, id: &ScheduledJobId) -> Result<(), SchedulerError> {
        self.store.update_status(id, ScheduledJobStatus::Cancelled)
    }

    /// Get a job by ID.
    pub fn get(&self, id: &ScheduledJobId) -> Result<ScheduledJob, SchedulerError> {
        self.store.get(id)
    }

    /// List all jobs.
    pub fn list(&self) -> Vec<ScheduledJob> {
        self.store.list()
    }
}

// ---------------------------------------------------------------------------
// ReminderNotificationBridge
// ---------------------------------------------------------------------------

/// Bridge that connects the scheduler to the notification system.
/// Polls for due jobs and emits notifications for each one.
pub struct ReminderNotificationBridge {
    scheduler: Scheduler,
    sink: std::sync::Arc<dyn notification_core::NotificationSink>,
    store: std::sync::Arc<dyn notification_core::NotificationStore>,
}

impl ReminderNotificationBridge {
    pub fn new(
        scheduler: Scheduler,
        sink: std::sync::Arc<dyn notification_core::NotificationSink>,
        store: std::sync::Arc<dyn notification_core::NotificationStore>,
    ) -> Self {
        Self {
            scheduler,
            sink,
            store,
        }
    }

    /// Poll scheduler for due jobs, create notifications, emit and store them.
    pub async fn process_due(&self) -> Vec<notification_core::Notification> {
        let due_jobs = self.scheduler.poll_due_jobs();
        let mut notifications = Vec::new();

        for job in due_jobs {
            let notification = notification_core::Notification {
                id: notification_core::NotificationId(format!("notif-{}", job.id)),
                title: "Reminder Due".to_string(),
                body: format!("Reminder {} is due.", job.reminder_id),
                notification_type: notification_core::NotificationType::ReminderDue,
                source_reminder_id: Some(job.reminder_id.clone()),
                created_at: self.scheduler.clock.now(),
                read_at: None,
            };

            // Emit to sink
            if let Err(e) = self.sink.emit(&notification).await {
                eprintln!("Failed to emit notification: {}", e);
            }

            // Store
            if let Err(e) = self.store.save(&notification).await {
                eprintln!("Failed to store notification: {}", e);
            }

            notifications.push(notification);
        }

        notifications
    }

    /// Access the scheduler.
    pub fn scheduler(&self) -> &Scheduler {
        &self.scheduler
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use notification_core::NotificationStore;

    fn ts() -> DateTime<Utc> {
        "2026-05-24T12:00:00Z".parse().unwrap()
    }

    // ---- Type roundtrip tests ----

    #[test]
    fn scheduled_job_id_roundtrips() {
        let id = ScheduledJobId::from("job-1");
        assert_eq!(id.to_string(), "job-1");
        let json = serde_json::to_string(&id).unwrap();
        let decoded: ScheduledJobId = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded, id);
    }

    #[test]
    fn scheduled_job_roundtrips() {
        let job = ScheduledJob {
            id: ScheduledJobId::from("job-1"),
            reminder_id: "reminder-1".to_string(),
            due_at: ts(),
            status: ScheduledJobStatus::Pending,
            created_at: ts(),
        };
        let json = serde_json::to_string_pretty(&job).unwrap();
        let decoded: ScheduledJob = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded, job);
    }

    #[test]
    fn scheduled_job_status_roundtrips() {
        let statuses = vec![
            ScheduledJobStatus::Pending,
            ScheduledJobStatus::Fired,
            ScheduledJobStatus::Cancelled,
        ];
        for status in statuses {
            let json = serde_json::to_string(&status).unwrap();
            let decoded: ScheduledJobStatus = serde_json::from_str(&json).unwrap();
            assert_eq!(decoded, status);
        }
    }

    // ---- FakeClock tests ----

    #[test]
    fn fake_clock_starts_at_initial_time() {
        let clock = FakeClock::new(ts());
        assert_eq!(clock.now(), ts());
    }

    #[test]
    fn fake_clock_advances() {
        let clock = FakeClock::new(ts());
        clock.advance(chrono::Duration::hours(1));
        assert_eq!(clock.now(), ts() + chrono::Duration::hours(1));
    }

    #[test]
    fn fake_clock_set() {
        let clock = FakeClock::new(ts());
        let new_time = ts() + chrono::Duration::days(1);
        clock.set(new_time);
        assert_eq!(clock.now(), new_time);
    }

    // ---- Scheduler tests ----

    #[test]
    fn scheduler_fires_due_job() {
        let clock = std::sync::Arc::new(FakeClock::new(ts()));
        let scheduler = Scheduler::new(clock.clone());

        scheduler
            .schedule(
                ScheduledJobId::from("job-1"),
                "reminder-1".to_string(),
                ts(),
                ts(),
            )
            .unwrap();

        let due = scheduler.poll_due_jobs();
        assert_eq!(due.len(), 1);
        assert_eq!(due[0].id, ScheduledJobId::from("job-1"));
        assert_eq!(due[0].status, ScheduledJobStatus::Fired);

        let job = scheduler.get(&ScheduledJobId::from("job-1")).unwrap();
        assert_eq!(job.status, ScheduledJobStatus::Fired);
    }

    #[test]
    fn scheduler_ignores_future_jobs() {
        let clock = std::sync::Arc::new(FakeClock::new(ts()));
        let scheduler = Scheduler::new(clock.clone());

        scheduler
            .schedule(
                ScheduledJobId::from("job-future"),
                "reminder-1".to_string(),
                ts() + chrono::Duration::hours(1),
                ts(),
            )
            .unwrap();

        let due = scheduler.poll_due_jobs();
        assert!(due.is_empty());

        let job = scheduler.get(&ScheduledJobId::from("job-future")).unwrap();
        assert_eq!(job.status, ScheduledJobStatus::Pending);
    }

    #[test]
    fn scheduler_ignores_already_fired_jobs() {
        let clock = std::sync::Arc::new(FakeClock::new(ts()));
        let scheduler = Scheduler::new(clock.clone());

        scheduler
            .schedule(
                ScheduledJobId::from("job-1"),
                "reminder-1".to_string(),
                ts(),
                ts(),
            )
            .unwrap();

        let due = scheduler.poll_due_jobs();
        assert_eq!(due.len(), 1);

        let due = scheduler.poll_due_jobs();
        assert!(due.is_empty());
    }

    #[test]
    fn scheduler_ignores_cancelled_jobs() {
        let clock = std::sync::Arc::new(FakeClock::new(ts()));
        let scheduler = Scheduler::new(clock.clone());

        scheduler
            .schedule(
                ScheduledJobId::from("job-1"),
                "reminder-1".to_string(),
                ts(),
                ts(),
            )
            .unwrap();

        scheduler.cancel(&ScheduledJobId::from("job-1")).unwrap();

        let due = scheduler.poll_due_jobs();
        assert!(due.is_empty());
    }

    #[test]
    fn scheduler_fires_multiple_due_jobs() {
        let clock = std::sync::Arc::new(FakeClock::new(ts()));
        let scheduler = Scheduler::new(clock.clone());

        scheduler
            .schedule(
                ScheduledJobId::from("job-1"),
                "reminder-1".to_string(),
                ts(),
                ts(),
            )
            .unwrap();
        scheduler
            .schedule(
                ScheduledJobId::from("job-2"),
                "reminder-2".to_string(),
                ts(),
                ts(),
            )
            .unwrap();

        let due = scheduler.poll_due_jobs();
        assert_eq!(due.len(), 2);
    }

    #[test]
    fn scheduler_fires_job_after_clock_advances() {
        let clock = std::sync::Arc::new(FakeClock::new(ts()));
        let scheduler = Scheduler::new(clock.clone());

        scheduler
            .schedule(
                ScheduledJobId::from("job-later"),
                "reminder-1".to_string(),
                ts() + chrono::Duration::hours(1),
                ts(),
            )
            .unwrap();

        // Not due yet
        let due = scheduler.poll_due_jobs();
        assert!(due.is_empty());

        // Advance clock by 1 hour
        clock.advance(chrono::Duration::hours(1));

        // Now should fire
        let due = scheduler.poll_due_jobs();
        assert_eq!(due.len(), 1);
        assert_eq!(due[0].id, ScheduledJobId::from("job-later"));
    }

    // ---- ReminderNotificationBridge tests ----

    #[tokio::test]
    async fn bridge_due_reminder_generates_notification() {
        let clock = std::sync::Arc::new(FakeClock::new(ts()));
        let scheduler = Scheduler::new(clock.clone());
        let sink = std::sync::Arc::new(notification_core::FakeNotificationSink::new());
        let store = std::sync::Arc::new(notification_core::MemoryNotificationStore::new());
        let bridge = ReminderNotificationBridge::new(scheduler, sink.clone(), store.clone());

        // Schedule a job due now
        bridge
            .scheduler()
            .schedule(
                ScheduledJobId::from("job-1"),
                "reminder-1".to_string(),
                ts(),
                ts(),
            )
            .unwrap();

        let notifications = bridge.process_due().await;
        assert_eq!(notifications.len(), 1);
        assert_eq!(
            notifications[0].notification_type,
            notification_core::NotificationType::ReminderDue
        );
        assert_eq!(sink.count(), 1);
        assert_eq!(store.list().await.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn bridge_completed_reminder_does_not_generate_notification() {
        let clock = std::sync::Arc::new(FakeClock::new(ts()));
        let scheduler = Scheduler::new(clock.clone());
        let sink = std::sync::Arc::new(notification_core::FakeNotificationSink::new());
        let store = std::sync::Arc::new(notification_core::MemoryNotificationStore::new());
        let bridge = ReminderNotificationBridge::new(scheduler, sink.clone(), store.clone());

        // Schedule and cancel before due
        bridge
            .scheduler()
            .schedule(
                ScheduledJobId::from("job-1"),
                "reminder-1".to_string(),
                ts(),
                ts(),
            )
            .unwrap();
        bridge
            .scheduler()
            .cancel(&ScheduledJobId::from("job-1"))
            .unwrap();

        let notifications = bridge.process_due().await;
        assert!(notifications.is_empty());
        assert_eq!(sink.count(), 0);
    }

    #[tokio::test]
    async fn bridge_cancelled_reminder_does_not_fire() {
        let clock = std::sync::Arc::new(FakeClock::new(ts()));
        let scheduler = Scheduler::new(clock.clone());
        let sink = std::sync::Arc::new(notification_core::FakeNotificationSink::new());
        let store = std::sync::Arc::new(notification_core::MemoryNotificationStore::new());
        let bridge = ReminderNotificationBridge::new(scheduler, sink.clone(), store.clone());

        // Schedule, then cancel
        bridge
            .scheduler()
            .schedule(
                ScheduledJobId::from("job-cancelled"),
                "reminder-cancel".to_string(),
                ts(),
                ts(),
            )
            .unwrap();
        bridge
            .scheduler()
            .cancel(&ScheduledJobId::from("job-cancelled"))
            .unwrap();

        let notifications = bridge.process_due().await;
        assert!(notifications.is_empty());
    }

    #[tokio::test]
    async fn bridge_snoozed_reminder_fires_after_duration() {
        let clock = std::sync::Arc::new(FakeClock::new(ts()));
        let scheduler = Scheduler::new(clock.clone());
        let sink = std::sync::Arc::new(notification_core::FakeNotificationSink::new());
        let store = std::sync::Arc::new(notification_core::MemoryNotificationStore::new());
        let bridge = ReminderNotificationBridge::new(scheduler, sink.clone(), store.clone());

        // Schedule a job due in 1 hour (simulating snooze)
        bridge
            .scheduler()
            .schedule(
                ScheduledJobId::from("job-snoozed"),
                "reminder-snooze".to_string(),
                ts() + chrono::Duration::hours(1),
                ts(),
            )
            .unwrap();

        // Not due yet
        let notifications = bridge.process_due().await;
        assert!(notifications.is_empty());

        // Advance clock
        clock.advance(chrono::Duration::hours(1));

        // Now should fire
        let notifications = bridge.process_due().await;
        assert_eq!(notifications.len(), 1);
        assert_eq!(sink.count(), 1);
    }

    // -----------------------------------------------------------------------
    // PR 149: Persistent scheduler store
    // -----------------------------------------------------------------------

    #[test]
    fn fs_jsonl_store_save_and_get() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("jobs.jsonl");
        let store = FsJsonlSchedulerStore::new(&path).unwrap();

        let job = ScheduledJob {
            id: ScheduledJobId::from("job-1"),
            reminder_id: "reminder-1".to_string(),
            due_at: ts() + chrono::Duration::hours(1),
            status: ScheduledJobStatus::Pending,
            created_at: ts(),
        };

        store.save(&job).unwrap();
        let retrieved = store.get(&ScheduledJobId::from("job-1")).unwrap();
        assert_eq!(retrieved.id.0, "job-1");
        assert_eq!(retrieved.reminder_id, "reminder-1");
    }

    #[test]
    fn fs_jsonl_store_persists_to_disk() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("jobs.jsonl");

        // Save a job
        {
            let store = FsJsonlSchedulerStore::new(&path).unwrap();
            let job = ScheduledJob {
                id: ScheduledJobId::from("job-1"),
                reminder_id: "reminder-1".to_string(),
                due_at: ts() + chrono::Duration::hours(1),
                status: ScheduledJobStatus::Pending,
                created_at: ts(),
            };
            store.save(&job).unwrap();
        }

        // Load from disk
        let store = FsJsonlSchedulerStore::new(&path).unwrap();
        let job = store.get(&ScheduledJobId::from("job-1")).unwrap();
        assert_eq!(job.id.0, "job-1");
    }

    #[test]
    fn fs_jsonl_store_update_status() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("jobs.jsonl");
        let store = FsJsonlSchedulerStore::new(&path).unwrap();

        let job = ScheduledJob {
            id: ScheduledJobId::from("job-1"),
            reminder_id: "reminder-1".to_string(),
            due_at: ts() + chrono::Duration::hours(1),
            status: ScheduledJobStatus::Pending,
            created_at: ts(),
        };

        store.save(&job).unwrap();
        store
            .update_status(&ScheduledJobId::from("job-1"), ScheduledJobStatus::Fired)
            .unwrap();

        let job = store.get(&ScheduledJobId::from("job-1")).unwrap();
        assert_eq!(job.status, ScheduledJobStatus::Fired);
    }

    #[test]
    fn fs_jsonl_store_delete() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("jobs.jsonl");
        let store = FsJsonlSchedulerStore::new(&path).unwrap();

        let job = ScheduledJob {
            id: ScheduledJobId::from("job-1"),
            reminder_id: "reminder-1".to_string(),
            due_at: ts() + chrono::Duration::hours(1),
            status: ScheduledJobStatus::Pending,
            created_at: ts(),
        };

        store.save(&job).unwrap();
        store.delete(&ScheduledJobId::from("job-1")).unwrap();

        let result = store.get(&ScheduledJobId::from("job-1"));
        assert!(matches!(result, Err(SchedulerError::NotFound(_))));
    }

    #[test]
    fn fs_jsonl_store_list() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("jobs.jsonl");
        let store = FsJsonlSchedulerStore::new(&path).unwrap();

        store
            .save(&ScheduledJob {
                id: ScheduledJobId::from("job-1"),
                reminder_id: "reminder-1".to_string(),
                due_at: ts() + chrono::Duration::hours(1),
                status: ScheduledJobStatus::Pending,
                created_at: ts(),
            })
            .unwrap();

        store
            .save(&ScheduledJob {
                id: ScheduledJobId::from("job-2"),
                reminder_id: "reminder-2".to_string(),
                due_at: ts() + chrono::Duration::hours(2),
                status: ScheduledJobStatus::Pending,
                created_at: ts(),
            })
            .unwrap();

        let jobs = store.list();
        assert_eq!(jobs.len(), 2);
    }

    #[test]
    fn fs_jsonl_store_overwrite() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("jobs.jsonl");
        let store = FsJsonlSchedulerStore::new(&path).unwrap();

        let job = ScheduledJob {
            id: ScheduledJobId::from("job-1"),
            reminder_id: "reminder-1".to_string(),
            due_at: ts() + chrono::Duration::hours(1),
            status: ScheduledJobStatus::Pending,
            created_at: ts(),
        };

        store.save(&job).unwrap();

        let mut updated = job.clone();
        updated.status = ScheduledJobStatus::Fired;
        store.save(&updated).unwrap();

        let retrieved = store.get(&ScheduledJobId::from("job-1")).unwrap();
        assert_eq!(retrieved.status, ScheduledJobStatus::Fired);
    }

    #[test]
    fn fs_jsonl_store_empty_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("empty.jsonl");
        std::fs::write(&path, "").unwrap();

        let store = FsJsonlSchedulerStore::new(&path).unwrap();
        assert!(store.list().is_empty());
    }

    #[test]
    fn fs_jsonl_store_path() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("jobs.jsonl");
        let store = FsJsonlSchedulerStore::new(&path).unwrap();
        assert_eq!(store.path(), path.as_path());
    }
}
