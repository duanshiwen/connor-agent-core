//! # Identity Core
//!
//! Domain model for AgentOS identity — local sovereignty, device IDs, and fake crypto.
//!
//! The identity model supports client-side sovereignty: the agentos_id is generated
//! locally and is not bound to any single server. The same identity can bind to
//! multiple servers.

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use rand_core::OsRng;
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
    #[error("invalid key: {0}")]
    InvalidKey(String),
    #[error("invalid encoding: {0}")]
    InvalidEncoding(String),
    #[error("crypto error: {0}")]
    Crypto(String),
    #[error("internal error: {0}")]
    Internal(String),
}

// ---------------------------------------------------------------------------
// Real crypto provider boundary
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CryptoAlgorithm {
    Ed25519,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KeyEncoding {
    Hex,
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SerializedPrivateKey {
    pub algorithm: CryptoAlgorithm,
    pub encoding: KeyEncoding,
    pub value: String,
}

impl fmt::Debug for SerializedPrivateKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SerializedPrivateKey")
            .field("algorithm", &self.algorithm)
            .field("encoding", &self.encoding)
            .field("value", &"<redacted>")
            .finish()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SerializedPublicKey {
    pub algorithm: CryptoAlgorithm,
    pub encoding: KeyEncoding,
    pub value: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SerializedSignature {
    pub algorithm: CryptoAlgorithm,
    pub encoding: KeyEncoding,
    pub value: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CryptoKeyPair {
    pub private_key: SerializedPrivateKey,
    pub public_key: SerializedPublicKey,
}

pub trait CryptoProvider: Send + Sync {
    fn generate_keypair(&self) -> Result<CryptoKeyPair, IdentityError>;
    fn sign(
        &self,
        message: &[u8],
        private_key: &SerializedPrivateKey,
    ) -> Result<SerializedSignature, IdentityError>;
    fn verify(
        &self,
        message: &[u8],
        signature: &SerializedSignature,
        public_key: &SerializedPublicKey,
    ) -> Result<bool, IdentityError>;
}

#[derive(Debug, Clone, Copy, Default)]
pub struct Ed25519CryptoProvider;

impl Ed25519CryptoProvider {
    pub fn new() -> Self {
        Self
    }

    pub fn sign_challenge(
        &self,
        challenge: &str,
        private_key: &SerializedPrivateKey,
        public_key: &SerializedPublicKey,
    ) -> Result<SignedChallenge, IdentityError> {
        let signature = self.sign(challenge.as_bytes(), private_key)?;
        Ok(SignedChallenge {
            challenge: challenge.to_string(),
            signature: signature.value,
            public_key: public_key.value.clone(),
        })
    }

    pub fn verify_challenge(&self, signed: &SignedChallenge) -> Result<bool, IdentityError> {
        let signature = SerializedSignature {
            algorithm: CryptoAlgorithm::Ed25519,
            encoding: KeyEncoding::Hex,
            value: signed.signature.clone(),
        };
        let public_key = SerializedPublicKey {
            algorithm: CryptoAlgorithm::Ed25519,
            encoding: KeyEncoding::Hex,
            value: signed.public_key.clone(),
        };
        self.verify(signed.challenge.as_bytes(), &signature, &public_key)
    }
}

impl CryptoProvider for Ed25519CryptoProvider {
    fn generate_keypair(&self) -> Result<CryptoKeyPair, IdentityError> {
        let mut rng = OsRng;
        let signing_key = SigningKey::generate(&mut rng);
        let verifying_key = signing_key.verifying_key();

        Ok(CryptoKeyPair {
            private_key: SerializedPrivateKey {
                algorithm: CryptoAlgorithm::Ed25519,
                encoding: KeyEncoding::Hex,
                value: encode_hex(&signing_key.to_bytes()),
            },
            public_key: SerializedPublicKey {
                algorithm: CryptoAlgorithm::Ed25519,
                encoding: KeyEncoding::Hex,
                value: encode_hex(&verifying_key.to_bytes()),
            },
        })
    }

    fn sign(
        &self,
        message: &[u8],
        private_key: &SerializedPrivateKey,
    ) -> Result<SerializedSignature, IdentityError> {
        ensure_ed25519_hex(private_key.algorithm, private_key.encoding)?;
        let private_key_bytes = decode_hex_exact(&private_key.value, 32)?;
        let private_key_array: [u8; 32] = private_key_bytes.try_into().map_err(|_| {
            IdentityError::InvalidKey("ed25519 private key must be 32 bytes".to_string())
        })?;
        let signing_key = SigningKey::from_bytes(&private_key_array);
        let signature = signing_key.sign(message);

        Ok(SerializedSignature {
            algorithm: CryptoAlgorithm::Ed25519,
            encoding: KeyEncoding::Hex,
            value: encode_hex(&signature.to_bytes()),
        })
    }

    fn verify(
        &self,
        message: &[u8],
        signature: &SerializedSignature,
        public_key: &SerializedPublicKey,
    ) -> Result<bool, IdentityError> {
        ensure_ed25519_hex(signature.algorithm, signature.encoding)?;
        ensure_ed25519_hex(public_key.algorithm, public_key.encoding)?;

        let public_key_bytes = decode_hex_exact(&public_key.value, 32)?;
        let public_key_array: [u8; 32] = public_key_bytes.try_into().map_err(|_| {
            IdentityError::InvalidKey("ed25519 public key must be 32 bytes".to_string())
        })?;
        let verifying_key = VerifyingKey::from_bytes(&public_key_array)
            .map_err(|err| IdentityError::InvalidKey(err.to_string()))?;

        let signature_bytes = decode_hex_exact(&signature.value, 64)?;
        let signature_array: [u8; 64] = signature_bytes.try_into().map_err(|_| {
            IdentityError::InvalidKey("ed25519 signature must be 64 bytes".to_string())
        })?;
        let signature = Signature::from_bytes(&signature_array);

        Ok(verifying_key.verify(message, &signature).is_ok())
    }
}

fn ensure_ed25519_hex(
    algorithm: CryptoAlgorithm,
    encoding: KeyEncoding,
) -> Result<(), IdentityError> {
    if algorithm != CryptoAlgorithm::Ed25519 {
        return Err(IdentityError::InvalidKey(format!(
            "unsupported algorithm: {algorithm:?}"
        )));
    }
    if encoding != KeyEncoding::Hex {
        return Err(IdentityError::InvalidEncoding(format!(
            "unsupported encoding: {encoding:?}"
        )));
    }
    Ok(())
}

fn encode_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(HEX[(byte >> 4) as usize] as char);
        output.push(HEX[(byte & 0x0f) as usize] as char);
    }
    output
}

fn decode_hex_exact(value: &str, expected_len: usize) -> Result<Vec<u8>, IdentityError> {
    if value.len() != expected_len * 2 {
        return Err(IdentityError::InvalidEncoding(format!(
            "hex value must be {} bytes / {} chars, got {} chars",
            expected_len,
            expected_len * 2,
            value.len()
        )));
    }

    let mut output = Vec::with_capacity(expected_len);
    let bytes = value.as_bytes();
    for pair in bytes.chunks_exact(2) {
        let high = decode_hex_nibble(pair[0])?;
        let low = decode_hex_nibble(pair[1])?;
        output.push((high << 4) | low);
    }
    Ok(output)
}

fn decode_hex_nibble(byte: u8) -> Result<u8, IdentityError> {
    match byte {
        b'0'..=b'9' => Ok(byte - b'0'),
        b'a'..=b'f' => Ok(byte - b'a' + 10),
        b'A'..=b'F' => Ok(byte - b'A' + 10),
        _ => Err(IdentityError::InvalidEncoding(
            "hex value contains non-hex character".to_string(),
        )),
    }
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

/// Minimal backend boundary for a macOS Keychain-like secret store.
pub trait MacOsKeychainBackend: Send + Sync {
    fn write_secret(
        &self,
        service: &str,
        account: &str,
        secret: &str,
    ) -> Result<(), CredentialError>;
    fn read_secret(&self, service: &str, account: &str) -> Result<String, CredentialError>;
    fn delete_secret(&self, service: &str, account: &str) -> Result<(), CredentialError>;
}

/// CredentialStore backed by a macOS Keychain-like backend plus in-memory metadata sidecar.
///
/// The backend stores only secret values. Metadata is kept in a deterministic in-memory sidecar
/// for PR99 and can be replaced by a persistent metadata store in a later PR.
pub struct MacOsKeychainCredentialStore<B> {
    service: String,
    backend: B,
    metadata: Mutex<BTreeMap<CredentialId, CredentialMetadata>>,
}

impl<B> MacOsKeychainCredentialStore<B>
where
    B: MacOsKeychainBackend,
{
    pub fn new(service: impl Into<String>, backend: B) -> Self {
        Self {
            service: service.into(),
            backend,
            metadata: Mutex::new(BTreeMap::new()),
        }
    }

    pub fn service(&self) -> &str {
        &self.service
    }

    fn account_for(id: &CredentialId) -> &str {
        id.0.as_str()
    }
}

impl<B> fmt::Debug for MacOsKeychainCredentialStore<B> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let metadata_count = self
            .metadata
            .lock()
            .map(|metadata| metadata.len())
            .unwrap_or(0);
        f.debug_struct("MacOsKeychainCredentialStore")
            .field("service", &self.service)
            .field("metadata_count", &metadata_count)
            .finish_non_exhaustive()
    }
}

#[async_trait]
impl<B> CredentialStore for MacOsKeychainCredentialStore<B>
where
    B: MacOsKeychainBackend,
{
    async fn write(&self, record: CredentialRecord) -> Result<(), CredentialError> {
        self.backend.write_secret(
            &self.service,
            Self::account_for(&record.metadata.id),
            record.secret.expose_secret(),
        )?;
        let mut metadata = self
            .metadata
            .lock()
            .map_err(|err| CredentialError::Internal(err.to_string()))?;
        metadata.insert(record.metadata.id.clone(), record.metadata);
        Ok(())
    }

    async fn read(&self, id: &CredentialId) -> Result<CredentialRecord, CredentialError> {
        let metadata = {
            let metadata = self
                .metadata
                .lock()
                .map_err(|err| CredentialError::Internal(err.to_string()))?;
            metadata
                .get(id)
                .cloned()
                .ok_or_else(|| CredentialError::NotFound(id.0.clone()))?
        };
        let secret = self
            .backend
            .read_secret(&self.service, Self::account_for(id))?;
        Ok(CredentialRecord {
            metadata,
            secret: SecretValue::new(secret),
        })
    }

    async fn delete(&self, id: &CredentialId) -> Result<(), CredentialError> {
        self.backend
            .delete_secret(&self.service, Self::account_for(id))?;
        let mut metadata = self
            .metadata
            .lock()
            .map_err(|err| CredentialError::Internal(err.to_string()))?;
        metadata.remove(id);
        Ok(())
    }

    async fn list_metadata(&self) -> Result<Vec<CredentialMetadata>, CredentialError> {
        let metadata = self
            .metadata
            .lock()
            .map_err(|err| CredentialError::Internal(err.to_string()))?;
        Ok(metadata.values().cloned().collect())
    }
}

#[cfg(all(feature = "macos-keychain", target_os = "macos"))]
#[derive(Debug, Clone, Copy, Default)]
pub struct SystemMacOsKeychainBackend;

#[cfg(all(feature = "macos-keychain", target_os = "macos"))]
impl MacOsKeychainBackend for SystemMacOsKeychainBackend {
    fn write_secret(
        &self,
        _service: &str,
        _account: &str,
        _secret: &str,
    ) -> Result<(), CredentialError> {
        Err(CredentialError::BackendUnavailable(
            "system macOS Keychain backend is feature-gated but not linked to Security.framework in PR99"
                .to_string(),
        ))
    }

    fn read_secret(&self, _service: &str, _account: &str) -> Result<String, CredentialError> {
        Err(CredentialError::BackendUnavailable(
            "system macOS Keychain backend is feature-gated but not linked to Security.framework in PR99"
                .to_string(),
        ))
    }

    fn delete_secret(&self, _service: &str, _account: &str) -> Result<(), CredentialError> {
        Err(CredentialError::BackendUnavailable(
            "system macOS Keychain backend is feature-gated but not linked to Security.framework in PR99"
                .to_string(),
        ))
    }
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

    #[derive(Default)]
    struct MockMacOsKeychainBackend {
        inner: Mutex<BTreeMap<(String, String), String>>,
        fail_next_read: Mutex<Option<CredentialError>>,
    }

    impl MockMacOsKeychainBackend {
        fn fail_next_read(&self, error: CredentialError) {
            *self.fail_next_read.lock().unwrap() = Some(error);
        }
    }

    impl MacOsKeychainBackend for MockMacOsKeychainBackend {
        fn write_secret(
            &self,
            service: &str,
            account: &str,
            secret: &str,
        ) -> Result<(), CredentialError> {
            self.inner.lock().unwrap().insert(
                (service.to_string(), account.to_string()),
                secret.to_string(),
            );
            Ok(())
        }

        fn read_secret(&self, service: &str, account: &str) -> Result<String, CredentialError> {
            if let Some(error) = self.fail_next_read.lock().unwrap().take() {
                return Err(error);
            }
            self.inner
                .lock()
                .unwrap()
                .get(&(service.to_string(), account.to_string()))
                .cloned()
                .ok_or_else(|| CredentialError::NotFound(account.to_string()))
        }

        fn delete_secret(&self, service: &str, account: &str) -> Result<(), CredentialError> {
            self.inner
                .lock()
                .unwrap()
                .remove(&(service.to_string(), account.to_string()));
            Ok(())
        }
    }

    #[tokio::test]
    async fn macos_keychain_store_mock_write_and_read_secret() {
        let store =
            MacOsKeychainCredentialStore::new("agentos.test", MockMacOsKeychainBackend::default());
        let record = sample_credential("cred-1", "keychain-secret");

        store.write(record.clone()).await.unwrap();
        let fetched = store.read(&CredentialId::from("cred-1")).await.unwrap();

        assert_eq!(fetched.metadata, record.metadata);
        assert_eq!(fetched.secret.expose_secret(), "keychain-secret");
    }

    #[tokio::test]
    async fn macos_keychain_store_mock_delete_secret() {
        let store =
            MacOsKeychainCredentialStore::new("agentos.test", MockMacOsKeychainBackend::default());
        store
            .write(sample_credential("cred-1", "keychain-secret"))
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
    async fn macos_keychain_store_mock_list_metadata_is_deterministic() {
        let store =
            MacOsKeychainCredentialStore::new("agentos.test", MockMacOsKeychainBackend::default());
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

    #[test]
    fn macos_keychain_store_debug_does_not_leak_secret() {
        let store =
            MacOsKeychainCredentialStore::new("agentos.test", MockMacOsKeychainBackend::default());

        let debug = format!("{store:?}");

        assert!(debug.contains("agentos.test"));
        assert!(!debug.contains("keychain-secret"));
    }

    #[tokio::test]
    async fn macos_keychain_backend_error_maps_to_credential_error() {
        let backend = MockMacOsKeychainBackend::default();
        backend.fail_next_read(CredentialError::BackendUnavailable(
            "mock offline".to_string(),
        ));
        let store = MacOsKeychainCredentialStore::new("agentos.test", backend);
        store
            .write(sample_credential("cred-1", "keychain-secret"))
            .await
            .unwrap();

        let result = store.read(&CredentialId::from("cred-1")).await;

        assert!(matches!(
            result,
            Err(CredentialError::BackendUnavailable(reason)) if reason == "mock offline"
        ));
    }

    #[cfg(all(feature = "macos-keychain", target_os = "macos"))]
    #[tokio::test]
    async fn real_macos_keychain_roundtrip_env_gated() {
        if std::env::var("AGENTOS_RUN_KEYCHAIN_TESTS").ok().as_deref() != Some("1") {
            return;
        }

        let store =
            MacOsKeychainCredentialStore::new("agentos.test.real", SystemMacOsKeychainBackend);
        let id = CredentialId::from("agentos-test-real-keychain");
        let mut record = sample_credential(&id.0, "real-keychain-secret");
        record.metadata.label = "Real Keychain Test".to_string();

        store.write(record).await.unwrap();
        let fetched = store.read(&id).await.unwrap();
        assert_eq!(fetched.secret.expose_secret(), "real-keychain-secret");
        store.delete(&id).await.unwrap();
    }

    #[test]
    fn ed25519_generate_keypair_uses_hex_serialization_policy() {
        let crypto = Ed25519CryptoProvider::new();
        let keypair = crypto.generate_keypair().unwrap();

        assert_eq!(keypair.private_key.algorithm, CryptoAlgorithm::Ed25519);
        assert_eq!(keypair.private_key.encoding, KeyEncoding::Hex);
        assert_eq!(keypair.private_key.value.len(), 64);
        assert!(
            keypair
                .private_key
                .value
                .chars()
                .all(|ch| ch.is_ascii_hexdigit())
        );
        assert_eq!(keypair.public_key.algorithm, CryptoAlgorithm::Ed25519);
        assert_eq!(keypair.public_key.encoding, KeyEncoding::Hex);
        assert_eq!(keypair.public_key.value.len(), 64);
        assert!(
            keypair
                .public_key
                .value
                .chars()
                .all(|ch| ch.is_ascii_hexdigit())
        );
    }

    #[test]
    fn ed25519_sign_and_verify_roundtrip() {
        let crypto = Ed25519CryptoProvider::new();
        let keypair = crypto.generate_keypair().unwrap();
        let signature = crypto.sign(b"challenge-100", &keypair.private_key).unwrap();

        assert_eq!(signature.algorithm, CryptoAlgorithm::Ed25519);
        assert_eq!(signature.encoding, KeyEncoding::Hex);
        assert_eq!(signature.value.len(), 128);
        assert!(
            crypto
                .verify(b"challenge-100", &signature, &keypair.public_key)
                .unwrap()
        );
    }

    #[test]
    fn ed25519_verify_rejects_tampered_message() {
        let crypto = Ed25519CryptoProvider::new();
        let keypair = crypto.generate_keypair().unwrap();
        let signature = crypto.sign(b"challenge-100", &keypair.private_key).unwrap();

        assert!(
            !crypto
                .verify(b"tampered", &signature, &keypair.public_key)
                .unwrap()
        );
    }

    #[test]
    fn ed25519_verify_rejects_wrong_public_key() {
        let crypto = Ed25519CryptoProvider::new();
        let keypair_a = crypto.generate_keypair().unwrap();
        let keypair_b = crypto.generate_keypair().unwrap();
        let signature = crypto
            .sign(b"challenge-100", &keypair_a.private_key)
            .unwrap();

        assert!(
            !crypto
                .verify(b"challenge-100", &signature, &keypair_b.public_key)
                .unwrap()
        );
    }

    #[test]
    fn serialized_private_key_debug_is_redacted() {
        let key = SerializedPrivateKey {
            algorithm: CryptoAlgorithm::Ed25519,
            encoding: KeyEncoding::Hex,
            value: "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".to_string(),
        };

        let debug = format!("{key:?}");

        assert!(debug.contains("SerializedPrivateKey"));
        assert!(debug.contains("<redacted>"));
        assert!(!debug.contains(&key.value));
    }

    #[test]
    fn ed25519_signed_challenge_roundtrip() {
        let crypto = Ed25519CryptoProvider::new();
        let keypair = crypto.generate_keypair().unwrap();
        let signed = crypto
            .sign_challenge("nonce-100", &keypair.private_key, &keypair.public_key)
            .unwrap();

        assert_eq!(signed.challenge, "nonce-100");
        assert!(crypto.verify_challenge(&signed).unwrap());
    }

    #[test]
    fn ed25519_invalid_key_returns_typed_error() {
        let crypto = Ed25519CryptoProvider::new();
        let bad_key = SerializedPrivateKey {
            algorithm: CryptoAlgorithm::Ed25519,
            encoding: KeyEncoding::Hex,
            value: "not-hex".to_string(),
        };

        let result = crypto.sign(b"challenge-100", &bad_key);

        assert!(
            matches!(result, Err(IdentityError::InvalidEncoding(reason)) if reason.contains("hex"))
        );
    }
}
