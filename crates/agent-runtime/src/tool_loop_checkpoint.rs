use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolLoopCheckpoint {
    pub run_id: String,
    pub turn: u32,
    pub kind: ToolLoopCheckpointKind,
    pub tool_result: Option<ToolResultCheckpoint>,
    pub created_at: DateTime<Utc>,
}

impl ToolLoopCheckpoint {
    pub fn before_model_call(run_id: impl Into<String>, turn: u32) -> Self {
        Self {
            run_id: run_id.into(),
            turn,
            kind: ToolLoopCheckpointKind::BeforeModelCall,
            tool_result: None,
            created_at: Utc::now(),
        }
    }

    pub fn after_model_call(run_id: impl Into<String>, turn: u32) -> Self {
        Self {
            run_id: run_id.into(),
            turn,
            kind: ToolLoopCheckpointKind::AfterModelCall,
            tool_result: None,
            created_at: Utc::now(),
        }
    }

    pub fn tool_result(
        run_id: impl Into<String>,
        turn: u32,
        tool_result: ToolResultCheckpoint,
    ) -> Self {
        Self {
            run_id: run_id.into(),
            turn,
            kind: ToolLoopCheckpointKind::ToolResult,
            tool_result: Some(tool_result),
            created_at: Utc::now(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolLoopCheckpointKind {
    BeforeModelCall,
    AfterModelCall,
    ToolResult,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolResultCheckpoint {
    pub tool_call_id: String,
    pub action_id: String,
    pub result_text: String,
    pub read_only: bool,
}

#[derive(Debug, Error)]
pub enum ToolLoopCheckpointError {
    #[error("tool loop checkpoint store poisoned")]
    StorePoisoned,
}

pub type ToolLoopCheckpointResult<T> = Result<T, ToolLoopCheckpointError>;

#[async_trait]
pub trait ToolLoopCheckpointStore: Send + Sync {
    async fn append(&self, checkpoint: ToolLoopCheckpoint) -> ToolLoopCheckpointResult<()>;
    async fn list(&self, run_id: &str) -> ToolLoopCheckpointResult<Vec<ToolLoopCheckpoint>>;
}

#[derive(Debug, Default, Clone)]
pub struct MemoryToolLoopCheckpointStore {
    checkpoints: Arc<Mutex<BTreeMap<String, Vec<ToolLoopCheckpoint>>>>,
}

impl MemoryToolLoopCheckpointStore {
    pub fn new() -> Self {
        Self::default()
    }
}

#[async_trait]
impl ToolLoopCheckpointStore for MemoryToolLoopCheckpointStore {
    async fn append(&self, checkpoint: ToolLoopCheckpoint) -> ToolLoopCheckpointResult<()> {
        let mut checkpoints = self
            .checkpoints
            .lock()
            .map_err(|_| ToolLoopCheckpointError::StorePoisoned)?;
        checkpoints
            .entry(checkpoint.run_id.clone())
            .or_default()
            .push(checkpoint);
        Ok(())
    }

    async fn list(&self, run_id: &str) -> ToolLoopCheckpointResult<Vec<ToolLoopCheckpoint>> {
        let checkpoints = self
            .checkpoints
            .lock()
            .map_err(|_| ToolLoopCheckpointError::StorePoisoned)?;
        Ok(checkpoints.get(run_id).cloned().unwrap_or_default())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ToolLoopResumePlan {
    completed_tool_results: BTreeMap<String, ToolResultCheckpoint>,
    skippable_tool_calls: BTreeSet<String>,
    last_started_turn: Option<u32>,
    last_completed_model_turn: Option<u32>,
}

impl ToolLoopResumePlan {
    pub fn from_checkpoints(checkpoints: Vec<ToolLoopCheckpoint>) -> Self {
        let mut plan = Self::default();
        for checkpoint in checkpoints {
            match checkpoint.kind {
                ToolLoopCheckpointKind::BeforeModelCall => {
                    plan.last_started_turn = Some(
                        plan.last_started_turn
                            .map_or(checkpoint.turn, |turn| turn.max(checkpoint.turn)),
                    );
                }
                ToolLoopCheckpointKind::AfterModelCall => {
                    plan.last_completed_model_turn = Some(
                        plan.last_completed_model_turn
                            .map_or(checkpoint.turn, |turn| turn.max(checkpoint.turn)),
                    );
                }
                ToolLoopCheckpointKind::ToolResult => {
                    if let Some(result) = checkpoint.tool_result {
                        let tool_call_id = result.tool_call_id.clone();
                        if result.read_only {
                            plan.skippable_tool_calls.insert(tool_call_id.clone());
                        }
                        plan.completed_tool_results
                            .entry(tool_call_id)
                            .or_insert(result);
                    }
                }
            }
        }
        plan
    }

    pub fn should_skip_tool_call(&self, tool_call_id: &str) -> bool {
        self.skippable_tool_calls.contains(tool_call_id)
    }

    pub fn completed_tool_result(&self, tool_call_id: &str) -> Option<&str> {
        self.completed_tool_results
            .get(tool_call_id)
            .map(|result| result.result_text.as_str())
    }

    pub fn completed_tool_results(&self) -> &BTreeMap<String, ToolResultCheckpoint> {
        &self.completed_tool_results
    }

    pub fn last_started_turn(&self) -> Option<u32> {
        self.last_started_turn
    }

    pub fn last_completed_model_turn(&self) -> Option<u32> {
        self.last_completed_model_turn
    }
}
