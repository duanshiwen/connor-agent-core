//! # Device Pairing Core
//!
//! Domain types and in-memory store for AgentOS device pairing and P2P trust.
//!
//! This crate provides the foundation for multi-device synchronization:
//! - Device identity (`DeviceId`)
//! - Pairing sessions with codes (`PairingSession`, `PairingCode`)
//! - Device handshake protocol types (`HandshakeChallenge`, `HandshakeResponse`)
//! - Trust records (`DeviceTrustRecord`, `TrustLevel`)
//! - In-memory `DeviceTrustStore` for tests and early runtime
//!
//! Future work:
//! - Real cryptographic handshake (ed25519 / age)
//! - mDNS / QR code pairing
//! - Common server signaling

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt;
use std::sync::{Arc, Mutex};

// ---------------------------------------------------------------------------
// Core types
// ---------------------------------------------------------------------------

/// Unique identifier for a device in the AgentOS federation.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct DeviceId(pub String);

impl fmt::Display for DeviceId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl From<String> for DeviceId {
    fn from(s: String) -> Self {
        Self(s)
    }
}

impl From<&str> for DeviceId {
    fn from(s: &str) -> Self {
        Self(s.to_string())
    }
}

/// A short-lived pairing code used to initiate device pairing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PairingCode(pub String);

impl fmt::Display for PairingCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl PairingCode {
    /// Generate a deterministic pairing code from device ids (for testing).
    pub fn from_devices(a: &DeviceId, b: &DeviceId) -> Self {
        let (first, second) = if a.0 <= b.0 { (a, b) } else { (b, a) };
        Self(format!("pair-{}-{}", first.0, second.0))
    }
}

/// Status of a pairing session.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PairingStatus {
    /// Waiting for the peer device to respond.
    Pending,
    /// Both devices have exchanged challenges.
    HandshakeInProgress,
    /// Pairing completed successfully.
    Completed,
    /// Pairing failed (timeout, mismatch, rejected).
    Failed { reason: String },
    /// Pairing was cancelled by one device.
    Cancelled,
}

/// A device pairing session.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PairingSession {
    pub session_id: String,
    pub code: PairingCode,
    pub initiator: DeviceId,
    pub peer: Option<DeviceId>,
    pub status: PairingStatus,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Handshake challenge sent from one device to another.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HandshakeChallenge {
    pub from_device: DeviceId,
    pub nonce: String,
    pub timestamp: DateTime<Utc>,
}

/// Handshake response to a challenge.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HandshakeResponse {
    pub from_device: DeviceId,
    pub challenge_nonce: String,
    pub signature: String,
    pub timestamp: DateTime<Utc>,
}

/// Trust level for a paired device.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TrustLevel {
    /// Basic trust — paired but limited access.
    Basic,
    /// Full trust — same user's device with full sync.
    Full,
    /// Temporary trust — for one-time operations.
    Temporary,
    /// Revoked — trust was explicitly revoked.
    Revoked,
}

/// A trust record for a paired device.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DeviceTrustRecord {
    pub device_id: DeviceId,
    pub peer_device_id: DeviceId,
    pub trust_level: TrustLevel,
    pub paired_at: DateTime<Utc>,
    pub last_verified_at: Option<DateTime<Utc>>,
    pub pairing_session_id: String,
}

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

/// Errors from device pairing operations.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PairingError {
    #[error("pairing session not found: {0}")]
    SessionNotFound(String),
    #[error("device not trusted: {0}")]
    DeviceNotTrusted(String),
    #[error("invalid pairing state: {expected}, actual: {actual}")]
    InvalidState { expected: String, actual: String },
    #[error("nonce mismatch")]
    NonceMismatch,
}

// ---------------------------------------------------------------------------
// DeviceTrustStore
// ---------------------------------------------------------------------------

/// In-memory store for device trust records.
#[derive(Debug, Clone, Default)]
pub struct DeviceTrustStore {
    trust_records: Arc<Mutex<HashMap<DeviceId, DeviceTrustRecord>>>,
    pairing_sessions: Arc<Mutex<HashMap<String, PairingSession>>>,
}

impl DeviceTrustStore {
    pub fn new() -> Self {
        Self::default()
    }

    /// Add or update a trust record.
    pub fn upsert_trust(&self, record: DeviceTrustRecord) -> Result<(), PairingError> {
        let mut records = self.trust_records.lock().unwrap();
        records.insert(record.device_id.clone(), record);
        Ok(())
    }

    /// Get trust record for a device.
    pub fn get_trust(&self, device_id: &DeviceId) -> Option<DeviceTrustRecord> {
        let records = self.trust_records.lock().unwrap();
        records.get(device_id).cloned()
    }

    /// Check if a device is trusted (trust level is not Revoked).
    pub fn is_trusted(&self, device_id: &DeviceId) -> bool {
        let records = self.trust_records.lock().unwrap();
        records
            .get(device_id)
            .map(|r| r.trust_level != TrustLevel::Revoked)
            .unwrap_or(false)
    }

