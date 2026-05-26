//! Segmented JSONL-based journal implementation.
//!
//! Events are stored as one JSON object per line under:
//!
//! ```text
//! {root_dir}/{conversation_id}/
//! ├── manifest.json
//! └── segments/
//!     ├── 00000000000000000000.jsonl
//!     ├── 00000000000000000001.jsonl
//!     └── ...
//! ```
//!
//! This keeps append-only JSONL debugging ergonomics while avoiding a single
//! ever-growing journal file for long conversations.

use async_trait::async_trait;
use conversation_core::{ConversationEventEnvelope, ConversationId};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use tokio::fs::{self, OpenOptions};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

use crate::ConversationJournal;

const DEFAULT_MAX_SEGMENT_BYTES: u64 = 64 * 1024 * 1024;
const MANIFEST_FILE: &str = "manifest.json";
const SEGMENTS_DIR: &str = "segments";

/// A persistent journal that stores events in segmented JSONL files.
pub struct JsonlConversationJournal {
    root_dir: PathBuf,
    max_segment_bytes: u64,
}

/// Integrity verification report for a single conversation journal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JournalIntegrityReport {
    pub conversation_id: ConversationId,
    pub verified_segments: usize,
    pub verified_events: u64,
    pub issues: Vec<JournalIntegrityIssue>,
}

impl JournalIntegrityReport {
    pub fn is_clean(&self) -> bool {
        self.issues.is_empty()
    }
}

