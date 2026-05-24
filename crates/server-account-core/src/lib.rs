//! # Server Account Core
//!
//! Domain model for server accounts — binding AgentOS identities to multiple servers.
//!
//! Supports the multi-server architecture where one identity can connect to
//! personal self-hosted servers, enterprise-managed servers, and open-source relays.

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt;
use std::sync::Mutex;

// ---------------------------------------------------------------------------
// Core types
// ---------------------------------------------------------------------------

/// Unique identifier for a server.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ServerId(pub String);

impl fmt::Display for ServerId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl From<String> for ServerId {
    fn from(s: String) -> Self {
        Self(s)
    }
}

impl From<&str> for ServerId {
    fn from(s: &str) -> Self {
        Self(s.to_string())
    }
}

/// Server endpoint URL.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ServerEndpoint(pub String);

impl fmt::Display for ServerEndpoint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl From<String> for ServerEndpoint {
    fn from(s: String) -> Self {
        Self(s)
    }
}

impl From<&str> for ServerEndpoint {
    fn from(s: &str) -> Self {
        Self(s.to_string())
    }
}

/// Unique identifier for a server account binding.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ServerAccountId(pub String);

impl fmt::Display for ServerAccountId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl From<String> for ServerAccountId {
    fn from(s: String) -> Self {
        Self(s)
    }
}

impl From<&str> for ServerAccountId {
    fn from(s: &str) -> Self {
        Self(s.to_string())
    }
}

/// Type of server.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ServerKind {
    /// Open-source relay server.
    OpenSourceRelay,
    /// Enterprise-managed server.
    EnterpriseManaged,
    /// Official AgentOS server.
    Official,
    /// Personal self-hosted server.
    PersonalSelfHosted,
}

impl fmt::Display for ServerKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ServerKind::OpenSourceRelay => write!(f, "OpenSourceRelay"),
            ServerKind::EnterpriseManaged => write!(f, "EnterpriseManaged"),
            ServerKind::Official => write!(f, "Official"),
            ServerKind::PersonalSelfHosted => write!(f, "PersonalSelfHosted"),
        }
    }
}

/// Connection policy for a server.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ConnectionPolicy {
    /// Anyone can connect.
    AllowAll,
    /// Requires an invite code.
    RequireInviteCode,
    /// Requires ownership notice acknowledgment (enterprise).
    RequireOwnershipNotice,
    /// Enterprise-managed with specific requirements.
    EnterpriseManaged { domain: String },
}

impl fmt::Display for ConnectionPolicy {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ConnectionPolicy::AllowAll => write!(f, "AllowAll"),
            ConnectionPolicy::RequireInviteCode => write!(f, "RequireInviteCode"),
            ConnectionPolicy::RequireOwnershipNotice => write!(f, "RequireOwnershipNotice"),
            ConnectionPolicy::EnterpriseManaged { domain } => {
                write!(f, "EnterpriseManaged({})", domain)
            }
        }
    }
}

/// Trust status of a server.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ServerTrustStatus {
    /// Server is trusted.
    Trusted,
    /// Server has not been verified.
    Unverified,
    /// Server is blocked.
    Blocked,
}

impl fmt::Display for ServerTrustStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ServerTrustStatus::Trusted => write!(f, "Trusted"),
            ServerTrustStatus::Unverified => write!(f, "Unverified"),
            ServerTrustStatus::Blocked => write!(f, "Blocked"),
        }
    }
}

/// A registered server.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ServerRegistration {
    pub id: ServerId,
    pub endpoint: ServerEndpoint,
    pub kind: ServerKind,
    pub connection_policy: ConnectionPolicy,
    pub trust_status: ServerTrustStatus,
    pub display_name: String,
    pub registered_at: DateTime<Utc>,
}

/// A binding between an AgentOS identity and a server account.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ServerAccountBinding {
    pub id: ServerAccountId,
    pub agentos_id: String,
    pub server_id: ServerId,
    pub bound_at: DateTime<Utc>,
}

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ServerAccountError {
    #[error("server not found: {0}")]
    ServerNotFound(String),
    #[error("binding not found: {0}")]
    BindingNotFound(String),
    #[error("server already registered: {0}")]
    AlreadyRegistered(String),
    #[error("connection policy violation: {0}")]
    PolicyViolation(String),
    #[error("internal error: {0}")]
    Internal(String),
}

// ---------------------------------------------------------------------------
// ServerRegistry trait
// ---------------------------------------------------------------------------

