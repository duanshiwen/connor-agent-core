//! # Calendar Entity
//!
//! Domain model and in-memory repository for calendar events in AgentOS.
//!
//! This crate provides:
//! - Calendar event identity and model
//! - Recurrence rules (RFC 5545 RRULE simplified)
//! - Calendar reminders
//! - In-memory calendar event store

use asset_core::WorkObjectId;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt;
use std::sync::{Arc, Mutex};

// ---------------------------------------------------------------------------
// Calendar Event Identity
// ---------------------------------------------------------------------------

/// Unique identifier for a calendar event.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct CalendarEventId(pub String);

impl fmt::Display for CalendarEventId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<&str> for CalendarEventId {
    fn from(s: &str) -> Self {
        Self(s.to_string())
    }
}

impl From<String> for CalendarEventId {
    fn from(s: String) -> Self {
        Self(s)
    }
}

// ---------------------------------------------------------------------------
// Calendar Event Status
// ---------------------------------------------------------------------------

/// Status of a calendar event.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CalendarEventStatus {
    Tentative,
    Confirmed,
    Cancelled,
}

impl fmt::Display for CalendarEventStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Tentative => write!(f, "tentative"),
            Self::Confirmed => write!(f, "confirmed"),
            Self::Cancelled => write!(f, "cancelled"),
        }
    }
}

// ---------------------------------------------------------------------------
// Calendar Event Transparency
// ---------------------------------------------------------------------------

/// Transparency of a calendar event (whether it blocks time).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EventTransparency {
    Opaque,
    Transparent,
}

impl fmt::Display for EventTransparency {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Opaque => write!(f, "opaque"),
            Self::Transparent => write!(f, "transparent"),
        }
    }
}

// ---------------------------------------------------------------------------
// Recurrence Rules
// ---------------------------------------------------------------------------

/// Frequency for recurrence rules.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RecurrenceFrequency {
    Daily,
    Weekly,
    Monthly,
    Yearly,
}

impl fmt::Display for RecurrenceFrequency {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Daily => write!(f, "DAILY"),
            Self::Weekly => write!(f, "WEEKLY"),
            Self::Monthly => write!(f, "MONTHLY"),
            Self::Yearly => write!(f, "YEARLY"),
        }
    }
}

/// Days of the week for recurrence rules.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Weekday {
    Monday,
    Tuesday,
    Wednesday,
    Thursday,
    Friday,
    Saturday,
    Sunday,
}

impl fmt::Display for Weekday {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Monday => write!(f, "MO"),
            Self::Tuesday => write!(f, "TU"),
            Self::Wednesday => write!(f, "WE"),
            Self::Thursday => write!(f, "TH"),
            Self::Friday => write!(f, "FR"),
            Self::Saturday => write!(f, "SA"),
            Self::Sunday => write!(f, "SU"),
        }
    }
}

/// Recurrence rule for a calendar event (simplified RFC 5545 RRULE).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecurrenceRule {
    pub frequency: RecurrenceFrequency,
    pub interval: u32,
    pub count: Option<u32>,
    pub until: Option<DateTime<Utc>>,
    pub by_day: Vec<Weekday>,
    pub by_month_day: Vec<i8>,
    pub by_month: Vec<u8>,
    pub exclude_dates: Vec<DateTime<Utc>>,
}

impl RecurrenceRule {
    pub fn new(frequency: RecurrenceFrequency) -> Self {
        Self {
            frequency,
            interval: 1,
            count: None,
            until: None,
            by_day: vec![],
            by_month_day: vec![],
            by_month: vec![],
            exclude_dates: vec![],
        }
    }

    pub fn with_interval(mut self, interval: u32) -> Self {
        self.interval = interval;
        self
    }

    pub fn with_count(mut self, count: u32) -> Self {
        self.count = Some(count);
        self
    }

    pub fn with_until(mut self, until: DateTime<Utc>) -> Self {
        self.until = Some(until);
        self
    }

    pub fn with_by_day(mut self, days: Vec<Weekday>) -> Self {
        self.by_day = days;
        self
    }

