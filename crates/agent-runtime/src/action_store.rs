use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use action_core::{ActionId, ActionRequest, ActionStatus};
use action_runtime::ActionRuntimeOutcome;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tokio::fs::{self, OpenOptions};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ActionRecord {
    pub action_id: ActionId,
    pub request: ActionRequest,
    pub status: ActionStatus,
    pub outcome: Option<ActionRuntimeOutcome>,
    pub audit_correlation_id: String,
    pub idempotency_key: String,
    pub requested_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl ActionRecord {
    pub fn requested(
        request: ActionRequest,
        audit_correlation_id: impl Into<String>,
        idempotency_key: impl Into<String>,
    ) -> Self {
        let now = Utc::now();
        Self {
            action_id: request.action_id.clone(),
            status: ActionStatus::Pending,
            request,
            outcome: None,
            audit_correlation_id: audit_correlation_id.into(),
            idempotency_key: idempotency_key.into(),
            requested_at: now,
            updated_at: now,
        }
    }
}

#[derive(Debug, Error)]
pub enum ActionStoreError {
    #[error("action not found: {action_id}")]
    NotFound { action_id: String },

    #[error("action already exists: {action_id}")]
    AlreadyExists { action_id: String },

    #[error("action already completed: {action_id}")]
    ActionAlreadyCompleted { action_id: String },

    #[error("action store io error: {reason}")]
    Io { reason: String },

    #[error("action store serialization error: {reason}")]
    Serde { reason: String },
}

impl From<std::io::Error> for ActionStoreError {
    fn from(value: std::io::Error) -> Self {
        Self::Io {
            reason: value.to_string(),
        }
    }
}

impl From<serde_json::Error> for ActionStoreError {
    fn from(value: serde_json::Error) -> Self {
        Self::Serde {
            reason: value.to_string(),
        }
    }
}

pub type ActionStoreResult<T> = Result<T, ActionStoreError>;

#[async_trait]
pub trait ActionStore: Send + Sync {
    async fn insert_request(&self, record: ActionRecord) -> ActionStoreResult<()>;
    async fn get(&self, action_id: &str) -> ActionStoreResult<Option<ActionRecord>>;
    async fn list(&self) -> ActionStoreResult<Vec<ActionRecord>>;
    async fn record_outcome(
        &self,
        action_id: &str,
        outcome: ActionRuntimeOutcome,
    ) -> ActionStoreResult<ActionRecord>;
}

#[derive(Debug, Default, Clone)]
pub struct MemoryActionStore {
    records: Arc<Mutex<BTreeMap<String, ActionRecord>>>,
}

impl MemoryActionStore {
    pub fn new() -> Self {
        Self::default()
    }
}

#[async_trait]
impl ActionStore for MemoryActionStore {
    async fn insert_request(&self, record: ActionRecord) -> ActionStoreResult<()> {
        let mut records = self.records.lock().expect("action store poisoned");
        let action_id = record.action_id.to_string();
        if records.contains_key(&action_id) {
            return Err(ActionStoreError::AlreadyExists { action_id });
        }
        records.insert(action_id, record);
        Ok(())
    }

    async fn get(&self, action_id: &str) -> ActionStoreResult<Option<ActionRecord>> {
        Ok(self
            .records
            .lock()
            .expect("action store poisoned")
            .get(action_id)
            .cloned())
    }

    async fn list(&self) -> ActionStoreResult<Vec<ActionRecord>> {
        Ok(self
            .records
            .lock()
            .expect("action store poisoned")
            .values()
            .cloned()
            .collect())
    }

    async fn record_outcome(
        &self,
        action_id: &str,
        outcome: ActionRuntimeOutcome,
    ) -> ActionStoreResult<ActionRecord> {
        let mut records = self.records.lock().expect("action store poisoned");
        let record = records
            .get_mut(action_id)
            .ok_or_else(|| ActionStoreError::NotFound {
                action_id: action_id.to_string(),
            })?;
        apply_outcome(record, outcome)?;
        Ok(record.clone())
    }
}

#[derive(Debug, Clone)]
pub struct JsonlActionStore {
    path: PathBuf,
    records: MemoryActionStore,
}

impl JsonlActionStore {
    pub async fn open(path: impl AsRef<Path>) -> ActionStoreResult<Self> {
        let path = path.as_ref().to_path_buf();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).await?;
        }
        let records = MemoryActionStore::new();
        if fs::try_exists(&path).await? {
            let file = OpenOptions::new().read(true).open(&path).await?;
            let mut lines = BufReader::new(file).lines();
            while let Some(line) = lines.next_line().await? {
                if line.trim().is_empty() {
                    continue;
                }
                let record: ActionRecord = serde_json::from_str(&line)?;
                records
                    .records
                    .lock()
                    .expect("action store poisoned")
                    .insert(record.action_id.to_string(), record);
            }
        } else {
            OpenOptions::new()
                .create(true)
                .append(true)
                .open(&path)
                .await?;
        }
        Ok(Self { path, records })
    }

    async fn append_record(&self, record: &ActionRecord) -> ActionStoreResult<()> {
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)
            .await?;
        let line = serde_json::to_string(record)?;
        file.write_all(line.as_bytes()).await?;
        file.write_all(b"\n").await?;
        file.flush().await?;
        Ok(())
    }
}

#[async_trait]
impl ActionStore for JsonlActionStore {
    async fn insert_request(&self, record: ActionRecord) -> ActionStoreResult<()> {
        self.records.insert_request(record.clone()).await?;
        self.append_record(&record).await
    }

    async fn get(&self, action_id: &str) -> ActionStoreResult<Option<ActionRecord>> {
        self.records.get(action_id).await
    }

    async fn list(&self) -> ActionStoreResult<Vec<ActionRecord>> {
        self.records.list().await
    }

    async fn record_outcome(
        &self,
        action_id: &str,
        outcome: ActionRuntimeOutcome,
    ) -> ActionStoreResult<ActionRecord> {
        let existing = self.records.get(action_id).await?;
        let record = self.records.record_outcome(action_id, outcome).await?;
        if existing.as_ref().and_then(|r| r.outcome.as_ref()) != record.outcome.as_ref() {
            self.append_record(&record).await?;
        }
        Ok(record)
    }
}

fn apply_outcome(
    record: &mut ActionRecord,
    outcome: ActionRuntimeOutcome,
) -> ActionStoreResult<()> {
    if let Some(existing) = &record.outcome {
        if existing == &outcome {
            return Ok(());
        }
        return Err(ActionStoreError::ActionAlreadyCompleted {
            action_id: record.action_id.to_string(),
        });
    }
    record.status = status_from_outcome(&outcome);
    record.outcome = Some(outcome);
    record.updated_at = Utc::now();
    Ok(())
}

fn status_from_outcome(outcome: &ActionRuntimeOutcome) -> ActionStatus {
    match outcome {
        ActionRuntimeOutcome::Completed { .. } => ActionStatus::Completed,
        ActionRuntimeOutcome::ApprovalRequired { .. } => ActionStatus::ApprovalRequired,
        ActionRuntimeOutcome::Denied { .. } => ActionStatus::Denied,
        ActionRuntimeOutcome::Failed { .. } => ActionStatus::Failed,
    }
}
