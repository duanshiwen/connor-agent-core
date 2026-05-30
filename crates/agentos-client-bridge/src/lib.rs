//! JSON-safe native bridge contract for AgentOS clients.
//!
//! This crate intentionally exposes a narrow, serialization-first boundary that
//! can later be wrapped by UniFFI, C ABI, Swift Package, or another host bridge.
//! The bridge does not require native clients to depend on Rust generics or
//! trait objects.

use client_substrate::{
    ClientEventCursor, ClientEventEnvelope, ClientSubstrate, ClientSubstrateError,
};
use serde::{Deserialize, Serialize};
use sync_runtime::{KnowledgeSyncProjection, ServerSyncApplyError, ServerSyncEvent};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum AgentOsClientBridgeError {
    #[error("invalid bridge argument: {reason}")]
    InvalidArgument { reason: String },
    #[error("substrate error: {reason}")]
    Substrate { reason: String },
    #[error("bridge serialization failed: {reason}")]
    Serialization { reason: String },
    #[error("server sync apply failed: {reason}")]
    ServerSyncApply { reason: String },
}

impl From<ClientSubstrateError> for AgentOsClientBridgeError {
    fn from(value: ClientSubstrateError) -> Self {
        Self::Substrate {
            reason: value.to_string(),
        }
    }
}

