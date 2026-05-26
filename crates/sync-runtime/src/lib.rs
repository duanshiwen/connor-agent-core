//! # Sync Runtime
//!
//! Domain types and merge logic for AgentOS P2P synchronization.
//!
//! This crate defines the core abstractions for syncing data between devices:
//! - Sync object identity and kind classification
//! - Sync manifests for tracking object versions per device
//! - Manifest diff computation
//! - Merge strategies: append-only dedup, LWW (Last-Write-Wins), conflict detection
//!
//! Design principles:
//! - Personal data only (no enterprise server data)
//! - Append-only logs merge by event id (no duplicates)
//! - Profile objects use LWW + field-level source metadata
//! - Asset metadata uses LWW + source device attribution

use chrono::{DateTime, Utc};
use device_pairing_core::DeviceId;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fmt;

// ---------------------------------------------------------------------------
// Sync Object Identity
// ---------------------------------------------------------------------------

/// Unique identifier for a syncable object across devices.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SyncObjectId(pub String);

impl fmt::Display for SyncObjectId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl From<String> for SyncObjectId {
    fn from(s: String) -> Self {
        Self(s)
    }
}

impl From<&str> for SyncObjectId {
    fn from(s: &str) -> Self {
        Self(s.to_string())
    }
}

/// Classification of syncable object kinds.
///
/// Only personal data is synced. Enterprise server data and
/// large binary blobs (raw video) are excluded.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SyncObjectKind {
    PersonalKnowledgeEntry,
    PersonProfile,
    RelationshipProfile,
    QuestionLedgerEntry,
    AnswerCachePackage,
    AssetMetadata,
    ConversationMetadata,
}

impl fmt::Display for SyncObjectKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::PersonalKnowledgeEntry => write!(f, "personal_knowledge_entry"),
            Self::PersonProfile => write!(f, "person_profile"),
            Self::RelationshipProfile => write!(f, "relationship_profile"),
            Self::QuestionLedgerEntry => write!(f, "question_ledger_entry"),
            Self::AnswerCachePackage => write!(f, "answer_cache_package"),
            Self::AssetMetadata => write!(f, "asset_metadata"),
            Self::ConversationMetadata => write!(f, "conversation_metadata"),
        }
    }
}

// ---------------------------------------------------------------------------
// Merge Policy
// ---------------------------------------------------------------------------

/// How a sync object should be merged when both devices have changes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MergePolicy {
    /// Append-only: merge by unique event id, skip duplicates.
    /// Used for: conversation journals, question ledger entries.
    AppendOnly,
    /// Last-Write-Wins: the entry with the newer `updated_at` wins.
    /// Used for: person profiles, asset metadata.
    LastWriteWins,
}

impl fmt::Display for MergePolicy {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::AppendOnly => write!(f, "append_only"),
            Self::LastWriteWins => write!(f, "last_write_wins"),
        }
    }
}

// ---------------------------------------------------------------------------
// Sync Record (one version of one object on one device)
// ---------------------------------------------------------------------------

/// A sync record represents one object's version on one device.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SyncRecord {
    pub object_id: SyncObjectId,
    pub kind: SyncObjectKind,
    pub source_device: DeviceId,
    pub version_hash: String,
    pub updated_at: DateTime<Utc>,
    pub merge_policy: MergePolicy,
    /// Opaque payload (JSON). For append-only, this is a single event.
    /// For LWW, this is the full object snapshot.
    pub payload: serde_json::Value,
}

// ---------------------------------------------------------------------------
// Sync Manifest (what a device has)
// ---------------------------------------------------------------------------

/// A manifest summarizes what sync objects a device currently has.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SyncManifest {
    pub device_id: DeviceId,
    pub agent_os_id: String,
    pub objects: HashMap<SyncObjectId, SyncRecord>,
    pub captured_at: DateTime<Utc>,
}

impl SyncManifest {
    pub fn new(device_id: DeviceId, agent_os_id: String, captured_at: DateTime<Utc>) -> Self {
        Self {
            device_id,
            agent_os_id,
            objects: HashMap::new(),
            captured_at,
        }
    }

    /// Insert or replace a sync record.
    pub fn upsert(&mut self, record: SyncRecord) {
        self.objects.insert(record.object_id.clone(), record);
    }

    /// Remove an object from the manifest.
    pub fn remove(&mut self, object_id: &SyncObjectId) -> Option<SyncRecord> {
        self.objects.remove(object_id)
    }

    /// Get a record by object id.
    pub fn get(&self, object_id: &SyncObjectId) -> Option<&SyncRecord> {
        self.objects.get(object_id)
    }

    /// Number of objects in this manifest.
    pub fn len(&self) -> usize {
        self.objects.len()
    }

    pub fn is_empty(&self) -> bool {
        self.objects.is_empty()
    }
}

// ---------------------------------------------------------------------------
// Manifest Diff
// ---------------------------------------------------------------------------