    pub fn with_exclude_dates(mut self, dates: Vec<DateTime<Utc>>) -> Self {
        self.exclude_dates = dates;
        self
    }
}

// ---------------------------------------------------------------------------
// Calendar Reminders
// ---------------------------------------------------------------------------

/// Type of calendar reminder.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReminderType {
    Email,
    Popup,
    Push,
}

impl fmt::Display for ReminderType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Email => write!(f, "email"),
            Self::Popup => write!(f, "popup"),
            Self::Push => write!(f, "push"),
        }
    }
}

/// Calendar event reminder.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CalendarReminder {
    pub reminder_type: ReminderType,
    pub minutes_before: u32,
}

impl CalendarReminder {
    pub fn new(reminder_type: ReminderType, minutes_before: u32) -> Self {
        Self {
            reminder_type,
            minutes_before,
        }
    }
}

// ---------------------------------------------------------------------------
// Calendar Event Attendee
// ---------------------------------------------------------------------------

/// Response status of an attendee.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AttendeeResponse {
    Accepted,
    Declined,
    Tentative,
    NeedsAction,
}

impl fmt::Display for AttendeeResponse {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Accepted => write!(f, "accepted"),
            Self::Declined => write!(f, "declined"),
            Self::Tentative => write!(f, "tentative"),
            Self::NeedsAction => write!(f, "needs_action"),
        }
    }
}

/// Calendar event attendee.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CalendarAttendee {
    pub email: String,
    pub display_name: Option<String>,
    pub response: AttendeeResponse,
    pub is_organizer: bool,
}

impl CalendarAttendee {
    pub fn new(email: impl Into<String>) -> Self {
        Self {
            email: email.into(),
            display_name: None,
            response: AttendeeResponse::NeedsAction,
            is_organizer: false,
        }
    }

    pub fn with_display_name(mut self, name: impl Into<String>) -> Self {
        self.display_name = Some(name.into());
        self
    }

    pub fn with_response(mut self, response: AttendeeResponse) -> Self {
        self.response = response;
        self
    }

    pub fn as_organizer(mut self) -> Self {
        self.is_organizer = true;
        self
    }
}

// ---------------------------------------------------------------------------
// Calendar Event Model
// ---------------------------------------------------------------------------

/// Location for a calendar event.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EventLocation {
    pub name: Option<String>,
    pub address: Option<String>,
    pub url: Option<String>,
}

impl Default for EventLocation {
    fn default() -> Self {
        Self::new()
    }
}

impl EventLocation {
    pub fn new() -> Self {
        Self {
            name: None,
            address: None,
            url: None,
        }
    }

    pub fn with_name(mut self, name: impl Into<String>) -> Self {
        self.name = Some(name.into());
        self
    }

    pub fn with_address(mut self, address: impl Into<String>) -> Self {
        self.address = Some(address.into());
        self
    }

    pub fn with_url(mut self, url: impl Into<String>) -> Self {
        self.url = Some(url.into());
        self
    }
}

