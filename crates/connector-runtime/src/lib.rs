//! # Connector Runtime
//!
//! Runtime for managing identity, server accounts, and multi-server connections.
//!
//! Coordinates identity-core and server-account-core to provide a unified API
//! for registering servers, binding accounts, and verifying challenges.

use chrono::Utc;
use identity_core::{AgentOsId, DeviceId, FakeCryptoProvider, IdentityStore, PublicIdentity};
use server_account_core::{
    ConnectionPolicy, MemoryServerAccountStore, MemoryServerRegistry, ServerAccountBinding,
    ServerAccountError, ServerAccountId, ServerAccountStore, ServerEndpoint, ServerId, ServerKind,
    ServerRegistration, ServerRegistry, ServerTrustStatus,
};
use std::sync::Arc;

// ---------------------------------------------------------------------------
// ConnectorRuntime
// ---------------------------------------------------------------------------

/// Unified runtime coordinating identity, server registry, and account bindings.
pub struct ConnectorRuntime {
    identity_store: Arc<dyn IdentityStore>,
    server_registry: Arc<dyn ServerRegistry>,
    account_store: Arc<dyn ServerAccountStore>,
    crypto: FakeCryptoProvider,
}

impl ConnectorRuntime {
    pub fn new(
        identity_store: Arc<dyn IdentityStore>,
        server_registry: Arc<dyn ServerRegistry>,
        account_store: Arc<dyn ServerAccountStore>,
    ) -> Self {
        Self {
            identity_store,
            server_registry,
            account_store,
            crypto: FakeCryptoProvider,
        }
    }

    /// Create a connector runtime with in-memory stores.
    pub fn with_memory_stores() -> Self {
        Self::new(
            Arc::new(identity_core::MemoryIdentityStore::new()),
            Arc::new(MemoryServerRegistry::new()),
            Arc::new(MemoryServerAccountStore::new()),
        )
    }

    /// Create a local identity.
    pub async fn create_identity(
        &self,
        display_name: &str,
    ) -> Result<PublicIdentity, ConnectorError> {
        let agentos_id = AgentOsId(format!("agent-{}", uuid_simple()));
        let device_id = DeviceId(format!("device-{}", uuid_simple()));
        let identity = PublicIdentity {
            agentos_id,
            device_id,
            display_name: display_name.to_string(),
            created_at: Utc::now(),
        };
        self.identity_store.save_identity(&identity).await?;
        Ok(identity)
    }

    /// Register a new server.
    pub async fn register_server(
        &self,
        endpoint: &str,
        kind: ServerKind,
        connection_policy: ConnectionPolicy,
        display_name: &str,
    ) -> Result<ServerId, ConnectorError> {
        let id = ServerId(format!("server-{}", uuid_simple()));
        let server = ServerRegistration {
            id: id.clone(),
            endpoint: ServerEndpoint::from(endpoint),
            kind,
            connection_policy,
            trust_status: ServerTrustStatus::Unverified,
            display_name: display_name.to_string(),
            registered_at: Utc::now(),
        };
        self.server_registry
            .register(&server)
            .await
            .map_err(ConnectorError::ServerAccount)?;
        Ok(id)
    }

    /// Bind an AgentOS identity to a server.
    pub async fn bind_account(
        &self,
        agentos_id: &str,
        server_id: &ServerId,
    ) -> Result<ServerAccountBinding, ConnectorError> {
        // Verify server exists
        self.server_registry
            .get(server_id)
            .await
            .map_err(ConnectorError::ServerAccount)?;

        let binding = ServerAccountBinding {
            id: ServerAccountId(format!("binding-{}", uuid_simple())),
            agentos_id: agentos_id.to_string(),
            server_id: server_id.clone(),
            bound_at: Utc::now(),
        };
        self.account_store
            .bind(&binding)
            .await
            .map_err(ConnectorError::ServerAccount)?;
        Ok(binding)
    }

    /// Verify a server challenge using fake crypto.
    pub fn verify_challenge(&self, challenge: &str, signature: &str) -> bool {
        self.crypto.verify(challenge, signature)
    }

    /// Check if enterprise ownership notice is required for a server.
    pub async fn check_enterprise_notice_required(
        &self,
        server_id: &ServerId,
    ) -> Result<bool, ConnectorError> {
        let server = self
            .server_registry
            .get(server_id)
            .await
            .map_err(ConnectorError::ServerAccount)?;
        Ok(matches!(
            server.connection_policy,
            ConnectionPolicy::RequireOwnershipNotice | ConnectionPolicy::EnterpriseManaged { .. }
        ))
    }

    /// List all servers.
    pub async fn list_servers(&self) -> Result<Vec<ServerRegistration>, ConnectorError> {
        self.server_registry
            .list()
            .await
            .map_err(ConnectorError::ServerAccount)
    }

    /// List all bindings for an agent.
    pub async fn list_bindings(
        &self,
        agentos_id: &str,
    ) -> Result<Vec<ServerAccountBinding>, ConnectorError> {
        self.account_store
            .list_by_agent(agentos_id)
            .await
            .map_err(ConnectorError::ServerAccount)
    }

