//! # P2P Sync Runtime
//!
//! Orchestration and transport for AgentOS multi-device synchronization.
//!
//! This crate provides the runtime that coordinates sync between trusted devices:
//! - `SyncTransport` trait for pluggable transport (fake → mDNS → QUIC/WebRTC)
//! - `MemorySyncTransport` for testing
//! - `P2pSyncOrchestrator` that drives the full sync protocol:
//!   1. Verify same AgentOsId
//!   2. Exchange manifests
//!   3. Compute diff
//!   4. Exchange missing objects
//!   5. Merge and converge

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use device_pairing_core::{DeviceId, DeviceTrustStore};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt;
use std::sync::{Arc, Mutex};
use sync_runtime::{
    ManifestDiff, MergeResult, SyncManifest, SyncObjectId, SyncRecord, diff_manifests,
    merge_manifests,
};

// ---------------------------------------------------------------------------
// Transport trait
// ---------------------------------------------------------------------------

/// Message exchanged between devices during sync.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SyncMessage {
    /// Request manifests from peer.
    ManifestRequest {
        from_device: DeviceId,
        agent_os_id: String,
    },
    /// Response with the device's manifest.
    ManifestResponse {
        from_device: DeviceId,
        manifest: SyncManifest,
    },
    /// Request specific objects by id.
    ObjectRequest {
        from_device: DeviceId,
        object_ids: Vec<SyncObjectId>,
    },
    /// Response with requested objects.
    ObjectResponse {
        from_device: DeviceId,
        objects: Vec<SyncRecord>,
    },
    /// Acknowledge sync completion.
    SyncAck {
        from_device: DeviceId,
        merged_count: usize,
    },
}

/// Transport error.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum TransportError {
    #[error("device not reachable: {0}")]
    DeviceNotReachable(String),
    #[error("message too large: {bytes} bytes")]
    MessageTooLarge { bytes: usize },
    #[error("timeout after {ms}ms")]
    Timeout { ms: u64 },
    #[error("serialization error: {0}")]
    Serialization(String),
    #[error("connection closed")]
    ConnectionClosed,
}

/// Async transport for sending sync messages between devices.
#[async_trait]
pub trait SyncTransport: Send + Sync + std::fmt::Debug {
    /// Send a message to a device and receive a response.
    async fn send(
        &self,
        to: &DeviceId,
        message: SyncMessage,
    ) -> Result<SyncMessage, TransportError>;

    /// Check if a device is currently reachable.
    async fn is_reachable(&self, device: &DeviceId) -> bool;
}

// ---------------------------------------------------------------------------
// MemorySyncTransport (for testing)
// ---------------------------------------------------------------------------

/// In-memory transport that simulates device-to-device messaging.
///
/// Devices are registered with handlers. When device A sends to device B,
/// the handler for B processes the message and returns a response.
#[derive(Clone)]
pub struct MemorySyncTransport {
    /// Maps device id → list of messages it has received.
    inbox: Arc<Mutex<HashMap<DeviceId, Vec<SyncMessage>>>>,
    /// Maps device id → its manifest (for simulating responses).
    manifests: Arc<Mutex<HashMap<DeviceId, SyncManifest>>>,
    /// Maps device id → its sync records (objects).
    records: Arc<Mutex<HashMap<DeviceId, HashMap<SyncObjectId, SyncRecord>>>>,
    /// Set of reachable devices.
    reachable: Arc<Mutex<HashMap<DeviceId, bool>>>,
}

impl fmt::Debug for MemorySyncTransport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("MemorySyncTransport")
            .field("devices", &self.reachable.lock().unwrap().len())
            .finish()
    }
}