/// Integrity issue detected while verifying a segmented conversation journal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JournalIntegrityIssue {
    MissingSegment {
        file_name: String,
    },
    SegmentByteMismatch {
        file_name: String,
        expected: u64,
        actual: u64,
    },
    SegmentChecksumMismatch {
        file_name: String,
        expected: String,
        actual: String,
    },
    SegmentEventCountMismatch {
        file_name: String,
        expected: u64,
        actual: u64,
    },
    TotalEventCountMismatch {
        expected: u64,
        actual: u64,
    },
    CorruptedEventLine {
        file_name: String,
        line_number: u64,
        message: String,
    },
    HashChainMismatch {
        file_name: String,
        expected_previous: String,
        actual_previous: Option<String>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct JournalManifest {
    version: u32,
    max_segment_bytes: u64,
    active_segment_index: u64,
    total_events: u64,
    segments: Vec<SegmentMetadata>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct SegmentMetadata {
    index: u64,
    file_name: String,
    event_count: u64,
    bytes: u64,
    #[serde(default)]
    checksum_sha256: Option<String>,
    #[serde(default)]
    previous_segment_checksum_sha256: Option<String>,
}

impl JournalManifest {
    fn new(max_segment_bytes: u64) -> Self {
        let first_segment = SegmentMetadata::new(0);
        Self {
            version: 1,
            max_segment_bytes,
            active_segment_index: 0,
            total_events: 0,
            segments: vec![first_segment],
        }
    }

    fn active_segment_mut(&mut self) -> &mut SegmentMetadata {
        self.segments
            .iter_mut()
            .find(|segment| segment.index == self.active_segment_index)
            .expect("manifest must contain active segment")
    }

    fn active_segment(&self) -> &SegmentMetadata {
        self.segments
            .iter()
            .find(|segment| segment.index == self.active_segment_index)
            .expect("manifest must contain active segment")
    }

    fn roll_segment(&mut self) {
        let next_index = self.active_segment_index + 1;
        self.active_segment_index = next_index;
        self.segments.push(SegmentMetadata::new(next_index));
    }

    fn has_integrity_metadata(&self) -> bool {
        self.segments
            .iter()
            .any(|segment| segment.checksum_sha256.is_some())
    }
}

impl SegmentMetadata {
    fn new(index: u64) -> Self {
        Self {
            index,
            file_name: segment_file_name(index),
            event_count: 0,
            bytes: 0,
            checksum_sha256: None,
            previous_segment_checksum_sha256: None,
        }
    }
}

impl JsonlConversationJournal {
    /// Create a new segmented JSONL journal rooted at the given directory.
    ///
    /// Events are stored under `{root_dir}/{conversation_id}/segments/*.jsonl`,
    /// with `{root_dir}/{conversation_id}/manifest.json` tracking the active
    /// segment and segment metadata.
    pub fn new(root_dir: impl Into<PathBuf>) -> Self {
        Self {
            root_dir: root_dir.into(),
            max_segment_bytes: DEFAULT_MAX_SEGMENT_BYTES,
        }
    }

    /// Create a journal with a custom maximum segment size.
    ///
    /// This is primarily useful for tests and small embedded deployments.
    pub fn with_max_segment_bytes(root_dir: impl Into<PathBuf>, max_segment_bytes: u64) -> Self {
        Self {
            root_dir: root_dir.into(),
            max_segment_bytes: max_segment_bytes.max(1),
        }
    }

    /// Verify the integrity of a conversation journal without mutating it.
    pub async fn verify(
        &self,
        conversation_id: &ConversationId,
    ) -> anyhow::Result<JournalIntegrityReport> {
        let Some(mut manifest) = self.load_manifest(conversation_id).await? else {
            return Ok(JournalIntegrityReport {
                conversation_id: conversation_id.clone(),
                verified_segments: 0,
                verified_events: 0,
                issues: Vec::new(),
            });
        };

        manifest.segments.sort_by_key(|segment| segment.index);

        let mut report = JournalIntegrityReport {
            conversation_id: conversation_id.clone(),
            verified_segments: 0,
            verified_events: 0,
            issues: Vec::new(),
        };
        let mut previous_actual_checksum: Option<String> = None;

        for segment in &manifest.segments {
            let path = self.segment_path(conversation_id, &segment.file_name);
            if !path.exists() {
                report.issues.push(JournalIntegrityIssue::MissingSegment {
                    file_name: segment.file_name.clone(),
                });
                previous_actual_checksum = None;
                continue;
            }

            let bytes = fs::read(&path).await?;
            report.verified_segments += 1;

            let actual_bytes = bytes.len() as u64;
            if actual_bytes != segment.bytes {
                report
                    .issues
                    .push(JournalIntegrityIssue::SegmentByteMismatch {
                        file_name: segment.file_name.clone(),
                        expected: segment.bytes,
                        actual: actual_bytes,
                    });
            }

            let actual_checksum = sha256_hex(&bytes);
            if let Some(expected_checksum) = &segment.checksum_sha256
                && expected_checksum != &actual_checksum
            {
                report
                    .issues
                    .push(JournalIntegrityIssue::SegmentChecksumMismatch {
                        file_name: segment.file_name.clone(),
                        expected: expected_checksum.clone(),
                        actual: actual_checksum.clone(),
                    });
            }

            if let Some(expected_previous) = &segment.previous_segment_checksum_sha256
                && Some(expected_previous) != previous_actual_checksum.as_ref()
            {
                report
                    .issues
                    .push(JournalIntegrityIssue::HashChainMismatch {
                        file_name: segment.file_name.clone(),
                        expected_previous: expected_previous.clone(),
                        actual_previous: previous_actual_checksum.clone(),
                    });
            }

            let (event_count, parse_issues) = verify_jsonl_event_lines(&segment.file_name, &bytes);
            report.verified_events += event_count;
            report.issues.extend(parse_issues);

            if event_count != segment.event_count {
                report
                    .issues
                    .push(JournalIntegrityIssue::SegmentEventCountMismatch {
                        file_name: segment.file_name.clone(),
                        expected: segment.event_count,
                        actual: event_count,
                    });
            }

            previous_actual_checksum = Some(actual_checksum);
        }

        if report.verified_events != manifest.total_events {
            report
                .issues
                .push(JournalIntegrityIssue::TotalEventCountMismatch {
                    expected: manifest.total_events,
                    actual: report.verified_events,
                });
        }

        Ok(report)
    }

    fn conversation_dir(&self, conversation_id: &ConversationId) -> PathBuf {
        self.root_dir.join(conversation_id.0.as_str())
    }

    fn manifest_path(&self, conversation_id: &ConversationId) -> PathBuf {
        self.conversation_dir(conversation_id).join(MANIFEST_FILE)
    }

    fn segments_dir(&self, conversation_id: &ConversationId) -> PathBuf {
        self.conversation_dir(conversation_id).join(SEGMENTS_DIR)
    }

    fn segment_path(&self, conversation_id: &ConversationId, file_name: &str) -> PathBuf {
        self.segments_dir(conversation_id).join(file_name)
    }

    async fn ensure_conversation_dirs(
        &self,
        conversation_id: &ConversationId,
    ) -> anyhow::Result<()> {
        fs::create_dir_all(self.segments_dir(conversation_id)).await?;
        Ok(())
    }

    async fn load_manifest(
        &self,
        conversation_id: &ConversationId,
    ) -> anyhow::Result<Option<JournalManifest>> {
        let path = self.manifest_path(conversation_id);
        if !path.exists() {
            return Ok(None);
        }

        let bytes = fs::read(path).await?;
        let manifest = serde_json::from_slice(&bytes)?;
        Ok(Some(manifest))
    }

    async fn save_manifest(
        &self,
        conversation_id: &ConversationId,
        manifest: &JournalManifest,
    ) -> anyhow::Result<()> {
        let path = self.manifest_path(conversation_id);
        let temp_path = path.with_extension("json.tmp");
        let bytes = serde_json::to_vec_pretty(manifest)?;

        fs::write(&temp_path, bytes).await?;
        fs::rename(&temp_path, &path).await?;
        Ok(())
    }

    async fn load_or_create_manifest(
        &self,
        conversation_id: &ConversationId,
    ) -> anyhow::Result<JournalManifest> {
        if let Some(manifest) = self.load_manifest(conversation_id).await? {
            return Ok(manifest);
        }

        Ok(JournalManifest::new(self.max_segment_bytes))
    }

    async fn load_events_from_file(path: &Path) -> anyhow::Result<Vec<ConversationEventEnvelope>> {
        if !path.exists() {
            return Ok(Vec::new());
        }

        let file = fs::File::open(path).await?;
        let reader = BufReader::new(file);
        let mut lines = reader.lines();
        let mut events = Vec::new();
        let mut line_number: u64 = 0;

        while let Some(line) = lines.next_line().await? {
            line_number += 1;
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            let event: ConversationEventEnvelope = serde_json::from_str(line).map_err(|e| {
                anyhow::anyhow!(
                    "journal integrity: corrupted event at line {} in {}: {}",
                    line_number,
                    path.display(),
                    e
                )
            })?;
            events.push(event);
        }

        Ok(events)
    }
}

#[async_trait]
impl ConversationJournal for JsonlConversationJournal {
    async fn append(&self, event: ConversationEventEnvelope) -> anyhow::Result<()> {
        self.ensure_conversation_dirs(&event.conversation_id)
            .await?;

        let mut json = serde_json::to_string(&event)?;
        json.push('\n');
        let line_bytes = json.len() as u64;

        let mut manifest = self.load_or_create_manifest(&event.conversation_id).await?;

        if manifest.active_segment().bytes > 0
            && manifest.active_segment().bytes + line_bytes > manifest.max_segment_bytes
        {
            manifest.roll_segment();
        }

        let active_file_name = manifest.active_segment().file_name.clone();
        let path = self.segment_path(&event.conversation_id, &active_file_name);

        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .await?;

        file.write_all(json.as_bytes()).await?;
        file.flush().await?;
        drop(file);

        let active_index = manifest.active_segment_index;
        let active_bytes = fs::read(&path).await?;
        let active_checksum = sha256_hex(&active_bytes);
        let previous_checksum = if active_index > 0 {
            manifest
                .segments
                .iter()
                .find(|segment| segment.index + 1 == active_index)
                .and_then(|segment| segment.checksum_sha256.clone())
        } else {
            None
        };

        let active_segment = manifest.active_segment_mut();
        active_segment.event_count += 1;
        active_segment.bytes += line_bytes;
        active_segment.checksum_sha256 = Some(active_checksum);
        active_segment.previous_segment_checksum_sha256 = previous_checksum;
        manifest.total_events += 1;

        self.save_manifest(&event.conversation_id, &manifest)
            .await?;

        Ok(())
    }

    async fn load(
        &self,
        conversation_id: &ConversationId,
    ) -> anyhow::Result<Vec<ConversationEventEnvelope>> {
        let Some(mut manifest) = self.load_manifest(conversation_id).await? else {
            return Ok(Vec::new());
        };

        if manifest.has_integrity_metadata() {
            let report = self.verify(conversation_id).await?;
            if !report.is_clean() {
                anyhow::bail!(
                    "journal integrity: verification failed for {} with {} issue(s): {:?}",
                    conversation_id.0,
                    report.issues.len(),
                    report.issues
                );
            }
        }

        manifest.segments.sort_by_key(|segment| segment.index);

        let mut events = Vec::new();
        for segment in &manifest.segments {
            let path = self.segment_path(conversation_id, &segment.file_name);

            // Verify segment file integrity if it exists.
            if path.exists() {
                let file_bytes = fs::read(&path).await?;
                let actual_bytes = file_bytes.len() as u64;
                if actual_bytes != segment.bytes {
                    anyhow::bail!(
                        "journal integrity: segment {} byte count mismatch: manifest={}, actual={}",
                        segment.file_name,
                        segment.bytes,
                        actual_bytes
                    );
                }
            }

            events.extend(Self::load_events_from_file(&path).await?);
        }

        // Verify manifest total_events matches actual loaded count.
        if events.len() as u64 != manifest.total_events {
            anyhow::bail!(
                "journal integrity: total_events mismatch: manifest={}, actual={}",
                manifest.total_events,
                events.len()
            );
        }

        Ok(events)
    }
}

fn segment_file_name(index: u64) -> String {
    format!("{index:020}.jsonl")
}

fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn verify_jsonl_event_lines(file_name: &str, bytes: &[u8]) -> (u64, Vec<JournalIntegrityIssue>) {
    let mut count = 0;
    let mut issues = Vec::new();

    for (index, raw_line) in bytes.split(|byte| *byte == b'\n').enumerate() {
        let line_number = index as u64 + 1;
        let line = match std::str::from_utf8(raw_line) {
            Ok(line) => line.trim(),
            Err(error) => {
                issues.push(JournalIntegrityIssue::CorruptedEventLine {
                    file_name: file_name.to_string(),
                    line_number,
                    message: error.to_string(),
                });
                continue;
            }
        };

        if line.is_empty() {
            continue;
        }

        match serde_json::from_str::<ConversationEventEnvelope>(line) {
            Ok(_) => count += 1,
            Err(error) => issues.push(JournalIntegrityIssue::CorruptedEventLine {
                file_name: file_name.to_string(),
                line_number,
                message: error.to_string(),
            }),
        }
    }

    (count, issues)
}

#[cfg(test)]
mod tests {
    use super::*;
    use conversation_core::*;

    fn make_event(conversation_id: &str, event_id: &str) -> ConversationEventEnvelope {
        ConversationEventEnvelope {
            schema_version: CURRENT_SCHEMA_VERSION,
            event_id: EventId::from(event_id),
            conversation_id: ConversationId::from(conversation_id),
            occurred_at: chrono::Utc::now(),
            actor_id: None,
            event: ConversationEvent::ConversationCreated {
                session: ConversationSession {
                    id: ConversationId::from(conversation_id),
                    kind: ConversationKind::Direct,
                    title: None,
                    participants: vec![],
                    created_at: chrono::Utc::now(),
                    updated_at: chrono::Utc::now(),
                    status: ConversationStatus::Active,
                },
            },
        }
    }

    async fn load_manifest_from_dir(dir: &Path, conversation_id: &str) -> JournalManifest {
        let manifest_path = dir.join(conversation_id).join(MANIFEST_FILE);
        let manifest_bytes = fs::read(&manifest_path).await.unwrap();
        serde_json::from_slice(&manifest_bytes).unwrap()
    }

    async fn save_manifest_to_dir(dir: &Path, conversation_id: &str, manifest: &JournalManifest) {
        let manifest_path = dir.join(conversation_id).join(MANIFEST_FILE);
        let bytes = serde_json::to_vec_pretty(manifest).unwrap();
        fs::write(&manifest_path, bytes).await.unwrap();
    }

    fn has_issue<F>(report: &JournalIntegrityReport, predicate: F) -> bool
    where
        F: Fn(&JournalIntegrityIssue) -> bool,
    {
        report.issues.iter().any(predicate)
    }

    #[tokio::test]
    async fn append_and_load_single_event() {
        let dir = tempfile::tempdir().unwrap();
        let journal = JsonlConversationJournal::new(dir.path());

        let event = make_event("conv-1", "evt-1");
        journal.append(event).await.unwrap();

        let loaded = journal.load(&ConversationId::from("conv-1")).await.unwrap();
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].event_id, EventId::from("evt-1"));
    }

    #[tokio::test]
    async fn append_multiple_events() {
        let dir = tempfile::tempdir().unwrap();
        let journal = JsonlConversationJournal::new(dir.path());

        journal.append(make_event("conv-1", "evt-1")).await.unwrap();
        journal.append(make_event("conv-1", "evt-2")).await.unwrap();
        journal.append(make_event("conv-1", "evt-3")).await.unwrap();

        let loaded = journal.load(&ConversationId::from("conv-1")).await.unwrap();
        assert_eq!(loaded.len(), 3);
        assert_eq!(loaded[0].event_id, EventId::from("evt-1"));
        assert_eq!(loaded[1].event_id, EventId::from("evt-2"));
        assert_eq!(loaded[2].event_id, EventId::from("evt-3"));
    }

    #[tokio::test]
    async fn reopen_preserves_events() {
        let dir = tempfile::tempdir().unwrap();
        let dir_path = dir.path().to_path_buf();

        // First session: write events
        {
            let journal = JsonlConversationJournal::new(&dir_path);
            journal.append(make_event("conv-1", "evt-1")).await.unwrap();
            journal.append(make_event("conv-1", "evt-2")).await.unwrap();
        }

        // Second session: reopen and verify
        {
            let journal = JsonlConversationJournal::new(&dir_path);
            let loaded = journal.load(&ConversationId::from("conv-1")).await.unwrap();
            assert_eq!(loaded.len(), 2);
            assert_eq!(loaded[0].event_id, EventId::from("evt-1"));
            assert_eq!(loaded[1].event_id, EventId::from("evt-2"));
        }
    }

    #[tokio::test]
    async fn load_nonexistent_returns_empty() {
        let dir = tempfile::tempdir().unwrap();
        let journal = JsonlConversationJournal::new(dir.path());

        let loaded = journal
            .load(&ConversationId::from("nonexistent"))
            .await
            .unwrap();
        assert!(loaded.is_empty());
    }

    #[tokio::test]
    async fn different_conversations_independent() {
        let dir = tempfile::tempdir().unwrap();
        let journal = JsonlConversationJournal::new(dir.path());

        journal
            .append(make_event("conv-a", "evt-a1"))
            .await
            .unwrap();
        journal
            .append(make_event("conv-b", "evt-b1"))
            .await
            .unwrap();
        journal
            .append(make_event("conv-a", "evt-a2"))
            .await
            .unwrap();

        let a = journal.load(&ConversationId::from("conv-a")).await.unwrap();
        let b = journal.load(&ConversationId::from("conv-b")).await.unwrap();

        assert_eq!(a.len(), 2);
        assert_eq!(b.len(), 1);
        assert_eq!(b[0].event_id, EventId::from("evt-b1"));
    }

    #[tokio::test]
    async fn rolls_to_multiple_segment_files_when_segment_is_full() {
        let dir = tempfile::tempdir().unwrap();
        let journal = JsonlConversationJournal::with_max_segment_bytes(dir.path(), 1);
        let conversation_id = ConversationId::from("conv-1");

        journal.append(make_event("conv-1", "evt-1")).await.unwrap();
        journal.append(make_event("conv-1", "evt-2")).await.unwrap();
        journal.append(make_event("conv-1", "evt-3")).await.unwrap();

        let loaded = journal.load(&conversation_id).await.unwrap();
        assert_eq!(loaded.len(), 3);
        assert_eq!(loaded[0].event_id, EventId::from("evt-1"));
        assert_eq!(loaded[1].event_id, EventId::from("evt-2"));
        assert_eq!(loaded[2].event_id, EventId::from("evt-3"));

        let segments_dir = dir.path().join("conv-1").join(SEGMENTS_DIR);
        assert!(segments_dir.join("00000000000000000000.jsonl").exists());
        assert!(segments_dir.join("00000000000000000001.jsonl").exists());
        assert!(segments_dir.join("00000000000000000002.jsonl").exists());
    }

    #[tokio::test]
    async fn manifest_total_events_matches_actual_count() {
        let dir = tempfile::tempdir().unwrap();
        let journal = JsonlConversationJournal::new(dir.path());

        journal.append(make_event("conv-1", "evt-1")).await.unwrap();
        journal.append(make_event("conv-1", "evt-2")).await.unwrap();
        journal.append(make_event("conv-1", "evt-3")).await.unwrap();

        // Verify manifest total matches actual loaded events.
        let manifest = load_manifest_from_dir(dir.path(), "conv-1").await;
        assert_eq!(manifest.total_events, 3);

        // Verify each segment metadata matches actual file.
        for segment in &manifest.segments {
            let seg_path = dir
                .path()
                .join("conv-1")
                .join(SEGMENTS_DIR)
                .join(&segment.file_name);
            if seg_path.exists() {
                let content = fs::read(&seg_path).await.unwrap();
                let line_count = content
                    .split(|b| *b == b'\n')
                    .filter(|line| !line.is_empty())
                    .count() as u64;
                assert_eq!(segment.event_count, line_count);
                assert_eq!(segment.bytes, content.len() as u64);
            }
        }
    }

    #[tokio::test]
    async fn corrupted_segment_detected_on_load() {
        let dir = tempfile::tempdir().unwrap();
        let journal = JsonlConversationJournal::new(dir.path());

        journal.append(make_event("conv-1", "evt-1")).await.unwrap();
        journal.append(make_event("conv-1", "evt-2")).await.unwrap();

        // Corrupt the segment file by appending garbage.
        let seg_path = dir
            .path()
            .join("conv-1")
            .join(SEGMENTS_DIR)
            .join("00000000000000000000.jsonl");
        let mut content = fs::read(&seg_path).await.unwrap();
        content.extend_from_slice(b"{corrupted garbage}\n");
        fs::write(&seg_path, &content).await.unwrap();

        // Load should detect corruption (byte count mismatch).
        let result = journal.load(&ConversationId::from("conv-1")).await;
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(
            err_msg.contains("integrity")
                || err_msg.contains("corrupt")
                || err_msg.contains("mismatch")
        );
    }

    #[tokio::test]
    async fn manifest_count_mismatch_detected() {
        let dir = tempfile::tempdir().unwrap();
        let journal = JsonlConversationJournal::new(dir.path());

        journal.append(make_event("conv-1", "evt-1")).await.unwrap();
        journal.append(make_event("conv-1", "evt-2")).await.unwrap();

        // Tamper with manifest to have wrong total_events.
        let mut manifest = load_manifest_from_dir(dir.path(), "conv-1").await;
        manifest.total_events = 999; // Wrong!
        save_manifest_to_dir(dir.path(), "conv-1", &manifest).await;

        // Load should detect the mismatch.
        let result = journal.load(&ConversationId::from("conv-1")).await;
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(
            err_msg.contains("integrity")
                || err_msg.contains("mismatch")
                || err_msg.contains("total_events")
        );
    }

    #[tokio::test]
    async fn writes_manifest_for_segmented_journal() {
        let dir = tempfile::tempdir().unwrap();
        let journal = JsonlConversationJournal::with_max_segment_bytes(dir.path(), 1);

        journal.append(make_event("conv-1", "evt-1")).await.unwrap();
        journal.append(make_event("conv-1", "evt-2")).await.unwrap();

        let manifest = load_manifest_from_dir(dir.path(), "conv-1").await;

        assert_eq!(manifest.version, 1);
        assert_eq!(manifest.total_events, 2);
        assert_eq!(manifest.segments.len(), 2);
        assert_eq!(manifest.active_segment_index, 1);
    }

    #[tokio::test]
    async fn verify_clean_journal_returns_no_issues() {
        let dir = tempfile::tempdir().unwrap();
        let journal = JsonlConversationJournal::new(dir.path());
        let conversation_id = ConversationId::from("conv-1");

        journal.append(make_event("conv-1", "evt-1")).await.unwrap();
        journal.append(make_event("conv-1", "evt-2")).await.unwrap();
        journal.append(make_event("conv-1", "evt-3")).await.unwrap();

        let report = journal.verify(&conversation_id).await.unwrap();

        assert!(report.issues.is_empty(), "issues: {:?}", report.issues);
        assert_eq!(report.verified_events, 3);
        assert!(report.verified_segments >= 1);
    }

    #[tokio::test]
    async fn manifest_records_segment_checksum_after_append() {
        let dir = tempfile::tempdir().unwrap();
        let journal = JsonlConversationJournal::with_max_segment_bytes(dir.path(), 1);

        journal.append(make_event("conv-1", "evt-1")).await.unwrap();
        journal.append(make_event("conv-1", "evt-2")).await.unwrap();

        let manifest = load_manifest_from_dir(dir.path(), "conv-1").await;
        assert!(manifest.segments.len() >= 2);
        for segment in &manifest.segments {
            let checksum = segment
                .checksum_sha256
                .as_ref()
                .expect("segment checksum should be recorded");
            assert_eq!(checksum.len(), 64);
        }
        assert!(
            manifest.segments[1]
                .previous_segment_checksum_sha256
                .is_some()
        );
    }

    #[tokio::test]
    async fn verify_detects_segment_checksum_mismatch() {
        let dir = tempfile::tempdir().unwrap();
        let journal = JsonlConversationJournal::new(dir.path());
        let conversation_id = ConversationId::from("conv-1");

        journal.append(make_event("conv-1", "evt-1")).await.unwrap();

        let seg_path = dir
            .path()
            .join("conv-1")
            .join(SEGMENTS_DIR)
            .join("00000000000000000000.jsonl");
        let mut content = fs::read(&seg_path).await.unwrap();
        content[0] = if content[0] == b'{' { b' ' } else { b'{' };
        fs::write(&seg_path, &content).await.unwrap();

        let mut manifest = load_manifest_from_dir(dir.path(), "conv-1").await;
        manifest.segments[0].bytes = content.len() as u64;
        save_manifest_to_dir(dir.path(), "conv-1", &manifest).await;

        let report = journal.verify(&conversation_id).await.unwrap();
        assert!(has_issue(&report, |issue| matches!(
            issue,
            JournalIntegrityIssue::SegmentChecksumMismatch { .. }
        )));
    }

    #[tokio::test]
    async fn verify_detects_missing_segment() {
        let dir = tempfile::tempdir().unwrap();
        let journal = JsonlConversationJournal::with_max_segment_bytes(dir.path(), 1);
        let conversation_id = ConversationId::from("conv-1");

        journal.append(make_event("conv-1", "evt-1")).await.unwrap();
        journal.append(make_event("conv-1", "evt-2")).await.unwrap();

        let seg_path = dir
            .path()
            .join("conv-1")
            .join(SEGMENTS_DIR)
            .join("00000000000000000001.jsonl");
        fs::remove_file(seg_path).await.unwrap();

        let report = journal.verify(&conversation_id).await.unwrap();
        assert!(has_issue(&report, |issue| matches!(
            issue,
            JournalIntegrityIssue::MissingSegment { file_name } if file_name == "00000000000000000001.jsonl"
        )));
    }

    #[tokio::test]
    async fn verify_detects_segment_event_count_mismatch() {
        let dir = tempfile::tempdir().unwrap();
        let journal = JsonlConversationJournal::new(dir.path());
        let conversation_id = ConversationId::from("conv-1");

        journal.append(make_event("conv-1", "evt-1")).await.unwrap();
        journal.append(make_event("conv-1", "evt-2")).await.unwrap();

        let mut manifest = load_manifest_from_dir(dir.path(), "conv-1").await;
        manifest.segments[0].event_count = 999;
        save_manifest_to_dir(dir.path(), "conv-1", &manifest).await;

        let report = journal.verify(&conversation_id).await.unwrap();
        assert!(has_issue(&report, |issue| matches!(
            issue,
            JournalIntegrityIssue::SegmentEventCountMismatch {
                expected: 999,
                actual: 2,
                ..
            }
        )));
    }

    #[tokio::test]
    async fn verify_detects_hash_chain_mismatch() {
        let dir = tempfile::tempdir().unwrap();
        let journal = JsonlConversationJournal::with_max_segment_bytes(dir.path(), 1);
        let conversation_id = ConversationId::from("conv-1");

        journal.append(make_event("conv-1", "evt-1")).await.unwrap();
        journal.append(make_event("conv-1", "evt-2")).await.unwrap();

        let mut manifest = load_manifest_from_dir(dir.path(), "conv-1").await;
        manifest.segments[1].previous_segment_checksum_sha256 = Some("0".repeat(64));
        save_manifest_to_dir(dir.path(), "conv-1", &manifest).await;

        let report = journal.verify(&conversation_id).await.unwrap();
        assert!(has_issue(&report, |issue| matches!(
            issue,
            JournalIntegrityIssue::HashChainMismatch { .. }
        )));
    }

    #[tokio::test]
    async fn load_rejects_checksum_mismatch_for_integrity_manifest() {
        let dir = tempfile::tempdir().unwrap();
        let journal = JsonlConversationJournal::new(dir.path());

        journal.append(make_event("conv-1", "evt-1")).await.unwrap();

        let seg_path = dir
            .path()
            .join("conv-1")
            .join(SEGMENTS_DIR)
            .join("00000000000000000000.jsonl");
        let mut content = fs::read(&seg_path).await.unwrap();
        content[0] = if content[0] == b'{' { b' ' } else { b'{' };
        fs::write(&seg_path, &content).await.unwrap();

        let mut manifest = load_manifest_from_dir(dir.path(), "conv-1").await;
        manifest.segments[0].bytes = content.len() as u64;
        save_manifest_to_dir(dir.path(), "conv-1", &manifest).await;

        let result = journal.load(&ConversationId::from("conv-1")).await;
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("journal integrity")
        );
    }

    #[tokio::test]
    async fn legacy_manifest_without_checksum_still_loads() {
        let dir = tempfile::tempdir().unwrap();
        let journal = JsonlConversationJournal::new(dir.path());

        journal.append(make_event("conv-1", "evt-1")).await.unwrap();
        journal.append(make_event("conv-1", "evt-2")).await.unwrap();

        let mut manifest = load_manifest_from_dir(dir.path(), "conv-1").await;
        for segment in &mut manifest.segments {
            segment.checksum_sha256 = None;
            segment.previous_segment_checksum_sha256 = None;
        }
        save_manifest_to_dir(dir.path(), "conv-1", &manifest).await;

        let loaded = journal.load(&ConversationId::from("conv-1")).await.unwrap();
        assert_eq!(loaded.len(), 2);
    }
}
