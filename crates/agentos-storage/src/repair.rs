//! Report-only storage repair and rebuild inspection APIs.
//!
//! These APIs intentionally do not mutate user data. They provide a storage
//! doctor boundary that can detect layout, artifact, journal, projection, and
//! artifact-reference issues before later repair commands decide what to change.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use artifact_core::ArtifactId;
use conversation_core::{ConversationEvent, ConversationId, EventId};
use conversation_journal::{ConversationJournal, JournalIntegrityIssue, JsonlConversationJournal};
use conversation_kernel::ConversationProjector;

use crate::{
    AgentOsStorage, ArtifactVerificationIssue, ArtifactVerificationReport, FsArtifactStore,
    STORAGE_LAYOUT_DIRECTORIES, StorageResult,
};

/// Severity for report-only storage repair issues.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StorageRepairSeverity {
    Info,
    Warning,
    Critical,
}

/// A broken artifact reference found in a conversation journal event.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BrokenArtifactReference {
    pub conversation_id: ConversationId,
    pub event_id: EventId,
    pub artifact_id: ArtifactId,
}

/// Artifact reference scan result.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ArtifactReferenceReport {
    pub referenced_artifacts: Vec<ArtifactId>,
    pub stored_artifacts: Vec<ArtifactId>,
    pub orphan_artifacts: Vec<ArtifactId>,
    pub broken_artifact_references: Vec<BrokenArtifactReference>,
}

/// Projection rebuild check result for all discovered conversations.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ProjectionRebuildReport {
    pub conversations_checked: usize,
    pub rebuilt_conversations: Vec<ConversationId>,
    pub failed_conversations: Vec<ConversationProjectionRebuildFailure>,
}

/// A projection rebuild failure for one conversation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConversationProjectionRebuildFailure {
    pub conversation_id: ConversationId,
    pub message: String,
}

/// A report-only storage repair issue.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StorageRepairIssue {
    MissingLayoutDirectory {
        path: String,
    },
    ArtifactIntegrityFailed {
        artifact_id: ArtifactId,
        issues: Vec<ArtifactVerificationIssue>,
    },
    ConversationJournalIntegrityFailed {
        conversation_id: ConversationId,
        issues: Vec<JournalIntegrityIssue>,
    },
    ConversationProjectionRebuildFailed {
        conversation_id: ConversationId,
        message: String,
    },
    OrphanArtifact {
        artifact_id: ArtifactId,
    },
    BrokenArtifactReference {
        conversation_id: ConversationId,
        event_id: EventId,
        artifact_id: ArtifactId,
    },
}

impl StorageRepairIssue {
    pub fn severity(&self) -> StorageRepairSeverity {
        match self {
            StorageRepairIssue::MissingLayoutDirectory { .. }
            | StorageRepairIssue::ArtifactIntegrityFailed { .. }
            | StorageRepairIssue::ConversationJournalIntegrityFailed { .. }
            | StorageRepairIssue::ConversationProjectionRebuildFailed { .. }
            | StorageRepairIssue::BrokenArtifactReference { .. } => StorageRepairSeverity::Critical,
            StorageRepairIssue::OrphanArtifact { .. } => StorageRepairSeverity::Warning,
        }
    }
}

/// Full storage repair inspection report.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StorageRepairReport {
    pub issues: Vec<StorageRepairIssue>,
    pub artifact_reports: Vec<ArtifactVerificationReport>,
    pub artifact_references: ArtifactReferenceReport,
    pub projection_rebuild: ProjectionRebuildReport,
}

impl StorageRepairReport {
    pub fn is_clean(&self) -> bool {
        self.issues.is_empty()
    }

    pub fn has_critical_issues(&self) -> bool {
        self.issues
            .iter()
            .any(|issue| issue.severity() == StorageRepairSeverity::Critical)
    }
}

/// Report-only storage repair entry point.
pub struct StorageRepair;