    /// Revoke trust for a device.
    pub fn revoke_trust(&self, device_id: &DeviceId) -> Result<(), PairingError> {
        let mut records = self.trust_records.lock().unwrap();
        match records.get_mut(device_id) {
            Some(record) => {
                record.trust_level = TrustLevel::Revoked;
                Ok(())
            }
            None => Err(PairingError::DeviceNotTrusted(device_id.0.clone())),
        }
    }

    /// Store a pairing session.
    pub fn store_session(&self, session: PairingSession) {
        let mut sessions = self.pairing_sessions.lock().unwrap();
        sessions.insert(session.session_id.clone(), session);
    }

    /// Get a pairing session by id.
    pub fn get_session(&self, session_id: &str) -> Option<PairingSession> {
        let sessions = self.pairing_sessions.lock().unwrap();
        sessions.get(session_id).cloned()
    }

    /// Update a pairing session.
    pub fn update_session(&self, session: PairingSession) -> Result<(), PairingError> {
        let mut sessions = self.pairing_sessions.lock().unwrap();
        if !sessions.contains_key(&session.session_id) {
            return Err(PairingError::SessionNotFound(session.session_id.clone()));
        }
        sessions.insert(session.session_id.clone(), session);
        Ok(())
    }

    /// List all trust records.
    pub fn list_trusted_devices(&self) -> Vec<DeviceTrustRecord> {
        let records = self.trust_records.lock().unwrap();
        records.values().cloned().collect()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn ts() -> DateTime<Utc> {
        "2026-05-25T12:00:00Z".parse().unwrap()
    }

    fn device_a() -> DeviceId {
        DeviceId::from("device-laptop")
    }

    fn device_b() -> DeviceId {
        DeviceId::from("device-phone")
    }

    // ---- Type roundtrip tests ----

    #[test]
    fn device_id_roundtrips() {
        let id = DeviceId::from("device-1");
        assert_eq!(id.to_string(), "device-1");
        let json = serde_json::to_string(&id).unwrap();
        let decoded: DeviceId = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded, id);
    }

    #[test]
    fn pairing_code_is_deterministic() {
        let code_a = PairingCode::from_devices(&device_a(), &device_b());
        let code_b = PairingCode::from_devices(&device_b(), &device_a());
        // Order-independent
        assert_eq!(code_a, code_b);
        assert!(code_a.0.contains("device-laptop"));
        assert!(code_a.0.contains("device-phone"));
    }

