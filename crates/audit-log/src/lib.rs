//! # Audit Log
//!
//! Records all action lifecycle events for compliance, debugging, and accountability.
//!
//! Every action that goes through the capability policy gets an audit record
//! regardless of whether it was allowed, denied, or required approval.

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tokio::fs::OpenOptions;
use tokio::io::AsyncWriteExt;

// ────────────────────────────────────────────────────────────────────────────
// Audit Event
// ────────────────────────────────────────────────────────────────────────────

/// A single audit record for an action lifecycle event.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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

impl AuditEvent {
    /// Compute a deterministic hash of the canonical JSON representation of this event.
    pub fn compute_hash(&self) -> anyhow::Result<String> {
        sha256_hex(serde_json::to_string(self)?.as_bytes())
    }
}

// ────────────────────────────────────────────────────────────────────────────
// Audit Integrity
// ────────────────────────────────────────────────────────────────────────────

pub const CURRENT_AUDIT_INTEGRITY_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditIntegrityEnvelope {
    pub schema_version: u32,
    pub event: AuditEvent,
    pub event_hash: String,
    pub previous_event_hash: Option<String>,
    pub chain_hash: String,
}

impl AuditIntegrityEnvelope {
    pub fn new(event: AuditEvent, previous_event_hash: Option<String>) -> anyhow::Result<Self> {
        let event_hash = event.compute_hash()?;
        let chain_hash = compute_chain_hash(previous_event_hash.as_deref(), &event_hash)?;
        Ok(Self {
            schema_version: CURRENT_AUDIT_INTEGRITY_SCHEMA_VERSION,
            event,
            event_hash,
            previous_event_hash,
            chain_hash,
        })
    }

