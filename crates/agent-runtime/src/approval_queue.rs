use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use action_core::ActionId;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use conversation_core::{ConversationId, ParticipantId};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tokio::fs::{self, OpenOptions};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalStatus {
    Pending,
    Approved,
    Denied,
    Revoked,
    Expired,
}

impl ApprovalStatus {
    pub fn is_terminal(&self) -> bool {
        matches!(
            self,
            Self::Approved | Self::Denied | Self::Revoked | Self::Expired
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalDecisionKind {
    Approved,
    Denied,
    Revoked,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApprovalDecision {
    pub kind: ApprovalDecisionKind,
    pub decided_by: ParticipantId,
    pub reason: String,
    pub decided_at: DateTime<Utc>,
}

impl ApprovalDecision {
    pub fn approved(decided_by: ParticipantId, reason: impl Into<String>) -> Self {
        Self {
            kind: ApprovalDecisionKind::Approved,
            decided_by,
            reason: reason.into(),
            decided_at: Utc::now(),
        }
    }

    pub fn denied(decided_by: ParticipantId, reason: impl Into<String>) -> Self {
        Self {
            kind: ApprovalDecisionKind::Denied,
            decided_by,
            reason: reason.into(),
            decided_at: Utc::now(),
        }
    }

    pub fn revoked(reason: impl Into<String>) -> Self {
        Self {
            kind: ApprovalDecisionKind::Revoked,
            decided_by: ParticipantId::from("system"),
            reason: reason.into(),
            decided_at: Utc::now(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApprovalRequest {
    pub approval_id: String,
    pub action_id: ActionId,
    pub conversation_id: ConversationId,
    pub requested_by: ParticipantId,
    pub reason: String,
    pub requested_at: DateTime<Utc>,
    pub expires_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApprovalRecord {
    pub approval_id: String,
    pub request: ApprovalRequest,
    pub status: ApprovalStatus,
    pub decision: Option<ApprovalDecision>,
    pub updated_at: DateTime<Utc>,
}

impl ApprovalRecord {
    pub fn pending(request: ApprovalRequest) -> Self {
        let approval_id = request.approval_id.clone();
        Self {
            approval_id,
            request,
            status: ApprovalStatus::Pending,
            decision: None,
            updated_at: Utc::now(),
        }
    }
}

#[derive(Debug, Error)]
pub enum ApprovalQueueError {
    #[error("approval not found: {approval_id}")]
    NotFound { approval_id: String },

    #[error("approval already exists: {approval_id}")]
    AlreadyExists { approval_id: String },

    #[error("approval already decided: {approval_id}")]
    AlreadyDecided { approval_id: String },

    #[error("approval expired: {approval_id}")]
    ApprovalExpired { approval_id: String },

    #[error("approval queue io error: {reason}")]
    Io { reason: String },

    #[error("approval queue serialization error: {reason}")]
    Serde { reason: String },
}

impl From<std::io::Error> for ApprovalQueueError {
    fn from(value: std::io::Error) -> Self {
        Self::Io {
            reason: value.to_string(),
        }
    }
}

impl From<serde_json::Error> for ApprovalQueueError {
    fn from(value: serde_json::Error) -> Self {
        Self::Serde {
            reason: value.to_string(),
        }
    }
}

pub type ApprovalQueueResult<T> = Result<T, ApprovalQueueError>;

#[async_trait]
pub trait ApprovalQueue: Send + Sync {
    async fn enqueue(&self, request: ApprovalRequest) -> ApprovalQueueResult<ApprovalRecord>;
    async fn get(&self, approval_id: &str) -> ApprovalQueueResult<Option<ApprovalRecord>>;
    async fn pending(&self) -> ApprovalQueueResult<Vec<ApprovalRecord>>;
    async fn approve(
        &self,
        approval_id: &str,
        decision: ApprovalDecision,
    ) -> ApprovalQueueResult<ApprovalRecord>;
    async fn deny(
        &self,
        approval_id: &str,
        decision: ApprovalDecision,
    ) -> ApprovalQueueResult<ApprovalRecord>;
    async fn revoke(&self, approval_id: &str, reason: &str) -> ApprovalQueueResult<ApprovalRecord>;
}

#[derive(Debug, Default, Clone)]
pub struct MemoryApprovalQueue {
    records: Arc<Mutex<BTreeMap<String, ApprovalRecord>>>,
}

impl MemoryApprovalQueue {
    pub fn new() -> Self {
        Self::default()
    }
}

#[async_trait]
impl ApprovalQueue for MemoryApprovalQueue {
    async fn enqueue(&self, request: ApprovalRequest) -> ApprovalQueueResult<ApprovalRecord> {
        let record = ApprovalRecord::pending(request);
        let mut records = self.records.lock().expect("approval queue poisoned");
        if records.contains_key(&record.approval_id) {
            return Err(ApprovalQueueError::AlreadyExists {
                approval_id: record.approval_id,
            });
        }
        records.insert(record.approval_id.clone(), record.clone());
        Ok(record)
    }

    async fn get(&self, approval_id: &str) -> ApprovalQueueResult<Option<ApprovalRecord>> {
        Ok(self
            .records
            .lock()
            .expect("approval queue poisoned")
            .get(approval_id)
            .cloned())
    }

    async fn pending(&self) -> ApprovalQueueResult<Vec<ApprovalRecord>> {
        let mut records = self.records.lock().expect("approval queue poisoned");
        expire_pending_records(&mut records);
        Ok(records
            .values()
            .filter(|record| record.status == ApprovalStatus::Pending)
            .cloned()
            .collect())
    }

    async fn approve(
        &self,
        approval_id: &str,
        decision: ApprovalDecision,
    ) -> ApprovalQueueResult<ApprovalRecord> {
        let mut records = self.records.lock().expect("approval queue poisoned");
        decide(
            &mut records,
            approval_id,
            ApprovalStatus::Approved,
            decision,
        )
    }

    async fn deny(
        &self,
        approval_id: &str,
        decision: ApprovalDecision,
    ) -> ApprovalQueueResult<ApprovalRecord> {
        let mut records = self.records.lock().expect("approval queue poisoned");
        decide(&mut records, approval_id, ApprovalStatus::Denied, decision)
    }

    async fn revoke(&self, approval_id: &str, reason: &str) -> ApprovalQueueResult<ApprovalRecord> {
        let mut records = self.records.lock().expect("approval queue poisoned");
        decide(
            &mut records,
            approval_id,
            ApprovalStatus::Revoked,
            ApprovalDecision::revoked(reason),
        )
    }
}

#[derive(Debug, Clone)]
pub struct JsonlApprovalQueue {
    path: PathBuf,
    records: MemoryApprovalQueue,
}

impl JsonlApprovalQueue {
    pub async fn open(path: impl AsRef<Path>) -> ApprovalQueueResult<Self> {
        let path = path.as_ref().to_path_buf();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).await?;
        }
        let records = MemoryApprovalQueue::new();
        if fs::try_exists(&path).await? {
            let file = OpenOptions::new().read(true).open(&path).await?;
            let mut lines = BufReader::new(file).lines();
            while let Some(line) = lines.next_line().await? {
                if line.trim().is_empty() {
                    continue;
                }
                let record: ApprovalRecord = serde_json::from_str(&line)?;
                records
                    .records
                    .lock()
                    .expect("approval queue poisoned")
                    .insert(record.approval_id.clone(), record);
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

    async fn append_record(&self, record: &ApprovalRecord) -> ApprovalQueueResult<()> {
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
impl ApprovalQueue for JsonlApprovalQueue {
    async fn enqueue(&self, request: ApprovalRequest) -> ApprovalQueueResult<ApprovalRecord> {
        let record = self.records.enqueue(request).await?;
        self.append_record(&record).await?;
        Ok(record)
    }

    async fn get(&self, approval_id: &str) -> ApprovalQueueResult<Option<ApprovalRecord>> {
        self.records.get(approval_id).await
    }

    async fn pending(&self) -> ApprovalQueueResult<Vec<ApprovalRecord>> {
        self.records.pending().await
    }

    async fn approve(
        &self,
        approval_id: &str,
        decision: ApprovalDecision,
    ) -> ApprovalQueueResult<ApprovalRecord> {
        let record = self.records.approve(approval_id, decision).await?;
        self.append_record(&record).await?;
        Ok(record)
    }

    async fn deny(
        &self,
        approval_id: &str,
        decision: ApprovalDecision,
    ) -> ApprovalQueueResult<ApprovalRecord> {
        let record = self.records.deny(approval_id, decision).await?;
        self.append_record(&record).await?;
        Ok(record)
    }

    async fn revoke(&self, approval_id: &str, reason: &str) -> ApprovalQueueResult<ApprovalRecord> {
        let record = self.records.revoke(approval_id, reason).await?;
        self.append_record(&record).await?;
        Ok(record)
    }
}

fn decide(
    records: &mut BTreeMap<String, ApprovalRecord>,
    approval_id: &str,
    status: ApprovalStatus,
    decision: ApprovalDecision,
) -> ApprovalQueueResult<ApprovalRecord> {
    let record = records
        .get_mut(approval_id)
        .ok_or_else(|| ApprovalQueueError::NotFound {
            approval_id: approval_id.to_string(),
        })?;
    if record.status.is_terminal() {
        return Err(ApprovalQueueError::AlreadyDecided {
            approval_id: approval_id.to_string(),
        });
    }
    if is_expired(record) {
        record.status = ApprovalStatus::Expired;
        record.updated_at = Utc::now();
        return Err(ApprovalQueueError::ApprovalExpired {
            approval_id: approval_id.to_string(),
        });
    }
    record.status = status;
    record.decision = Some(decision);
    record.updated_at = Utc::now();
    Ok(record.clone())
}

fn expire_pending_records(records: &mut BTreeMap<String, ApprovalRecord>) {
    for record in records.values_mut() {
        if record.status == ApprovalStatus::Pending && is_expired(record) {
            record.status = ApprovalStatus::Expired;
            record.updated_at = Utc::now();
        }
    }
}

fn is_expired(record: &ApprovalRecord) -> bool {
    record
        .request
        .expires_at
        .is_some_and(|expires_at| expires_at <= Utc::now())
}
