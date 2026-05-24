//! # Audit Log
//!
//! Records all action lifecycle events for compliance, debugging, and accountability.
//!
//! Every action that goes through the capability policy gets an audit record
//! regardless of whether it was allowed, denied, or required approval.

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tokio::fs::OpenOptions;
use tokio::io::AsyncWriteExt;

// ────────────────────────────────────────────────────────────────────────────
// Audit Event
// ────────────────────────────────────────────────────────────────────────────

/// A single audit record for an action lifecycle event.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditEvent {
    /// Unique audit event ID.
    pub audit_id: String,
    /// The action being audited.
    pub action_id: String,
    /// The action kind (e.g., "knowledge.search").
    pub action_kind: String,
    /// Who requested the action.
    pub requested_by: String,
    /// Who approved the action (if approved).
    pub approved_by: Option<String>,
    /// Summary of the action input.
    pub input_summary: String,
    /// Side effect classification.
    pub side_effect: String,
    /// Policy decision: allow / ask / deny.
    pub policy_decision: String,
    /// Final status: completed / failed / denied / cancelled.
    pub result_status: String,
    /// Summary of the result.
    pub result_summary: Option<String>,
    /// Related conversation ID.
    pub conversation_id: Option<String>,
    /// Related message ID.
    pub message_id: Option<String>,
    /// When the audit event was recorded.
    pub timestamp: DateTime<Utc>,
}

// ────────────────────────────────────────────────────────────────────────────
// Audit Log trait
// ────────────────────────────────────────────────────────────────────────────

/// Trait for audit log sinks.
#[async_trait]
pub trait AuditLog: Send + Sync {
    /// Record an audit event.
    async fn record(&self, event: AuditEvent) -> anyhow::Result<()>;

    /// Retrieve all audit events (for querying/testing).
    async fn list(&self) -> anyhow::Result<Vec<AuditEvent>>;
}

// ────────────────────────────────────────────────────────────────────────────
// Memory Audit Sink
// ────────────────────────────────────────────────────────────────────────────

/// In-memory audit sink for testing.
#[derive(Debug, Clone, Default)]
pub struct MemoryAuditSink {
    events: Arc<Mutex<Vec<AuditEvent>>>,
}

impl MemoryAuditSink {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn count(&self) -> usize {
        self.events.lock().unwrap().len()
    }
}

#[async_trait]
impl AuditLog for MemoryAuditSink {
    async fn record(&self, event: AuditEvent) -> anyhow::Result<()> {
        self.events.lock().unwrap().push(event);
        Ok(())
    }

    async fn list(&self) -> anyhow::Result<Vec<AuditEvent>> {
        Ok(self.events.lock().unwrap().clone())
    }
}

// ────────────────────────────────────────────────────────────────────────────
// JSONL Audit Sink
// ────────────────────────────────────────────────────────────────────────────

/// Persistent JSONL audit sink.
pub struct JsonlAuditSink {
    file_path: PathBuf,
}

impl JsonlAuditSink {
    pub fn new(file_path: impl Into<PathBuf>) -> Self {
        Self {
            file_path: file_path.into(),
        }
    }
}

#[async_trait]
impl AuditLog for JsonlAuditSink {
    async fn record(&self, event: AuditEvent) -> anyhow::Result<()> {
        let mut json = serde_json::to_string(&event)?;
        json.push('\n');

        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.file_path)
            .await?;

        file.write_all(json.as_bytes()).await?;
        file.flush().await?;
        Ok(())
    }

    async fn list(&self) -> anyhow::Result<Vec<AuditEvent>> {
        if !self.file_path.exists() {
            return Ok(Vec::new());
        }

        let content = tokio::fs::read_to_string(&self.file_path).await?;
        let mut events = Vec::new();
        for line in content.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            let event: AuditEvent = serde_json::from_str(line)?;
            events.push(event);
        }
        Ok(events)
    }
}

// ────────────────────────────────────────────────────────────────────────────
// Tests
// ────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_audit_event(action_id: &str) -> AuditEvent {
        AuditEvent {
            audit_id: format!("audit-{action_id}"),
            action_id: action_id.to_string(),
            action_kind: "knowledge.search".to_string(),
            requested_by: "u1".to_string(),
            approved_by: None,
            input_summary: "query: test".to_string(),
            side_effect: "read_only".to_string(),
            policy_decision: "allow".to_string(),
            result_status: "completed".to_string(),
            result_summary: Some("found 5 results".to_string()),
            conversation_id: Some("conv-1".to_string()),
            message_id: Some("msg-1".to_string()),
            timestamp: Utc::now(),
        }
    }

    #[test]
    fn audit_event_serde_roundtrip() {
        let event = sample_audit_event("action-001");
        let json = serde_json::to_string_pretty(&event).unwrap();
        let decoded: AuditEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded.audit_id, event.audit_id);
        assert_eq!(decoded.action_id, event.action_id);
        assert_eq!(decoded.policy_decision, "allow");
    }

    #[tokio::test]
    async fn memory_sink_records_events() {
        let sink = MemoryAuditSink::new();
        sink.record(sample_audit_event("action-001")).await.unwrap();
        sink.record(sample_audit_event("action-002")).await.unwrap();

        assert_eq!(sink.count(), 2);
        let events = sink.list().await.unwrap();
        assert_eq!(events[0].action_id, "action-001");
        assert_eq!(events[1].action_id, "action-002");
    }

    #[tokio::test]
    async fn memory_sink_default_is_empty() {
        let sink = MemoryAuditSink::new();
        assert_eq!(sink.count(), 0);
        assert!(sink.list().await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn jsonl_sink_writes_and_reads_back() {
        let dir = tempfile::tempdir().unwrap();
        let file_path = dir.path().join("audit.jsonl");
        let sink = JsonlAuditSink::new(&file_path);

        sink.record(sample_audit_event("action-001")).await.unwrap();
        sink.record(sample_audit_event("action-002")).await.unwrap();

        let events = sink.list().await.unwrap();
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].action_id, "action-001");
        assert_eq!(events[1].action_id, "action-002");
    }

    #[tokio::test]
    async fn jsonl_sink_returns_empty_for_nonexistent_file() {
        let dir = tempfile::tempdir().unwrap();
        let file_path = dir.path().join("nonexistent.jsonl");
        let sink = JsonlAuditSink::new(&file_path);

        let events = sink.list().await.unwrap();
        assert!(events.is_empty());
    }

    #[tokio::test]
    async fn jsonl_sink_appends_across_sessions() {
        let dir = tempfile::tempdir().unwrap();
        let file_path = dir.path().join("audit.jsonl");

        // Session 1: write events.
        {
            let sink = JsonlAuditSink::new(&file_path);
            sink.record(sample_audit_event("action-001")).await.unwrap();
        }

        // Session 2: append more events.
        {
            let sink = JsonlAuditSink::new(&file_path);
            sink.record(sample_audit_event("action-002")).await.unwrap();
        }

        // Session 3: read all.
        {
            let sink = JsonlAuditSink::new(&file_path);
            let events = sink.list().await.unwrap();
            assert_eq!(events.len(), 2);
        }
    }
}