/// Calendar event.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CalendarEvent {
    pub id: CalendarEventId,
    pub title: String,
    pub description: Option<String>,
    pub start_time: DateTime<Utc>,
    pub end_time: DateTime<Utc>,
    pub all_day: bool,
    pub status: CalendarEventStatus,
    pub transparency: EventTransparency,
    pub location: Option<EventLocation>,
    pub attendees: Vec<CalendarAttendee>,
    pub reminders: Vec<CalendarReminder>,
    pub recurrence: Option<RecurrenceRule>,
    pub work_object_id: Option<WorkObjectId>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl CalendarEvent {
    pub fn new(
        id: impl Into<CalendarEventId>,
        title: impl Into<String>,
        start_time: DateTime<Utc>,
        end_time: DateTime<Utc>,
    ) -> Self {
        let now = Utc::now();
        Self {
            id: id.into(),
            title: title.into(),
            description: None,
            start_time,
            end_time,
            all_day: false,
            status: CalendarEventStatus::Confirmed,
            transparency: EventTransparency::Opaque,
            location: None,
            attendees: vec![],
            reminders: vec![],
            recurrence: None,
            work_object_id: None,
            created_at: now,
            updated_at: now,
        }
    }

    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }

    pub fn with_all_day(mut self, all_day: bool) -> Self {
        self.all_day = all_day;
        self
    }

    pub fn with_status(mut self, status: CalendarEventStatus) -> Self {
        self.status = status;
        self
    }

    pub fn with_transparency(mut self, transparency: EventTransparency) -> Self {
        self.transparency = transparency;
        self
    }

    pub fn with_location(mut self, location: EventLocation) -> Self {
        self.location = Some(location);
        self
    }

    pub fn with_attendee(mut self, attendee: CalendarAttendee) -> Self {
        self.attendees.push(attendee);
        self
    }

    pub fn with_reminder(mut self, reminder: CalendarReminder) -> Self {
        self.reminders.push(reminder);
        self
    }

    pub fn with_recurrence(mut self, recurrence: RecurrenceRule) -> Self {
        self.recurrence = Some(recurrence);
        self
    }

    pub fn with_work_object_id(mut self, work_object_id: WorkObjectId) -> Self {
        self.work_object_id = Some(work_object_id);
        self
    }

    /// Check if this event overlaps with a time range.
    pub fn overlaps(&self, start: DateTime<Utc>, end: DateTime<Utc>) -> bool {
        self.start_time < end && self.end_time > start
    }

    /// Get the duration of this event.
    pub fn duration(&self) -> chrono::Duration {
        self.end_time - self.start_time
    }
}

// ---------------------------------------------------------------------------
// Calendar Event Store
// ---------------------------------------------------------------------------

/// Error type for calendar event operations.
#[derive(Debug, thiserror::Error)]
pub enum CalendarEventError {
    #[error("event not found: {0}")]
    NotFound(String),
    #[error("lock error: {0}")]
    LockError(String),
}

/// Trait for storing calendar events.
#[async_trait::async_trait]
pub trait CalendarEventStore: Send + Sync {
    async fn save(&self, event: &CalendarEvent) -> Result<(), CalendarEventError>;
    async fn get(&self, id: &CalendarEventId) -> Result<CalendarEvent, CalendarEventError>;
    async fn delete(&self, id: &CalendarEventId) -> Result<(), CalendarEventError>;
    async fn list_all(&self) -> Result<Vec<CalendarEvent>, CalendarEventError>;
    async fn list_by_work_object(
        &self,
        work_object_id: &WorkObjectId,
    ) -> Result<Vec<CalendarEvent>, CalendarEventError>;
    async fn list_by_time_range(
        &self,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
    ) -> Result<Vec<CalendarEvent>, CalendarEventError>;
}

/// In-memory implementation of CalendarEventStore.
#[derive(Debug, Clone, Default)]
pub struct MemoryCalendarEventStore {
    events: Arc<Mutex<HashMap<CalendarEventId, CalendarEvent>>>,
}

impl MemoryCalendarEventStore {
    pub fn new() -> Self {
        Self {
            events: Arc::new(Mutex::new(HashMap::new())),
        }
    }
}

#[async_trait::async_trait]
impl CalendarEventStore for MemoryCalendarEventStore {
    async fn save(&self, event: &CalendarEvent) -> Result<(), CalendarEventError> {
        let mut events = self
            .events
            .lock()
            .map_err(|e| CalendarEventError::LockError(e.to_string()))?;
        events.insert(event.id.clone(), event.clone());
        Ok(())
    }

    async fn get(&self, id: &CalendarEventId) -> Result<CalendarEvent, CalendarEventError> {
        let events = self
            .events
            .lock()
            .map_err(|e| CalendarEventError::LockError(e.to_string()))?;
        events
            .get(id)
            .cloned()
            .ok_or_else(|| CalendarEventError::NotFound(id.0.clone()))
    }

    async fn delete(&self, id: &CalendarEventId) -> Result<(), CalendarEventError> {
        let mut events = self
            .events
            .lock()
            .map_err(|e| CalendarEventError::LockError(e.to_string()))?;
        events.remove(id);
        Ok(())
    }

