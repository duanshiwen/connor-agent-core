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

// ===========================================================================
// Encrypted Sync Transfer
// ===========================================================================

/// A shared session key for encrypting sync payloads.
///
/// In a real implementation, this would use proper cryptographic key derivation.
/// For now, we use a simple key for demonstration.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SharedSessionKey {
    pub key_id: String,
    pub key_bytes: Vec<u8>,
    pub created_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
}

impl SharedSessionKey {
    /// Create a new shared session key.
    pub fn new(key_id: String, key_bytes: Vec<u8>, ttl_seconds: i64) -> Self {
        let now = Utc::now();
        Self {
            key_id,
            key_bytes,
            created_at: now,
            expires_at: now + chrono::Duration::seconds(ttl_seconds),
        }
    }

    /// Check if the key has expired.
    pub fn is_expired(&self) -> bool {
        Utc::now() > self.expires_at
    }

    /// Get the key bytes.
    pub fn bytes(&self) -> &[u8] {
        &self.key_bytes
    }
}

/// An encrypted payload envelope for sync transfer.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EncryptedPayload {
    pub key_id: String,
    pub nonce: Vec<u8>,
    pub ciphertext: Vec<u8>,
    pub tag: Vec<u8>,
    pub content_hash: String,
    pub encrypted_at: DateTime<Utc>,
}

/// Error type for encryption operations.
#[derive(Debug, thiserror::Error)]
pub enum EncryptionError {
    #[error("key expired")]
    KeyExpired,
    #[error("decryption failed: {0}")]
    DecryptionFailed(String),
    #[error("hash mismatch: expected {expected}, got {actual}")]
    HashMismatch { expected: String, actual: String },
    #[error("invalid key: {0}")]
    InvalidKey(String),
    #[error("internal error: {0}")]
    Internal(String),
}

/// Encrypt a sync object using a shared session key.
///
/// In a real implementation, this would use AES-GCM or similar.
/// For now, we use a simple XOR cipher for demonstration.
pub fn encrypt_sync_object(
    key: &SharedSessionKey,
    object: &SyncObject,
) -> Result<EncryptedPayload, EncryptionError> {
    if key.is_expired() {
        return Err(EncryptionError::KeyExpired);
    }

    let content_hash = compute_hash(&object.content);
    let nonce = generate_nonce();

    // Simple XOR encryption (for demonstration only)
    let ciphertext: Vec<u8> = object
        .content
        .iter()
        .zip(key.key_bytes.iter().cycle())
        .map(|(c, k)| c ^ k)
        .collect();

    // Generate a simple tag (in real implementation, this would be a MAC)
    let tag = compute_tag(&ciphertext, &key.key_bytes);

    Ok(EncryptedPayload {
        key_id: key.key_id.clone(),
        nonce,
        ciphertext,
        tag,
        content_hash,
        encrypted_at: Utc::now(),
    })
}

/// Decrypt an encrypted payload using a shared session key.
///
/// Returns the decrypted content if successful.
pub fn decrypt_payload(
    key: &SharedSessionKey,
    payload: &EncryptedPayload,
) -> Result<Vec<u8>, EncryptionError> {
    if key.is_expired() {
        return Err(EncryptionError::KeyExpired);
    }

    if key.key_id != payload.key_id {
        return Err(EncryptionError::InvalidKey("key ID mismatch".to_string()));
    }

    // Verify tag
    let expected_tag = compute_tag(&payload.ciphertext, &key.key_bytes);
    if payload.tag != expected_tag {
        return Err(EncryptionError::DecryptionFailed(
            "tag mismatch".to_string(),
        ));
    }

    // Simple XOR decryption
    let plaintext: Vec<u8> = payload
        .ciphertext
        .iter()
        .zip(key.key_bytes.iter().cycle())
        .map(|(c, k)| c ^ k)
        .collect();

    // Verify content hash
    let actual_hash = compute_hash(&plaintext);
    if actual_hash != payload.content_hash {
        return Err(EncryptionError::HashMismatch {
            expected: payload.content_hash.clone(),
            actual: actual_hash,
        });
    }

    Ok(plaintext)
}