    /// Disconnect a server (unregister + unbind all).
    pub async fn disconnect_server(&self, server_id: &ServerId) -> Result<(), ConnectorError> {
        // Unbind all accounts for this server
        let bindings = self
            .account_store
            .list_by_server(server_id)
            .await
            .map_err(ConnectorError::ServerAccount)?;
        for binding in bindings {
            self.account_store
                .unbind(&binding.id)
                .await
                .map_err(ConnectorError::ServerAccount)?;
        }
        // Unregister server
        self.server_registry
            .unregister(server_id)
            .await
            .map_err(ConnectorError::ServerAccount)?;
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

#[derive(Debug, thiserror::Error)]
pub enum ConnectorError {
    #[error("server account error: {0}")]
    ServerAccount(#[from] ServerAccountError),
    #[error("identity error: {0}")]
    Identity(#[from] identity_core::IdentityError),
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn uuid_simple() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let t = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    format!("{:x}", t)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn register_server_returns_id() {
        let runtime = ConnectorRuntime::with_memory_stores();
        let server_id = runtime
            .register_server(
                "https://my-server.example.com",
                ServerKind::PersonalSelfHosted,
                ConnectionPolicy::AllowAll,
                "My Server",
            )
            .await
            .unwrap();

        let servers = runtime.list_servers().await.unwrap();
        assert_eq!(servers.len(), 1);
        assert_eq!(servers[0].id, server_id);
    }

    #[tokio::test]
    async fn bind_account_creates_binding() {
        let runtime = ConnectorRuntime::with_memory_stores();
        let server_id = runtime
            .register_server(
                "https://relay.example.com",
                ServerKind::OpenSourceRelay,
                ConnectionPolicy::AllowAll,
                "Relay",
            )
            .await
            .unwrap();

        let binding = runtime.bind_account("agent-1", &server_id).await.unwrap();
        assert_eq!(binding.agentos_id, "agent-1");
        assert_eq!(binding.server_id, server_id);

        let bindings = runtime.list_bindings("agent-1").await.unwrap();
        assert_eq!(bindings.len(), 1);
    }

    #[tokio::test]
    async fn one_identity_binds_multiple_servers() {
        let runtime = ConnectorRuntime::with_memory_stores();
        let s1 = runtime
            .register_server(
                "https://personal.example.com",
                ServerKind::PersonalSelfHosted,
                ConnectionPolicy::AllowAll,
                "Personal",
            )
            .await
            .unwrap();
        let s2 = runtime
            .register_server(
                "https://enterprise.example.com",
                ServerKind::EnterpriseManaged,
                ConnectionPolicy::RequireOwnershipNotice,
                "Enterprise",
            )
            .await
            .unwrap();

        runtime.bind_account("agent-1", &s1).await.unwrap();
        runtime.bind_account("agent-1", &s2).await.unwrap();

        let bindings = runtime.list_bindings("agent-1").await.unwrap();
        assert_eq!(bindings.len(), 2);
        let server_ids: Vec<_> = bindings.iter().map(|b| &b.server_id).collect();
        assert!(server_ids.contains(&&s1));
        assert!(server_ids.contains(&&s2));
    }

    #[tokio::test]
    async fn enterprise_server_requires_ownership_notice() {
        let runtime = ConnectorRuntime::with_memory_stores();
        let server_id = runtime
            .register_server(
                "https://corp.example.com",
                ServerKind::EnterpriseManaged,
                ConnectionPolicy::RequireOwnershipNotice,
                "Corp Server",
            )
            .await
            .unwrap();

        let required = runtime
            .check_enterprise_notice_required(&server_id)
            .await
            .unwrap();
        assert!(required);
    }

    #[tokio::test]
    async fn personal_server_does_not_require_ownership_notice() {
        let runtime = ConnectorRuntime::with_memory_stores();
        let server_id = runtime
            .register_server(
                "https://personal.example.com",
                ServerKind::PersonalSelfHosted,
                ConnectionPolicy::AllowAll,
                "Personal",
            )
            .await
            .unwrap();

        let required = runtime
            .check_enterprise_notice_required(&server_id)
            .await
            .unwrap();
        assert!(!required);
    }

    #[tokio::test]
    async fn disconnect_server_removes_server_and_bindings() {
        let runtime = ConnectorRuntime::with_memory_stores();
        let server_id = runtime
            .register_server(
                "https://temp.example.com",
                ServerKind::PersonalSelfHosted,
                ConnectionPolicy::AllowAll,
                "Temp",
            )
            .await
            .unwrap();

        runtime.bind_account("agent-1", &server_id).await.unwrap();
        runtime.disconnect_server(&server_id).await.unwrap();

        let servers = runtime.list_servers().await.unwrap();
        assert!(servers.is_empty());

        let bindings = runtime.list_bindings("agent-1").await.unwrap();
        assert!(bindings.is_empty());
    }

    #[tokio::test]
    async fn verify_challenge_with_fake_crypto() {
        let runtime = ConnectorRuntime::with_memory_stores();
        assert!(runtime.verify_challenge("nonce-123", "signed:nonce-123"));
        assert!(!runtime.verify_challenge("nonce-123", "wrong"));
    }
}
