use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use conversation_core::{ConversationId, MessageId, ParticipantId};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;
use tokio::fs::{self, OpenOptions};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

use crate::DurableAgentRunStatus;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct RunEventId(pub u64);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunJournalCursor {
    pub last_seen: Option<RunEventId>,
}

impl RunJournalCursor {
    pub fn beginning() -> Self {
        Self { last_seen: None }
    }

    pub fn after(event_id: RunEventId) -> Self {
        Self {
            last_seen: Some(event_id),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunEventKind {
    RunRequested,
    RunStarted,
    ModelCallStarted,
    ModelCallCompleted,
    ModelCallFailed,
    ToolCallRequested,
    ApprovalRequested,
    ApprovalGranted,
    ApprovalDenied,
    ToolCallStarted,
    ToolCallCompleted,
    ToolCallFailed,
    AssistantOutputDelta,
    AssistantOutputFinalized,
    RunCompleted,
    RunFailed,
    RunCancelled,
    RunTimedOut,
    RetryScheduled,
    RecoveryCheckpointed,
    RecoveryNeeded,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RunEvent {
    pub event_id: RunEventId,
    pub run_id: String,
    pub kind: RunEventKind,
    pub occurred_at: DateTime<Utc>,
    pub conversation_id: Option<ConversationId>,
    pub trigger_message_id: Option<MessageId>,
    pub actor: Option<ParticipantId>,
    pub payload: Value,
}

impl RunEvent {
    pub fn new(event_id: RunEventId, run_id: impl Into<String>, kind: RunEventKind) -> Self {
        Self {
            event_id,
            run_id: run_id.into(),
            kind,
            occurred_at: Utc::now(),
            conversation_id: None,
            trigger_message_id: None,
            actor: None,
            payload: Value::Null,
        }
    }

    pub fn with_conversation(mut self, conversation_id: ConversationId) -> Self {
        self.conversation_id = Some(conversation_id);
        self
    }

    pub fn with_trigger_message(mut self, trigger_message_id: MessageId) -> Self {
        self.trigger_message_id = Some(trigger_message_id);
        self
    }

    pub fn with_actor(mut self, actor: ParticipantId) -> Self {
        self.actor = Some(actor);
        self
    }

    pub fn with_payload(mut self, payload: Value) -> Self {
        self.payload = payload;
        self
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RunCheckpoint {
    pub run_id: String,
    pub event_cursor: RunJournalCursor,
    pub status: DurableAgentRunStatus,
    pub checkpointed_at: DateTime<Utc>,
    pub payload: Value,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunRecoveryAction {
    Resume,
    RestorePendingApproval,
    NeedsInspection,
    MarkCancelledAfterRestart,
    IgnoreTerminal,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecoverableRun {
    pub run_id: String,
    pub last_kind: RunEventKind,
    pub recommended_action: RunRecoveryAction,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunRecoveryReport {
    pub recoverable_runs: Vec<RecoverableRun>,
}

#[derive(Debug, Error)]
pub enum RunJournalError {
    #[error("run event already exists: {event_id}")]
    AlreadyExists { event_id: u64 },
    #[error("run event id must be monotonic: last={last}, next={next}")]
    NonMonotonicId { last: u64, next: u64 },
    #[error("run journal io error: {reason}")]
    Io { reason: String },
    #[error("run journal serialization error: {reason}")]
    Serde { reason: String },
}

impl From<std::io::Error> for RunJournalError {
    fn from(value: std::io::Error) -> Self {
        Self::Io {
            reason: value.to_string(),
        }
    }
}

impl From<serde_json::Error> for RunJournalError {
    fn from(value: serde_json::Error) -> Self {
        Self::Serde {
            reason: value.to_string(),
        }
    }
}

pub type RunJournalResult<T> = Result<T, RunJournalError>;

#[async_trait]
pub trait RunJournal: Send + Sync {
    async fn append_run_event(&self, event: RunEvent) -> RunJournalResult<RunJournalCursor>;
    async fn events_for_run(&self, run_id: &str) -> RunJournalResult<Vec<RunEvent>>;
    async fn events_after(
        &self,
        cursor: Option<RunJournalCursor>,
    ) -> RunJournalResult<Vec<RunEvent>>;
    async fn latest_cursor(&self) -> RunJournalResult<Option<RunJournalCursor>>;
    async fn checkpoint_run(&self, checkpoint: RunCheckpoint) -> RunJournalResult<()>;
    async fn checkpoints(&self, run_id: &str) -> RunJournalResult<Vec<RunCheckpoint>>;
    async fn recover_pending_runs(&self) -> RunJournalResult<RunRecoveryReport>;
}

#[derive(Debug, Default, Clone)]
pub struct MemoryRunJournal {
    events: Arc<Mutex<Vec<RunEvent>>>,
    event_ids: Arc<Mutex<BTreeSet<u64>>>,
    checkpoints: Arc<Mutex<BTreeMap<String, Vec<RunCheckpoint>>>>,
}

impl MemoryRunJournal {
    pub fn new() -> Self {
        Self::default()
    }
}

#[async_trait]
impl RunJournal for MemoryRunJournal {
    async fn append_run_event(&self, event: RunEvent) -> RunJournalResult<RunJournalCursor> {
        let mut events = self.events.lock().expect("run journal events poisoned");
        let mut ids = self.event_ids.lock().expect("run journal ids poisoned");
        append_event_to_memory(&mut events, &mut ids, event)
    }

    async fn events_for_run(&self, run_id: &str) -> RunJournalResult<Vec<RunEvent>> {
        Ok(self
            .events
            .lock()
            .expect("run journal events poisoned")
            .iter()
            .filter(|event| event.run_id == run_id)
            .cloned()
            .collect())
    }

    async fn events_after(
        &self,
        cursor: Option<RunJournalCursor>,
    ) -> RunJournalResult<Vec<RunEvent>> {
        let last_seen = cursor.and_then(|cursor| cursor.last_seen).map(|id| id.0);
        Ok(self
            .events
            .lock()
            .expect("run journal events poisoned")
            .iter()
            .filter(|event| last_seen.is_none_or(|last_seen| event.event_id.0 > last_seen))
            .cloned()
            .collect())
    }

    async fn latest_cursor(&self) -> RunJournalResult<Option<RunJournalCursor>> {
        Ok(self
            .events
            .lock()
            .expect("run journal events poisoned")
            .last()
            .map(|event| RunJournalCursor::after(event.event_id)))
    }

    async fn checkpoint_run(&self, checkpoint: RunCheckpoint) -> RunJournalResult<()> {
        self.checkpoints
            .lock()
            .expect("run journal checkpoints poisoned")
            .entry(checkpoint.run_id.clone())
            .or_default()
            .push(checkpoint);
        Ok(())
    }

    async fn checkpoints(&self, run_id: &str) -> RunJournalResult<Vec<RunCheckpoint>> {
        Ok(self
            .checkpoints
            .lock()
            .expect("run journal checkpoints poisoned")
            .get(run_id)
            .cloned()
            .unwrap_or_default())
    }

    async fn recover_pending_runs(&self) -> RunJournalResult<RunRecoveryReport> {
        Ok(build_recovery_report(
            &self
                .events
                .lock()
                .expect("run journal events poisoned")
                .clone(),
        ))
    }
}

fn append_event_to_memory(
    events: &mut Vec<RunEvent>,
    ids: &mut BTreeSet<u64>,
    event: RunEvent,
) -> RunJournalResult<RunJournalCursor> {
    if ids.contains(&event.event_id.0) {
        return Err(RunJournalError::AlreadyExists {
            event_id: event.event_id.0,
        });
    }
    if let Some(last) = events.last()
        && event.event_id.0 <= last.event_id.0
    {
        return Err(RunJournalError::NonMonotonicId {
            last: last.event_id.0,
            next: event.event_id.0,
        });
    }
    ids.insert(event.event_id.0);
    let cursor = RunJournalCursor::after(event.event_id);
    events.push(event);
    Ok(cursor)
}

#[derive(Debug, Clone)]
pub struct JsonlRunJournal {
    events_path: PathBuf,
    checkpoints_path: PathBuf,
    memory: MemoryRunJournal,
}

impl JsonlRunJournal {
    pub async fn open(root: impl AsRef<Path>) -> RunJournalResult<Self> {
        let root = root.as_ref().to_path_buf();
        fs::create_dir_all(&root).await?;
        let events_path = root.join("run-events.jsonl");
        let checkpoints_path = root.join("run-checkpoints.jsonl");
        let memory = MemoryRunJournal::new();

        if fs::try_exists(&events_path).await? {
            let file = OpenOptions::new().read(true).open(&events_path).await?;
            let mut lines = BufReader::new(file).lines();
            while let Some(line) = lines.next_line().await? {
                if line.trim().is_empty() {
                    continue;
                }
                let event: RunEvent = serde_json::from_str(&line)?;
                memory.append_run_event(event).await?;
            }
        } else {
            OpenOptions::new()
                .create(true)
                .append(true)
                .open(&events_path)
                .await?;
        }

        if fs::try_exists(&checkpoints_path).await? {
            let file = OpenOptions::new()
                .read(true)
                .open(&checkpoints_path)
                .await?;
            let mut lines = BufReader::new(file).lines();
            while let Some(line) = lines.next_line().await? {
                if line.trim().is_empty() {
                    continue;
                }
                let checkpoint: RunCheckpoint = serde_json::from_str(&line)?;
                memory.checkpoint_run(checkpoint).await?;
            }
        } else {
            OpenOptions::new()
                .create(true)
                .append(true)
                .open(&checkpoints_path)
                .await?;
        }

        Ok(Self {
            events_path,
            checkpoints_path,
            memory,
        })
    }

    async fn append_jsonl<T: Serialize>(&self, path: &Path, item: &T) -> RunJournalResult<()> {
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .await?;
        let line = serde_json::to_string(item)?;
        file.write_all(line.as_bytes()).await?;
        file.write_all(b"\n").await?;
        file.flush().await?;
        Ok(())
    }
}

#[async_trait]
impl RunJournal for JsonlRunJournal {
    async fn append_run_event(&self, event: RunEvent) -> RunJournalResult<RunJournalCursor> {
        let cursor = self.memory.append_run_event(event.clone()).await?;
        self.append_jsonl(&self.events_path, &event).await?;
        Ok(cursor)
    }

    async fn events_for_run(&self, run_id: &str) -> RunJournalResult<Vec<RunEvent>> {
        self.memory.events_for_run(run_id).await
    }

    async fn events_after(
        &self,
        cursor: Option<RunJournalCursor>,
    ) -> RunJournalResult<Vec<RunEvent>> {
        self.memory.events_after(cursor).await
    }

    async fn latest_cursor(&self) -> RunJournalResult<Option<RunJournalCursor>> {
        self.memory.latest_cursor().await
    }

    async fn checkpoint_run(&self, checkpoint: RunCheckpoint) -> RunJournalResult<()> {
        self.memory.checkpoint_run(checkpoint.clone()).await?;
        self.append_jsonl(&self.checkpoints_path, &checkpoint).await
    }

    async fn checkpoints(&self, run_id: &str) -> RunJournalResult<Vec<RunCheckpoint>> {
        self.memory.checkpoints(run_id).await
    }

    async fn recover_pending_runs(&self) -> RunJournalResult<RunRecoveryReport> {
        self.memory.recover_pending_runs().await
    }
}

pub fn build_recovery_report(events: &[RunEvent]) -> RunRecoveryReport {
    let mut latest_by_run: BTreeMap<String, RunEvent> = BTreeMap::new();
    for event in events {
        latest_by_run.insert(event.run_id.clone(), event.clone());
    }

    let recoverable_runs = latest_by_run
        .into_iter()
        .filter_map(|(run_id, event)| {
            let recommended_action = match event.kind {
                RunEventKind::RunCompleted
                | RunEventKind::RunFailed
                | RunEventKind::RunCancelled
                | RunEventKind::RunTimedOut => return None,
                RunEventKind::ApprovalRequested => RunRecoveryAction::RestorePendingApproval,
                RunEventKind::ToolCallStarted => RunRecoveryAction::NeedsInspection,
                RunEventKind::ToolCallRequested | RunEventKind::ApprovalGranted => {
                    RunRecoveryAction::NeedsInspection
                }
                RunEventKind::RunStarted
                | RunEventKind::ModelCallStarted
                | RunEventKind::AssistantOutputDelta => {
                    RunRecoveryAction::MarkCancelledAfterRestart
                }
                RunEventKind::RunRequested
                | RunEventKind::ModelCallCompleted
                | RunEventKind::ModelCallFailed
                | RunEventKind::ApprovalDenied
                | RunEventKind::ToolCallCompleted
                | RunEventKind::ToolCallFailed
                | RunEventKind::AssistantOutputFinalized
                | RunEventKind::RetryScheduled
                | RunEventKind::RecoveryCheckpointed
                | RunEventKind::RecoveryNeeded => RunRecoveryAction::Resume,
            };
            Some(RecoverableRun {
                run_id,
                last_kind: event.kind,
                recommended_action,
            })
        })
        .collect();

    RunRecoveryReport { recoverable_runs }
}
