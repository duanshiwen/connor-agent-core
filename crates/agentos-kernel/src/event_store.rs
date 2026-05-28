use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;
use tokio::fs::{self, OpenOptions};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

pub const CURRENT_KERNEL_EVENT_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct KernelEventId(pub u64);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct KernelEventCursor {
    pub last_seen: Option<KernelEventId>,
}

impl KernelEventCursor {
    pub fn beginning() -> Self {
        Self { last_seen: None }
    }

    pub fn after(event_id: KernelEventId) -> Self {
        Self {
            last_seen: Some(event_id),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct KernelAggregateRef {
    pub aggregate_type: String,
    pub aggregate_id: String,
}

impl KernelAggregateRef {
    pub fn new(aggregate_type: impl Into<String>, aggregate_id: impl Into<String>) -> Self {
        Self {
            aggregate_type: aggregate_type.into(),
            aggregate_id: aggregate_id.into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KernelRedactionClass {
    PublicMetadata,
    UserContent,
    SensitiveContent,
    Secret,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KernelEventActor {
    pub actor_id: String,
    pub actor_kind: String,
}

impl KernelEventActor {
    pub fn system() -> Self {
        Self {
            actor_id: "system".to_string(),
            actor_kind: "system".to_string(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KernelEventKind(pub String);

impl From<&str> for KernelEventKind {
    fn from(value: &str) -> Self {
        Self(value.to_string())
    }
}

impl From<String> for KernelEventKind {
    fn from(value: String) -> Self {
        Self(value)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct KernelEventEnvelope {
    pub event_id: KernelEventId,
    pub schema_version: u32,
    pub event_kind: KernelEventKind,
    pub aggregate: KernelAggregateRef,
    pub occurred_at: DateTime<Utc>,
    pub actor: KernelEventActor,
    pub causation_id: Option<String>,
    pub correlation_id: Option<String>,
    pub redaction_class: KernelRedactionClass,
    pub payload: Value,
}

impl KernelEventEnvelope {
    pub fn new(
        event_id: KernelEventId,
        event_kind: impl Into<KernelEventKind>,
        aggregate: KernelAggregateRef,
        payload: Value,
    ) -> Self {
        Self {
            event_id,
            schema_version: CURRENT_KERNEL_EVENT_SCHEMA_VERSION,
            event_kind: event_kind.into(),
            aggregate,
            occurred_at: Utc::now(),
            actor: KernelEventActor::system(),
            causation_id: None,
            correlation_id: None,
            redaction_class: KernelRedactionClass::PublicMetadata,
            payload,
        }
    }

    pub fn with_actor(mut self, actor: KernelEventActor) -> Self {
        self.actor = actor;
        self
    }

    pub fn with_causation_id(mut self, causation_id: impl Into<String>) -> Self {
        self.causation_id = Some(causation_id.into());
        self
    }

    pub fn with_correlation_id(mut self, correlation_id: impl Into<String>) -> Self {
        self.correlation_id = Some(correlation_id.into());
        self
    }

    pub fn with_redaction_class(mut self, redaction_class: KernelRedactionClass) -> Self {
        self.redaction_class = redaction_class;
        self
    }
}

#[derive(Debug, Error)]
pub enum KernelEventStoreError {
    #[error("kernel event already exists: {event_id}")]
    AlreadyExists { event_id: u64 },
    #[error("kernel event id must be monotonic: last={last}, next={next}")]
    NonMonotonicId { last: u64, next: u64 },
    #[error("kernel event store io error: {reason}")]
    Io { reason: String },
    #[error("kernel event store serialization error: {reason}")]
    Serde { reason: String },
}

impl From<std::io::Error> for KernelEventStoreError {
    fn from(value: std::io::Error) -> Self {
        Self::Io {
            reason: value.to_string(),
        }
    }
}

impl From<serde_json::Error> for KernelEventStoreError {
    fn from(value: serde_json::Error) -> Self {
        Self::Serde {
            reason: value.to_string(),
        }
    }
}

pub type KernelEventStoreResult<T> = Result<T, KernelEventStoreError>;

#[async_trait::async_trait]
pub trait KernelEventStore: Send + Sync {
    async fn append(&self, event: KernelEventEnvelope)
    -> KernelEventStoreResult<KernelEventCursor>;

    async fn append_idempotent(
        &self,
        event: KernelEventEnvelope,
    ) -> KernelEventStoreResult<KernelEventCursor>;

    async fn events_after(
        &self,
        cursor: Option<KernelEventCursor>,
    ) -> KernelEventStoreResult<Vec<KernelEventEnvelope>>;

    async fn latest_cursor(&self) -> KernelEventStoreResult<Option<KernelEventCursor>>;

    async fn events_for_aggregate(
        &self,
        aggregate: &KernelAggregateRef,
    ) -> KernelEventStoreResult<Vec<KernelEventEnvelope>>;
}

#[derive(Debug, Default, Clone)]
pub struct MemoryKernelEventStore {
    events: Arc<Mutex<Vec<KernelEventEnvelope>>>,
    ids: Arc<Mutex<BTreeSet<u64>>>,
}

impl MemoryKernelEventStore {
    pub fn new() -> Self {
        Self::default()
    }
}

#[async_trait::async_trait]
impl KernelEventStore for MemoryKernelEventStore {
    async fn append(
        &self,
        event: KernelEventEnvelope,
    ) -> KernelEventStoreResult<KernelEventCursor> {
        let mut events = self.events.lock().expect("kernel event store poisoned");
        let mut ids = self.ids.lock().expect("kernel event store ids poisoned");
        append_to_memory(&mut events, &mut ids, event)
    }

    async fn append_idempotent(
        &self,
        event: KernelEventEnvelope,
    ) -> KernelEventStoreResult<KernelEventCursor> {
        let mut events = self.events.lock().expect("kernel event store poisoned");
        let mut ids = self.ids.lock().expect("kernel event store ids poisoned");
        if ids.contains(&event.event_id.0) {
            return Ok(KernelEventCursor::after(event.event_id));
        }
        append_to_memory(&mut events, &mut ids, event)
    }

    async fn events_after(
        &self,
        cursor: Option<KernelEventCursor>,
    ) -> KernelEventStoreResult<Vec<KernelEventEnvelope>> {
        let last_seen = cursor.and_then(|cursor| cursor.last_seen).map(|id| id.0);
        Ok(self
            .events
            .lock()
            .expect("kernel event store poisoned")
            .iter()
            .filter(|event| last_seen.is_none_or(|last_seen| event.event_id.0 > last_seen))
            .cloned()
            .collect())
    }

    async fn latest_cursor(&self) -> KernelEventStoreResult<Option<KernelEventCursor>> {
        Ok(self
            .events
            .lock()
            .expect("kernel event store poisoned")
            .last()
            .map(|event| KernelEventCursor::after(event.event_id)))
    }

    async fn events_for_aggregate(
        &self,
        aggregate: &KernelAggregateRef,
    ) -> KernelEventStoreResult<Vec<KernelEventEnvelope>> {
        Ok(self
            .events
            .lock()
            .expect("kernel event store poisoned")
            .iter()
            .filter(|event| &event.aggregate == aggregate)
            .cloned()
            .collect())
    }
}

fn append_to_memory(
    events: &mut Vec<KernelEventEnvelope>,
    ids: &mut BTreeSet<u64>,
    event: KernelEventEnvelope,
) -> KernelEventStoreResult<KernelEventCursor> {
    if ids.contains(&event.event_id.0) {
        return Err(KernelEventStoreError::AlreadyExists {
            event_id: event.event_id.0,
        });
    }
    if let Some(last) = events.last()
        && event.event_id.0 <= last.event_id.0
    {
        return Err(KernelEventStoreError::NonMonotonicId {
            last: last.event_id.0,
            next: event.event_id.0,
        });
    }
    ids.insert(event.event_id.0);
    let cursor = KernelEventCursor::after(event.event_id);
    events.push(event);
    Ok(cursor)
}

#[derive(Debug, Clone)]
pub struct JsonlKernelEventStore {
    path: PathBuf,
    memory: MemoryKernelEventStore,
}

impl JsonlKernelEventStore {
    pub async fn open(path: impl AsRef<Path>) -> KernelEventStoreResult<Self> {
        let path = path.as_ref().to_path_buf();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).await?;
        }
        let memory = MemoryKernelEventStore::new();
        if fs::try_exists(&path).await? {
            let file = OpenOptions::new().read(true).open(&path).await?;
            let mut lines = BufReader::new(file).lines();
            while let Some(line) = lines.next_line().await? {
                if line.trim().is_empty() {
                    continue;
                }
                let event: KernelEventEnvelope = serde_json::from_str(&line)?;
                memory.append_idempotent(event).await?;
            }
        } else {
            OpenOptions::new()
                .create(true)
                .append(true)
                .open(&path)
                .await?;
        }
        Ok(Self { path, memory })
    }

    async fn append_line(&self, event: &KernelEventEnvelope) -> KernelEventStoreResult<()> {
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)
            .await?;
        let line = serde_json::to_string(event)?;
        file.write_all(line.as_bytes()).await?;
        file.write_all(b"\n").await?;
        file.flush().await?;
        Ok(())
    }
}

#[async_trait::async_trait]
impl KernelEventStore for JsonlKernelEventStore {
    async fn append(
        &self,
        event: KernelEventEnvelope,
    ) -> KernelEventStoreResult<KernelEventCursor> {
        let cursor = self.memory.append(event.clone()).await?;
        self.append_line(&event).await?;
        Ok(cursor)
    }

    async fn append_idempotent(
        &self,
        event: KernelEventEnvelope,
    ) -> KernelEventStoreResult<KernelEventCursor> {
        let existing = self.memory.events_after(None).await?;
        if existing
            .iter()
            .any(|stored| stored.event_id == event.event_id)
        {
            return Ok(KernelEventCursor::after(event.event_id));
        }
        let cursor = self.memory.append(event.clone()).await?;
        self.append_line(&event).await?;
        Ok(cursor)
    }

    async fn events_after(
        &self,
        cursor: Option<KernelEventCursor>,
    ) -> KernelEventStoreResult<Vec<KernelEventEnvelope>> {
        self.memory.events_after(cursor).await
    }

    async fn latest_cursor(&self) -> KernelEventStoreResult<Option<KernelEventCursor>> {
        self.memory.latest_cursor().await
    }

    async fn events_for_aggregate(
        &self,
        aggregate: &KernelAggregateRef,
    ) -> KernelEventStoreResult<Vec<KernelEventEnvelope>> {
        self.memory.events_for_aggregate(aggregate).await
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct KernelProjectionSnapshot {
    pub projection_name: String,
    pub projection_version: u32,
    pub cursor: Option<KernelEventCursor>,
    pub checksum: Option<String>,
    pub payload: Value,
    pub created_at: DateTime<Utc>,
}

pub fn group_events_by_aggregate(
    events: &[KernelEventEnvelope],
) -> BTreeMap<KernelAggregateRef, Vec<KernelEventEnvelope>> {
    let mut grouped: BTreeMap<KernelAggregateRef, Vec<KernelEventEnvelope>> = BTreeMap::new();
    for event in events {
        grouped
            .entry(event.aggregate.clone())
            .or_default()
            .push(event.clone());
    }
    grouped
}