impl StorageRepair {
    /// Inspect storage without mutating it.
    pub async fn inspect(storage: &AgentOsStorage) -> StorageResult<StorageRepairReport> {
        let mut issues = Vec::new();

        for directory in STORAGE_LAYOUT_DIRECTORIES {
            let path = storage.path_for(directory);
            if !path.is_dir() {
                issues.push(StorageRepairIssue::MissingLayoutDirectory {
                    path: path.display().to_string(),
                });
            }
        }

        let artifacts = FsArtifactStore::for_storage(storage)?;
        let artifact_reports = artifacts.verify_all()?;
        for report in &artifact_reports {
            if !report.verified {
                issues.push(StorageRepairIssue::ArtifactIntegrityFailed {
                    artifact_id: report.artifact_id.clone(),
                    issues: report.issues.clone(),
                });
            }
        }

        for conversation_id in discover_conversation_ids(storage)? {
            let journal = JsonlConversationJournal::new(storage.path_for("conversations"));
            let report = journal
                .verify(&conversation_id)
                .await
                .map_err(anyhow_to_io)?;
            if !report.is_clean() {
                issues.push(StorageRepairIssue::ConversationJournalIntegrityFailed {
                    conversation_id,
                    issues: report.issues,
                });
            }
        }

        let artifact_references = Self::detect_artifact_references(storage).await?;
        for artifact_id in &artifact_references.orphan_artifacts {
            issues.push(StorageRepairIssue::OrphanArtifact {
                artifact_id: artifact_id.clone(),
            });
        }
        for broken in &artifact_references.broken_artifact_references {
            issues.push(StorageRepairIssue::BrokenArtifactReference {
                conversation_id: broken.conversation_id.clone(),
                event_id: broken.event_id.clone(),
                artifact_id: broken.artifact_id.clone(),
            });
        }

        let projection_rebuild = Self::rebuild_conversation_projections(storage).await?;
        for failure in &projection_rebuild.failed_conversations {
            issues.push(StorageRepairIssue::ConversationProjectionRebuildFailed {
                conversation_id: failure.conversation_id.clone(),
                message: failure.message.clone(),
            });
        }

        sort_issues(&mut issues);

        Ok(StorageRepairReport {
            issues,
            artifact_reports,
            artifact_references,
            projection_rebuild,
        })
    }

    /// Replay all discovered conversation journals to prove projections can be rebuilt.
    pub async fn rebuild_conversation_projections(
        storage: &AgentOsStorage,
    ) -> StorageResult<ProjectionRebuildReport> {
        let journal = JsonlConversationJournal::new(storage.path_for("conversations"));
        let mut conversation_ids = discover_conversation_ids(storage)?;
        conversation_ids.sort_by(|a, b| a.0.cmp(&b.0));

        let mut report = ProjectionRebuildReport {
            conversations_checked: conversation_ids.len(),
            rebuilt_conversations: Vec::new(),
            failed_conversations: Vec::new(),
        };

        for conversation_id in conversation_ids {
            match journal.load(&conversation_id).await {
                Ok(events) => match ConversationProjector::project(&events) {
                    Ok(_) => report.rebuilt_conversations.push(conversation_id),
                    Err(error) => {
                        report
                            .failed_conversations
                            .push(ConversationProjectionRebuildFailure {
                                conversation_id,
                                message: error.to_string(),
                            })
                    }
                },
                Err(error) => {
                    report
                        .failed_conversations
                        .push(ConversationProjectionRebuildFailure {
                            conversation_id,
                            message: error.to_string(),
                        })
                }
            }
        }

        report.rebuilt_conversations.sort_by(|a, b| a.0.cmp(&b.0));
        report
            .failed_conversations
            .sort_by(|a, b| a.conversation_id.0.cmp(&b.conversation_id.0));

        Ok(report)
    }