/// Result of comparing two manifests.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ManifestDiff {
    /// Objects the local device has that the remote doesn't.
    pub local_only: Vec<SyncObjectId>,
    /// Objects the remote device has that the local doesn't.
    pub remote_only: Vec<SyncObjectId>,
    /// Objects both have, but with different version hashes.
    pub conflicting: Vec<SyncObjectId>,
    /// Objects both have with the same version hash (already in sync).
    pub in_sync: Vec<SyncObjectId>,
}

impl ManifestDiff {
    pub fn is_empty(&self) -> bool {
        self.local_only.is_empty() && self.remote_only.is_empty() && self.conflicting.is_empty()
    }

    pub fn needs_sync(&self) -> bool {
        !self.local_only.is_empty() || !self.remote_only.is_empty() || !self.conflicting.is_empty()
    }
}

/// Compute the diff between a local and remote manifest.
pub fn diff_manifests(local: &SyncManifest, remote: &SyncManifest) -> ManifestDiff {
    let local_ids: HashSet<&SyncObjectId> = local.objects.keys().collect();
    let remote_ids: HashSet<&SyncObjectId> = remote.objects.keys().collect();

    let local_only: Vec<SyncObjectId> = local_ids
        .difference(&remote_ids)
        .map(|id| (*id).clone())
        .collect();

    let remote_only: Vec<SyncObjectId> = remote_ids
        .difference(&local_ids)
        .map(|id| (*id).clone())
        .collect();

    let common: Vec<&SyncObjectId> = local_ids.intersection(&remote_ids).cloned().collect();

    let mut conflicting = Vec::new();
    let mut in_sync = Vec::new();

    for id in common {
        let local_record = local.objects.get(id).unwrap();
        let remote_record = remote.objects.get(id).unwrap();
        if local_record.version_hash == remote_record.version_hash {
            in_sync.push(id.clone());
        } else {
            conflicting.push(id.clone());
        }
    }

    ManifestDiff {
        local_only,
        remote_only,
        conflicting,
        in_sync,
    }
}

// ---------------------------------------------------------------------------
// Sync Conflict
// ---------------------------------------------------------------------------

/// A conflict detected during merge.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SyncConflict {
    pub object_id: SyncObjectId,
    pub kind: SyncObjectKind,
    pub local_record: SyncRecord,
    pub remote_record: SyncRecord,
    pub resolution: ConflictResolution,
}

/// How a conflict was resolved.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConflictResolution {
    /// Local wins (newer timestamp or higher device priority).
    LocalWins,
    /// Remote wins (newer timestamp or higher device priority).
    RemoteWins,
    /// Both records kept (append-only merge).
    Merged,
}

// ---------------------------------------------------------------------------
// Merge Result
// ---------------------------------------------------------------------------

/// Result of a merge operation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MergeResult {
    /// Records that were added to the local manifest.
    pub added: Vec<SyncObjectId>,
    /// Records that were updated in the local manifest.
    pub updated: Vec<SyncObjectId>,
    /// Records that were already in sync (skipped).
    pub skipped: Vec<SyncObjectId>,
    /// Conflicts detected and resolved.
    pub conflicts: Vec<SyncConflict>,
}

impl MergeResult {
    pub fn total_changes(&self) -> usize {
        self.added.len() + self.updated.len() + self.conflicts.len()
    }
}

// ---------------------------------------------------------------------------
// Merge Engine
// ---------------------------------------------------------------------------

/// Merge remote records into the local manifest.
///
/// For each record in the remote manifest:
/// - If not in local → add it.
/// - If in local with same hash → skip.
/// - If in local with different hash → apply merge policy:
///   - AppendOnly: keep both payloads (dedup by event id).
///   - LastWriteWins: keep the one with newer `updated_at`.
pub fn merge_manifests(local: &mut SyncManifest, remote: &SyncManifest) -> MergeResult {
    let mut result = MergeResult {
        added: Vec::new(),
        updated: Vec::new(),
        skipped: Vec::new(),
        conflicts: Vec::new(),
    };

    for (object_id, remote_record) in &remote.objects {
        match local.objects.get(object_id) {
            None => {
                // New object from remote
                local
                    .objects
                    .insert(object_id.clone(), remote_record.clone());
                result.added.push(object_id.clone());
            }
            Some(local_record) => {
                if local_record.version_hash == remote_record.version_hash {
                    // Already in sync
                    result.skipped.push(object_id.clone());
                } else {
                    // Conflict — apply merge policy
                    let conflict = resolve_conflict(object_id, local_record, remote_record);
                    match &conflict.resolution {
                        ConflictResolution::RemoteWins => {
                            local
                                .objects
                                .insert(object_id.clone(), remote_record.clone());
                            result.updated.push(object_id.clone());
                        }
                        ConflictResolution::LocalWins => {
                            // Keep local, no change
                            result.skipped.push(object_id.clone());
                        }
                        ConflictResolution::Merged => {
                            // For append-only: combine payloads
                            let merged = merge_append_only_payloads(local_record, remote_record);
                            local.objects.insert(object_id.clone(), merged);
                            result.updated.push(object_id.clone());
                        }
                    }
                    result.conflicts.push(conflict);
                }
            }
        }
    }

    result
}

