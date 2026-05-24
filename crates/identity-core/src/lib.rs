//! # Identity Core
//!
//! Domain model for AgentOS identity — local sovereignty, device IDs, and fake crypto.
//!
//! The identity model supports client-side sovereignty: the agentos_id is generated
//! locally and is not bound to any single server. The same identity can bind to
//! multiple servers.

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt;
use std::sync::Mutex;

// ---------------------------------------------------------------------------
// Core types
// ---------------------------------------------------------------------------

/// Global unique identity ID — generated locally, not bound to any server.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct AgentOsId(pub String);

impl fmt::Display for AgentOsId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl From<String> for AgentOsId {
    fn from(s: String) -> Self {
        Self(s)
    }
}

impl From<&str> for AgentOsId {
    fn from(s: &str) -> Self {
        Self(s.to_string())
    }
}

/// Device identifier.
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

/// Reference to a local master key (opaque, stored in keychain/secure enclave).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct LocalMasterKeyRef(pub String);

impl fmt::Display for LocalMasterKeyRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl From<String> for LocalMasterKeyRef {
    fn from(s: String) -> Self {
        Self(s)
    }
}

impl From<&str> for LocalMasterKeyRef {
    fn from(s: &str) -> Self {
        Self(s.to_string())
    }
}

/// Public identity — the shareable part of an identity.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PublicIdentity {
    pub agentos_id: AgentOsId,
    pub device_id: DeviceId,
    pub display_name: String,
    pub created_at: DateTime<Utc>,
}

/// Proof of identity ownership (signature over a challenge).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IdentityProof {
    pub agentos_id: AgentOsId,
    pub device_id: DeviceId,
    pub challenge: String,
    pub signature: String,
    pub created_at: DateTime<Utc>,
}

/// Signed challenge — used for server verification.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SignedChallenge {
    pub challenge: String,
    pub signature: String,
    pub public_key: String,
}

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum IdentityError {
    #[error("identity not found: {0}")]
    NotFound(String),
    #[error("invalid signature")]
    InvalidSignature,
    #[error("internal error: {0}")]
    Internal(String),
}

// ---------------------------------------------------------------------------
// IdentityStore trait
// ---------------------------------------------------------------------------

/// Trait for storing and retrieving identity information.
#[async_trait]
pub trait IdentityStore: Send + Sync {
    async fn save_identity(&self, identity: &PublicIdentity) -> Result<(), IdentityError>;
    async fn get_identity(&self, id: &AgentOsId) -> Result<PublicIdentity, IdentityError>;
    async fn list_identities(&self) -> Result<Vec<PublicIdentity>, IdentityError>;
}

// ---------------------------------------------------------------------------
// MemoryIdentityStore
// ---------------------------------------------------------------------------

pub struct MemoryIdentityStore {
    inner: Mutex<HashMap<String, PublicIdentity>>,
}

impl MemoryIdentityStore {
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(HashMap::new()),
        }
    }
}

impl Default for MemoryIdentityStore {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl IdentityStore for MemoryIdentityStore {
    async fn save_identity(&self, identity: &PublicIdentity) -> Result<(), IdentityError> {
        let mut store = self.inner.lock().unwrap();
        store.insert(identity.agentos_id.0.clone(), identity.clone());
        Ok(())
    }

    async fn get_identity(&self, id: &AgentOsId) -> Result<PublicIdentity, IdentityError> {
        let store = self.inner.lock().unwrap();
        store
            .get(&id.0)
            .cloned()
            .ok_or_else(|| IdentityError::NotFound(id.0.clone()))
    }

    async fn list_identities(&self) -> Result<Vec<PublicIdentity>, IdentityError> {
        let store = self.inner.lock().unwrap();
        Ok(store.values().cloned().collect())
    }
}

// ---------------------------------------------------------------------------
// FakeCryptoProvider — fake signature/verify for testing
// ---------------------------------------------------------------------------

/// Fake crypto provider for testing. Signatures are just "signed:{challenge}".
pub struct FakeCryptoProvider;

impl FakeCryptoProvider {
    pub fn sign(&self, challenge: &str, _key: &LocalMasterKeyRef) -> String {
        format!("signed:{}", challenge)
    }

    pub fn verify(&self, challenge: &str, signature: &str) -> bool {
        signature == format!("signed:{}", challenge)
    }

    pub fn create_proof(
        &self,
        agentos_id: &AgentOsId,
        device_id: &DeviceId,
        challenge: &str,
        key: &LocalMasterKeyRef,
    ) -> IdentityProof {
        IdentityProof {
            agentos_id: agentos_id.clone(),
            device_id: device_id.clone(),
            challenge: challenge.to_string(),
            signature: self.sign(challenge, key),
            created_at: Utc::now(),
        }
    }