/// Trait for managing server registrations.
#[async_trait]
pub trait ServerRegistry: Send + Sync {
    async fn register(&self, server: &ServerRegistration) -> Result<(), ServerAccountError>;
    async fn get(&self, id: &ServerId) -> Result<ServerRegistration, ServerAccountError>;
    async fn list(&self) -> Result<Vec<ServerRegistration>, ServerAccountError>;
    async fn unregister(&self, id: &ServerId) -> Result<(), ServerAccountError>;
}

// ---------------------------------------------------------------------------
// ServerAccountStore trait
// ---------------------------------------------------------------------------

/// Trait for managing account bindings.
#[async_trait]
pub trait ServerAccountStore: Send + Sync {
    async fn bind(&self, binding: &ServerAccountBinding) -> Result<(), ServerAccountError>;
    async fn unbind(&self, id: &ServerAccountId) -> Result<(), ServerAccountError>;
    async fn get(&self, id: &ServerAccountId) -> Result<ServerAccountBinding, ServerAccountError>;
    async fn list_by_agent(
        &self,
        agentos_id: &str,
    ) -> Result<Vec<ServerAccountBinding>, ServerAccountError>;
    async fn list_by_server(
        &self,
        server_id: &ServerId,
    ) -> Result<Vec<ServerAccountBinding>, ServerAccountError>;
}

// ---------------------------------------------------------------------------
// MemoryServerRegistry
// ---------------------------------------------------------------------------

pub struct MemoryServerRegistry {
    inner: Mutex<HashMap<String, ServerRegistration>>,
}

impl MemoryServerRegistry {
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(HashMap::new()),
        }
    }
}

impl Default for MemoryServerRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl ServerRegistry for MemoryServerRegistry {
    async fn register(&self, server: &ServerRegistration) -> Result<(), ServerAccountError> {
        let mut store = self.inner.lock().unwrap();
        if store.contains_key(&server.id.0) {
            return Err(ServerAccountError::AlreadyRegistered(server.id.0.clone()));
        }
        store.insert(server.id.0.clone(), server.clone());
        Ok(())
    }

    async fn get(&self, id: &ServerId) -> Result<ServerRegistration, ServerAccountError> {
        let store = self.inner.lock().unwrap();
        store
            .get(&id.0)
            .cloned()
            .ok_or_else(|| ServerAccountError::ServerNotFound(id.0.clone()))
    }

    async fn list(&self) -> Result<Vec<ServerRegistration>, ServerAccountError> {
        let store = self.inner.lock().unwrap();
        Ok(store.values().cloned().collect())
    }

    async fn unregister(&self, id: &ServerId) -> Result<(), ServerAccountError> {
        let mut store = self.inner.lock().unwrap();
        store.remove(&id.0);
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// MemoryServerAccountStore
// ---------------------------------------------------------------------------

pub struct MemoryServerAccountStore {
    inner: Mutex<HashMap<String, ServerAccountBinding>>,
}

impl MemoryServerAccountStore {
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(HashMap::new()),
        }
    }
}

impl Default for MemoryServerAccountStore {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl ServerAccountStore for MemoryServerAccountStore {
    async fn bind(&self, binding: &ServerAccountBinding) -> Result<(), ServerAccountError> {
        let mut store = self.inner.lock().unwrap();
        store.insert(binding.id.0.clone(), binding.clone());
        Ok(())
    }

    async fn unbind(&self, id: &ServerAccountId) -> Result<(), ServerAccountError> {
        let mut store = self.inner.lock().unwrap();
        store.remove(&id.0);
        Ok(())
    }

    async fn get(&self, id: &ServerAccountId) -> Result<ServerAccountBinding, ServerAccountError> {
        let store = self.inner.lock().unwrap();
        store
            .get(&id.0)
            .cloned()
            .ok_or_else(|| ServerAccountError::BindingNotFound(id.0.clone()))
    }

    async fn list_by_agent(
        &self,
        agentos_id: &str,
    ) -> Result<Vec<ServerAccountBinding>, ServerAccountError> {
        let store = self.inner.lock().unwrap();
        Ok(store
            .values()
            .filter(|b| b.agentos_id == agentos_id)
            .cloned()
            .collect())
    }