/// Resolve a conflict between local and remote records.
fn resolve_conflict(
    object_id: &SyncObjectId,
    local: &SyncRecord,
    remote: &SyncRecord,
) -> SyncConflict {
    let resolution = match &local.merge_policy {
        MergePolicy::AppendOnly => ConflictResolution::Merged,
        MergePolicy::LastWriteWins => {
            if remote.updated_at > local.updated_at {
                ConflictResolution::RemoteWins
            } else if local.updated_at > remote.updated_at {
                ConflictResolution::LocalWins
            } else {
                // Same timestamp — use device id lexicographic order for determinism
                if remote.source_device.0 > local.source_device.0 {
                    ConflictResolution::RemoteWins
                } else {
                    ConflictResolution::LocalWins
                }
            }
        }
    };

    SyncConflict {
        object_id: object_id.clone(),
        kind: local.kind.clone(),
        local_record: local.clone(),
        remote_record: remote.clone(),
        resolution,
    }
}

/// Merge two append-only records by combining their payloads.
///
/// Expects payloads to be arrays of events. Deduplicates by the first
/// available identity field: `id`, `event_id`, or `message_id`.
fn merge_append_only_payloads(local: &SyncRecord, remote: &SyncRecord) -> SyncRecord {
    let local_events = extract_events(&local.payload);
    let remote_events = extract_events(&remote.payload);

    // Deduplicate by event id
    let mut seen: HashSet<String> = HashSet::new();
    let mut merged_events: Vec<serde_json::Value> = Vec::new();

    for event in local_events.iter().chain(remote_events.iter()) {
        let event_id = extract_event_id(event);
        if let Some(ref id) = event_id {
            if seen.contains(id) {
                continue;
            }
            seen.insert(id.clone());
        }
        merged_events.push(event.clone());
    }

    // Use the newer updated_at
    let updated_at = if remote.updated_at > local.updated_at {
        remote.updated_at
    } else {
        local.updated_at
    };

    SyncRecord {
        object_id: local.object_id.clone(),
        kind: local.kind.clone(),
        source_device: local.source_device.clone(),
        version_hash: format!("merged-{}", uuid::Uuid::new_v4()),
        updated_at,
        merge_policy: local.merge_policy.clone(),
        payload: serde_json::json!(merged_events),
    }
}

/// Extract events from a payload (expects array or wraps single object).
fn extract_events(payload: &serde_json::Value) -> Vec<serde_json::Value> {
    match payload {
        serde_json::Value::Array(arr) => arr.clone(),
        other => vec![other.clone()],
    }
}

/// Extract an event id from a JSON value.
fn extract_event_id(event: &serde_json::Value) -> Option<String> {
    event
        .get("id")
        .or_else(|| event.get("event_id"))
        .or_else(|| event.get("message_id"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
}

// ===========================================================================
// Sync Object Store
// ===========================================================================

/// Metadata for a sync object.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SyncObjectMetadata {
    pub object_id: SyncObjectId,
    pub kind: SyncObjectKind,
    pub content_hash: String,
    pub size_bytes: u64,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub source_device: DeviceId,
}

/// A sync object with its content.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SyncObject {
    pub metadata: SyncObjectMetadata,
    pub content: Vec<u8>,
}

impl SyncObject {
    pub fn new(
        object_id: SyncObjectId,
        kind: SyncObjectKind,
        content: Vec<u8>,
        source_device: DeviceId,
    ) -> Self {
        let now = Utc::now();
        let content_hash = compute_hash(&content);
        Self {
            metadata: SyncObjectMetadata {
                object_id,
                kind,
                content_hash,
                size_bytes: content.len() as u64,
                created_at: now,
                updated_at: now,
                source_device,
            },
            content,
        }
    }

    /// Verify the content hash matches the metadata.
    pub fn verify_hash(&self) -> bool {
        compute_hash(&self.content) == self.metadata.content_hash
    }
}

/// Compute a simple hash of content for integrity checking.
pub fn compute_hash(content: &[u8]) -> String {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut hasher = DefaultHasher::new();
    content.hash(&mut hasher);
    format!("{:x}", hasher.finish())
}

/// Error type for sync object store operations.
#[derive(Debug, thiserror::Error)]
pub enum SyncObjectStoreError {
    #[error("object not found: {0}")]
    NotFound(String),
    #[error("hash mismatch: expected {expected}, got {actual}")]
    HashMismatch { expected: String, actual: String },
    #[error("internal error: {0}")]
    Internal(String),
}