    pub fn verify(
        &self,
        expected_previous_hash: Option<&str>,
    ) -> anyhow::Result<Vec<AuditIntegrityIssue>> {
        let mut issues = Vec::new();
        let actual_event_hash = self.event.compute_hash()?;
        if self.event_hash != actual_event_hash {
            issues.push(AuditIntegrityIssue::EventHashMismatch {
                audit_id: self.event.audit_id.clone(),
                expected: self.event_hash.clone(),
                actual: actual_event_hash.clone(),
            });
        }

        if self.previous_event_hash.as_deref() != expected_previous_hash {
            issues.push(AuditIntegrityIssue::PreviousHashMismatch {
                audit_id: self.event.audit_id.clone(),
                expected: expected_previous_hash.map(ToString::to_string),
                actual: self.previous_event_hash.clone(),
            });
        }

        let actual_chain_hash =
            compute_chain_hash(self.previous_event_hash.as_deref(), &self.event_hash)?;
        if self.chain_hash != actual_chain_hash {
            issues.push(AuditIntegrityIssue::ChainHashMismatch {
                audit_id: self.event.audit_id.clone(),
                expected: self.chain_hash.clone(),
                actual: actual_chain_hash,
            });
        }

        Ok(issues)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuditSegmentChecksum {
    pub event_count: u64,
    pub checksum: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuditIntegrityReport {
    pub verified: bool,
    pub event_count: u64,
    pub segment_checksum: Option<AuditSegmentChecksum>,
    pub issues: Vec<AuditIntegrityIssue>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AuditIntegrityIssue {
    ParseError {
        line: usize,
        reason: String,
    },
    LegacyEventWithoutIntegrity {
        line: usize,
        audit_id: String,
    },
    EventHashMismatch {
        audit_id: String,
        expected: String,
        actual: String,
    },
    PreviousHashMismatch {
        audit_id: String,
        expected: Option<String>,
        actual: Option<String>,
    },
    ChainHashMismatch {
        audit_id: String,
        expected: String,
        actual: String,
    },
}

impl AuditIntegrityReport {
    pub fn clean(event_count: u64, segment_checksum: Option<AuditSegmentChecksum>) -> Self {
        Self {
            verified: true,
            event_count,
            segment_checksum,
            issues: Vec::new(),
        }
    }

    pub fn with_issues(
        event_count: u64,
        segment_checksum: Option<AuditSegmentChecksum>,
        issues: Vec<AuditIntegrityIssue>,
    ) -> Self {
        Self {
            verified: issues.is_empty(),
            event_count,
            segment_checksum,
            issues,
        }
    }
}

// ────────────────────────────────────────────────────────────────────────────
// Audit Query
// ────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuditQuery {
    pub action_id: Option<String>,
    pub action_kind: Option<String>,
    pub requested_by: Option<String>,
    pub approved_by: Option<String>,
    pub user: Option<String>,
    pub conversation_id: Option<String>,
    pub message_id: Option<String>,
    pub resource: Option<String>,
    pub policy_decision: Option<String>,
    pub result_status: Option<String>,
    pub from: Option<DateTime<Utc>>,
    pub to: Option<DateTime<Utc>>,
    pub pagination: AuditPagination,
}

impl Default for AuditQuery {
    fn default() -> Self {
        Self {
            action_id: None,
            action_kind: None,
            requested_by: None,
            approved_by: None,
            user: None,
            conversation_id: None,
            message_id: None,
            resource: None,
            policy_decision: None,
            result_status: None,
            from: None,
            to: None,
            pagination: AuditPagination::default(),
        }
    }
}

impl AuditQuery {
    pub fn matches(&self, event: &AuditEvent) -> bool {
        option_matches(self.action_id.as_deref(), &event.action_id)
            && option_matches(self.action_kind.as_deref(), &event.action_kind)
            && option_matches(self.requested_by.as_deref(), &event.requested_by)
            && option_matches_optional(self.approved_by.as_deref(), event.approved_by.as_deref())
            && user_matches(self.user.as_deref(), event)
            && option_matches_optional(
                self.conversation_id.as_deref(),
                event.conversation_id.as_deref(),
            )
            && option_matches_optional(self.message_id.as_deref(), event.message_id.as_deref())
            && resource_matches(self.resource.as_deref(), event)
            && option_matches(self.policy_decision.as_deref(), &event.policy_decision)
            && option_matches(self.result_status.as_deref(), &event.result_status)
            && self
                .from
                .map(|from| event.timestamp >= from)
                .unwrap_or(true)
            && self.to.map(|to| event.timestamp <= to).unwrap_or(true)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuditPagination {
    pub offset: usize,
    pub limit: usize,
}

impl Default for AuditPagination {
    fn default() -> Self {
        Self {
            offset: 0,
            limit: 100,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AuditQueryResult {
    pub events: Vec<AuditEvent>,
    pub total_matched: usize,
    pub offset: usize,
    pub limit: usize,
    pub has_more: bool,
}

#[async_trait]
pub trait AuditLogQueryExt: AuditLog {
    async fn query(&self, query: AuditQuery) -> anyhow::Result<AuditQueryResult> {
        let mut events: Vec<_> = self
            .list()
            .await?
            .into_iter()
            .filter(|event| query.matches(event))
            .collect();
        events.sort_by(|left, right| {
            left.timestamp
                .cmp(&right.timestamp)
                .then_with(|| left.audit_id.cmp(&right.audit_id))
        });

        let total_matched = events.len();
        let offset = query.pagination.offset;
        let limit = query.pagination.limit;
        let paged_events = if limit == 0 {
            Vec::new()
        } else {
            events.into_iter().skip(offset).take(limit).collect()
        };
        let has_more = offset.saturating_add(limit) < total_matched;

        Ok(AuditQueryResult {
            events: paged_events,
            total_matched,
            offset,
            limit,
            has_more,
        })
    }
}

impl<T: AuditLog + ?Sized> AuditLogQueryExt for T {}

fn option_matches(expected: Option<&str>, actual: &str) -> bool {
    expected.map(|expected| expected == actual).unwrap_or(true)
}

fn option_matches_optional(expected: Option<&str>, actual: Option<&str>) -> bool {
    expected
        .map(|expected| Some(expected) == actual)
        .unwrap_or(true)
}

fn user_matches(user: Option<&str>, event: &AuditEvent) -> bool {
    user.map(|user| event.requested_by == user || event.approved_by.as_deref() == Some(user))
        .unwrap_or(true)
}

fn resource_matches(resource: Option<&str>, event: &AuditEvent) -> bool {
    resource
        .map(|resource| {
            event.input_summary.contains(resource)
                || event
                    .result_summary
                    .as_deref()
                    .map(|summary| summary.contains(resource))
                    .unwrap_or(false)
        })
        .unwrap_or(true)
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

    pub async fn verify(&self) -> anyhow::Result<AuditIntegrityReport> {
        if !self.file_path.exists() {
            return Ok(AuditIntegrityReport::clean(
                0,
                Some(AuditSegmentChecksum {
                    event_count: 0,
                    checksum: sha256_hex(&[])?,
                }),
            ));
        }

        let content = tokio::fs::read_to_string(&self.file_path).await?;
        let mut issues = Vec::new();
        let mut event_count = 0;
        let mut previous_chain_hash: Option<String> = None;

        for (index, line) in content.lines().enumerate() {
            let line_no = index + 1;
            let line = line.trim();
            if line.is_empty() {
                continue;
            }

            match parse_audit_line(line) {
                Ok(AuditLine::Envelope(envelope)) => {
                    event_count += 1;
                    issues.extend(envelope.verify(previous_chain_hash.as_deref())?);
                    previous_chain_hash = Some(envelope.chain_hash);
                }
                Ok(AuditLine::Legacy(event)) => {
                    event_count += 1;
                    issues.push(AuditIntegrityIssue::LegacyEventWithoutIntegrity {
                        line: line_no,
                        audit_id: event.audit_id,
                    });
                    previous_chain_hash = None;
                }
                Err(reason) => issues.push(AuditIntegrityIssue::ParseError {
                    line: line_no,
                    reason,
                }),
            }
        }

        Ok(AuditIntegrityReport::with_issues(
            event_count,
            Some(self.compute_segment_checksum().await?),
            issues,
        ))
    }

    pub async fn compute_segment_checksum(&self) -> anyhow::Result<AuditSegmentChecksum> {
        if !self.file_path.exists() {
            return Ok(AuditSegmentChecksum {
                event_count: 0,
                checksum: sha256_hex(&[])?,
            });
        }

        let bytes = tokio::fs::read(&self.file_path).await?;
        let content = String::from_utf8_lossy(&bytes);
        let event_count = content
            .lines()
            .filter(|line| !line.trim().is_empty())
            .count() as u64;
        Ok(AuditSegmentChecksum {
            event_count,
            checksum: sha256_hex(&bytes)?,
        })
    }

    async fn previous_chain_hash(&self) -> anyhow::Result<Option<String>> {
        if !self.file_path.exists() {
            return Ok(None);
        }

        let content = tokio::fs::read_to_string(&self.file_path).await?;
        for line in content.lines().rev() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            if let Ok(AuditLine::Envelope(envelope)) = parse_audit_line(line) {
                return Ok(Some(envelope.chain_hash));
            }
            return Ok(None);
        }
        Ok(None)
    }
}

enum AuditLine {
    Envelope(AuditIntegrityEnvelope),
    Legacy(AuditEvent),
}

fn parse_audit_line(line: &str) -> Result<AuditLine, String> {
    let value: serde_json::Value = serde_json::from_str(line).map_err(|err| err.to_string())?;
    if value.get("event").is_some() && value.get("event_hash").is_some() {
        serde_json::from_value(value)
            .map(AuditLine::Envelope)
            .map_err(|err| err.to_string())
    } else {
        serde_json::from_value(value)
            .map(AuditLine::Legacy)
            .map_err(|err| err.to_string())
    }
}

fn compute_chain_hash(
    previous_chain_hash: Option<&str>,
    event_hash: &str,
) -> anyhow::Result<String> {
    let input = format!("{}\n{}", previous_chain_hash.unwrap_or(""), event_hash);
    sha256_hex(input.as_bytes())
}

fn sha256_hex(bytes: &[u8]) -> anyhow::Result<String> {
    let digest = Sha256::digest(bytes);
    Ok(digest.iter().map(|byte| format!("{byte:02x}")).collect())
}

#[async_trait]
impl AuditLog for JsonlAuditSink {
    async fn record(&self, event: AuditEvent) -> anyhow::Result<()> {
        let previous_hash = self.previous_chain_hash().await?;
        let envelope = AuditIntegrityEnvelope::new(event, previous_hash)?;
        let mut json = serde_json::to_string(&envelope)?;
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
            match parse_audit_line(line) {
                Ok(AuditLine::Envelope(envelope)) => events.push(envelope.event),
                Ok(AuditLine::Legacy(event)) => events.push(event),
                Err(reason) => anyhow::bail!(reason),
            }
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

    fn audit_ts() -> DateTime<Utc> {
        "2026-05-26T08:00:00Z".parse().unwrap()
    }

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
            timestamp: audit_ts(),
        }
    }

    fn query_event(
        audit_id: &str,
        action_id: &str,
        requested_by: &str,
        approved_by: Option<&str>,
        timestamp: &str,
    ) -> AuditEvent {
        let mut event = sample_audit_event(action_id);
        event.audit_id = audit_id.to_string();
        event.requested_by = requested_by.to_string();
        event.approved_by = approved_by.map(ToString::to_string);
        event.timestamp = timestamp.parse().unwrap();
        event
    }

    fn query_events() -> Vec<AuditEvent> {
        vec![
            query_event(
                "audit-003",
                "action-003",
                "user-2",
                Some("approver-1"),
                "2026-05-26T08:03:00Z",
            ),
            query_event(
                "audit-001",
                "action-001",
                "user-1",
                None,
                "2026-05-26T08:01:00Z",
            ),
            query_event(
                "audit-002",
                "action-002",
                "user-1",
                Some("approver-2"),
                "2026-05-26T08:02:00Z",
            ),
        ]
    }

    #[tokio::test]
    async fn audit_query_filters_by_action_id() {
        let sink = MemoryAuditSink::new();
        for event in query_events() {
            sink.record(event).await.unwrap();
        }

        let result = sink
            .query(AuditQuery {
                action_id: Some("action-002".to_string()),
                ..AuditQuery::default()
            })
            .await
            .unwrap();

        assert_eq!(result.total_matched, 1);
        assert_eq!(result.events[0].audit_id, "audit-002");
    }

    #[tokio::test]
    async fn audit_query_filters_by_requested_user() {
        let sink = MemoryAuditSink::new();
        for event in query_events() {
            sink.record(event).await.unwrap();
        }

        let result = sink
            .query(AuditQuery {
                requested_by: Some("user-1".to_string()),
                ..AuditQuery::default()
            })
            .await
            .unwrap();

        assert_eq!(result.total_matched, 2);
        assert_eq!(
            result
                .events
                .iter()
                .map(|event| event.audit_id.as_str())
                .collect::<Vec<_>>(),
            vec!["audit-001", "audit-002"]
        );
    }

    #[tokio::test]
    async fn audit_query_filters_by_user_across_requested_and_approved() {
        let sink = MemoryAuditSink::new();
        for event in query_events() {
            sink.record(event).await.unwrap();
        }

        let result = sink
            .query(AuditQuery {
                user: Some("approver-1".to_string()),
                ..AuditQuery::default()
            })
            .await
            .unwrap();

        assert_eq!(result.total_matched, 1);
        assert_eq!(result.events[0].action_id, "action-003");
    }

    #[tokio::test]
    async fn audit_query_filters_by_time_range() {
        let sink = MemoryAuditSink::new();
        for event in query_events() {
            sink.record(event).await.unwrap();
        }

        let result = sink
            .query(AuditQuery {
                from: Some("2026-05-26T08:02:00Z".parse().unwrap()),
                to: Some("2026-05-26T08:03:00Z".parse().unwrap()),
                ..AuditQuery::default()
            })
            .await
            .unwrap();

        assert_eq!(result.total_matched, 2);
        assert_eq!(result.events[0].audit_id, "audit-002");
        assert_eq!(result.events[1].audit_id, "audit-003");
    }

    #[tokio::test]
    async fn audit_query_filters_by_conversation_run() {
        let sink = MemoryAuditSink::new();
        let mut first = query_event(
            "audit-001",
            "action-001",
            "user-1",
            None,
            "2026-05-26T08:01:00Z",
        );
        first.conversation_id = Some("run-1".to_string());
        let mut second = query_event(
            "audit-002",
            "action-002",
            "user-1",
            None,
            "2026-05-26T08:02:00Z",
        );
        second.conversation_id = Some("run-2".to_string());
        sink.record(first).await.unwrap();
        sink.record(second).await.unwrap();

        let result = sink
            .query(AuditQuery {
                conversation_id: Some("run-2".to_string()),
                ..AuditQuery::default()
            })
            .await
            .unwrap();

        assert_eq!(result.total_matched, 1);
        assert_eq!(result.events[0].conversation_id.as_deref(), Some("run-2"));
    }

    #[tokio::test]
    async fn audit_query_filters_by_resource_text() {
        let sink = MemoryAuditSink::new();
        let mut first = query_event(
            "audit-001",
            "action-001",
            "user-1",
            None,
            "2026-05-26T08:01:00Z",
        );
        first.input_summary = "resource: knowledge/kb-main".to_string();
        let mut second = query_event(
            "audit-002",
            "action-002",
            "user-1",
            None,
            "2026-05-26T08:02:00Z",
        );
        second.result_summary = Some("resource: mail/thread-2".to_string());
        sink.record(first).await.unwrap();
        sink.record(second).await.unwrap();

        let result = sink
            .query(AuditQuery {
                resource: Some("mail/thread-2".to_string()),
                ..AuditQuery::default()
            })
            .await
            .unwrap();

        assert_eq!(result.total_matched, 1);
        assert_eq!(result.events[0].audit_id, "audit-002");
    }

    #[tokio::test]
    async fn audit_query_paginates_results_and_reports_has_more() {
        let sink = MemoryAuditSink::new();
        for event in query_events() {
            sink.record(event).await.unwrap();
        }

        let result = sink
            .query(AuditQuery {
                pagination: AuditPagination {
                    offset: 1,
                    limit: 1,
                },
                ..AuditQuery::default()
            })
            .await
            .unwrap();

        assert_eq!(result.total_matched, 3);
        assert_eq!(result.offset, 1);
        assert_eq!(result.limit, 1);
        assert!(result.has_more);
        assert_eq!(result.events[0].audit_id, "audit-002");
    }

    #[tokio::test]
    async fn audit_query_sorts_by_timestamp_then_audit_id() {
        let sink = MemoryAuditSink::new();
        sink.record(query_event(
            "audit-b",
            "action-b",
            "user-1",
            None,
            "2026-05-26T08:01:00Z",
        ))
        .await
        .unwrap();
        sink.record(query_event(
            "audit-a",
            "action-a",
            "user-1",
            None,
            "2026-05-26T08:01:00Z",
        ))
        .await
        .unwrap();

        let result = sink.query(AuditQuery::default()).await.unwrap();

        assert_eq!(
            result
                .events
                .iter()
                .map(|event| event.audit_id.as_str())
                .collect::<Vec<_>>(),
            vec!["audit-a", "audit-b"]
        );
    }

    #[tokio::test]
    async fn jsonl_audit_sink_supports_query_ext() {
        let dir = tempfile::tempdir().unwrap();
        let file_path = dir.path().join("audit.jsonl");
        let sink = JsonlAuditSink::new(&file_path);
        for event in query_events() {
            sink.record(event).await.unwrap();
        }

        let result = sink
            .query(AuditQuery {
                requested_by: Some("user-2".to_string()),
                ..AuditQuery::default()
            })
            .await
            .unwrap();

        assert_eq!(result.total_matched, 1);
        assert_eq!(result.events[0].audit_id, "audit-003");
    }

    #[test]
    fn audit_event_hash_is_deterministic() {
        let event = sample_audit_event("action-001");
        let hash = event.compute_hash().unwrap();

        assert_eq!(hash.len(), 64);
        assert_eq!(hash, event.compute_hash().unwrap());

        let mut changed = event.clone();
        changed.result_status = "failed".to_string();
        assert_ne!(hash, changed.compute_hash().unwrap());
    }

    #[test]
    fn integrity_envelope_chains_events() {
        let first = AuditIntegrityEnvelope::new(sample_audit_event("action-001"), None).unwrap();
        let second = AuditIntegrityEnvelope::new(
            sample_audit_event("action-002"),
            Some(first.chain_hash.clone()),
        )
        .unwrap();

        assert_eq!(first.previous_event_hash, None);
        assert_eq!(second.previous_event_hash, Some(first.chain_hash.clone()));
        assert_ne!(first.chain_hash, second.chain_hash);
        assert!(first.verify(None).unwrap().is_empty());
        assert!(second.verify(Some(&first.chain_hash)).unwrap().is_empty());
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
    async fn jsonl_sink_writes_integrity_envelopes_and_lists_events() {
        let dir = tempfile::tempdir().unwrap();
        let file_path = dir.path().join("audit.jsonl");
        let sink = JsonlAuditSink::new(&file_path);

        sink.record(sample_audit_event("action-001")).await.unwrap();

        let content = tokio::fs::read_to_string(&file_path).await.unwrap();
        let envelope: AuditIntegrityEnvelope = serde_json::from_str(content.trim()).unwrap();
        assert_eq!(
            envelope.schema_version,
            CURRENT_AUDIT_INTEGRITY_SCHEMA_VERSION
        );
        assert_eq!(envelope.event.action_id, "action-001");
        assert_eq!(sink.list().await.unwrap()[0].action_id, "action-001");
    }

    #[tokio::test]
    async fn jsonl_sink_verify_clean_log_succeeds() {
        let dir = tempfile::tempdir().unwrap();
        let file_path = dir.path().join("audit.jsonl");
        let sink = JsonlAuditSink::new(&file_path);

        sink.record(sample_audit_event("action-001")).await.unwrap();
        sink.record(sample_audit_event("action-002")).await.unwrap();

        let report = sink.verify().await.unwrap();
        assert!(report.verified, "issues: {:?}", report.issues);
        assert_eq!(report.event_count, 2);
        assert_eq!(report.segment_checksum.unwrap().event_count, 2);
    }

    #[tokio::test]
    async fn jsonl_sink_verify_detects_event_tampering() {
        let dir = tempfile::tempdir().unwrap();
        let file_path = dir.path().join("audit.jsonl");
        let sink = JsonlAuditSink::new(&file_path);

        sink.record(sample_audit_event("action-001")).await.unwrap();
        let content = tokio::fs::read_to_string(&file_path).await.unwrap();
        let tampered = content.replace("completed", "failed");
        tokio::fs::write(&file_path, tampered).await.unwrap();

        let report = sink.verify().await.unwrap();
        assert!(!report.verified);
        assert!(
            report
                .issues
                .iter()
                .any(|issue| matches!(issue, AuditIntegrityIssue::EventHashMismatch { .. }))
        );
    }

    #[tokio::test]
    async fn jsonl_sink_verify_detects_chain_tampering() {
        let dir = tempfile::tempdir().unwrap();
        let file_path = dir.path().join("audit.jsonl");
        let sink = JsonlAuditSink::new(&file_path);

        sink.record(sample_audit_event("action-001")).await.unwrap();
        sink.record(sample_audit_event("action-002")).await.unwrap();
        let mut lines: Vec<String> = tokio::fs::read_to_string(&file_path)
            .await
            .unwrap()
            .lines()
            .map(ToString::to_string)
            .collect();
        let mut second: AuditIntegrityEnvelope = serde_json::from_str(&lines[1]).unwrap();
        second.previous_event_hash = Some("0".repeat(64));
        lines[1] = serde_json::to_string(&second).unwrap();
        tokio::fs::write(&file_path, format!("{}\n", lines.join("\n")))
            .await
            .unwrap();

        let report = sink.verify().await.unwrap();
        assert!(!report.verified);
        assert!(report.issues.iter().any(|issue| matches!(
            issue,
            AuditIntegrityIssue::PreviousHashMismatch { .. }
                | AuditIntegrityIssue::ChainHashMismatch { .. }
        )));
    }

    #[tokio::test]
    async fn jsonl_sink_segment_checksum_changes_when_file_tampered() {
        let dir = tempfile::tempdir().unwrap();
        let file_path = dir.path().join("audit.jsonl");
        let sink = JsonlAuditSink::new(&file_path);

        sink.record(sample_audit_event("action-001")).await.unwrap();
        let before = sink.compute_segment_checksum().await.unwrap();
        tokio::fs::write(
            &file_path,
            tokio::fs::read_to_string(&file_path)
                .await
                .unwrap()
                .replace("action-001", "action-999"),
        )
        .await
        .unwrap();
        let after = sink.compute_segment_checksum().await.unwrap();

        assert_ne!(before.checksum, after.checksum);
    }

    #[tokio::test]
    async fn jsonl_sink_list_reads_legacy_raw_events() {
        let dir = tempfile::tempdir().unwrap();
        let file_path = dir.path().join("audit.jsonl");
        let event = sample_audit_event("action-legacy");
        tokio::fs::write(
            &file_path,
            format!("{}\n", serde_json::to_string(&event).unwrap()),
        )
        .await
        .unwrap();
        let sink = JsonlAuditSink::new(&file_path);

        let events = sink.list().await.unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].action_id, "action-legacy");
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