/// Generate a simple nonce (in real implementation, this would be random).
fn generate_nonce() -> Vec<u8> {
    use std::time::{SystemTime, UNIX_EPOCH};
    let duration = SystemTime::now().duration_since(UNIX_EPOCH).unwrap();
    duration.as_nanos().to_be_bytes().to_vec()
}

/// Compute a simple tag for integrity checking.
fn compute_tag(ciphertext: &[u8], key: &[u8]) -> Vec<u8> {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut hasher = DefaultHasher::new();
    ciphertext.hash(&mut hasher);
    key.hash(&mut hasher);
    hasher.finish().to_be_bytes().to_vec()
}

// ===========================================================================
// Sync Transport Boundary
// ===========================================================================

/// Error type for transport operations.
#[derive(Debug, thiserror::Error)]
pub enum TransportError {
    #[error("connection failed: {0}")]
    ConnectionFailed(String),
    #[error("send failed: {0}")]
    SendFailed(String),
    #[error("receive failed: {0}")]
    ReceiveFailed(String),
    #[error("discovery failed: {0}")]
    DiscoveryFailed(String),
    #[error("signaling failed: {0}")]
    SignalingFailed(String),
    #[error("timeout")]
    Timeout,
    #[error("internal error: {0}")]
    Internal(String),
}

/// Transport message types.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum TransportMessage {
    /// Sync object transfer.
    SyncObject(EncryptedPayload),
    /// Manifest exchange.
    Manifest(SyncManifest),
    /// Discovery announcement.
    DiscoveryAnnouncement {
        device_id: DeviceId,
        agent_os_id: String,
        capabilities: Vec<String>,
    },
    /// Discovery response.
    DiscoveryResponse {
        device_id: DeviceId,
        agent_os_id: String,
        accepted: bool,
    },
    /// WebRTC signaling: offer.
    SignalingOffer {
        from_device: DeviceId,
        to_device: DeviceId,
        sdp: String,
    },
    /// WebRTC signaling: answer.
    SignalingAnswer {
        from_device: DeviceId,
        to_device: DeviceId,
        sdp: String,
    },
    /// WebRTC signaling: ICE candidate.
    SignalingIceCandidate {
        from_device: DeviceId,
        to_device: DeviceId,
        candidate: String,
    },
}

/// Trait for sync transports.
#[async_trait::async_trait]
pub trait SyncTransport: Send + Sync {
    /// Send a message to a specific device.
    async fn send(&self, to: &DeviceId, message: TransportMessage) -> Result<(), TransportError>;

    /// Receive a message (blocking).
    async fn receive(&self) -> Result<(DeviceId, TransportMessage), TransportError>;

    /// Check if connected to a device.
    fn is_connected(&self, device_id: &DeviceId) -> bool;

    /// Get list of connected devices.
    fn connected_devices(&self) -> Vec<DeviceId>;

    /// Disconnect from a device.
    async fn disconnect(&self, device_id: &DeviceId) -> Result<(), TransportError>;
}

/// Trait for LAN discovery.
#[async_trait::async_trait]
pub trait LandDiscovery: Send + Sync {
    /// Announce presence on the LAN.
    async fn announce(&self) -> Result<(), TransportError>;

    /// Listen for discovery announcements.
    async fn listen(&self) -> Result<Vec<DiscoveryAnnouncement>, TransportError>;

    /// Respond to a discovery announcement.
    async fn respond(&self, to: &DeviceId, accepted: bool) -> Result<(), TransportError>;
}

/// A discovered device on the LAN.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DiscoveryAnnouncement {
    pub device_id: DeviceId,
    pub agent_os_id: String,
    pub capabilities: Vec<String>,
    pub discovered_at: DateTime<Utc>,
}

/// Trait for WebRTC signaling.
#[async_trait::async_trait]
pub trait WebRtcSignaling: Send + Sync {
    /// Send an SDP offer.
    async fn send_offer(&self, to: &DeviceId, sdp: String) -> Result<(), TransportError>;

