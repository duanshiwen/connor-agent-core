//! JSONL-based journal implementation.
//!
//! Events are stored as one JSON object per line in files located at:
//! `.agent-os/conversations/{conversation_id}/journal.jsonl`

use async_trait::async_trait;
use conversation_core::{ConversationEventEnvelope, ConversationId};
use std::path::PathBuf;
use tokio::fs::{self, OpenOptions};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

use crate::ConversationJournal;

/// A persistent journal that stores events as JSONL files on disk.
pub struct JsonlConversationJournal {
    root_dir: PathBuf,
}

impl JsonlConversationJournal {
    /// Create a new journal rooted at the given directory.
    ///
    /// Events will be stored at `{root_dir}/{conversation_id}/journal.jsonl`.
    pub fn new(root_dir: impl Into<PathBuf>) -> Self {
        Self {
            root_dir: root_dir.into(),
        }
    }

    fn journal_path(&self, conversation_id: &ConversationId) -> PathBuf {
        self.root_dir
            .join(conversation_id.0.as_str())
            .join("journal.jsonl")
    }
}

#[async_trait]
impl ConversationJournal for JsonlConversationJournal {
    async fn append(&self, event: ConversationEventEnvelope) -> anyhow::Result<()> {
        let path = self.journal_path(&event.conversation_id);

        // Ensure parent directory exists.
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).await?;
        }

        let mut json = serde_json::to_string(&event)?;
        json.push('\n');

        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .await?;

        file.write_all(json.as_bytes()).await?;
        file.flush().await?;

        Ok(())
    }

    async fn load(
        &self,
        conversation_id: &ConversationId,
    ) -> anyhow::Result<Vec<ConversationEventEnvelope>> {
        let path = self.journal_path(conversation_id);

        if !path.exists() {
            return Ok(Vec::new());
        }

        let file = fs::File::open(&path).await?;
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
}
