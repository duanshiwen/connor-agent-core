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
use thiserror::Error;

#[derive(Debug, Error)]
pub enum AgentOsClientBridgeError {
    #[error("invalid bridge argument: {reason}")]
    InvalidArgument { reason: String },
    #[error("substrate error: {reason}")]
    Substrate { reason: String },
    #[error("bridge serialization failed: {reason}")]
    Serialization { reason: String },
}

impl From<ClientSubstrateError> for AgentOsClientBridgeError {
    fn from(value: ClientSubstrateError) -> Self {
        Self::Substrate {
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
    pub fn for_tests() -> Result<Self, AgentOsClientBridgeError> {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bridge_exposes_api_version() {
        let bridge = AgentOsClientBridge::for_tests().unwrap();
        assert_eq!(bridge.api_version(), 1);
    }

    #[test]
    fn bridge_returns_json_projection() {
        let bridge = AgentOsClientBridge::for_tests().unwrap();
        let response = bridge.conversation_list_projection_json().unwrap();
        assert!(response.ok);
        assert!(response.json.contains("conversations"));
    }

    #[test]
    fn bridge_rejects_invalid_cursor_json() {
        let bridge = AgentOsClientBridge::for_tests().unwrap();
        let err = bridge.events_after_json("not-json").unwrap_err();
        assert!(matches!(
            err,
            AgentOsClientBridgeError::InvalidArgument { .. }
        ));
    }
}