    /// Send an SDP answer.
    async fn send_answer(&self, to: &DeviceId, sdp: String) -> Result<(), TransportError>;

    /// Send an ICE candidate.
    async fn send_ice_candidate(
        &self,
        to: &DeviceId,
        candidate: String,
    ) -> Result<(), TransportError>;

    /// Receive signaling messages.
    async fn receive_signaling(&self) -> Result<TransportMessage, TransportError>;
}

/// Memory-based transport for testing.
pub struct MemoryTransport {
    inbox: std::sync::Arc<std::sync::Mutex<Vec<(DeviceId, TransportMessage)>>>,
    connected: std::sync::Arc<std::sync::Mutex<std::collections::HashSet<DeviceId>>>,
}

impl MemoryTransport {
    pub fn new() -> Self {
        Self {
            inbox: std::sync::Arc::new(std::sync::Mutex::new(Vec::new())),
            connected: std::sync::Arc::new(std::sync::Mutex::new(std::collections::HashSet::new())),
        }
    }

    /// Simulate receiving a message (for testing).
    pub fn inject_message(&self, from: DeviceId, message: TransportMessage) {
        self.inbox.lock().unwrap().push((from, message));
    }

    /// Simulate connecting to a device.
    pub fn simulate_connect(&self, device_id: DeviceId) {
        self.connected.lock().unwrap().insert(device_id);
    }
}

impl Default for MemoryTransport {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait]
impl SyncTransport for MemoryTransport {
    async fn send(&self, to: &DeviceId, message: TransportMessage) -> Result<(), TransportError> {
        if !self.is_connected(to) {
            return Err(TransportError::ConnectionFailed(format!(
                "not connected to {}",
                to
            )));
        }
        // In memory transport, we just add to inbox
        self.inbox.lock().unwrap().push((to.clone(), message));
        Ok(())
    }

    async fn receive(&self) -> Result<(DeviceId, TransportMessage), TransportError> {
        let mut inbox = self.inbox.lock().unwrap();
        if inbox.is_empty() {
            return Err(TransportError::Timeout);
        }
        Ok(inbox.remove(0))
    }

    fn is_connected(&self, device_id: &DeviceId) -> bool {
        self.connected.lock().unwrap().contains(device_id)
    }

    fn connected_devices(&self) -> Vec<DeviceId> {
        self.connected.lock().unwrap().iter().cloned().collect()
    }

    async fn disconnect(&self, device_id: &DeviceId) -> Result<(), TransportError> {
        self.connected.lock().unwrap().remove(device_id);
        Ok(())
    }
}

// ===========================================================================
// Enterprise Data Sync Exclusion
// ===========================================================================

/// Data ownership type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum DataOwnership {
    /// Data owned by the personal user.
    Personal,
    /// Data owned by an enterprise organization.
    Enterprise,
}

impl fmt::Display for DataOwnership {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DataOwnership::Personal => write!(f, "personal"),
            DataOwnership::Enterprise => write!(f, "enterprise"),
        }
    }
}

/// Sync policy for data.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SyncPolicy {
    /// Allow personal data to sync via P2P.
    PersonalOnly,
    /// Allow enterprise data to sync via enterprise sync.
    EnterpriseOnly,
    /// Allow both personal and enterprise data to sync.
    All,
}

impl fmt::Display for SyncPolicy {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SyncPolicy::PersonalOnly => write!(f, "personal_only"),
            SyncPolicy::EnterpriseOnly => write!(f, "enterprise_only"),
            SyncPolicy::All => write!(f, "all"),
        }
    }
}

/// Marker for data ownership on sync objects.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DataOwnershipMarker {
    pub ownership: DataOwnership,
    pub organization_id: Option<String>,
    pub marked_at: DateTime<Utc>,
}

impl DataOwnershipMarker {
    /// Create a personal ownership marker.
    pub fn personal() -> Self {
        Self {
            ownership: DataOwnership::Personal,
            organization_id: None,
            marked_at: Utc::now(),
        }
    }