    async fn list_all(&self) -> Result<Vec<CalendarEvent>, CalendarEventError> {
        let events = self
            .events
            .lock()
            .map_err(|e| CalendarEventError::LockError(e.to_string()))?;
        Ok(events.values().cloned().collect())
    }

    async fn list_by_work_object(
        &self,
        work_object_id: &WorkObjectId,
    ) -> Result<Vec<CalendarEvent>, CalendarEventError> {
        let events = self
            .events
            .lock()
            .map_err(|e| CalendarEventError::LockError(e.to_string()))?;
        Ok(events
            .values()
            .filter(|e| e.work_object_id.as_ref() == Some(work_object_id))
            .cloned()
            .collect())
    }

    async fn list_by_time_range(
        &self,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
    ) -> Result<Vec<CalendarEvent>, CalendarEventError> {
        let events = self
            .events
            .lock()
            .map_err(|e| CalendarEventError::LockError(e.to_string()))?;
        Ok(events
            .values()
            .filter(|e| e.overlaps(start, end))
            .cloned()
            .collect())
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn ts() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 5, 27, 10, 0, 0).unwrap()
    }

    fn ts_end() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 5, 27, 11, 0, 0).unwrap()
    }

    #[test]
    fn calendar_event_id_display() {
        let id = CalendarEventId::from("event-1");
        assert_eq!(id.to_string(), "event-1");
    }

    #[test]
    fn calendar_event_status_display() {
        assert_eq!(CalendarEventStatus::Tentative.to_string(), "tentative");
        assert_eq!(CalendarEventStatus::Confirmed.to_string(), "confirmed");
        assert_eq!(CalendarEventStatus::Cancelled.to_string(), "cancelled");
    }

    #[test]
    fn event_transparency_display() {
        assert_eq!(EventTransparency::Opaque.to_string(), "opaque");
        assert_eq!(EventTransparency::Transparent.to_string(), "transparent");
    }

    #[test]
    fn recurrence_frequency_display() {
        assert_eq!(RecurrenceFrequency::Daily.to_string(), "DAILY");
        assert_eq!(RecurrenceFrequency::Weekly.to_string(), "WEEKLY");
        assert_eq!(RecurrenceFrequency::Monthly.to_string(), "MONTHLY");
        assert_eq!(RecurrenceFrequency::Yearly.to_string(), "YEARLY");
    }

    #[test]
    fn weekday_display() {
        assert_eq!(Weekday::Monday.to_string(), "MO");
        assert_eq!(Weekday::Tuesday.to_string(), "TU");
        assert_eq!(Weekday::Wednesday.to_string(), "WE");
        assert_eq!(Weekday::Thursday.to_string(), "TH");
        assert_eq!(Weekday::Friday.to_string(), "FR");
        assert_eq!(Weekday::Saturday.to_string(), "SA");
        assert_eq!(Weekday::Sunday.to_string(), "SU");
    }

    #[test]
    fn recurrence_rule_roundtrip() {
        let rule = RecurrenceRule::new(RecurrenceFrequency::Weekly)
            .with_interval(2)
            .with_count(10)
            .with_by_day(vec![Weekday::Monday, Weekday::Wednesday]);

        let json = serde_json::to_string_pretty(&rule).unwrap();
        let decoded: RecurrenceRule = serde_json::from_str(&json).unwrap();

        assert_eq!(decoded.frequency, RecurrenceFrequency::Weekly);
        assert_eq!(decoded.interval, 2);
        assert_eq!(decoded.count, Some(10));
        assert_eq!(decoded.by_day, vec![Weekday::Monday, Weekday::Wednesday]);
    }

    #[test]
    fn reminder_type_display() {
        assert_eq!(ReminderType::Email.to_string(), "email");
        assert_eq!(ReminderType::Popup.to_string(), "popup");
        assert_eq!(ReminderType::Push.to_string(), "push");
    }

    #[test]
    fn calendar_reminder_roundtrip() {
        let reminder = CalendarReminder::new(ReminderType::Popup, 15);
        let json = serde_json::to_string_pretty(&reminder).unwrap();
        let decoded: CalendarReminder = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded.reminder_type, ReminderType::Popup);
        assert_eq!(decoded.minutes_before, 15);
    }

    #[test]
    fn attendee_response_display() {
        assert_eq!(AttendeeResponse::Accepted.to_string(), "accepted");
        assert_eq!(AttendeeResponse::Declined.to_string(), "declined");
        assert_eq!(AttendeeResponse::Tentative.to_string(), "tentative");
        assert_eq!(AttendeeResponse::NeedsAction.to_string(), "needs_action");
    }

    #[test]
    fn calendar_attendee_roundtrip() {
        let attendee = CalendarAttendee::new("alice@example.com")
            .with_display_name("Alice")
            .with_response(AttendeeResponse::Accepted)
            .as_organizer();

        let json = serde_json::to_string_pretty(&attendee).unwrap();
        let decoded: CalendarAttendee = serde_json::from_str(&json).unwrap();

        assert_eq!(decoded.email, "alice@example.com");
        assert_eq!(decoded.display_name.as_deref(), Some("Alice"));
        assert_eq!(decoded.response, AttendeeResponse::Accepted);
        assert!(decoded.is_organizer);
    }

    #[test]
    fn event_location_roundtrip() {
        let location = EventLocation::new()
            .with_name("Conference Room A")
            .with_address("123 Main St")
            .with_url("https://meet.google.com/abc");

        let json = serde_json::to_string_pretty(&location).unwrap();
        let decoded: EventLocation = serde_json::from_str(&json).unwrap();

        assert_eq!(decoded.name.as_deref(), Some("Conference Room A"));
        assert_eq!(decoded.address.as_deref(), Some("123 Main St"));
        assert_eq!(decoded.url.as_deref(), Some("https://meet.google.com/abc"));
    }

    #[test]
    fn calendar_event_roundtrip() {
        let event = CalendarEvent::new("event-1", "Team Meeting", ts(), ts_end())
            .with_description("Weekly team sync")
            .with_all_day(false)
            .with_status(CalendarEventStatus::Confirmed)
            .with_transparency(EventTransparency::Opaque)
            .with_location(EventLocation::new().with_name("Room A"))
            .with_attendee(CalendarAttendee::new("bob@example.com"))
            .with_reminder(CalendarReminder::new(ReminderType::Popup, 10))
            .with_recurrence(RecurrenceRule::new(RecurrenceFrequency::Weekly))
            .with_work_object_id(WorkObjectId::from("project-1"));

        let json = serde_json::to_string_pretty(&event).unwrap();
        let decoded: CalendarEvent = serde_json::from_str(&json).unwrap();

        assert_eq!(decoded.id.0, "event-1");
        assert_eq!(decoded.title, "Team Meeting");
        assert_eq!(decoded.description.as_deref(), Some("Weekly team sync"));
        assert!(!decoded.all_day);
        assert_eq!(decoded.status, CalendarEventStatus::Confirmed);
        assert!(decoded.location.is_some());
        assert_eq!(decoded.attendees.len(), 1);
        assert_eq!(decoded.reminders.len(), 1);
        assert!(decoded.recurrence.is_some());
        assert!(decoded.work_object_id.is_some());
    }

    #[test]
    fn calendar_event_overlaps() {
        let event = CalendarEvent::new("event-1", "Meeting", ts(), ts_end());

        // Overlapping ranges
        assert!(event.overlaps(ts(), ts_end()));
        assert!(event.overlaps(
            Utc.with_ymd_and_hms(2026, 5, 27, 9, 30, 0).unwrap(),
            Utc.with_ymd_and_hms(2026, 5, 27, 10, 30, 0).unwrap()
        ));
        assert!(event.overlaps(
            Utc.with_ymd_and_hms(2026, 5, 27, 10, 30, 0).unwrap(),
            Utc.with_ymd_and_hms(2026, 5, 27, 11, 30, 0).unwrap()
        ));

        // Non-overlapping ranges
        assert!(!event.overlaps(
            Utc.with_ymd_and_hms(2026, 5, 27, 11, 0, 0).unwrap(),
            Utc.with_ymd_and_hms(2026, 5, 27, 12, 0, 0).unwrap()
        ));
        assert!(!event.overlaps(
            Utc.with_ymd_and_hms(2026, 5, 27, 8, 0, 0).unwrap(),
            Utc.with_ymd_and_hms(2026, 5, 27, 10, 0, 0).unwrap()
        ));
    }

    #[test]
    fn calendar_event_duration() {
        let event = CalendarEvent::new("event-1", "Meeting", ts(), ts_end());
        assert_eq!(event.duration(), chrono::Duration::hours(1));
    }

    #[tokio::test]
    async fn memory_store_save_and_get() {
        let store = MemoryCalendarEventStore::new();
        let event = CalendarEvent::new("event-1", "Meeting", ts(), ts_end());

        store.save(&event).await.unwrap();
        let retrieved = store.get(&CalendarEventId::from("event-1")).await.unwrap();

        assert_eq!(retrieved.id.0, "event-1");
        assert_eq!(retrieved.title, "Meeting");
    }

    #[tokio::test]
    async fn memory_store_get_not_found() {
        let store = MemoryCalendarEventStore::new();
        let result = store.get(&CalendarEventId::from("nonexistent")).await;
        assert!(matches!(result, Err(CalendarEventError::NotFound(_))));
    }

    #[tokio::test]
    async fn memory_store_delete() {
        let store = MemoryCalendarEventStore::new();
        let event = CalendarEvent::new("event-1", "Meeting", ts(), ts_end());

        store.save(&event).await.unwrap();
        store
            .delete(&CalendarEventId::from("event-1"))
            .await
            .unwrap();

        let result = store.get(&CalendarEventId::from("event-1")).await;
        assert!(matches!(result, Err(CalendarEventError::NotFound(_))));
    }

    #[tokio::test]
    async fn memory_store_list_all() {
        let store = MemoryCalendarEventStore::new();

        store
            .save(&CalendarEvent::new("event-1", "Meeting 1", ts(), ts_end()))
            .await
            .unwrap();
        store
            .save(&CalendarEvent::new("event-2", "Meeting 2", ts(), ts_end()))
            .await
            .unwrap();

        let all = store.list_all().await.unwrap();
        assert_eq!(all.len(), 2);
    }

    #[tokio::test]
    async fn memory_store_list_by_work_object() {
        let store = MemoryCalendarEventStore::new();

        store
            .save(
                &CalendarEvent::new("event-1", "Meeting 1", ts(), ts_end())
                    .with_work_object_id(WorkObjectId::from("project-1")),
            )
            .await
            .unwrap();
        store
            .save(&CalendarEvent::new("event-2", "Meeting 2", ts(), ts_end()))
            .await
            .unwrap();

        let filtered = store
            .list_by_work_object(&WorkObjectId::from("project-1"))
            .await
            .unwrap();
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].id.0, "event-1");
    }

    #[tokio::test]
    async fn memory_store_list_by_time_range() {
        let store = MemoryCalendarEventStore::new();

        store
            .save(&CalendarEvent::new("event-1", "Meeting", ts(), ts_end()))
            .await
            .unwrap();

        // Within range
        let events = store
            .list_by_time_range(
                Utc.with_ymd_and_hms(2026, 5, 27, 9, 0, 0).unwrap(),
                Utc.with_ymd_and_hms(2026, 5, 27, 12, 0, 0).unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(events.len(), 1);

        // Outside range
        let events = store
            .list_by_time_range(
                Utc.with_ymd_and_hms(2026, 5, 27, 12, 0, 0).unwrap(),
                Utc.with_ymd_and_hms(2026, 5, 27, 13, 0, 0).unwrap(),
            )
            .await
            .unwrap();
        assert!(events.is_empty());
    }

    #[tokio::test]
    async fn memory_store_save_overwrites() {
        let store = MemoryCalendarEventStore::new();

        store
            .save(&CalendarEvent::new("event-1", "Original", ts(), ts_end()))
            .await
            .unwrap();
        store
            .save(&CalendarEvent::new("event-1", "Updated", ts(), ts_end()))
            .await
            .unwrap();

        let event = store.get(&CalendarEventId::from("event-1")).await.unwrap();
        assert_eq!(event.title, "Updated");
    }
}