    /// Compare stored artifacts with artifact references found in conversation journals.
    pub async fn detect_artifact_references(
        storage: &AgentOsStorage,
    ) -> StorageResult<ArtifactReferenceReport> {
        let artifact_store = FsArtifactStore::for_storage(storage)?;
        let stored_artifacts = artifact_store.list_artifact_ids()?;
        let stored_by_id = stored_artifacts
            .iter()
            .cloned()
            .map(|id| (id.0.clone(), id))
            .collect::<HashMap<_, _>>();
        let journal = JsonlConversationJournal::new(storage.path_for("conversations"));

        let mut referenced = HashMap::<String, (ArtifactId, Vec<(ConversationId, EventId)>)>::new();
        for conversation_id in discover_conversation_ids(storage)? {
            let events = match journal.load(&conversation_id).await {
                Ok(events) => events,
                Err(_) => continue,
            };
            for event in events {
                if let ConversationEvent::ArtifactLinkedToConversation { artifact } = event.event {
                    referenced
                        .entry(artifact.id.0.clone())
                        .or_insert_with(|| (artifact.id, Vec::new()))
                        .1
                        .push((event.conversation_id, event.event_id));
                }
            }
        }

        let mut referenced_artifacts = referenced
            .values()
            .map(|(artifact_id, _)| artifact_id.clone())
            .collect::<Vec<_>>();
        referenced_artifacts.sort_by(|a, b| a.0.cmp(&b.0));

        let mut orphan_artifacts = stored_artifacts
            .iter()
            .filter(|artifact_id| !referenced.contains_key(&artifact_id.0))
            .cloned()
            .collect::<Vec<_>>();
        orphan_artifacts.sort_by(|a, b| a.0.cmp(&b.0));

        let mut broken_artifact_references = Vec::new();
        let mut referenced_entries = referenced.into_values().collect::<Vec<_>>();
        referenced_entries.sort_by(|a, b| a.0.0.cmp(&b.0.0));
        for (artifact_id, refs) in referenced_entries {
            if !stored_by_id.contains_key(&artifact_id.0) {
                for (conversation_id, event_id) in refs {
                    broken_artifact_references.push(BrokenArtifactReference {
                        conversation_id,
                        event_id,
                        artifact_id: artifact_id.clone(),
                    });
                }
            }
        }
        broken_artifact_references.sort_by(|a, b| {
            a.conversation_id
                .0
                .cmp(&b.conversation_id.0)
                .then_with(|| a.event_id.0.cmp(&b.event_id.0))
                .then_with(|| a.artifact_id.0.cmp(&b.artifact_id.0))
        });

        Ok(ArtifactReferenceReport {
            referenced_artifacts,
            stored_artifacts,
            orphan_artifacts,
            broken_artifact_references,
        })
    }
}

fn discover_conversation_ids(storage: &AgentOsStorage) -> StorageResult<Vec<ConversationId>> {
    let conversations_root = storage.path_for("conversations");
    discover_conversation_ids_at(&conversations_root)
}

fn discover_conversation_ids_at(path: &Path) -> StorageResult<Vec<ConversationId>> {
    let mut ids = Vec::new();
    if !path.exists() {
        return Ok(ids);
    }

    let entries = fs::read_dir(path).map_err(|source| crate::StorageError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    for entry in entries {
        let entry = entry.map_err(|source| crate::StorageError::Io {
            path: path.to_path_buf(),
            source,
        })?;
        let entry_path = entry.path();
        if entry_path.is_dir() && entry_path.join("manifest.json").is_file() {
            if let Some(name) = entry.file_name().to_str() {
                ids.push(ConversationId::from(name));
            }
        }
    }
    ids.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(ids)
}

fn sort_issues(issues: &mut [StorageRepairIssue]) {
    issues.sort_by_key(issue_sort_key);
}

fn issue_sort_key(issue: &StorageRepairIssue) -> String {
    match issue {
        StorageRepairIssue::MissingLayoutDirectory { path } => format!("0:{path}"),
        StorageRepairIssue::ArtifactIntegrityFailed { artifact_id, .. } => {
            format!("1:{}", artifact_id.0)
        }
        StorageRepairIssue::ConversationJournalIntegrityFailed {
            conversation_id, ..
        } => format!("2:{}", conversation_id.0),
        StorageRepairIssue::ConversationProjectionRebuildFailed {
            conversation_id, ..
        } => format!("3:{}", conversation_id.0),
        StorageRepairIssue::OrphanArtifact { artifact_id } => format!("4:{}", artifact_id.0),
        StorageRepairIssue::BrokenArtifactReference {
            conversation_id,
            event_id,
            artifact_id,
        } => format!("5:{}:{}:{}", conversation_id.0, event_id.0, artifact_id.0),
    }
}

fn anyhow_to_io(error: impl std::fmt::Display) -> crate::StorageError {
    crate::StorageError::Io {
        path: PathBuf::from("conversation journal verify"),
        source: std::io::Error::other(error.to_string()),
    }
}