/// Trait for storing sync objects.
#[async_trait::async_trait]
pub trait SyncObjectStore: Send + Sync {
    async fn put(&self, object: &SyncObject) -> Result<(), SyncObjectStoreError>;
    async fn get(&self, id: &SyncObjectId) -> Result<SyncObject, SyncObjectStoreError>;
    async fn get_metadata(
        &self,
        id: &SyncObjectId,
    ) -> Result<SyncObjectMetadata, SyncObjectStoreError>;
    async fn delete(&self, id: &SyncObjectId) -> Result<(), SyncObjectStoreError>;
    async fn list(&self) -> Result<Vec<SyncObjectMetadata>, SyncObjectStoreError>;
    async fn contains(&self, id: &SyncObjectId) -> Result<bool, SyncObjectStoreError>;
    async fn verify(&self, id: &SyncObjectId) -> Result<bool, SyncObjectStoreError>;
}

/// In-memory implementation of SyncObjectStore.
#[derive(Debug, Clone, Default)]
pub struct MemorySyncObjectStore {
    objects: std::sync::Arc<std::sync::Mutex<HashMap<SyncObjectId, SyncObject>>>,
}

impl MemorySyncObjectStore {
    pub fn new() -> Self {
        Self {
            objects: std::sync::Arc::new(std::sync::Mutex::new(HashMap::new())),
        }
    }
}

#[async_trait::async_trait]
impl SyncObjectStore for MemorySyncObjectStore {
    async fn put(&self, object: &SyncObject) -> Result<(), SyncObjectStoreError> {
        let mut objects = self
            .objects
            .lock()
            .map_err(|e| SyncObjectStoreError::Internal(e.to_string()))?;
        objects.insert(object.metadata.object_id.clone(), object.clone());
        Ok(())
    }

    async fn get(&self, id: &SyncObjectId) -> Result<SyncObject, SyncObjectStoreError> {
        let objects = self
            .objects
            .lock()
            .map_err(|e| SyncObjectStoreError::Internal(e.to_string()))?;
        objects
            .get(id)
            .cloned()
            .ok_or_else(|| SyncObjectStoreError::NotFound(id.0.clone()))
    }

    async fn get_metadata(
        &self,
        id: &SyncObjectId,
    ) -> Result<SyncObjectMetadata, SyncObjectStoreError> {
        let objects = self
            .objects
            .lock()
            .map_err(|e| SyncObjectStoreError::Internal(e.to_string()))?;
        objects
            .get(id)
            .map(|o| o.metadata.clone())
            .ok_or_else(|| SyncObjectStoreError::NotFound(id.0.clone()))
    }

    async fn delete(&self, id: &SyncObjectId) -> Result<(), SyncObjectStoreError> {
        let mut objects = self
            .objects
            .lock()
            .map_err(|e| SyncObjectStoreError::Internal(e.to_string()))?;
        objects.remove(id);
        Ok(())
    }

    async fn list(&self) -> Result<Vec<SyncObjectMetadata>, SyncObjectStoreError> {
        let objects = self
            .objects
            .lock()
            .map_err(|e| SyncObjectStoreError::Internal(e.to_string()))?;
        Ok(objects.values().map(|o| o.metadata.clone()).collect())
    }

    async fn contains(&self, id: &SyncObjectId) -> Result<bool, SyncObjectStoreError> {
        let objects = self
            .objects
            .lock()
            .map_err(|e| SyncObjectStoreError::Internal(e.to_string()))?;
        Ok(objects.contains_key(id))
    }

    async fn verify(&self, id: &SyncObjectId) -> Result<bool, SyncObjectStoreError> {
        let objects = self
            .objects
            .lock()
            .map_err(|e| SyncObjectStoreError::Internal(e.to_string()))?;
        match objects.get(id) {
            Some(obj) => Ok(obj.verify_hash()),
            None => Err(SyncObjectStoreError::NotFound(id.0.clone())),
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn ts(offset_secs: i64) -> DateTime<Utc> {
        "2026-05-25T12:00:00Z".parse::<DateTime<Utc>>().unwrap()
            + chrono::Duration::seconds(offset_secs)
    }

    fn device_a() -> DeviceId {
        DeviceId::from("device-laptop")
    }

    fn device_b() -> DeviceId {
        DeviceId::from("device-phone")
    }

    fn agent_id() -> String {
        "agent-001".to_string()
    }

    fn make_record(
        id: &str,
        kind: SyncObjectKind,
        device: DeviceId,
        hash: &str,
        offset: i64,
        policy: MergePolicy,
    ) -> SyncRecord {
        SyncRecord {
            object_id: SyncObjectId::from(id),
            kind,
            source_device: device,
            version_hash: hash.to_string(),
            updated_at: ts(offset),
            merge_policy: policy,
            payload: serde_json::json!({"data": id}),
        }
    }

    // ---- Type roundtrip tests ----

    #[test]
    fn sync_object_id_roundtrips() {
        let id = SyncObjectId::from("obj-1");
        let json = serde_json::to_string(&id).unwrap();
        let decoded: SyncObjectId = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded, id);
        assert_eq!(id.to_string(), "obj-1");
    }

    #[test]
    fn sync_object_kind_serde() {
        let kind = SyncObjectKind::PersonalKnowledgeEntry;
        assert_eq!(
            serde_json::to_string(&kind).unwrap(),
            "\"personal_knowledge_entry\""
        );
        let decoded: SyncObjectKind = serde_json::from_str("\"person_profile\"").unwrap();
        assert_eq!(decoded, SyncObjectKind::PersonProfile);
    }

    #[test]
    fn merge_policy_serde() {
        let json = serde_json::to_string(&MergePolicy::AppendOnly).unwrap();
        assert_eq!(json, "\"append_only\"");
        let decoded: MergePolicy = serde_json::from_str("\"last_write_wins\"").unwrap();
        assert_eq!(decoded, MergePolicy::LastWriteWins);
    }

    #[test]
    fn sync_record_roundtrips() {
        let record = make_record(
            "obj-1",
            SyncObjectKind::AssetMetadata,
            device_a(),
            "hash-abc",
            0,
            MergePolicy::LastWriteWins,
        );
        let json = serde_json::to_string_pretty(&record).unwrap();
        let decoded: SyncRecord = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded, record);
    }