    /// Create an enterprise ownership marker.
    pub fn enterprise(organization_id: String) -> Self {
        Self {
            ownership: DataOwnership::Enterprise,
            organization_id: Some(organization_id),
            marked_at: Utc::now(),
        }
    }

    /// Check if this is personal data.
    pub fn is_personal(&self) -> bool {
        self.ownership == DataOwnership::Personal
    }

    /// Check if this is enterprise data.
    pub fn is_enterprise(&self) -> bool {
        self.ownership == DataOwnership::Enterprise
    }
}

/// Sync filter for filtering objects based on ownership and policy.
pub struct SyncFilter {
    policy: SyncPolicy,
}

impl SyncFilter {
    /// Create a new sync filter with the given policy.
    pub fn new(policy: SyncPolicy) -> Self {
        Self { policy }
    }

    /// Check if an object should be included in sync.
    pub fn should_include(&self, marker: &DataOwnershipMarker) -> bool {
        match self.policy {
            SyncPolicy::PersonalOnly => marker.is_personal(),
            SyncPolicy::EnterpriseOnly => marker.is_enterprise(),
            SyncPolicy::All => true,
        }
    }

    /// Filter a list of sync objects based on ownership.
    pub fn filter_objects(
        &self,
        objects: &[(SyncObjectId, DataOwnershipMarker)],
    ) -> Vec<SyncObjectId> {
        objects
            .iter()
            .filter(|(_, marker)| self.should_include(marker))
            .map(|(id, _)| id.clone())
            .collect()
    }

    /// Filter a manifest to only include objects that pass the sync policy.
    pub fn filter_manifest(
        &self,
        manifest: &SyncManifest,
        ownership_map: &std::collections::HashMap<SyncObjectId, DataOwnershipMarker>,
    ) -> SyncManifest {
        let mut filtered = SyncManifest::new(
            manifest.device_id.clone(),
            manifest.agent_os_id.clone(),
            manifest.captured_at,
        );

        for (id, record) in &manifest.objects {
            if let Some(marker) = ownership_map.get(id) && self.should_include(marker) {
                filtered.upsert(record.clone());
            }
        }

        filtered
    }
}

/// Extension trait for SyncRecord to add ownership marker.
pub trait SyncRecordExt {
    fn with_ownership(self, marker: DataOwnershipMarker) -> SyncRecordWithOwnership;
}

/// A sync record with ownership information.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SyncRecordWithOwnership {
    pub record: SyncRecord,
    pub ownership: DataOwnershipMarker,
}

impl SyncRecordWithOwnership {
    pub fn new(record: SyncRecord, ownership: DataOwnershipMarker) -> Self {
        Self { record, ownership }
    }
}

impl SyncRecordExt for SyncRecord {
    fn with_ownership(self, marker: DataOwnershipMarker) -> SyncRecordWithOwnership {
        SyncRecordWithOwnership::new(self, marker)
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

    // -----------------------------------------------------------------------
    // PR 152: Encrypted sync transfer
    // -----------------------------------------------------------------------

    #[test]
    fn shared_session_key_new() {
        let key = SharedSessionKey::new(
            "key-1".to_string(),
            vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16],
            3600,
        );

        assert_eq!(key.key_id, "key-1");
        assert_eq!(key.key_bytes.len(), 16);
        assert!(!key.is_expired());
    }

    #[test]
    fn shared_session_key_expired() {
        let key = SharedSessionKey::new(
            "key-1".to_string(),
            vec![1, 2, 3, 4],
            -1, // Already expired
        );

        assert!(key.is_expired());
    }

    #[test]
    fn encrypt_decrypt_roundtrip() {
        let key = SharedSessionKey::new(
            "key-1".to_string(),
            vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16],
            3600,
        );

        let obj = SyncObject::new(
            SyncObjectId::from("obj-1"),
            SyncObjectKind::PersonalKnowledgeEntry,
            b"hello world".to_vec(),
            DeviceId::from("device-1"),
        );

        let encrypted = encrypt_sync_object(&key, &obj).unwrap();
        let decrypted = decrypt_payload(&key, &encrypted).unwrap();

