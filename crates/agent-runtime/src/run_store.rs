use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use conversation_core::{ConversationId, MessageId, ParticipantId};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tokio::fs::{self, OpenOptions};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DurableAgentRunStatus {
    Queued,
    Running,
    WaitingForApproval,
    Completed,
    Failed,
    Cancelled,
    TimedOut,
}

impl DurableAgentRunStatus {
    pub fn is_terminal(&self) -> bool {
        matches!(
            self,
            Self::Completed | Self::Failed | Self::Cancelled | Self::TimedOut
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentRunRecord {
    pub run_id: String,
    pub conversation_id: ConversationId,
    pub trigger_message_id: MessageId,
    pub requested_by: ParticipantId,
    pub status: DurableAgentRunStatus,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub error_message: Option<String>,
}

#[derive(Debug, Error)]
pub enum AgentRunStoreError {
    #[error("agent run not found: {run_id}")]
    NotFound { run_id: String },

    #[error("agent run already exists: {run_id}")]
    AlreadyExists { run_id: String },

    #[error("invalid agent run transition for {run_id}: {from:?} -> {to:?}")]
    InvalidTransition {
        run_id: String,
        from: DurableAgentRunStatus,
        to: DurableAgentRunStatus,
    },

    #[error("agent run store io error: {reason}")]
    Io { reason: String },

    #[error("agent run store serialization error: {reason}")]
    Serde { reason: String },
}

impl AgentRunStoreError {
    pub fn run_id(&self) -> &str {
        match self {
            Self::NotFound { run_id }
            | Self::AlreadyExists { run_id }
            | Self::InvalidTransition { run_id, .. } => run_id,
            Self::Io { .. } | Self::Serde { .. } => "",
        }
    }
}

impl From<std::io::Error> for AgentRunStoreError {
    fn from(value: std::io::Error) -> Self {
        Self::Io {
            reason: value.to_string(),
        }
    }
}

impl From<serde_json::Error> for AgentRunStoreError {
    fn from(value: serde_json::Error) -> Self {
        Self::Serde {
            reason: value.to_string(),
        }
    }
}

pub type AgentRunStoreResult<T> = Result<T, AgentRunStoreError>;

#[async_trait]
pub trait AgentRunStore: Send + Sync {
    async fn insert(&self, record: AgentRunRecord) -> AgentRunStoreResult<()>;
    async fn get(&self, run_id: &str) -> AgentRunStoreResult<Option<AgentRunRecord>>;
    async fn list(&self) -> AgentRunStoreResult<Vec<AgentRunRecord>>;
    async fn transition(
        &self,
        run_id: &str,
        status: DurableAgentRunStatus,
    ) -> AgentRunStoreResult<AgentRunRecord>;
}

#[derive(Debug, Default, Clone)]
pub struct MemoryAgentRunStore {
    records: Arc<Mutex<BTreeMap<String, AgentRunRecord>>>,
}

impl MemoryAgentRunStore {
    pub fn new() -> Self {
        Self::default()
    }
}

#[async_trait]
impl AgentRunStore for MemoryAgentRunStore {
    async fn insert(&self, record: AgentRunRecord) -> AgentRunStoreResult<()> {
        let mut records = self.records.lock().expect("agent run store poisoned");
        if records.contains_key(&record.run_id) {
            return Err(AgentRunStoreError::AlreadyExists {
                run_id: record.run_id,
            });
        }
        records.insert(record.run_id.clone(), record);
        Ok(())
    }

    async fn get(&self, run_id: &str) -> AgentRunStoreResult<Option<AgentRunRecord>> {
        Ok(self
            .records
            .lock()
            .expect("agent run store poisoned")
            .get(run_id)
            .cloned())
    }

    async fn list(&self) -> AgentRunStoreResult<Vec<AgentRunRecord>> {
        Ok(self
            .records
            .lock()
            .expect("agent run store poisoned")
            .values()
            .cloned()
            .collect())
    }

    async fn transition(
        &self,
        run_id: &str,
        status: DurableAgentRunStatus,
    ) -> AgentRunStoreResult<AgentRunRecord> {
        let mut records = self.records.lock().expect("agent run store poisoned");
        let record = records
            .get_mut(run_id)
            .ok_or_else(|| AgentRunStoreError::NotFound {
                run_id: run_id.to_string(),
            })?;
        transition_record(record, status)?;
        Ok(record.clone())
    }
}

#[derive(Debug, Clone)]
pub struct JsonlAgentRunStore {
    path: PathBuf,
    records: MemoryAgentRunStore,
}

impl JsonlAgentRunStore {
    pub async fn open(path: impl AsRef<Path>) -> AgentRunStoreResult<Self> {
        let path = path.as_ref().to_path_buf();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).await?;
        }
        let records = MemoryAgentRunStore::new();
        if fs::try_exists(&path).await? {
            let file = OpenOptions::new().read(true).open(&path).await?;
            let mut lines = BufReader::new(file).lines();
            while let Some(line) = lines.next_line().await? {
                if line.trim().is_empty() {
                    continue;
                }
                let record: AgentRunRecord = serde_json::from_str(&line)?;
                records
                    .records
                    .lock()
                    .expect("agent run store poisoned")
                    .insert(record.run_id.clone(), record);
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

    async fn append_record(&self, record: &AgentRunRecord) -> AgentRunStoreResult<()> {
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
impl AgentRunStore for JsonlAgentRunStore {
    async fn insert(&self, record: AgentRunRecord) -> AgentRunStoreResult<()> {
        self.records.insert(record.clone()).await?;
        self.append_record(&record).await
    }

    async fn get(&self, run_id: &str) -> AgentRunStoreResult<Option<AgentRunRecord>> {
        self.records.get(run_id).await
    }

    async fn list(&self) -> AgentRunStoreResult<Vec<AgentRunRecord>> {
        self.records.list().await
    }

    async fn transition(
        &self,
        run_id: &str,
        status: DurableAgentRunStatus,
    ) -> AgentRunStoreResult<AgentRunRecord> {
        let record = self.records.transition(run_id, status).await?;
        self.append_record(&record).await?;
        Ok(record)
    }
}

fn transition_record(
    record: &mut AgentRunRecord,
    to: DurableAgentRunStatus,
) -> AgentRunStoreResult<()> {
    let from = record.status.clone();
    if !is_valid_transition(&from, &to) {
        return Err(AgentRunStoreError::InvalidTransition {
            run_id: record.run_id.clone(),
            from,
            to,
        });
    }
    record.status = to;
    record.updated_at = Utc::now();
    Ok(())
}

fn is_valid_transition(from: &DurableAgentRunStatus, to: &DurableAgentRunStatus) -> bool {
    if from == to {
        return true;
    }
    if from.is_terminal() {
        return false;
    }
    matches!(
        (from, to),
        (
            DurableAgentRunStatus::Queued,
            DurableAgentRunStatus::Running
        ) | (
            DurableAgentRunStatus::Queued,
            DurableAgentRunStatus::Cancelled
        ) | (
            DurableAgentRunStatus::Queued,
            DurableAgentRunStatus::TimedOut
        ) | (
            DurableAgentRunStatus::Running,
            DurableAgentRunStatus::WaitingForApproval
        ) | (
            DurableAgentRunStatus::Running,
            DurableAgentRunStatus::Completed
        ) | (
            DurableAgentRunStatus::Running,
            DurableAgentRunStatus::Failed
        ) | (
            DurableAgentRunStatus::Running,
            DurableAgentRunStatus::Cancelled
        ) | (
            DurableAgentRunStatus::Running,
            DurableAgentRunStatus::TimedOut
        ) | (
            DurableAgentRunStatus::WaitingForApproval,
            DurableAgentRunStatus::Running
        ) | (
            DurableAgentRunStatus::WaitingForApproval,
            DurableAgentRunStatus::Cancelled
        ) | (
            DurableAgentRunStatus::WaitingForApproval,
            DurableAgentRunStatus::TimedOut
        )
    )
}
