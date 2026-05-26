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
use std::collections::{BTreeMap, HashMap};
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
// Credential store boundary
// ---------------------------------------------------------------------------

/// Unique identifier for a stored credential.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct CredentialId(pub String);

impl fmt::Display for CredentialId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl From<String> for CredentialId {
    fn from(s: String) -> Self {
        Self(s)
    }
}

impl From<&str> for CredentialId {
    fn from(s: &str) -> Self {
        Self(s.to_string())
    }
}

/// Scope describing where a credential is valid.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum CredentialScope {
    Global,
    Agent { agentos_id: AgentOsId },
    Device { device_id: DeviceId },
    Server { server_id: String },
    Connector { connector_id: String },
    Custom { namespace: String, id: String },
}

/// Non-secret metadata for a credential.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CredentialMetadata {
    pub id: CredentialId,
    pub scope: CredentialScope,
    pub label: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Secret wrapper with explicit exposure and redacted debug output.
#[derive(Clone, PartialEq, Eq)]
pub struct SecretValue(String);

impl SecretValue {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn expose_secret(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for SecretValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("SecretValue").field(&"<redacted>").finish()
    }
}

/// Full credential record. Debug output redacts the secret value.
#[derive(Clone, PartialEq, Eq)]
pub struct CredentialRecord {
    pub metadata: CredentialMetadata,
    pub secret: SecretValue,
}

impl CredentialRecord {
    pub fn new(
        id: CredentialId,
        scope: CredentialScope,
        label: impl Into<String>,
        secret: SecretValue,
        now: DateTime<Utc>,
    ) -> Self {
        Self {
            metadata: CredentialMetadata {
                id,
                scope,
                label: label.into(),
                created_at: now,
                updated_at: now,
            },
            secret,
        }
    }
}

impl fmt::Debug for CredentialRecord {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CredentialRecord")
            .field("metadata", &self.metadata)
            .field("secret", &self.secret)
            .finish()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CredentialError {
    #[error("credential not found: {0}")]
    NotFound(String),
    #[error("credential already exists: {0}")]
    AlreadyExists(String),
    #[error("credential backend unavailable: {0}")]
    BackendUnavailable(String),
    #[error("credential encryption unavailable: {0}")]
    EncryptionUnavailable(String),
    #[error("credential internal error: {0}")]
    Internal(String),
}

/// Trait for storing, retrieving, deleting, and listing credential metadata.
#[async_trait]
pub trait CredentialStore: Send + Sync {
    async fn write(&self, record: CredentialRecord) -> Result<(), CredentialError>;
    async fn read(&self, id: &CredentialId) -> Result<CredentialRecord, CredentialError>;
    async fn delete(&self, id: &CredentialId) -> Result<(), CredentialError>;
    async fn list_metadata(&self) -> Result<Vec<CredentialMetadata>, CredentialError>;
}

/// Deterministic in-memory credential store for tests and local composition.
pub struct MemoryCredentialStore {
    inner: Mutex<BTreeMap<CredentialId, CredentialRecord>>,
}

impl MemoryCredentialStore {
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(BTreeMap::new()),
        }
    }
}

impl Default for MemoryCredentialStore {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl CredentialStore for MemoryCredentialStore {
    async fn write(&self, record: CredentialRecord) -> Result<(), CredentialError> {
        let mut store = self
            .inner
            .lock()
            .map_err(|err| CredentialError::Internal(err.to_string()))?;
        store.insert(record.metadata.id.clone(), record);
        Ok(())
    }

    async fn read(&self, id: &CredentialId) -> Result<CredentialRecord, CredentialError> {
        let store = self
            .inner
            .lock()
            .map_err(|err| CredentialError::Internal(err.to_string()))?;
        store
            .get(id)
            .cloned()
            .ok_or_else(|| CredentialError::NotFound(id.0.clone()))
    }

    async fn delete(&self, id: &CredentialId) -> Result<(), CredentialError> {
        let mut store = self
            .inner
            .lock()
            .map_err(|err| CredentialError::Internal(err.to_string()))?;
        store.remove(id);
        Ok(())
    }

    async fn list_metadata(&self) -> Result<Vec<CredentialMetadata>, CredentialError> {
        let store = self
            .inner
            .lock()
            .map_err(|err| CredentialError::Internal(err.to_string()))?;
        Ok(store
            .values()
            .map(|record| record.metadata.clone())
            .collect())
    }
}

/// Encryption boundary for future encrypted credential file storage.
pub trait CredentialCipher: Send + Sync {
    fn encrypt(&self, plaintext: &[u8]) -> Result<Vec<u8>, CredentialError>;
    fn decrypt(&self, ciphertext: &[u8]) -> Result<Vec<u8>, CredentialError>;
}

/// Skeleton for encrypted file credential storage.
///
/// PR98 intentionally does not implement real filesystem persistence or encryption.
pub struct EncryptedFileCredentialStore {
    path: String,
}

impl EncryptedFileCredentialStore {
    pub fn new(path: impl Into<String>) -> Self {
        Self { path: path.into() }
    }

    pub fn path(&self) -> &str {
        &self.path
    }