    pub fn verify_proof(&self, proof: &IdentityProof) -> bool {
        self.verify(&proof.challenge, &proof.signature)
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn ts() -> DateTime<Utc> {
        "2026-05-24T12:00:00Z".parse().unwrap()
    }

    // ---- Type roundtrip tests ----

    #[test]
    fn agent_os_id_roundtrips() {
        let id = AgentOsId::from("agent-1");
        assert_eq!(id.to_string(), "agent-1");
        let json = serde_json::to_string(&id).unwrap();
        let decoded: AgentOsId = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded, id);
    }

    #[test]
    fn device_id_roundtrips() {
        let id = DeviceId::from("device-1");
        assert_eq!(id.to_string(), "device-1");
        let json = serde_json::to_string(&id).unwrap();
        let decoded: DeviceId = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded, id);
    }

    #[test]
    fn public_identity_roundtrips() {
        let identity = PublicIdentity {
            agentos_id: AgentOsId::from("agent-1"),
            device_id: DeviceId::from("device-1"),
            display_name: "Test User".to_string(),
            created_at: ts(),
        };
        let json = serde_json::to_string_pretty(&identity).unwrap();
        let decoded: PublicIdentity = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded, identity);
    }

    #[test]
    fn identity_proof_roundtrips() {
        let proof = IdentityProof {
            agentos_id: AgentOsId::from("agent-1"),
            device_id: DeviceId::from("device-1"),
            challenge: "test-challenge".to_string(),
            signature: "signed:test-challenge".to_string(),
            created_at: ts(),
        };
        let json = serde_json::to_string_pretty(&proof).unwrap();
        let decoded: IdentityProof = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded, proof);
    }

    #[test]
    fn signed_challenge_roundtrips() {
        let challenge = SignedChallenge {
            challenge: "nonce-123".to_string(),
            signature: "signed:nonce-123".to_string(),
            public_key: "pubkey-abc".to_string(),
        };
        let json = serde_json::to_string_pretty(&challenge).unwrap();
        let decoded: SignedChallenge = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded, challenge);
    }

    // ---- FakeCrypto tests ----

    #[test]
    fn fake_sign_produces_expected_signature() {
        let crypto = FakeCryptoProvider;
        let key = LocalMasterKeyRef::from("master-key-1");
        let sig = crypto.sign("challenge-1", &key);
        assert_eq!(sig, "signed:challenge-1");
    }

    #[test]
    fn fake_verify_accepts_valid_signature() {
        let crypto = FakeCryptoProvider;
        assert!(crypto.verify("challenge-1", "signed:challenge-1"));
    }

    #[test]
    fn fake_verify_rejects_invalid_signature() {
        let crypto = FakeCryptoProvider;
        assert!(!crypto.verify("challenge-1", "wrong-signature"));
    }

    #[test]
    fn fake_create_proof_and_verify() {
        let crypto = FakeCryptoProvider;
        let key = LocalMasterKeyRef::from("master-key-1");
        let proof = crypto.create_proof(
            &AgentOsId::from("agent-1"),
            &DeviceId::from("device-1"),
            "challenge-abc",
            &key,
        );
        assert!(crypto.verify_proof(&proof));
    }

    #[test]
    fn fake_proof_with_tampered_challenge_fails() {
        let crypto = FakeCryptoProvider;
        let key = LocalMasterKeyRef::from("master-key-1");
        let mut proof = crypto.create_proof(
            &AgentOsId::from("agent-1"),
            &DeviceId::from("device-1"),
            "challenge-abc",
            &key,
        );
        proof.challenge = "tampered".to_string();
        assert!(!crypto.verify_proof(&proof));
    }

    // ---- MemoryIdentityStore tests ----

    #[tokio::test]
    async fn store_save_and_get_identity() {
        let store = MemoryIdentityStore::new();
        let identity = PublicIdentity {
            agentos_id: AgentOsId::from("agent-1"),
            device_id: DeviceId::from("device-1"),
            display_name: "Test User".to_string(),
            created_at: ts(),
        };
        store.save_identity(&identity).await.unwrap();
        let fetched = store
            .get_identity(&AgentOsId::from("agent-1"))
            .await
            .unwrap();
        assert_eq!(fetched, identity);
    }

    #[tokio::test]
    async fn store_list_identities() {
        let store = MemoryIdentityStore::new();
        let id1 = PublicIdentity {
            agentos_id: AgentOsId::from("agent-1"),
            device_id: DeviceId::from("device-1"),
            display_name: "User 1".to_string(),
            created_at: ts(),
        };
        let id2 = PublicIdentity {
            agentos_id: AgentOsId::from("agent-2"),
            device_id: DeviceId::from("device-2"),
            display_name: "User 2".to_string(),
            created_at: ts(),
        };
        store.save_identity(&id1).await.unwrap();
        store.save_identity(&id2).await.unwrap();
        let all = store.list_identities().await.unwrap();
        assert_eq!(all.len(), 2);
    }

    #[tokio::test]
    async fn store_get_not_found() {
        let store = MemoryIdentityStore::new();
        let result = store.get_identity(&AgentOsId::from("nonexistent")).await;
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), IdentityError::NotFound(_)));
    }
}
