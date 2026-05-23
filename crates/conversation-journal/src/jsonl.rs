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
}

impl SegmentMetadata {
    fn new(index: u64) -> Self {
        Self {
            index,
            file_name: segment_file_name(index),
            event_count: 0,
            bytes: 0,
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

        while let Some(line) = lines.next_line().await? {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            let event: ConversationEventEnvelope = serde_json::from_str(line)?;
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

        let active_segment = manifest.active_segment_mut();
        active_segment.event_count += 1;
        active_segment.bytes += line_bytes;
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

        manifest.segments.sort_by_key(|segment| segment.index);

        let mut events = Vec::new();
        for segment in manifest.segments {
            let path = self.segment_path(conversation_id, &segment.file_name);
            events.extend(Self::load_events_from_file(&path).await?);
        }

        Ok(events)
    }
}

fn segment_file_name(index: u64) -> String {
    format!("{index:020}.jsonl")
}

#[cfg(test)]
mod tests {
    use super::*;
    use conversation_core::*;

    fn make_event(conversation_id: &str, event_id: &str) -> ConversationEventEnvelope {
        ConversationEventEnvelope {
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
    async fn writes_manifest_for_segmented_journal() {
        let dir = tempfile::tempdir().unwrap();
        let journal = JsonlConversationJournal::with_max_segment_bytes(dir.path(), 1);

        journal.append(make_event("conv-1", "evt-1")).await.unwrap();
        journal.append(make_event("conv-1", "evt-2")).await.unwrap();

        let manifest_path = dir.path().join("conv-1").join(MANIFEST_FILE);
        let manifest_bytes = fs::read(manifest_path).await.unwrap();
        let manifest: JournalManifest = serde_json::from_slice(&manifest_bytes).unwrap();

        assert_eq!(manifest.version, 1);
        assert_eq!(manifest.total_events, 2);
        assert_eq!(manifest.segments.len(), 2);
        assert_eq!(manifest.active_segment_index, 1);
    }
}