    fn skeleton_error(&self) -> CredentialError {
        CredentialError::EncryptionUnavailable(format!(
            "encrypted file credential store skeleton is not yet implemented for {}",
            self.path
        ))
    }
}

impl fmt::Debug for EncryptedFileCredentialStore {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("EncryptedFileCredentialStore")
            .field("path", &self.path)
            .finish()
    }
}

#[async_trait]
impl CredentialStore for EncryptedFileCredentialStore {
    async fn write(&self, _record: CredentialRecord) -> Result<(), CredentialError> {
        Err(self.skeleton_error())
    }

    async fn read(&self, _id: &CredentialId) -> Result<CredentialRecord, CredentialError> {
        Err(self.skeleton_error())
    }

    async fn delete(&self, _id: &CredentialId) -> Result<(), CredentialError> {
        Err(self.skeleton_error())
    }

    async fn list_metadata(&self) -> Result<Vec<CredentialMetadata>, CredentialError> {
        Err(self.skeleton_error())
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

    fn sample_credential(id: &str, secret: &str) -> CredentialRecord {
        CredentialRecord::new(
            CredentialId::from(id),
            CredentialScope::Agent {
                agentos_id: AgentOsId::from("agent-1"),
            },
            format!("Credential {id}"),
            SecretValue::new(secret),
            ts(),
        )
    }

    #[test]
    fn secret_value_debug_is_redacted() {
        let secret = SecretValue::new("super-secret-token");

        let debug = format!("{secret:?}");

        assert_eq!(secret.expose_secret(), "super-secret-token");
        assert!(!debug.contains("super-secret-token"));
        assert!(debug.contains("<redacted>"));
    }

    #[test]
    fn credential_record_debug_is_redacted() {
        let record = sample_credential("cred-1", "secret-value");

        let debug = format!("{record:?}");

        assert!(!debug.contains("secret-value"));
        assert!(debug.contains("<redacted>"));
        assert!(debug.contains("cred-1"));
    }

    #[tokio::test]
    async fn memory_credential_store_write_and_read_secret() {
        let store = MemoryCredentialStore::new();
        let record = sample_credential("cred-1", "secret-value");

        store.write(record.clone()).await.unwrap();
        let fetched = store.read(&CredentialId::from("cred-1")).await.unwrap();

        assert_eq!(fetched.metadata, record.metadata);
        assert_eq!(fetched.secret.expose_secret(), "secret-value");
    }

    #[tokio::test]
    async fn memory_credential_store_delete_secret() {
        let store = MemoryCredentialStore::new();
        store
            .write(sample_credential("cred-1", "secret-value"))
            .await
            .unwrap();

        store.delete(&CredentialId::from("cred-1")).await.unwrap();
        let result = store.read(&CredentialId::from("cred-1")).await;

        assert!(matches!(
            result,
            Err(CredentialError::NotFound(id)) if id == "cred-1"
        ));
    }

    #[tokio::test]
    async fn memory_credential_store_list_metadata_does_not_include_secret() {
        let store = MemoryCredentialStore::new();
        store
            .write(sample_credential("cred-1", "secret-value"))
            .await
            .unwrap();

        let metadata = store.list_metadata().await.unwrap();
        let debug = format!("{metadata:?}");

        assert_eq!(metadata.len(), 1);
        assert_eq!(metadata[0].id, CredentialId::from("cred-1"));
        assert!(!debug.contains("secret-value"));
    }

    #[tokio::test]
    async fn memory_credential_store_list_metadata_is_deterministic() {
        let store = MemoryCredentialStore::new();
        store.write(sample_credential("cred-b", "b")).await.unwrap();
        store.write(sample_credential("cred-a", "a")).await.unwrap();
        store.write(sample_credential("cred-c", "c")).await.unwrap();

        let ids: Vec<_> = store
            .list_metadata()
            .await
            .unwrap()
            .into_iter()
            .map(|metadata| metadata.id.0)
            .collect();

        assert_eq!(ids, vec!["cred-a", "cred-b", "cred-c"]);
    }

    #[tokio::test]
    async fn encrypted_file_credential_store_skeleton_returns_typed_error() {
        let store = EncryptedFileCredentialStore::new("/tmp/agentos-credentials.enc");

        let result = store.read(&CredentialId::from("cred-1")).await;

        assert!(matches!(
            result,
            Err(CredentialError::EncryptionUnavailable(reason)) if reason.contains("skeleton")
        ));
    }

    #[test]
    fn credential_scope_roundtrips_for_agent_and_connector() {
        let agent_scope = CredentialScope::Agent {
            agentos_id: AgentOsId::from("agent-1"),
        };
        let connector_scope = CredentialScope::Connector {
            connector_id: "github".to_string(),
        };

        let agent_json = serde_json::to_string(&agent_scope).unwrap();
        let connector_json = serde_json::to_string(&connector_scope).unwrap();

        assert_eq!(
            serde_json::from_str::<CredentialScope>(&agent_json).unwrap(),
            agent_scope
        );
        assert_eq!(
            serde_json::from_str::<CredentialScope>(&connector_json).unwrap(),
            connector_scope
        );
    }
}