impl MemorySyncTransport {
    pub fn new() -> Self {
        Self {
            inbox: Arc::new(Mutex::new(HashMap::new())),
            manifests: Arc::new(Mutex::new(HashMap::new())),
            records: Arc::new(Mutex::new(HashMap::new())),
            reachable: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Register a device with its manifest and records.
    pub fn register_device(
        &self,
        device_id: DeviceId,
        manifest: SyncManifest,
        records: HashMap<SyncObjectId, SyncRecord>,
    ) {
        self.manifests
            .lock()
            .unwrap()
            .insert(device_id.clone(), manifest);
        self.records
            .lock()
            .unwrap()
            .insert(device_id.clone(), records);
        self.reachable.lock().unwrap().insert(device_id, true);
    }

    /// Set whether a device is reachable.
    pub fn set_reachable(&self, device_id: &DeviceId, reachable: bool) {
        self.reachable
            .lock()
            .unwrap()
            .insert(device_id.clone(), reachable);
    }

    /// Get messages received by a device.
    pub fn get_inbox(&self, device_id: &DeviceId) -> Vec<SyncMessage> {
        self.inbox
            .lock()
            .unwrap()
            .get(device_id)
            .cloned()
            .unwrap_or_default()
    }
}

impl Default for MemorySyncTransport {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl SyncTransport for MemorySyncTransport {
    async fn send(
        &self,
        to: &DeviceId,
        message: SyncMessage,
    ) -> Result<SyncMessage, TransportError> {
        // Check reachability
        if !self.is_reachable(to).await {
            return Err(TransportError::DeviceNotReachable(to.0.clone()));
        }

        // Record the incoming message
        {
            let mut inbox = self.inbox.lock().unwrap();
            inbox.entry(to.clone()).or_default().push(message.clone());
        }

        // Generate response based on message type
        match message {
            SyncMessage::ManifestRequest { .. } => {
                let manifest = self
                    .manifests
                    .lock()
                    .unwrap()
                    .get(to)
                    .cloned()
                    .unwrap_or_else(|| SyncManifest::new(to.clone(), String::new(), Utc::now()));

                Ok(SyncMessage::ManifestResponse {
                    from_device: to.clone(),
                    manifest,
                })
            }
            SyncMessage::ObjectRequest { object_ids, .. } => {
                let all_records = self.records.lock().unwrap();
                let device_records = all_records.get(to).cloned().unwrap_or_default();
                let requested: Vec<SyncRecord> = object_ids
                    .iter()
                    .filter_map(|id| device_records.get(id).cloned())
                    .collect();

                Ok(SyncMessage::ObjectResponse {
                    from_device: to.clone(),
                    objects: requested,
                })
            }
            _ => Err(TransportError::Serialization(
                "unexpected message type".to_string(),
            )),
        }
    }

    async fn is_reachable(&self, device: &DeviceId) -> bool {
        self.reachable
            .lock()
            .unwrap()
            .get(device)
            .copied()
            .unwrap_or(false)
    }
}

// ---------------------------------------------------------------------------
// Sync Error
// ---------------------------------------------------------------------------

/// Errors from the sync orchestration.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SyncError {
    #[error("agent OS id mismatch: local={local}, remote={remote}")]
    AgentOsIdMismatch { local: String, remote: String },
    #[error("device not trusted: {0}")]
    DeviceNotTrusted(String),
    #[error("transport error: {0}")]
    Transport(#[from] TransportError),
    #[error("sync already in progress")]
    AlreadyInProgress,
}

// ---------------------------------------------------------------------------
// Sync Outcome
// ---------------------------------------------------------------------------

/// Result of a complete sync session.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SyncOutcome {
    pub peer_device: DeviceId,
    pub diff: ManifestDiff,
    pub merge_result: MergeResult,
    pub objects_received: usize,
    pub completed_at: DateTime<Utc>,
}

// ---------------------------------------------------------------------------
// P2pSyncOrchestrator
// ---------------------------------------------------------------------------

/// Orchestrates a full P2P sync session between two trusted devices.
///
/// Protocol:
/// 1. Verify both devices share the same AgentOsId
/// 2. Exchange manifests via transport
/// 3. Compute diff
/// 4. Request missing/conflicting objects from peer
/// 5. Merge into local manifest
/// 6. Send ack
pub struct P2pSyncOrchestrator {
    local_device: DeviceId,
    agent_os_id: String,
    trust_store: DeviceTrustStore,
}

impl P2pSyncOrchestrator {
    pub fn new(local_device: DeviceId, agent_os_id: String, trust_store: DeviceTrustStore) -> Self {
        Self {
            local_device,
            agent_os_id,
            trust_store,
        }
    }

    /// Run a full sync session with a peer device.
    ///
    /// Returns `SyncOutcome` with details of what was synced.
    pub async fn sync(
        self,
        transport: &dyn SyncTransport,
        local_manifest: &mut SyncManifest,
        peer_device: &DeviceId,
    ) -> Result<SyncOutcome, SyncError> {
        // Step 1: Verify trust
        if !self.trust_store.is_trusted(peer_device) {
            return Err(SyncError::DeviceNotTrusted(peer_device.0.clone()));
        }

        // Step 2: Exchange manifests
        let response = transport
            .send(
                peer_device,
                SyncMessage::ManifestRequest {
                    from_device: self.local_device.clone(),
                    agent_os_id: self.agent_os_id.clone(),
                },
            )
            .await?;

        let remote_manifest = match response {
            SyncMessage::ManifestResponse { manifest, .. } => manifest,
            _ => {
                return Err(SyncError::Transport(TransportError::Serialization(
                    "expected manifest response".to_string(),
                )));
            }
        };

        // Step 2b: Verify same AgentOsId
        if remote_manifest.agent_os_id != self.agent_os_id {
            return Err(SyncError::AgentOsIdMismatch {
                local: self.agent_os_id.clone(),
                remote: remote_manifest.agent_os_id.clone(),
            });
        }

        // Step 3: Compute diff
        let diff = diff_manifests(local_manifest, &remote_manifest);

        // Step 4: Request missing/conflicting objects
        let mut needed_ids: Vec<SyncObjectId> = Vec::new();
        needed_ids.extend(diff.remote_only.clone());
        needed_ids.extend(diff.conflicting.clone());

        let objects_received = if !needed_ids.is_empty() {
            let obj_response = transport
                .send(
                    peer_device,
                    SyncMessage::ObjectRequest {
                        from_device: self.local_device.clone(),
                        object_ids: needed_ids,
                    },
                )
                .await?;

            match obj_response {
                SyncMessage::ObjectResponse { objects, .. } => {
                    // Merge received objects into local manifest
                    for record in objects {
                        local_manifest.upsert(record);
                    }
                    local_manifest.objects.len()
                }
                _ => {
                    return Err(SyncError::Transport(TransportError::Serialization(
                        "expected object response".to_string(),
                    )));
                }
            }
        } else {
            0
        };

        // Step 5: Merge manifests (handles LWW/AppendOnly for conflicts)
        let merge_result = merge_manifests(local_manifest, &remote_manifest);

        // Step 6: Send ack
        let _ = transport
            .send(
                peer_device,
                SyncMessage::SyncAck {
                    from_device: self.local_device.clone(),
                    merged_count: merge_result.total_changes(),
                },
            )
            .await;

        Ok(SyncOutcome {
            peer_device: peer_device.clone(),
            diff,
            merge_result,
            objects_received,
            completed_at: Utc::now(),
        })
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use sync_runtime::{MergePolicy, SyncObjectKind};

    fn ts(offset: i64) -> DateTime<Utc> {
        "2026-05-25T12:00:00Z".parse::<DateTime<Utc>>().unwrap() + chrono::Duration::seconds(offset)
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

    fn setup_trust_store() -> DeviceTrustStore {
        let store = DeviceTrustStore::new();
        store
            .upsert_trust(device_pairing_core::DeviceTrustRecord {
                device_id: device_a(),
                peer_device_id: device_b(),
                trust_level: TrustLevel::Full,
                paired_at: ts(0),
                last_verified_at: None,
                pairing_session_id: "s1".to_string(),
            })
            .unwrap();
        store
            .upsert_trust(device_pairing_core::DeviceTrustRecord {
                device_id: device_b(),
                peer_device_id: device_a(),
                trust_level: TrustLevel::Full,
                paired_at: ts(0),
                last_verified_at: None,
                pairing_session_id: "s1".to_string(),
            })
            .unwrap();
        store
    }

    fn make_record(id: &str, device: DeviceId, hash: &str) -> SyncRecord {
        SyncRecord {
            object_id: SyncObjectId::from(id),
            kind: SyncObjectKind::AssetMetadata,
            source_device: device,
            version_hash: hash.to_string(),
            updated_at: ts(0),
            merge_policy: MergePolicy::LastWriteWins,
            payload: serde_json::json!({"data": id}),
        }
    }

    // ---- SyncMessage roundtrip ----

    #[test]
    fn sync_message_manifest_request_roundtrips() {
        let msg = SyncMessage::ManifestRequest {
            from_device: device_a(),
            agent_os_id: agent_id(),
        };
        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains("manifest_request"));
        let decoded: SyncMessage = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded, msg);
    }

    #[test]
    fn sync_message_object_response_roundtrips() {
        let msg = SyncMessage::ObjectResponse {
            from_device: device_b(),
            objects: vec![make_record("obj-1", device_b(), "hash-1")],
        };
        let json = serde_json::to_string(&msg).unwrap();
        let decoded: SyncMessage = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded, msg);
    }

    // ---- MemorySyncTransport tests ----

    #[tokio::test]
    async fn transport_registers_and_reaches_device() {
        let transport = MemorySyncTransport::new();
        let manifest = SyncManifest::new(device_a(), agent_id(), ts(0));
        transport.register_device(device_a(), manifest, HashMap::new());

        assert!(transport.is_reachable(&device_a()).await);
        assert!(!transport.is_reachable(&device_b()).await);
    }

    #[tokio::test]
    async fn transport_manifest_request_returns_manifest() {
        let transport = MemorySyncTransport::new();
        let mut manifest = SyncManifest::new(device_b(), agent_id(), ts(0));
        manifest.upsert(make_record("obj-1", device_b(), "hash-1"));
        transport.register_device(device_b(), manifest, HashMap::new());

        let response = transport
            .send(
                &device_b(),
                SyncMessage::ManifestRequest {
                    from_device: device_a(),
                    agent_os_id: agent_id(),
                },
            )
            .await
            .unwrap();

        match response {
            SyncMessage::ManifestResponse { manifest, .. } => {
                assert_eq!(manifest.len(), 1);
            }
            _ => panic!("expected manifest response"),
        }
    }

    #[tokio::test]
    async fn transport_object_request_returns_records() {
        let transport = MemorySyncTransport::new();
        let manifest = SyncManifest::new(device_b(), agent_id(), ts(0));
        let mut records = HashMap::new();
        records.insert(
            SyncObjectId::from("obj-1"),
            make_record("obj-1", device_b(), "hash-1"),
        );
        records.insert(
            SyncObjectId::from("obj-2"),
            make_record("obj-2", device_b(), "hash-2"),
        );
        transport.register_device(device_b(), manifest, records);

        let response = transport
            .send(
                &device_b(),
                SyncMessage::ObjectRequest {
                    from_device: device_a(),
                    object_ids: vec![SyncObjectId::from("obj-1")],
                },
            )
            .await
            .unwrap();

        match response {
            SyncMessage::ObjectResponse { objects, .. } => {
                assert_eq!(objects.len(), 1);
                assert_eq!(objects[0].object_id, SyncObjectId::from("obj-1"));
            }
            _ => panic!("expected object response"),
        }
    }

    #[tokio::test]
    async fn transport_unreachable_device_returns_error() {
        let transport = MemorySyncTransport::new();
        transport.set_reachable(&device_b(), false);

        let result = transport
            .send(
                &device_b(),
                SyncMessage::ManifestRequest {
                    from_device: device_a(),
                    agent_os_id: agent_id(),
                },
            )
            .await;

        assert!(matches!(result, Err(TransportError::DeviceNotReachable(_))));
    }

    // ---- P2pSyncOrchestrator E2E tests ----

    #[tokio::test]
    async fn e2e_sync_both_devices_converge() {
        let transport = MemorySyncTransport::new();

        // Device A has obj-1
        let mut manifest_a = SyncManifest::new(device_a(), agent_id(), ts(0));
        manifest_a.upsert(make_record("obj-1", device_a(), "hash-1"));
        let mut records_a = HashMap::new();
        records_a.insert(
            SyncObjectId::from("obj-1"),
            make_record("obj-1", device_a(), "hash-1"),
        );

        // Device B has obj-2
        let mut manifest_b = SyncManifest::new(device_b(), agent_id(), ts(0));
        manifest_b.upsert(make_record("obj-2", device_b(), "hash-2"));
        let mut records_b = HashMap::new();
        records_b.insert(
            SyncObjectId::from("obj-2"),
            make_record("obj-2", device_b(), "hash-2"),
        );

        transport.register_device(device_a(), manifest_a.clone(), records_a);
        transport.register_device(device_b(), manifest_b.clone(), records_b);

        let trust_store = setup_trust_store();

        // Sync A → B
        let orchestrator_a = P2pSyncOrchestrator::new(device_a(), agent_id(), trust_store.clone());

        let outcome = orchestrator_a
            .sync(&transport, &mut manifest_a, &device_b())
            .await
            .unwrap();

        assert_eq!(outcome.peer_device, device_b());
        // A should now have both objects
        assert_eq!(manifest_a.len(), 2);
        assert!(manifest_a.get(&SyncObjectId::from("obj-1")).is_some());
        assert!(manifest_a.get(&SyncObjectId::from("obj-2")).is_some());
    }

    #[tokio::test]
    async fn e2e_sync_rejects_untrusted_device() {
        let transport = MemorySyncTransport::new();
        let manifest = SyncManifest::new(device_a(), agent_id(), ts(0));
        transport.register_device(device_a(), manifest.clone(), HashMap::new());

        // No trust established
        let trust_store = DeviceTrustStore::new();

        let orchestrator = P2pSyncOrchestrator::new(device_a(), agent_id(), trust_store);

        let mut local = manifest;
        let result = orchestrator.sync(&transport, &mut local, &device_b()).await;

        assert!(matches!(result, Err(SyncError::DeviceNotTrusted(_))));
    }

    #[tokio::test]
    async fn e2e_sync_rejects_mismatched_agent_id() {
        let transport = MemorySyncTransport::new();

        let manifest_a = SyncManifest::new(device_a(), agent_id(), ts(0));
        let manifest_b = SyncManifest::new(device_b(), "different-agent".to_string(), ts(0));

        transport.register_device(device_a(), manifest_a.clone(), HashMap::new());
        transport.register_device(device_b(), manifest_b, HashMap::new());

        let trust_store = setup_trust_store();

        let orchestrator = P2pSyncOrchestrator::new(device_a(), agent_id(), trust_store);

        let mut local = manifest_a;
        let result = orchestrator.sync(&transport, &mut local, &device_b()).await;

        assert!(matches!(result, Err(SyncError::AgentOsIdMismatch { .. })));
    }

    #[tokio::test]
    async fn e2e_sync_no_changes_when_identical() {
        let transport = MemorySyncTransport::new();

        let record = make_record("obj-1", device_a(), "hash-1");
        let mut manifest_a = SyncManifest::new(device_a(), agent_id(), ts(0));
        manifest_a.upsert(record.clone());
        let mut manifest_b = SyncManifest::new(device_b(), agent_id(), ts(0));
        manifest_b.upsert(record);

        let mut records = HashMap::new();
        records.insert(
            SyncObjectId::from("obj-1"),
            make_record("obj-1", device_a(), "hash-1"),
        );

        transport.register_device(device_a(), manifest_a.clone(), records.clone());
        transport.register_device(device_b(), manifest_b, records);

        let trust_store = setup_trust_store();

        let orchestrator = P2pSyncOrchestrator::new(device_a(), agent_id(), trust_store);

        let mut local = manifest_a;
        let outcome = orchestrator
            .sync(&transport, &mut local, &device_b())
            .await
            .unwrap();

        assert!(outcome.diff.local_only.is_empty());
        assert!(outcome.diff.remote_only.is_empty());
        assert!(outcome.diff.conflicting.is_empty());
        assert_eq!(local.len(), 1); // Still just 1 object
    }

    #[tokio::test]
    async fn e2e_sync_conflict_resolved_by_lww() {
        let transport = MemorySyncTransport::new();

        // Both have obj-1 with different hashes, B is newer
        let mut manifest_a = SyncManifest::new(device_a(), agent_id(), ts(0));
        manifest_a.upsert(SyncRecord {
            object_id: SyncObjectId::from("obj-1"),
            kind: SyncObjectKind::AssetMetadata,
            source_device: device_a(),
            version_hash: "hash-old".to_string(),
            updated_at: ts(0),
            merge_policy: MergePolicy::LastWriteWins,
            payload: serde_json::json!({"data": "old"}),
        });

        let mut manifest_b = SyncManifest::new(device_b(), agent_id(), ts(0));
        manifest_b.upsert(SyncRecord {
            object_id: SyncObjectId::from("obj-1"),
            kind: SyncObjectKind::AssetMetadata,
            source_device: device_b(),
            version_hash: "hash-new".to_string(),
            updated_at: ts(100),
            merge_policy: MergePolicy::LastWriteWins,
            payload: serde_json::json!({"data": "new"}),
        });

        let mut records_b = HashMap::new();
        records_b.insert(
            SyncObjectId::from("obj-1"),
            SyncRecord {
                object_id: SyncObjectId::from("obj-1"),
                kind: SyncObjectKind::AssetMetadata,
                source_device: device_b(),
                version_hash: "hash-new".to_string(),
                updated_at: ts(100),
                merge_policy: MergePolicy::LastWriteWins,
                payload: serde_json::json!({"data": "new"}),
            },
        );

        transport.register_device(device_a(), manifest_a.clone(), HashMap::new());
        transport.register_device(device_b(), manifest_b, records_b);

        let trust_store = setup_trust_store();

        let orchestrator = P2pSyncOrchestrator::new(device_a(), agent_id(), trust_store);

        let mut local = manifest_a;
        let outcome = orchestrator
            .sync(&transport, &mut local, &device_b())
            .await
            .unwrap();

        // After sync, local should have the newer version
        let record = local.get(&SyncObjectId::from("obj-1")).unwrap();
        assert_eq!(record.version_hash, "hash-new");
    }
}