    async fn list_by_server(
        &self,
        server_id: &ServerId,
    ) -> Result<Vec<ServerAccountBinding>, ServerAccountError> {
        let store = self.inner.lock().unwrap();
        Ok(store
            .values()
            .filter(|b| b.server_id == *server_id)
            .cloned()
            .collect())
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

    fn sample_server(id: &str) -> ServerRegistration {
        ServerRegistration {
            id: ServerId::from(id),
            endpoint: ServerEndpoint::from(format!("https://{}.example.com", id)),
            kind: ServerKind::PersonalSelfHosted,
            connection_policy: ConnectionPolicy::AllowAll,
            trust_status: ServerTrustStatus::Unverified,
            display_name: format!("Server {}", id),
            registered_at: ts(),
        }
    }

    fn sample_binding(id: &str, agentos_id: &str, server_id: &str) -> ServerAccountBinding {
        ServerAccountBinding {
            id: ServerAccountId::from(id),
            agentos_id: agentos_id.to_string(),
            server_id: ServerId::from(server_id),
            bound_at: ts(),
        }
    }

    // ---- Type roundtrip tests ----

    #[test]
    fn server_id_roundtrips() {
        let id = ServerId::from("server-1");
        assert_eq!(id.to_string(), "server-1");
        let json = serde_json::to_string(&id).unwrap();
        let decoded: ServerId = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded, id);
    }

    #[test]
    fn server_registration_roundtrips() {
        let server = sample_server("s-1");
        let json = serde_json::to_string_pretty(&server).unwrap();
        let decoded: ServerRegistration = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded, server);
    }

    #[test]
    fn server_account_binding_roundtrips() {
        let binding = sample_binding("b-1", "agent-1", "server-1");
        let json = serde_json::to_string_pretty(&binding).unwrap();
        let decoded: ServerAccountBinding = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded, binding);
    }

    #[test]
    fn server_kind_roundtrips() {
        let kinds = vec![
            ServerKind::OpenSourceRelay,
            ServerKind::EnterpriseManaged,
            ServerKind::Official,
            ServerKind::PersonalSelfHosted,
        ];
        for kind in kinds {
            let json = serde_json::to_string(&kind).unwrap();
            let decoded: ServerKind = serde_json::from_str(&json).unwrap();
            assert_eq!(decoded, kind);
        }
    }

    #[test]
    fn connection_policy_roundtrips() {
        let policies = vec![
            ConnectionPolicy::AllowAll,
            ConnectionPolicy::RequireInviteCode,
            ConnectionPolicy::RequireOwnershipNotice,
            ConnectionPolicy::EnterpriseManaged {
                domain: "corp.com".to_string(),
            },
        ];
        for policy in policies {
            let json = serde_json::to_string(&policy).unwrap();
            let decoded: ConnectionPolicy = serde_json::from_str(&json).unwrap();
            assert_eq!(decoded, policy);
        }
    }

    // ---- ServerRegistry tests ----

    #[tokio::test]
    async fn registry_register_and_list() {
        let registry = MemoryServerRegistry::new();
        let s1 = sample_server("s-1");
        let s2 = sample_server("s-2");
        registry.register(&s1).await.unwrap();
        registry.register(&s2).await.unwrap();
        let all = registry.list().await.unwrap();
        assert_eq!(all.len(), 2);
    }

    #[tokio::test]
    async fn registry_rejects_duplicate_registration() {
        let registry = MemoryServerRegistry::new();
        let s1 = sample_server("s-1");
        registry.register(&s1).await.unwrap();
        let result = registry.register(&s1).await;
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            ServerAccountError::AlreadyRegistered(_)
        ));
    }

    #[tokio::test]
    async fn registry_get_not_found() {
        let registry = MemoryServerRegistry::new();
        let result = registry.get(&ServerId::from("nonexistent")).await;
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            ServerAccountError::ServerNotFound(_)
        ));
    }

    // ---- ServerAccountStore tests ----

    #[tokio::test]
    async fn store_bind_and_list_by_agent() {
        let store = MemoryServerAccountStore::new();
        let b1 = sample_binding("b-1", "agent-1", "server-1");
        let b2 = sample_binding("b-2", "agent-1", "server-2");
        store.bind(&b1).await.unwrap();
        store.bind(&b2).await.unwrap();
        let bindings = store.list_by_agent("agent-1").await.unwrap();
        assert_eq!(bindings.len(), 2);
    }

    #[tokio::test]
    async fn store_list_by_server() {
        let store = MemoryServerAccountStore::new();
        let b1 = sample_binding("b-1", "agent-1", "server-1");
        let b2 = sample_binding("b-2", "agent-2", "server-1");
        store.bind(&b1).await.unwrap();
        store.bind(&b2).await.unwrap();
        let bindings = store
            .list_by_server(&ServerId::from("server-1"))
            .await
            .unwrap();
        assert_eq!(bindings.len(), 2);
    }

    #[tokio::test]
    async fn store_unbind_removes_binding() {
        let store = MemoryServerAccountStore::new();
        let b1 = sample_binding("b-1", "agent-1", "server-1");
        store.bind(&b1).await.unwrap();
        store.unbind(&ServerAccountId::from("b-1")).await.unwrap();
        let result = store.get(&ServerAccountId::from("b-1")).await;
        assert!(result.is_err());
    }
}