    #[test]
    fn sync_manifest_roundtrips() {
        let mut manifest = SyncManifest::new(device_a(), agent_id(), ts(0));
        manifest.upsert(make_record(
            "obj-1",
            SyncObjectKind::AssetMetadata,
            device_a(),
            "hash-1",
            0,
            MergePolicy::LastWriteWins,
        ));
        let json = serde_json::to_string_pretty(&manifest).unwrap();
        let decoded: SyncManifest = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded.len(), 1);
    }

    #[test]
    fn manifest_diff_roundtrips() {
        let diff = ManifestDiff {
            local_only: vec![SyncObjectId::from("a")],
            remote_only: vec![SyncObjectId::from("b")],
            conflicting: vec![SyncObjectId::from("c")],
            in_sync: vec![SyncObjectId::from("d")],
        };
        let json = serde_json::to_string(&diff).unwrap();
        let decoded: ManifestDiff = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded, diff);
    }

    // ---- Manifest diff tests ----

    #[test]
    fn diff_identical_manifests() {
        let mut local = SyncManifest::new(device_a(), agent_id(), ts(0));
        local.upsert(make_record(
            "obj-1",
            SyncObjectKind::AssetMetadata,
            device_a(),
            "hash-1",
            0,
            MergePolicy::LastWriteWins,
        ));

        let mut remote = SyncManifest::new(device_b(), agent_id(), ts(0));
        remote.upsert(make_record(
            "obj-1",
            SyncObjectKind::AssetMetadata,
            device_a(),
            "hash-1",
            0,
            MergePolicy::LastWriteWins,
        ));

        let diff = diff_manifests(&local, &remote);
        assert!(diff.local_only.is_empty());
        assert!(diff.remote_only.is_empty());
        assert!(diff.conflicting.is_empty());
        assert_eq!(diff.in_sync.len(), 1);
        assert!(!diff.needs_sync());
    }

    #[test]
    fn diff_detects_local_only() {
        let mut local = SyncManifest::new(device_a(), agent_id(), ts(0));
        local.upsert(make_record(
            "obj-1",
            SyncObjectKind::AssetMetadata,
            device_a(),
            "hash-1",
            0,
            MergePolicy::LastWriteWins,
        ));

        let remote = SyncManifest::new(device_b(), agent_id(), ts(0));

        let diff = diff_manifests(&local, &remote);
        assert_eq!(diff.local_only.len(), 1);
        assert!(diff.remote_only.is_empty());
        assert!(diff.needs_sync());
    }

    #[test]
    fn diff_detects_remote_only() {
        let local = SyncManifest::new(device_a(), agent_id(), ts(0));

        let mut remote = SyncManifest::new(device_b(), agent_id(), ts(0));
        remote.upsert(make_record(
            "obj-1",
            SyncObjectKind::AssetMetadata,
            device_b(),
            "hash-1",
            0,
            MergePolicy::LastWriteWins,
        ));

        let diff = diff_manifests(&local, &remote);
        assert!(diff.local_only.is_empty());
        assert_eq!(diff.remote_only.len(), 1);
        assert!(diff.needs_sync());
    }

    #[test]
    fn diff_detects_conflict() {
        let mut local = SyncManifest::new(device_a(), agent_id(), ts(0));
        local.upsert(make_record(
            "obj-1",
            SyncObjectKind::AssetMetadata,
            device_a(),
            "hash-local",
            0,
            MergePolicy::LastWriteWins,
        ));

        let mut remote = SyncManifest::new(device_b(), agent_id(), ts(0));
        remote.upsert(make_record(
            "obj-1",
            SyncObjectKind::AssetMetadata,
            device_b(),
            "hash-remote",
            10,
            MergePolicy::LastWriteWins,
        ));

        let diff = diff_manifests(&local, &remote);
        assert!(diff.local_only.is_empty());
        assert!(diff.remote_only.is_empty());
        assert_eq!(diff.conflicting.len(), 1);
        assert!(diff.needs_sync());
    }

    // ---- Merge tests ----

    #[test]
    fn merge_adds_new_objects() {
        let mut local = SyncManifest::new(device_a(), agent_id(), ts(0));
        let mut remote = SyncManifest::new(device_b(), agent_id(), ts(0));
        remote.upsert(make_record(
            "obj-1",
            SyncObjectKind::AssetMetadata,
            device_b(),
            "hash-1",
            0,
            MergePolicy::LastWriteWins,
        ));

        let result = merge_manifests(&mut local, &remote);
        assert_eq!(result.added.len(), 1);
        assert!(result.skipped.is_empty());
        assert!(result.conflicts.is_empty());
        assert_eq!(local.len(), 1);
    }

    #[test]
    fn merge_skips_same_hash() {
        let record = make_record(
            "obj-1",
            SyncObjectKind::AssetMetadata,
            device_a(),
            "hash-1",
            0,
            MergePolicy::LastWriteWins,
        );
        let mut local = SyncManifest::new(device_a(), agent_id(), ts(0));
        local.upsert(record.clone());
        let mut remote = SyncManifest::new(device_b(), agent_id(), ts(0));
        remote.upsert(record);

        let result = merge_manifests(&mut local, &remote);
        assert!(result.added.is_empty());
        assert_eq!(result.skipped.len(), 1);
        assert!(result.conflicts.is_empty());
    }

    #[test]
    fn merge_lww_picks_remote_when_newer() {
        let mut local = SyncManifest::new(device_a(), agent_id(), ts(0));
        local.upsert(make_record(
            "obj-1",
            SyncObjectKind::AssetMetadata,
            device_a(),
            "hash-old",
            0,
            MergePolicy::LastWriteWins,
        ));

        let mut remote = SyncManifest::new(device_b(), agent_id(), ts(0));
        remote.upsert(make_record(
            "obj-1",
            SyncObjectKind::AssetMetadata,
            device_b(),
            "hash-new",
            100,
            MergePolicy::LastWriteWins,
        ));

        let result = merge_manifests(&mut local, &remote);
        assert_eq!(result.updated.len(), 1);
        assert_eq!(result.conflicts.len(), 1);
        assert_eq!(
            result.conflicts[0].resolution,
            ConflictResolution::RemoteWins
        );
        assert_eq!(
            local
                .get(&SyncObjectId::from("obj-1"))
                .unwrap()
                .version_hash,
            "hash-new"
        );
    }

    #[test]
    fn merge_lww_picks_local_when_newer() {
        let mut local = SyncManifest::new(device_a(), agent_id(), ts(0));
        local.upsert(make_record(
            "obj-1",
            SyncObjectKind::AssetMetadata,
            device_a(),
            "hash-new",
            100,
            MergePolicy::LastWriteWins,
        ));

        let mut remote = SyncManifest::new(device_b(), agent_id(), ts(0));
        remote.upsert(make_record(
            "obj-1",
            SyncObjectKind::AssetMetadata,
            device_b(),
            "hash-old",
            0,
            MergePolicy::LastWriteWins,
        ));

        let result = merge_manifests(&mut local, &remote);
        assert_eq!(result.skipped.len(), 1);
        assert_eq!(result.conflicts.len(), 1);
        assert_eq!(
            result.conflicts[0].resolution,
            ConflictResolution::LocalWins
        );
        assert_eq!(
            local
                .get(&SyncObjectId::from("obj-1"))
                .unwrap()
                .version_hash,
            "hash-new"
        );
    }

    #[test]
    fn merge_lww_same_timestamp_uses_device_id_order() {
        let mut local = SyncManifest::new(device_a(), agent_id(), ts(0));
        local.upsert(make_record(
            "obj-1",
            SyncObjectKind::AssetMetadata,
            device_a(),
            "hash-a",
            0,
            MergePolicy::LastWriteWins,
        ));

        let mut remote = SyncManifest::new(device_b(), agent_id(), ts(0));
        remote.upsert(make_record(
            "obj-1",
            SyncObjectKind::AssetMetadata,
            device_b(),
            "hash-b",
            0,
            MergePolicy::LastWriteWins,
        ));

        let result = merge_manifests(&mut local, &remote);
        assert_eq!(result.conflicts.len(), 1);
        // device-b > device-a lexicographically → remote wins
        assert_eq!(
            result.conflicts[0].resolution,
            ConflictResolution::RemoteWins
        );
    }

    #[test]
    fn merge_append_only_deduplicates_events() {
        let mut local = SyncManifest::new(device_a(), agent_id(), ts(0));
        local.upsert(SyncRecord {
            object_id: SyncObjectId::from("journal-1"),
            kind: SyncObjectKind::ConversationMetadata,
            source_device: device_a(),
            version_hash: "hash-a".to_string(),
            updated_at: ts(0),
            merge_policy: MergePolicy::AppendOnly,
            payload: serde_json::json!([
                {"event_id": "evt-1", "data": "a"},
                {"event_id": "evt-2", "data": "b"}
            ]),
        });

        let mut remote = SyncManifest::new(device_b(), agent_id(), ts(0));
        remote.upsert(SyncRecord {
            object_id: SyncObjectId::from("journal-1"),
            kind: SyncObjectKind::ConversationMetadata,
            source_device: device_b(),
            version_hash: "hash-b".to_string(),
            updated_at: ts(10),
            merge_policy: MergePolicy::AppendOnly,
            payload: serde_json::json!([
                {"event_id": "evt-2", "data": "b"},
                {"event_id": "evt-3", "data": "c"}
            ]),
        });

        let result = merge_manifests(&mut local, &remote);
        assert_eq!(result.updated.len(), 1);
        assert_eq!(result.conflicts.len(), 1);
        assert_eq!(result.conflicts[0].resolution, ConflictResolution::Merged);

        // Check merged payload has 3 unique events
        let merged_record = local.get(&SyncObjectId::from("journal-1")).unwrap();
        let events = merged_record.payload.as_array().unwrap();
        assert_eq!(events.len(), 3);

        let event_ids: Vec<&str> = events
            .iter()
            .map(|e| e["event_id"].as_str().unwrap())
            .collect();
        assert!(event_ids.contains(&"evt-1"));
        assert!(event_ids.contains(&"evt-2"));
        assert!(event_ids.contains(&"evt-3"));
    }

    #[test]
    fn merge_result_total_changes() {
        let result = MergeResult {
            added: vec![SyncObjectId::from("a")],
            updated: vec![SyncObjectId::from("b"), SyncObjectId::from("c")],
            skipped: vec![],
            conflicts: vec![],
        };
        assert_eq!(result.total_changes(), 3);
    }

    #[test]
    fn merge_full_convergence_scenario() {
        // Device A has obj-1, obj-2
        let mut manifest_a = SyncManifest::new(device_a(), agent_id(), ts(0));
        manifest_a.upsert(make_record(
            "obj-1",
            SyncObjectKind::AssetMetadata,
            device_a(),
            "hash-1",
            0,
            MergePolicy::LastWriteWins,
        ));
        manifest_a.upsert(make_record(
            "obj-2",
            SyncObjectKind::PersonProfile,
            device_a(),
            "hash-2",
            0,
            MergePolicy::LastWriteWins,
        ));

        // Device B has obj-2 (same), obj-3
        let mut manifest_b = SyncManifest::new(device_b(), agent_id(), ts(0));
        manifest_b.upsert(make_record(
            "obj-2",
            SyncObjectKind::PersonProfile,
            device_a(),
            "hash-2",
            0,
            MergePolicy::LastWriteWins,
        ));
        manifest_b.upsert(make_record(
            "obj-3",
            SyncObjectKind::RelationshipProfile,
            device_b(),
            "hash-3",
            0,
            MergePolicy::LastWriteWins,
        ));

        // Merge B into A
        let result_a = merge_manifests(&mut manifest_a, &manifest_b);
        assert_eq!(result_a.added.len(), 1); // obj-3
        assert_eq!(result_a.skipped.len(), 1); // obj-2

        // Merge A into B
        let result_b = merge_manifests(&mut manifest_b, &manifest_a);
        assert_eq!(result_b.added.len(), 1); // obj-1 (now in A from merge above, still new to B)

        // Both should now have 3 objects
        assert_eq!(manifest_a.len(), 3);
        assert_eq!(manifest_b.len(), 3);
    }

    // -----------------------------------------------------------------------
    // PR 151: Sync object store
    // -----------------------------------------------------------------------

    #[test]
    fn compute_hash_deterministic() {
        let content = b"hello world";
        let hash1 = compute_hash(content);
        let hash2 = compute_hash(content);
        assert_eq!(hash1, hash2);
    }

    #[test]
    fn compute_hash_different_content() {
        let hash1 = compute_hash(b"hello");
        let hash2 = compute_hash(b"world");
        assert_ne!(hash1, hash2);
    }

    #[test]
    fn sync_object_new_and_verify() {
        let obj = SyncObject::new(
            SyncObjectId::from("obj-1"),
            SyncObjectKind::PersonalKnowledgeEntry,
            b"content".to_vec(),
            DeviceId::from("device-1"),
        );

        assert_eq!(obj.metadata.object_id.0, "obj-1");
        assert_eq!(obj.metadata.size_bytes, 7);
        assert!(obj.verify_hash());
    }

    #[test]
    fn sync_object_verify_fails_on_tamper() {
        let mut obj = SyncObject::new(
            SyncObjectId::from("obj-1"),
            SyncObjectKind::PersonalKnowledgeEntry,
            b"content".to_vec(),
            DeviceId::from("device-1"),
        );

        assert!(obj.verify_hash());

        // Tamper with content
        obj.content = b"tampered".to_vec();
        assert!(!obj.verify_hash());
    }

    #[tokio::test]
    async fn memory_store_put_and_get() {
        let store = MemorySyncObjectStore::new();
        let obj = SyncObject::new(
            SyncObjectId::from("obj-1"),
            SyncObjectKind::PersonalKnowledgeEntry,
            b"content".to_vec(),
            DeviceId::from("device-1"),
        );

        store.put(&obj).await.unwrap();
        let retrieved = store.get(&SyncObjectId::from("obj-1")).await.unwrap();
        assert_eq!(retrieved.metadata.object_id.0, "obj-1");
        assert_eq!(retrieved.content, b"content");
    }

    #[tokio::test]
    async fn memory_store_get_not_found() {
        let store = MemorySyncObjectStore::new();
        let result = store.get(&SyncObjectId::from("nonexistent")).await;
        assert!(matches!(result, Err(SyncObjectStoreError::NotFound(_))));
    }

    #[tokio::test]
    async fn memory_store_get_metadata() {
        let store = MemorySyncObjectStore::new();
        let obj = SyncObject::new(
            SyncObjectId::from("obj-1"),
            SyncObjectKind::PersonalKnowledgeEntry,
            b"content".to_vec(),
            DeviceId::from("device-1"),
        );

        store.put(&obj).await.unwrap();
        let metadata = store
            .get_metadata(&SyncObjectId::from("obj-1"))
            .await
            .unwrap();
        assert_eq!(metadata.object_id.0, "obj-1");
        assert_eq!(metadata.size_bytes, 7);
    }

    #[tokio::test]
    async fn memory_store_delete() {
        let store = MemorySyncObjectStore::new();
        let obj = SyncObject::new(
            SyncObjectId::from("obj-1"),
            SyncObjectKind::PersonalKnowledgeEntry,
            b"content".to_vec(),
            DeviceId::from("device-1"),
        );

        store.put(&obj).await.unwrap();
        store.delete(&SyncObjectId::from("obj-1")).await.unwrap();

        let result = store.get(&SyncObjectId::from("obj-1")).await;
        assert!(matches!(result, Err(SyncObjectStoreError::NotFound(_))));
    }

    #[tokio::test]
    async fn memory_store_list() {
        let store = MemorySyncObjectStore::new();

        store
            .put(&SyncObject::new(
                SyncObjectId::from("obj-1"),
                SyncObjectKind::PersonalKnowledgeEntry,
                b"content1".to_vec(),
                DeviceId::from("device-1"),
            ))
            .await
            .unwrap();

        store
            .put(&SyncObject::new(
                SyncObjectId::from("obj-2"),
                SyncObjectKind::AssetMetadata,
                b"content2".to_vec(),
                DeviceId::from("device-1"),
            ))
            .await
            .unwrap();

        let list = store.list().await.unwrap();
        assert_eq!(list.len(), 2);
    }

    #[tokio::test]
    async fn memory_store_contains() {
        let store = MemorySyncObjectStore::new();

        assert!(!store.contains(&SyncObjectId::from("obj-1")).await.unwrap());

        store
            .put(&SyncObject::new(
                SyncObjectId::from("obj-1"),
                SyncObjectKind::PersonalKnowledgeEntry,
                b"content".to_vec(),
                DeviceId::from("device-1"),
            ))
            .await
            .unwrap();

        assert!(store.contains(&SyncObjectId::from("obj-1")).await.unwrap());
    }

    #[tokio::test]
    async fn memory_store_verify() {
        let store = MemorySyncObjectStore::new();

        store
            .put(&SyncObject::new(
                SyncObjectId::from("obj-1"),
                SyncObjectKind::PersonalKnowledgeEntry,
                b"content".to_vec(),
                DeviceId::from("device-1"),
            ))
            .await
            .unwrap();

        assert!(store.verify(&SyncObjectId::from("obj-1")).await.unwrap());
    }

    #[tokio::test]
    async fn memory_store_overwrite() {
        let store = MemorySyncObjectStore::new();

        store
            .put(&SyncObject::new(
                SyncObjectId::from("obj-1"),
                SyncObjectKind::PersonalKnowledgeEntry,
                b"original".to_vec(),
                DeviceId::from("device-1"),
            ))
            .await
            .unwrap();

        store
            .put(&SyncObject::new(
                SyncObjectId::from("obj-1"),
                SyncObjectKind::PersonalKnowledgeEntry,
                b"updated".to_vec(),
                DeviceId::from("device-2"),
            ))
            .await
            .unwrap();

        let obj = store.get(&SyncObjectId::from("obj-1")).await.unwrap();
        assert_eq!(obj.content, b"updated");
        assert_eq!(obj.metadata.source_device.0, "device-2");
    }
}