impl From<ServerSyncApplyError> for AgentOsClientBridgeError {
    fn from(value: ServerSyncApplyError) -> Self {
        Self::ServerSyncApply {
            reason: value.to_string(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BridgeResponse {
    pub ok: bool,
    pub json: String,
}

impl BridgeResponse {
    pub fn from_serializable<T: Serialize>(value: &T) -> Result<Self, AgentOsClientBridgeError> {
        Ok(Self {
            ok: true,
            json: serde_json::to_string(value).map_err(|source| {
                AgentOsClientBridgeError::Serialization {
                    reason: source.to_string(),
                }
            })?,
        })
    }
}

#[derive(Clone)]
pub struct AgentOsClientBridge {
    substrate: ClientSubstrate,
}

impl AgentOsClientBridge {
    /// Construct a deterministic test/development bridge. Production hosts
    /// should construct `ClientSubstrate` with production dependencies and pass
    /// it through `from_substrate`.
    pub fn for_local_development() -> Result<Self, AgentOsClientBridgeError> {
        let substrate = ClientSubstrate::builder().build()?;
        Ok(Self { substrate })
    }

    pub fn from_substrate(substrate: ClientSubstrate) -> Self {
        Self { substrate }
    }

    pub fn api_version(&self) -> u32 {
        client_substrate::CLIENT_SUBSTRATE_API_VERSION
    }

    pub fn latest_event_cursor_json(&self) -> Result<BridgeResponse, AgentOsClientBridgeError> {
        BridgeResponse::from_serializable(&self.substrate.latest_event_cursor())
    }

    pub fn events_after_json(
        &self,
        cursor_json: &str,
    ) -> Result<BridgeResponse, AgentOsClientBridgeError> {
        let cursor: ClientEventCursor = serde_json::from_str(cursor_json).map_err(|source| {
            AgentOsClientBridgeError::InvalidArgument {
                reason: format!("invalid event cursor json: {source}"),
            }
        })?;
        let events: Vec<ClientEventEnvelope> = self.substrate.events_after(cursor);
        BridgeResponse::from_serializable(&events)
    }

    pub fn conversation_list_projection_json(
        &self,
    ) -> Result<BridgeResponse, AgentOsClientBridgeError> {
        BridgeResponse::from_serializable(&self.substrate.conversation_list_projection())
    }

    pub fn run_projection_json(&self) -> Result<BridgeResponse, AgentOsClientBridgeError> {
        BridgeResponse::from_serializable(&self.substrate.run_projection())
    }

    pub fn approval_projection_json(&self) -> Result<BridgeResponse, AgentOsClientBridgeError> {
        BridgeResponse::from_serializable(&self.substrate.approval_projection())
    }

    pub fn storage_health_report_json(&self) -> Result<BridgeResponse, AgentOsClientBridgeError> {
        BridgeResponse::from_serializable(&self.substrate.storage_health_report())
    }

    pub fn knowledge_projection_json(&self) -> Result<BridgeResponse, AgentOsClientBridgeError> {
        BridgeResponse::from_serializable(&self.substrate.knowledge_projection())
    }

    pub fn asset_projection_json(&self) -> Result<BridgeResponse, AgentOsClientBridgeError> {
        BridgeResponse::from_serializable(&self.substrate.asset_projection())
    }

    /// Apply backend M2.3 knowledge sync events to a JSON-encoded knowledge projection.
    ///
    /// `projection_json` should be a serialized `KnowledgeSyncProjection`. Empty input
    /// starts from an empty projection. `events_json` should be a JSON array of
    /// `ServerSyncEvent` values returned by the backend `/api/v1/sync/events` API.
    ///
    /// The returned JSON is the updated `KnowledgeSyncProjection`, including the advanced
    /// cursor. Hosts should durably persist this JSON before acking the backend cursor.
    pub fn apply_knowledge_sync_events_json(
        &self,
        projection_json: &str,
        events_json: &str,
    ) -> Result<BridgeResponse, AgentOsClientBridgeError> {
        apply_knowledge_sync_events_json(projection_json, events_json)
    }

    pub async fn shutdown(&self) -> Result<(), AgentOsClientBridgeError> {
        self.substrate
            .host_api_for_bridge()
            .shutdown()
            .await
            .map_err(|source| AgentOsClientBridgeError::Substrate {
                reason: source.to_string(),
            })
    }
}

/// Stateless JSON-safe helper for applying backend M2.3 knowledge sync events.
///
/// This free function mirrors the bridge method so FFI or UniFFI layers can expose it
/// without requiring an `AgentOsClientBridge` instance when they only need reducer logic.
pub fn apply_knowledge_sync_events_json(
    projection_json: &str,
    events_json: &str,
) -> Result<BridgeResponse, AgentOsClientBridgeError> {
    let mut projection = if projection_json.trim().is_empty() {
        KnowledgeSyncProjection::new()
    } else {
        serde_json::from_str::<KnowledgeSyncProjection>(projection_json).map_err(|source| {
            AgentOsClientBridgeError::InvalidArgument {
                reason: format!("invalid knowledge projection json: {source}"),
            }
        })?
    };
    let events: Vec<ServerSyncEvent> = serde_json::from_str(events_json).map_err(|source| {
        AgentOsClientBridgeError::InvalidArgument {
            reason: format!("invalid server sync events json: {source}"),
        }
    })?;
    projection.apply_events(&events)?;
    BridgeResponse::from_serializable(&projection)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bridge_exposes_api_version() {
        let bridge = AgentOsClientBridge::for_local_development().unwrap();
        assert_eq!(bridge.api_version(), 1);
    }

    #[test]
    fn bridge_returns_json_projection() {
        let bridge = AgentOsClientBridge::for_local_development().unwrap();
        let response = bridge.conversation_list_projection_json().unwrap();
        assert!(response.ok);
        assert!(response.json.contains("conversations"));
    }

    #[test]
    fn bridge_rejects_invalid_cursor_json() {
        let bridge = AgentOsClientBridge::for_local_development().unwrap();
        let err = bridge.events_after_json("not-json").unwrap_err();
        assert!(matches!(
            err,
            AgentOsClientBridgeError::InvalidArgument { .. }
        ));
    }

    #[test]
    fn bridge_exposes_health_and_knowledge_asset_projections() {
        let bridge = AgentOsClientBridge::for_local_development().unwrap();
        assert!(
            bridge
                .storage_health_report_json()
                .unwrap()
                .json
                .contains("healthy")
        );
        assert!(
            bridge
                .knowledge_projection_json()
                .unwrap()
                .json
                .contains("results")
        );
        assert!(
            bridge
                .asset_projection_json()
                .unwrap()
                .json
                .contains("assets")
        );
    }

    #[test]
    fn bridge_applies_knowledge_sync_events_json() {
        let bridge = AgentOsClientBridge::for_local_development().unwrap();
        let events = serde_json::json!([
            {
                "id": "evt-1",
                "user_id": "user-1",
                "device_id": "device-b",
                "event_type": "knowledge.created",
                "schema_version": 1,
                "object_type": "knowledge",
                "object_id": "notes/alpha",
                "operation": "created",
                "source_device_id": "device-a",
                "client_event_id": "client-1",
                "payload": {
                    "entry_id": "notes/alpha",
                    "object_id": "notes/alpha",
                    "title": "Alpha",
                    "content_markdown": "# Alpha",
                    "summary": "summary",
                    "tags": ["agentos"],
                    "metadata": {},
                    "source_uri": "",
                    "status": "active",
                    "version": 1,
                    "content_hash": "hash-1",
                    "updated_by_device_id": "device-a",
                    "updated_at": "2026-05-30T02:00:00Z"
                },
                "timestamp": "2026-05-30T02:00:01Z",
                "sequence": 1
            },
            {
                "id": "evt-2",
                "user_id": "user-1",
                "device_id": "device-b",
                "event_type": "knowledge.deleted",
                "schema_version": 1,
                "object_type": "knowledge",
                "object_id": "notes/alpha",
                "operation": "deleted",
                "source_device_id": "device-a",
                "client_event_id": "client-2",
                "payload": {
                    "entry_id": "notes/alpha",
                    "object_id": "notes/alpha",
                    "title": "Alpha",
                    "content_markdown": "# Alpha",
                    "summary": "summary",
                    "tags": ["agentos"],
                    "metadata": {},
                    "source_uri": "",
                    "status": "deleted",
                    "version": 2,
                    "content_hash": "hash-2",
                    "updated_by_device_id": "device-a",
                    "updated_at": "2026-05-30T02:05:00Z",
                    "deleted_at": "2026-05-30T02:05:00Z"
                },
                "timestamp": "2026-05-30T02:05:01Z",
                "sequence": 2
            }
        ]);

        let response = bridge
            .apply_knowledge_sync_events_json("", &events.to_string())
            .unwrap();
        assert!(response.ok);
        let projection: sync_runtime::KnowledgeSyncProjection =
            serde_json::from_str(&response.json).unwrap();
        assert_eq!(projection.cursor.last_applied_sequence, 2);
        let entry = projection.entries.get("notes/alpha").unwrap();
        assert_eq!(
            entry.status,
            sync_runtime::KnowledgeEntrySyncStatus::Deleted
        );
        assert_eq!(entry.version, 2);
    }

    #[test]
    fn bridge_rejects_invalid_knowledge_sync_json() {
        let err = apply_knowledge_sync_events_json("", "not-json").unwrap_err();
        assert!(matches!(
            err,
            AgentOsClientBridgeError::InvalidArgument { .. }
        ));
    }
}