        assert_eq!(decrypted, b"hello world");
    }

    #[test]
    fn encrypt_decrypt_different_content() {
        let key = SharedSessionKey::new(
            "key-1".to_string(),
            vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16],
            3600,
        );

        let obj = SyncObject::new(
            SyncObjectId::from("obj-1"),
            SyncObjectKind::PersonalKnowledgeEntry,
            b"different content".to_vec(),
            DeviceId::from("device-1"),
        );

        let encrypted = encrypt_sync_object(&key, &obj).unwrap();
        let decrypted = decrypt_payload(&key, &encrypted).unwrap();

        assert_eq!(decrypted, b"different content");
    }

    #[test]
    fn decrypt_fails_with_wrong_key() {
        let key1 = SharedSessionKey::new(
            "key-1".to_string(),
            vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16],
            3600,
        );

        let key2 = SharedSessionKey::new(
            "key-2".to_string(),
            vec![16, 15, 14, 13, 12, 11, 10, 9, 8, 7, 6, 5, 4, 3, 2, 1],
            3600,
        );

        let obj = SyncObject::new(
            SyncObjectId::from("obj-1"),
            SyncObjectKind::PersonalKnowledgeEntry,
            b"hello world".to_vec(),
            DeviceId::from("device-1"),
        );

        let encrypted = encrypt_sync_object(&key1, &obj).unwrap();
        let result = decrypt_payload(&key2, &encrypted);

        assert!(matches!(result, Err(EncryptionError::InvalidKey(_))));
    }

    #[test]
    fn decrypt_fails_with_tampered_ciphertext() {
        let key = SharedSessionKey::new(
            "key-1".to_string(),
            vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16],
            3600,
        );

        let obj = SyncObject::new(
            SyncObjectId::from("obj-1"),
            SyncObjectKind::PersonalKnowledgeEntry,
            b"hello world".to_vec(),
            DeviceId::from("device-1"),
        );

        let mut encrypted = encrypt_sync_object(&key, &obj).unwrap();

        // Tamper with ciphertext
        if !encrypted.ciphertext.is_empty() {
            encrypted.ciphertext[0] ^= 0xff;
        }

        let result = decrypt_payload(&key, &encrypted);
        // Should fail with either HashMismatch or DecryptionFailed (tag mismatch)
        assert!(result.is_err());
    }

    #[test]
    fn encrypt_fails_with_expired_key() {
        let key = SharedSessionKey::new(
            "key-1".to_string(),
            vec![1, 2, 3, 4],
            -1, // Already expired
        );

        let obj = SyncObject::new(
            SyncObjectId::from("obj-1"),
            SyncObjectKind::PersonalKnowledgeEntry,
            b"hello world".to_vec(),
            DeviceId::from("device-1"),
        );

        let result = encrypt_sync_object(&key, &obj);
        assert!(matches!(result, Err(EncryptionError::KeyExpired)));
    }

    #[test]
    fn decrypt_fails_with_expired_key() {
        let key = SharedSessionKey::new(
            "key-1".to_string(),
            vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16],
            3600,
        );

        let obj = SyncObject::new(
            SyncObjectId::from("obj-1"),
            SyncObjectKind::PersonalKnowledgeEntry,
            b"hello world".to_vec(),
            DeviceId::from("device-1"),
        );

        let encrypted = encrypt_sync_object(&key, &obj).unwrap();

        // Create an expired key with same ID
        let expired_key = SharedSessionKey::new(
            "key-1".to_string(),
            vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16],
            -1,
        );

        let result = decrypt_payload(&expired_key, &encrypted);
        assert!(matches!(result, Err(EncryptionError::KeyExpired)));
    }

    #[test]
    fn encrypted_payload_fields() {
        let key = SharedSessionKey::new(
            "key-1".to_string(),
            vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16],
            3600,
        );

        let obj = SyncObject::new(
            SyncObjectId::from("obj-1"),
            SyncObjectKind::PersonalKnowledgeEntry,
            b"hello world".to_vec(),
            DeviceId::from("device-1"),
        );

        let encrypted = encrypt_sync_object(&key, &obj).unwrap();

        assert_eq!(encrypted.key_id, "key-1");
        assert!(!encrypted.nonce.is_empty());
        assert!(!encrypted.ciphertext.is_empty());
        assert!(!encrypted.tag.is_empty());
        assert!(!encrypted.content_hash.is_empty());
    }

    // -----------------------------------------------------------------------
    // PR 153: LAN/WebRTC transport boundary
    // -----------------------------------------------------------------------

    #[test]
    fn memory_transport_new() {
        let transport = MemoryTransport::new();
        assert!(transport.connected_devices().is_empty());
    }

    #[test]
    fn memory_transport_connect() {
        let transport = MemoryTransport::new();
        let device = DeviceId::from("device-1");

        transport.simulate_connect(device.clone());
        assert!(transport.is_connected(&device));
        assert_eq!(transport.connected_devices().len(), 1);
    }

    #[test]
    fn memory_transport_disconnect() {
        let transport = MemoryTransport::new();
        let device = DeviceId::from("device-1");

        transport.simulate_connect(device.clone());
        assert!(transport.is_connected(&device));

        // Use block_on for async in sync test
        tokio::runtime::Runtime::new().unwrap().block_on(async {
            transport.disconnect(&device).await.unwrap();
        });

        assert!(!transport.is_connected(&device));
    }

    #[tokio::test]
    async fn memory_transport_send_receive() {
        let transport = MemoryTransport::new();
        let device = DeviceId::from("device-1");

        transport.simulate_connect(device.clone());

        let message = TransportMessage::DiscoveryAnnouncement {
            device_id: DeviceId::from("device-2"),
            agent_os_id: "agent-1".to_string(),
            capabilities: vec!["sync".to_string()],
        };

        transport.send(&device, message.clone()).await.unwrap();
        let (from, received) = transport.receive().await.unwrap();

        assert_eq!(from, device);
        assert_eq!(received, message);
    }

    #[tokio::test]
    async fn memory_transport_send_fails_not_connected() {
        let transport = MemoryTransport::new();
        let device = DeviceId::from("device-1");

        let message = TransportMessage::DiscoveryAnnouncement {
            device_id: DeviceId::from("device-2"),
            agent_os_id: "agent-1".to_string(),
            capabilities: vec!["sync".to_string()],
        };

        let result = transport.send(&device, message).await;
        assert!(matches!(result, Err(TransportError::ConnectionFailed(_))));
    }

    #[tokio::test]
    async fn memory_transport_receive_timeout() {
        let transport = MemoryTransport::new();

        let result = transport.receive().await;
        assert!(matches!(result, Err(TransportError::Timeout)));
    }

    #[tokio::test]
    async fn memory_transport_inject_message() {
        let transport = MemoryTransport::new();
        let device = DeviceId::from("device-1");

        let message = TransportMessage::DiscoveryResponse {
            device_id: DeviceId::from("device-2"),
            agent_os_id: "agent-1".to_string(),
            accepted: true,
        };

        transport.inject_message(device.clone(), message.clone());
        let (from, received) = transport.receive().await.unwrap();

        assert_eq!(from, device);
        assert_eq!(received, message);
    }

    #[test]
    fn transport_message_variants() {
        let discovery = TransportMessage::DiscoveryAnnouncement {
            device_id: DeviceId::from("device-1"),
            agent_os_id: "agent-1".to_string(),
            capabilities: vec!["sync".to_string()],
        };

        let signaling_offer = TransportMessage::SignalingOffer {
            from_device: DeviceId::from("device-1"),
            to_device: DeviceId::from("device-2"),
            sdp: "offer-sdp".to_string(),
        };

        let signaling_answer = TransportMessage::SignalingAnswer {
            from_device: DeviceId::from("device-1"),
            to_device: DeviceId::from("device-2"),
            sdp: "answer-sdp".to_string(),
        };

        let ice_candidate = TransportMessage::SignalingIceCandidate {
            from_device: DeviceId::from("device-1"),
            to_device: DeviceId::from("device-2"),
            candidate: "ice-candidate".to_string(),
        };

        // Verify they can be created and cloned
        let _ = discovery.clone();
        let _ = signaling_offer.clone();
        let _ = signaling_answer.clone();
        let _ = ice_candidate.clone();
    }

    #[test]
    fn discovery_announcement_fields() {
        let announcement = DiscoveryAnnouncement {
            device_id: DeviceId::from("device-1"),
            agent_os_id: "agent-1".to_string(),
            capabilities: vec!["sync".to_string(), "calendar".to_string()],
            discovered_at: Utc::now(),
        };

        assert_eq!(announcement.device_id.0, "device-1");
        assert_eq!(announcement.agent_os_id, "agent-1");
        assert_eq!(announcement.capabilities.len(), 2);
    }

    // -----------------------------------------------------------------------
    // PR 154: Enterprise data sync exclusion
    // -----------------------------------------------------------------------

    #[test]
    fn data_ownership_personal() {
        let marker = DataOwnershipMarker::personal();
        assert!(marker.is_personal());
        assert!(!marker.is_enterprise());
        assert_eq!(marker.ownership, DataOwnership::Personal);
        assert!(marker.organization_id.is_none());
    }

    #[test]
    fn data_ownership_enterprise() {
        let marker = DataOwnershipMarker::enterprise("org-1".to_string());
        assert!(!marker.is_personal());
        assert!(marker.is_enterprise());
        assert_eq!(marker.ownership, DataOwnership::Enterprise);
        assert_eq!(marker.organization_id.as_deref(), Some("org-1"));
    }

    #[test]
    fn data_ownership_display() {
        assert_eq!(DataOwnership::Personal.to_string(), "personal");
        assert_eq!(DataOwnership::Enterprise.to_string(), "enterprise");
    }

    #[test]
    fn sync_policy_display() {
        assert_eq!(SyncPolicy::PersonalOnly.to_string(), "personal_only");
        assert_eq!(SyncPolicy::EnterpriseOnly.to_string(), "enterprise_only");
        assert_eq!(SyncPolicy::All.to_string(), "all");
    }

    #[test]
    fn sync_filter_personal_only() {
        let filter = SyncFilter::new(SyncPolicy::PersonalOnly);

        let personal = DataOwnershipMarker::personal();
        let enterprise = DataOwnershipMarker::enterprise("org-1".to_string());

        assert!(filter.should_include(&personal));
        assert!(!filter.should_include(&enterprise));
    }

    #[test]
    fn sync_filter_enterprise_only() {
        let filter = SyncFilter::new(SyncPolicy::EnterpriseOnly);

        let personal = DataOwnershipMarker::personal();
        let enterprise = DataOwnershipMarker::enterprise("org-1".to_string());

        assert!(!filter.should_include(&personal));
        assert!(filter.should_include(&enterprise));
    }

    #[test]
    fn sync_filter_all() {
        let filter = SyncFilter::new(SyncPolicy::All);

        let personal = DataOwnershipMarker::personal();
        let enterprise = DataOwnershipMarker::enterprise("org-1".to_string());

        assert!(filter.should_include(&personal));
        assert!(filter.should_include(&enterprise));
    }

    #[test]
    fn sync_filter_objects() {
        let filter = SyncFilter::new(SyncPolicy::PersonalOnly);

        let objects = vec![
            (SyncObjectId::from("obj-1"), DataOwnershipMarker::personal()),
            (
                SyncObjectId::from("obj-2"),
                DataOwnershipMarker::enterprise("org-1".to_string()),
            ),
            (SyncObjectId::from("obj-3"), DataOwnershipMarker::personal()),
        ];

        let filtered = filter.filter_objects(&objects);
        assert_eq!(filtered.len(), 2);
        assert!(filtered.contains(&SyncObjectId::from("obj-1")));
        assert!(filtered.contains(&SyncObjectId::from("obj-3")));
    }

    #[test]
    fn sync_filter_manifest() {
        let filter = SyncFilter::new(SyncPolicy::PersonalOnly);

        let mut manifest =
            SyncManifest::new(DeviceId::from("device-1"), "agent-1".to_string(), ts(0));

        manifest.upsert(SyncRecord {
            object_id: SyncObjectId::from("obj-1"),
            kind: SyncObjectKind::PersonalKnowledgeEntry,
            source_device: DeviceId::from("device-1"),
            version_hash: "hash1".to_string(),
            updated_at: ts(0),
            merge_policy: MergePolicy::LastWriteWins,
            payload: serde_json::json!(null),
        });

        manifest.upsert(SyncRecord {
            object_id: SyncObjectId::from("obj-2"),
            kind: SyncObjectKind::AssetMetadata,
            source_device: DeviceId::from("device-1"),
            version_hash: "hash2".to_string(),
            updated_at: ts(0),
            merge_policy: MergePolicy::LastWriteWins,
            payload: serde_json::json!(null),
        });

        let mut ownership_map = std::collections::HashMap::new();
        ownership_map.insert(SyncObjectId::from("obj-1"), DataOwnershipMarker::personal());
        ownership_map.insert(
            SyncObjectId::from("obj-2"),
            DataOwnershipMarker::enterprise("org-1".to_string()),
        );

        let filtered = filter.filter_manifest(&manifest, &ownership_map);
        assert_eq!(filtered.len(), 1);
        assert!(filtered.get(&SyncObjectId::from("obj-1")).is_some());
        assert!(filtered.get(&SyncObjectId::from("obj-2")).is_none());
    }

    #[test]
    fn sync_record_with_ownership() {
        let record = SyncRecord {
            object_id: SyncObjectId::from("obj-1"),
            kind: SyncObjectKind::PersonalKnowledgeEntry,
            source_device: DeviceId::from("device-1"),
            version_hash: "hash1".to_string(),
            updated_at: ts(0),
            merge_policy: MergePolicy::LastWriteWins,
            payload: serde_json::json!(null),
        };

        let marker = DataOwnershipMarker::personal();
        let with_ownership = record.with_ownership(marker);

        assert_eq!(with_ownership.record.object_id.0, "obj-1");
        assert!(with_ownership.ownership.is_personal());
    }

    #[test]
    fn enterprise_owned_object_excluded_from_personal_manifest() {
        // This is the key test: enterprise-owned objects should not enter personal P2P manifest
        let filter = SyncFilter::new(SyncPolicy::PersonalOnly);

        let mut manifest =
            SyncManifest::new(DeviceId::from("device-1"), "agent-1".to_string(), ts(0));

        // Add personal object
        manifest.upsert(SyncRecord {
            object_id: SyncObjectId::from("personal-obj"),
            kind: SyncObjectKind::PersonalKnowledgeEntry,
            source_device: DeviceId::from("device-1"),
            version_hash: "hash1".to_string(),
            updated_at: ts(0),
            merge_policy: MergePolicy::LastWriteWins,
            payload: serde_json::json!(null),
        });

        // Add enterprise object
        manifest.upsert(SyncRecord {
            object_id: SyncObjectId::from("enterprise-obj"),
            kind: SyncObjectKind::AssetMetadata,
            source_device: DeviceId::from("device-1"),
            version_hash: "hash2".to_string(),
            updated_at: ts(0),
            merge_policy: MergePolicy::LastWriteWins,
            payload: serde_json::json!(null),
        });

        let mut ownership_map = std::collections::HashMap::new();
        ownership_map.insert(
            SyncObjectId::from("personal-obj"),
            DataOwnershipMarker::personal(),
        );
        ownership_map.insert(
            SyncObjectId::from("enterprise-obj"),
            DataOwnershipMarker::enterprise("org-1".to_string()),
        );

        let filtered = filter.filter_manifest(&manifest, &ownership_map);

        // Only personal object should be in the filtered manifest
        assert_eq!(filtered.len(), 1);
        assert!(filtered.get(&SyncObjectId::from("personal-obj")).is_some());
        assert!(
            filtered
                .get(&SyncObjectId::from("enterprise-obj"))
                .is_none()
        );
    }
}