    #[test]
    fn pairing_code_roundtrips() {
        let code = PairingCode::from_devices(&device_a(), &device_b());
        let json = serde_json::to_string(&code).unwrap();
        let decoded: PairingCode = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded, code);
    }

    #[test]
    fn pairing_status_serde_variants() {
        let pending = PairingStatus::Pending;
        assert_eq!(
            serde_json::from_str::<PairingStatus>(&serde_json::to_string(&pending).unwrap())
                .unwrap(),
            pending
        );

        let failed = PairingStatus::Failed {
            reason: "timeout".to_string(),
        };
        let json = serde_json::to_string(&failed).unwrap();
        assert!(json.contains("timeout"));
        let decoded: PairingStatus = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded, failed);
    }

    #[test]
    fn pairing_session_roundtrips() {
        let session = PairingSession {
            session_id: "session-1".to_string(),
            code: PairingCode::from_devices(&device_a(), &device_b()),
            initiator: device_a(),
            peer: None,
            status: PairingStatus::Pending,
            created_at: ts(),
            updated_at: ts(),
        };
        let json = serde_json::to_string_pretty(&session).unwrap();
        let decoded: PairingSession = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded, session);
    }

    #[test]
    fn trust_level_serde_uses_snake_case() {
        assert_eq!(
            serde_json::to_string(&TrustLevel::Full).unwrap(),
            "\"full\""
        );
        assert_eq!(
            serde_json::to_string(&TrustLevel::Revoked).unwrap(),
            "\"revoked\""
        );
    }

    #[test]
    fn device_trust_record_roundtrips() {
        let record = DeviceTrustRecord {
            device_id: device_a(),
            peer_device_id: device_b(),
            trust_level: TrustLevel::Full,
            paired_at: ts(),
            last_verified_at: Some(ts()),
            pairing_session_id: "session-1".to_string(),
        };
        let json = serde_json::to_string_pretty(&record).unwrap();
        let decoded: DeviceTrustRecord = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded, record);
    }

    #[test]
    fn handshake_challenge_roundtrips() {
        let challenge = HandshakeChallenge {
            from_device: device_a(),
            nonce: "nonce-123".to_string(),
            timestamp: ts(),
        };
        let json = serde_json::to_string(&challenge).unwrap();
        let decoded: HandshakeChallenge = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded, challenge);
    }

    #[test]
    fn handshake_response_roundtrips() {
        let response = HandshakeResponse {
            from_device: device_b(),
            challenge_nonce: "nonce-123".to_string(),
            signature: "sig-456".to_string(),
            timestamp: ts(),
        };
        let json = serde_json::to_string(&response).unwrap();
        let decoded: HandshakeResponse = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded, response);
    }

    // ---- DeviceTrustStore tests ----

    #[test]
    fn store_upsert_and_get_trust() {
        let store = DeviceTrustStore::new();
        let record = DeviceTrustRecord {
            device_id: device_a(),
            peer_device_id: device_b(),
            trust_level: TrustLevel::Full,
            paired_at: ts(),
            last_verified_at: None,
            pairing_session_id: "s1".to_string(),
        };

        store.upsert_trust(record.clone()).unwrap();
        let retrieved = store.get_trust(&device_a()).unwrap();
        assert_eq!(retrieved.trust_level, TrustLevel::Full);
    }

    #[test]
    fn store_is_trusted_returns_true_for_full_trust() {
        let store = DeviceTrustStore::new();
        store
            .upsert_trust(DeviceTrustRecord {
                device_id: device_a(),
                peer_device_id: device_b(),
                trust_level: TrustLevel::Full,
                paired_at: ts(),
                last_verified_at: None,
                pairing_session_id: "s1".to_string(),
            })
            .unwrap();

        assert!(store.is_trusted(&device_a()));
    }

    #[test]
    fn store_is_trusted_returns_false_for_revoked() {
        let store = DeviceTrustStore::new();
        store
            .upsert_trust(DeviceTrustRecord {
                device_id: device_a(),
                peer_device_id: device_b(),
                trust_level: TrustLevel::Revoked,
                paired_at: ts(),
                last_verified_at: None,
                pairing_session_id: "s1".to_string(),
            })
            .unwrap();

        assert!(!store.is_trusted(&device_a()));
    }

    #[test]
    fn store_is_trusted_returns_false_for_unknown_device() {
        let store = DeviceTrustStore::new();
        assert!(!store.is_trusted(&device_a()));
    }

    #[test]
    fn store_revoke_trust() {
        let store = DeviceTrustStore::new();
        store
            .upsert_trust(DeviceTrustRecord {
                device_id: device_a(),
                peer_device_id: device_b(),
                trust_level: TrustLevel::Full,
                paired_at: ts(),
                last_verified_at: None,
                pairing_session_id: "s1".to_string(),
            })
            .unwrap();

        store.revoke_trust(&device_a()).unwrap();
        let record = store.get_trust(&device_a()).unwrap();
        assert_eq!(record.trust_level, TrustLevel::Revoked);
        assert!(!store.is_trusted(&device_a()));
    }

    #[test]
    fn store_revoke_trust_fails_for_unknown_device() {
        let store = DeviceTrustStore::new();
        let result = store.revoke_trust(&device_a());
        assert!(matches!(result, Err(PairingError::DeviceNotTrusted(_))));
    }

    #[test]
    fn store_session_and_retrieve() {
        let store = DeviceTrustStore::new();
        let session = PairingSession {
            session_id: "s1".to_string(),
            code: PairingCode::from_devices(&device_a(), &device_b()),
            initiator: device_a(),
            peer: None,
            status: PairingStatus::Pending,
            created_at: ts(),
            updated_at: ts(),
        };

        store.store_session(session.clone());
        let retrieved = store.get_session("s1").unwrap();
        assert_eq!(retrieved.status, PairingStatus::Pending);
    }

    #[test]
    fn store_update_session() {
        let store = DeviceTrustStore::new();
        let session = PairingSession {
            session_id: "s1".to_string(),
            code: PairingCode::from_devices(&device_a(), &device_b()),
            initiator: device_a(),
            peer: None,
            status: PairingStatus::Pending,
            created_at: ts(),
            updated_at: ts(),
        };

        store.store_session(session.clone());

        let mut updated = session;
        updated.status = PairingStatus::Completed;
        updated.peer = Some(device_b());
        store.update_session(updated).unwrap();

        let retrieved = store.get_session("s1").unwrap();
        assert_eq!(retrieved.status, PairingStatus::Completed);
        assert_eq!(retrieved.peer, Some(device_b()));
    }

    #[test]
    fn store_update_session_fails_for_missing() {
        let store = DeviceTrustStore::new();
        let session = PairingSession {
            session_id: "missing".to_string(),
            code: PairingCode::from_devices(&device_a(), &device_b()),
            initiator: device_a(),
            peer: None,
            status: PairingStatus::Pending,
            created_at: ts(),
            updated_at: ts(),
        };

        let result = store.update_session(session);
        assert!(matches!(result, Err(PairingError::SessionNotFound(_))));
    }

    #[test]
    fn store_list_trusted_devices() {
        let store = DeviceTrustStore::new();
        store
            .upsert_trust(DeviceTrustRecord {
                device_id: device_a(),
                peer_device_id: device_b(),
                trust_level: TrustLevel::Full,
                paired_at: ts(),
                last_verified_at: None,
                pairing_session_id: "s1".to_string(),
            })
            .unwrap();

        let devices = store.list_trusted_devices();
        assert_eq!(devices.len(), 1);
        assert_eq!(devices[0].device_id, device_a());
    }
}
