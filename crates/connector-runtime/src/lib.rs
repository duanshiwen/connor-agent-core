//! # Connector Runtime
//!
//! Runtime for managing identity, server accounts, and multi-server connections.
//!
//! Coordinates identity-core and server-account-core to provide a unified API
//! for registering servers, binding accounts, and verifying challenges.

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use identity_core::{
    AgentOsId, CredentialStore, DeviceId, FakeCryptoProvider, IdentityStore, OAuthCredentialRef,
    OAuthTokenRefreshError, OAuthTokenRefresher, OAuthTokenSet, PublicIdentity, SecretValue,
    refresh_oauth_credential,
};
use serde::{Deserialize, Serialize};
use server_account_core::{
    AccountBindingAuditEvent, AccountBindingAuditOutcome, BindingApproval, ConnectionPolicy,
    MemoryServerAccountStore, MemoryServerRegistry, ServerAccountBinding, ServerAccountError,
    ServerAccountId, ServerAccountStore, ServerBindingDecision, ServerEndpoint, ServerId,
    ServerKind, ServerRegistration, ServerRegistry, ServerTrustStatus,
    evaluate_server_binding_trust,
};
use std::collections::HashMap;
use std::fmt;
use std::sync::{Arc, Mutex};

// ---------------------------------------------------------------------------
// ConnectorRuntime
// ---------------------------------------------------------------------------

/// Unified runtime coordinating identity, server registry, and account bindings.
pub struct ConnectorRuntime {
    identity_store: Arc<dyn IdentityStore>,
    server_registry: Arc<dyn ServerRegistry>,
    account_store: Arc<dyn ServerAccountStore>,
    crypto: FakeCryptoProvider,
    binding_audit_events: Arc<Mutex<Vec<AccountBindingAuditEvent>>>,
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
            binding_audit_events: Arc::new(Mutex::new(Vec::new())),
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
        self.bind_account_with_approval(agentos_id, server_id, BindingApproval::NotRequired)
            .await
    }

    /// Bind an AgentOS identity to a server with an explicit trust approval decision.
    pub async fn bind_account_with_approval(
        &self,
        agentos_id: &str,
        server_id: &ServerId,
        approval: BindingApproval,
    ) -> Result<ServerAccountBinding, ConnectorError> {
        let server = self
            .server_registry
            .get(server_id)
            .await
            .map_err(ConnectorError::ServerAccount)?;

        match evaluate_server_binding_trust(&server, &approval) {
            ServerBindingDecision::Allowed => {
                self.record_binding_audit_event(
                    agentos_id,
                    &server,
                    approval,
                    AccountBindingAuditOutcome::Allowed,
                    "server binding allowed",
                )?;
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
            ServerBindingDecision::RequiresApproval(reason) => {
                self.record_binding_audit_event(
                    agentos_id,
                    &server,
                    approval,
                    AccountBindingAuditOutcome::RequiresApproval,
                    reason.clone(),
                )?;
                Err(ConnectorError::BindingRequiresApproval(reason))
            }
            ServerBindingDecision::Rejected(reason) => {
                self.record_binding_audit_event(
                    agentos_id,
                    &server,
                    approval,
                    AccountBindingAuditOutcome::Rejected,
                    reason.clone(),
                )?;
                Err(ConnectorError::BindingRejected(reason))
            }
        }
    }

    fn record_binding_audit_event(
        &self,
        agentos_id: &str,
        server: &ServerRegistration,
        approval: BindingApproval,
        outcome: AccountBindingAuditOutcome,
        reason: impl Into<String>,
    ) -> Result<(), ConnectorError> {
        let mut events = self
            .binding_audit_events
            .lock()
            .map_err(|err| ConnectorError::Audit(err.to_string()))?;
        let event = AccountBindingAuditEvent::new(
            format!("binding-audit-{}", events.len() + 1),
            agentos_id,
            server.id.clone(),
            server.trust_status,
            approval,
            outcome,
            reason,
            Utc::now(),
        );
        events.push(event);
        Ok(())
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

    /// List in-memory account binding audit events in insertion order.
    pub fn list_binding_audit_events(&self) -> Vec<AccountBindingAuditEvent> {
        self.binding_audit_events
            .lock()
            .map(|events| events.clone())
            .unwrap_or_default()
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

// ===========================================================================
// External Connector Framework
// ===========================================================================

// ---------------------------------------------------------------------------
// External Service Types
// ---------------------------------------------------------------------------

/// Supported external service types.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExternalServiceKind {
    Gmail,
    GitHub,
    Linear,
    Slack,
    Notion,
    Calendar,
    Custom(String),
}

impl fmt::Display for ExternalServiceKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Gmail => write!(f, "gmail"),
            Self::GitHub => write!(f, "github"),
            Self::Linear => write!(f, "linear"),
            Self::Slack => write!(f, "slack"),
            Self::Notion => write!(f, "notion"),
            Self::Calendar => write!(f, "calendar"),
            Self::Custom(s) => write!(f, "{}", s),
        }
    }
}

// ---------------------------------------------------------------------------
// External Service Credentials
// ---------------------------------------------------------------------------

/// OAuth/API credentials for an external service.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExternalServiceCredentials {
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub expires_at: Option<DateTime<Utc>>,
    pub scopes: Vec<String>,
    pub token_type: String,
}

impl ExternalServiceCredentials {
    pub fn new(access_token: impl Into<String>) -> Self {
        Self {
            access_token: access_token.into(),
            refresh_token: None,
            expires_at: None,
            scopes: vec![],
            token_type: "Bearer".to_string(),
        }
    }

    pub fn with_refresh_token(mut self, refresh_token: impl Into<String>) -> Self {
        self.refresh_token = Some(refresh_token.into());
        self
    }

    pub fn with_expiry(mut self, expires_at: DateTime<Utc>) -> Self {
        self.expires_at = Some(expires_at);
        self
    }

    pub fn with_scopes(mut self, scopes: Vec<String>) -> Self {
        self.scopes = scopes;
        self
    }

    /// Check if the credential is expired.
    pub fn is_expired(&self) -> bool {
        match self.expires_at {
            Some(expiry) => Utc::now() > expiry,
            None => false,
        }
    }

    pub fn needs_refresh(&self, now: DateTime<Utc>, refresh_skew_secs: i64) -> bool {
        self.expires_at.is_some_and(|expires_at| {
            expires_at <= now + chrono::Duration::seconds(refresh_skew_secs.max(0))
        })
    }

    pub fn to_oauth_token_set(&self) -> OAuthTokenSet {
        OAuthTokenSet::new(
            SecretValue::new(self.access_token.clone()),
            self.refresh_token.clone().map(SecretValue::new),
            self.token_type.clone(),
            self.expires_at,
            self.scopes.clone(),
        )
    }

    pub fn from_oauth_token_set(token_set: OAuthTokenSet) -> Self {
        Self {
            access_token: token_set.access_token.expose_secret().to_string(),
            refresh_token: token_set
                .refresh_token
                .map(|secret| secret.expose_secret().to_string()),
            expires_at: token_set.expires_at,
            scopes: token_set.scopes,
            token_type: token_set.token_type,
        }
    }
}

// ---------------------------------------------------------------------------
// External Resource Types
// ---------------------------------------------------------------------------

/// Kind of external resource.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExternalResourceKind {
    Email,
    EmailThread,
    Issue,
    PullRequest,
    Document,
    Message,
    CalendarEvent,
    Contact,
    Custom(String),
}

impl fmt::Display for ExternalResourceKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Email => write!(f, "email"),
            Self::EmailThread => write!(f, "email_thread"),
            Self::Issue => write!(f, "issue"),
            Self::PullRequest => write!(f, "pull_request"),
            Self::Document => write!(f, "document"),
            Self::Message => write!(f, "message"),
            Self::CalendarEvent => write!(f, "calendar_event"),
            Self::Contact => write!(f, "contact"),
            Self::Custom(s) => write!(f, "{}", s),
        }
    }
}

/// Reference to an external resource.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExternalResourceRef {
    pub service: ExternalServiceKind,
    pub resource_kind: ExternalResourceKind,
    pub resource_id: String,
    pub url: Option<String>,
    pub title: Option<String>,
}

impl ExternalResourceRef {
    pub fn new(
        service: ExternalServiceKind,
        resource_kind: ExternalResourceKind,
        resource_id: impl Into<String>,
    ) -> Self {
        Self {
            service,
            resource_kind,
            resource_id: resource_id.into(),
            url: None,
            title: None,
        }
    }

    pub fn with_url(mut self, url: impl Into<String>) -> Self {
        self.url = Some(url.into());
        self
    }

    pub fn with_title(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }
}

// ---------------------------------------------------------------------------
// External Connector Trait
// ---------------------------------------------------------------------------

/// Metadata about a synced external resource.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExternalResourceMetadata {
    pub resource_ref: ExternalResourceRef,
    pub synced_at: DateTime<Utc>,
    pub etag: Option<String>,
    pub raw_size_bytes: Option<u64>,
}

/// Result of listing external resources.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExternalResourceList {
    pub resources: Vec<ExternalResourceMetadata>,
    pub next_page_token: Option<String>,
    pub total_count: Option<u64>,
}

/// Provider that refreshes OAuth credentials through the shared credential store.
pub struct ConnectorOAuthTokenRefreshProvider {
    credential_store: Arc<dyn CredentialStore>,
    token_refresher: Arc<dyn OAuthTokenRefresher>,
    refresh_skew_secs: i64,
}

impl ConnectorOAuthTokenRefreshProvider {
    pub fn new(
        credential_store: Arc<dyn CredentialStore>,
        token_refresher: Arc<dyn OAuthTokenRefresher>,
    ) -> Self {
        Self {
            credential_store,
            token_refresher,
            refresh_skew_secs: 300,
        }
    }

    pub fn with_refresh_skew_secs(mut self, refresh_skew_secs: i64) -> Self {
        self.refresh_skew_secs = refresh_skew_secs.max(0);
        self
    }

    pub async fn refresh_if_needed(
        &self,
        credential_ref: &OAuthCredentialRef,
        now: DateTime<Utc>,
    ) -> Result<ExternalServiceCredentials, ExternalConnectorError> {
        let refreshed = refresh_oauth_credential(
            self.credential_store.as_ref(),
            self.token_refresher.as_ref(),
            credential_ref,
            now,
            self.refresh_skew_secs,
        )
        .await
        .map_err(ExternalConnectorError::OAuthRefresh)?;
        Ok(ExternalServiceCredentials::from_oauth_token_set(refreshed))
    }
}

impl fmt::Debug for ConnectorOAuthTokenRefreshProvider {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ConnectorOAuthTokenRefreshProvider")
            .field("refresh_skew_secs", &self.refresh_skew_secs)
            .finish_non_exhaustive()
    }
}

/// Trait for external service connectors.
#[async_trait]
pub trait ExternalConnector: Send + Sync {
    /// The kind of external service this connector handles.
    fn service_kind(&self) -> ExternalServiceKind;

    /// Check if the connector is authenticated and ready.
    fn is_authenticated(&self) -> bool;

    /// Get the current credentials (for inspection, not mutation).
    fn credentials(&self) -> Option<&ExternalServiceCredentials>;

    /// List resources from the external service.
    async fn list_resources(
        &self,
        resource_kind: &ExternalResourceKind,
        page_token: Option<&str>,
        limit: Option<usize>,
    ) -> Result<ExternalResourceList, ExternalConnectorError>;

    /// Get a single resource by ID.
    async fn get_resource(
        &self,
        resource_kind: &ExternalResourceKind,
        resource_id: &str,
    ) -> Result<ExternalResourceMetadata, ExternalConnectorError>;

    /// Refresh the OAuth token if supported.
    async fn refresh_token(&mut self) -> Result<(), ExternalConnectorError> {
        Err(ExternalConnectorError::TokenRefreshNotSupported)
    }
}

/// Identifier for an external connector instance.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ExternalConnectorId(pub String);

impl fmt::Display for ExternalConnectorId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<&str> for ExternalConnectorId {
    fn from(s: &str) -> Self {
        Self(s.to_string())
    }
}

impl From<String> for ExternalConnectorId {
    fn from(s: String) -> Self {
        Self(s)
    }
}

/// Registration record for an external connector.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExternalConnectorRegistration {
    pub id: ExternalConnectorId,
    pub service: ExternalServiceKind,
    pub display_name: String,
    pub connected_at: DateTime<Utc>,
    pub last_synced_at: Option<DateTime<Utc>>,
}

// ---------------------------------------------------------------------------
// External Connector Registry
// ---------------------------------------------------------------------------

/// Trait for storing external connector registrations.
#[async_trait]
pub trait ExternalConnectorRegistry: Send + Sync {
    async fn register(
        &self,
        registration: &ExternalConnectorRegistration,
    ) -> Result<(), ExternalConnectorError>;
    async fn unregister(&self, id: &ExternalConnectorId) -> Result<(), ExternalConnectorError>;
    async fn get(
        &self,
        id: &ExternalConnectorId,
    ) -> Result<ExternalConnectorRegistration, ExternalConnectorError>;
    async fn list(&self) -> Result<Vec<ExternalConnectorRegistration>, ExternalConnectorError>;
    async fn list_by_service(
        &self,
        service: &ExternalServiceKind,
    ) -> Result<Vec<ExternalConnectorRegistration>, ExternalConnectorError>;
}

/// In-memory implementation of ExternalConnectorRegistry.
#[derive(Debug, Clone, Default)]
pub struct MemoryExternalConnectorRegistry {
    connectors: Arc<Mutex<HashMap<ExternalConnectorId, ExternalConnectorRegistration>>>,
}

impl MemoryExternalConnectorRegistry {
    pub fn new() -> Self {
        Self {
            connectors: Arc::new(Mutex::new(HashMap::new())),
        }
    }
}

#[async_trait]
impl ExternalConnectorRegistry for MemoryExternalConnectorRegistry {
    async fn register(
        &self,
        registration: &ExternalConnectorRegistration,
    ) -> Result<(), ExternalConnectorError> {
        let mut connectors = self
            .connectors
            .lock()
            .map_err(|e| ExternalConnectorError::RegistryLock(e.to_string()))?;
        connectors.insert(registration.id.clone(), registration.clone());
        Ok(())
    }

    async fn unregister(&self, id: &ExternalConnectorId) -> Result<(), ExternalConnectorError> {
        let mut connectors = self
            .connectors
            .lock()
            .map_err(|e| ExternalConnectorError::RegistryLock(e.to_string()))?;
        connectors.remove(id);
        Ok(())
    }

    async fn get(
        &self,
        id: &ExternalConnectorId,
    ) -> Result<ExternalConnectorRegistration, ExternalConnectorError> {
        let connectors = self
            .connectors
            .lock()
            .map_err(|e| ExternalConnectorError::RegistryLock(e.to_string()))?;
        connectors
            .get(id)
            .cloned()
            .ok_or_else(|| ExternalConnectorError::ConnectorNotFound(id.0.clone()))
    }

    async fn list(&self) -> Result<Vec<ExternalConnectorRegistration>, ExternalConnectorError> {
        let connectors = self
            .connectors
            .lock()
            .map_err(|e| ExternalConnectorError::RegistryLock(e.to_string()))?;
        Ok(connectors.values().cloned().collect())
    }

    async fn list_by_service(
        &self,
        service: &ExternalServiceKind,
    ) -> Result<Vec<ExternalConnectorRegistration>, ExternalConnectorError> {
        let connectors = self
            .connectors
            .lock()
            .map_err(|e| ExternalConnectorError::RegistryLock(e.to_string()))?;
        Ok(connectors
            .values()
            .filter(|c| c.service == *service)
            .cloned()
            .collect())
    }
}

// ---------------------------------------------------------------------------
// External Connector Errors
// ---------------------------------------------------------------------------

#[derive(Debug, thiserror::Error)]
pub enum ExternalConnectorError {
    #[error("connector not found: {0}")]
    ConnectorNotFound(String),
    #[error("authentication required")]
    AuthenticationRequired,
    #[error("token expired")]
    TokenExpired,
    #[error("token refresh not supported")]
    TokenRefreshNotSupported,
    #[error("oauth refresh error: {0}")]
    OAuthRefresh(#[from] OAuthTokenRefreshError),
    #[error("service unavailable: {0}")]
    ServiceUnavailable(String),
    #[error("rate limited, retry after {0}s")]
    RateLimited(u64),
    #[error("resource not found: {0}")]
    ResourceNotFound(String),
    #[error("invalid request: {0}")]
    InvalidRequest(String),
    #[error("registry lock error: {0}")]
    RegistryLock(String),
    #[error("io error: {0}")]
    Io(String),
}

// ===========================================================================
// Gmail Connector (Read-only)
// ===========================================================================

/// Gmail thread metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GmailThread {
    pub thread_id: String,
    pub snippet: Option<String>,
    pub history_id: Option<String>,
    pub messages_count: Option<u32>,
    pub labels: Vec<String>,
}

/// Gmail message metadata (lightweight).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GmailMessage {
    pub message_id: String,
    pub thread_id: String,
    pub subject: Option<String>,
    pub from: Option<String>,
    pub to: Vec<String>,
    pub date: Option<String>,
    pub snippet: Option<String>,
    pub label_ids: Vec<String>,
}

/// Gmail API response for threads list.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[allow(dead_code)]
struct GmailThreadsResponse {
    threads: Option<Vec<GmailThreadItem>>,
    next_page_token: Option<String>,
    result_size_estimate: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct GmailThreadItem {
    id: String,
    snippet: Option<String>,
    history_id: Option<String>,
}

/// Gmail connector configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GmailConnectorConfig {
    pub max_results_per_page: usize,
    pub query_filter: Option<String>,
    pub label_ids: Vec<String>,
}

impl Default for GmailConnectorConfig {
    fn default() -> Self {
        Self {
            max_results_per_page: 20,
            query_filter: None,
            label_ids: vec!["INBOX".to_string()],
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GmailProviderRetryConfig {
    pub max_attempts: u32,
    pub base_delay_ms: u64,
    pub max_delay_ms: u64,
    pub timeout_ms: u64,
}

impl Default for GmailProviderRetryConfig {
    fn default() -> Self {
        Self {
            max_attempts: 3,
            base_delay_ms: 250,
            max_delay_ms: 5_000,
            timeout_ms: 10_000,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum GmailProviderErrorClass {
    RateLimited,
    Timeout,
    TransientProvider,
    Authentication,
    InvalidRequest,
    NotFound,
    Unknown,
}

impl GmailProviderErrorClass {
    pub fn is_retryable(self) -> bool {
        matches!(
            self,
            Self::RateLimited | Self::Timeout | Self::TransientProvider
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum GmailProviderRetryDecision {
    Retry {
        error_class: GmailProviderErrorClass,
        delay_ms: u64,
        retry_after_ms: Option<u64>,
    },
    DoNotRetry {
        error_class: GmailProviderErrorClass,
        reason: String,
    },
    Exhausted {
        error_class: GmailProviderErrorClass,
        attempts: u32,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GmailProviderRetryPolicy {
    config: GmailProviderRetryConfig,
}

impl GmailProviderRetryPolicy {
    pub fn new(config: GmailProviderRetryConfig) -> Self {
        Self { config }
    }

    pub fn config(&self) -> &GmailProviderRetryConfig {
        &self.config
    }

    pub fn classify_error(&self, error: &ExternalConnectorError) -> GmailProviderErrorClass {
        classify_gmail_provider_error(error)
    }

    pub fn decide(
        &self,
        error: &ExternalConnectorError,
        attempt: u32,
    ) -> GmailProviderRetryDecision {
        let error_class = self.classify_error(error);
        if !error_class.is_retryable() {
            return GmailProviderRetryDecision::DoNotRetry {
                error_class,
                reason: non_retryable_gmail_reason(error_class).to_string(),
            };
        }
        if attempt >= self.config.max_attempts {
            return GmailProviderRetryDecision::Exhausted {
                error_class,
                attempts: attempt,
            };
        }
        let retry_after_ms = match error {
            ExternalConnectorError::RateLimited(seconds) => Some(seconds.saturating_mul(1_000)),
            _ => None,
        };
        let delay_ms = retry_after_ms.unwrap_or_else(|| {
            let multiplier = 1_u64
                .checked_shl(attempt.saturating_sub(1))
                .unwrap_or(u64::MAX);
            self.config
                .base_delay_ms
                .saturating_mul(multiplier)
                .min(self.config.max_delay_ms)
        });
        GmailProviderRetryDecision::Retry {
            error_class,
            delay_ms,
            retry_after_ms,
        }
    }
}

impl Default for GmailProviderRetryPolicy {
    fn default() -> Self {
        Self::new(GmailProviderRetryConfig::default())
    }
}

pub fn classify_gmail_provider_error(error: &ExternalConnectorError) -> GmailProviderErrorClass {
    match error {
        ExternalConnectorError::RateLimited(_) => GmailProviderErrorClass::RateLimited,
        ExternalConnectorError::AuthenticationRequired
        | ExternalConnectorError::TokenExpired
        | ExternalConnectorError::OAuthRefresh(_) => GmailProviderErrorClass::Authentication,
        ExternalConnectorError::InvalidRequest(_) => GmailProviderErrorClass::InvalidRequest,
        ExternalConnectorError::ResourceNotFound(_) => GmailProviderErrorClass::NotFound,
        ExternalConnectorError::ServiceUnavailable(message) => {
            let normalized = message.to_ascii_lowercase();
            if normalized.contains("timeout") || normalized.contains("timed out") {
                GmailProviderErrorClass::Timeout
            } else if normalized.contains("503")
                || normalized.contains("500")
                || normalized.contains("backend")
                || normalized.contains("unavailable")
                || normalized.contains("transient")
            {
                GmailProviderErrorClass::TransientProvider
            } else {
                GmailProviderErrorClass::Unknown
            }
        }
        _ => GmailProviderErrorClass::Unknown,
    }
}

fn non_retryable_gmail_reason(error_class: GmailProviderErrorClass) -> &'static str {
    match error_class {
        GmailProviderErrorClass::Authentication => {
            "authentication/credential failure is not retryable"
        }
        GmailProviderErrorClass::InvalidRequest => "invalid request is not retryable",
        GmailProviderErrorClass::NotFound => "resource not found is not retryable",
        GmailProviderErrorClass::Unknown => "unknown Gmail provider error is not retryable",
        GmailProviderErrorClass::RateLimited
        | GmailProviderErrorClass::Timeout
        | GmailProviderErrorClass::TransientProvider => "retryable Gmail provider error",
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ConnectorOperationKind {
    ListResources,
    GetResource,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ConnectorOperationOutcome {
    Started,
    Succeeded,
    Failed,
    Denied,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ConnectorHostAccountLifecycle {
    Active,
    Disabled,
    Offboarded,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConnectorOperationAuditEvent {
    pub service: ExternalServiceKind,
    pub connector_id: String,
    pub account_id: Option<String>,
    pub operation: ConnectorOperationKind,
    pub resource_kind: ExternalResourceKind,
    pub resource_id: Option<String>,
    pub outcome: ConnectorOperationOutcome,
    pub error_class: Option<GmailProviderErrorClass>,
    pub reason: String,
    pub occurred_at: DateTime<Utc>,
    pub payload_redaction: String,
    pub credential_redaction: String,
}

impl ConnectorOperationAuditEvent {
    pub fn started(
        service: ExternalServiceKind,
        connector_id: impl Into<String>,
        account_id: Option<&str>,
        operation: ConnectorOperationKind,
        resource_kind: ExternalResourceKind,
        resource_id: Option<&str>,
        occurred_at: DateTime<Utc>,
    ) -> Self {
        Self::new(
            service,
            connector_id,
            account_id,
            operation,
            resource_kind,
            resource_id,
            ConnectorOperationOutcome::Started,
            None,
            "connector operation started",
            occurred_at,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn result(
        service: ExternalServiceKind,
        connector_id: impl Into<String>,
        account_id: Option<&str>,
        operation: ConnectorOperationKind,
        resource_kind: ExternalResourceKind,
        resource_id: Option<&str>,
        outcome: ConnectorOperationOutcome,
        error_class: Option<GmailProviderErrorClass>,
        reason: impl Into<String>,
        occurred_at: DateTime<Utc>,
    ) -> Self {
        Self::new(
            service,
            connector_id,
            account_id,
            operation,
            resource_kind,
            resource_id,
            outcome,
            error_class,
            reason,
            occurred_at,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn new(
        service: ExternalServiceKind,
        connector_id: impl Into<String>,
        account_id: Option<&str>,
        operation: ConnectorOperationKind,
        resource_kind: ExternalResourceKind,
        resource_id: Option<&str>,
        outcome: ConnectorOperationOutcome,
        error_class: Option<GmailProviderErrorClass>,
        reason: impl Into<String>,
        occurred_at: DateTime<Utc>,
    ) -> Self {
        let payload_redaction = connector_payload_redaction(&service).to_string();
        Self {
            service,
            connector_id: connector_id.into(),
            account_id: account_id.map(str::to_string),
            operation,
            resource_kind,
            resource_id: resource_id.map(str::to_string),
            outcome,
            error_class,
            reason: reason.into(),
            occurred_at,
            payload_redaction,
            credential_redaction: "oauth token material omitted".to_string(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ConnectorAccountAccessDecision {
    Allowed,
    Denied(Box<ConnectorOperationAuditEvent>),
}

#[allow(clippy::too_many_arguments)]
pub fn evaluate_connector_account_access(
    service: ExternalServiceKind,
    connector_id: impl Into<String>,
    account_id: Option<&str>,
    lifecycle: ConnectorHostAccountLifecycle,
    operation: ConnectorOperationKind,
    resource_kind: ExternalResourceKind,
    resource_id: Option<&str>,
    occurred_at: DateTime<Utc>,
) -> ConnectorAccountAccessDecision {
    match lifecycle {
        ConnectorHostAccountLifecycle::Active => ConnectorAccountAccessDecision::Allowed,
        ConnectorHostAccountLifecycle::Disabled | ConnectorHostAccountLifecycle::Offboarded => {
            let reason = match lifecycle {
                ConnectorHostAccountLifecycle::Disabled => "connector account is disabled",
                ConnectorHostAccountLifecycle::Offboarded => "connector account is offboarded",
                ConnectorHostAccountLifecycle::Active => unreachable!(),
            };
            ConnectorAccountAccessDecision::Denied(Box::new(ConnectorOperationAuditEvent::result(
                service,
                connector_id,
                account_id,
                operation,
                resource_kind,
                resource_id,
                ConnectorOperationOutcome::Denied,
                Some(GmailProviderErrorClass::Authentication),
                reason,
                occurred_at,
            )))
        }
    }
}

fn connector_payload_redaction(service: &ExternalServiceKind) -> &'static str {
    match service {
        ExternalServiceKind::Gmail => "gmail message content omitted",
        _ => "connector payload omitted",
    }
}

/// Read-only Gmail connector.
#[allow(dead_code)]
pub struct GmailConnector {
    credentials: ExternalServiceCredentials,
    config: GmailConnectorConfig,
    last_synced_at: Option<DateTime<Utc>>,
    credential_ref: Option<OAuthCredentialRef>,
    token_refresh_provider: Option<Arc<ConnectorOAuthTokenRefreshProvider>>,
}

#[allow(dead_code)]
impl GmailConnector {
    pub fn new(credentials: ExternalServiceCredentials, config: GmailConnectorConfig) -> Self {
        Self {
            credentials,
            config,
            last_synced_at: None,
            credential_ref: None,
            token_refresh_provider: None,
        }
    }

    pub fn with_credentials(credentials: ExternalServiceCredentials) -> Self {
        Self::new(credentials, GmailConnectorConfig::default())
    }

    pub fn with_token_refresh_provider(
        mut self,
        credential_ref: OAuthCredentialRef,
        provider: Arc<ConnectorOAuthTokenRefreshProvider>,
    ) -> Self {
        self.credential_ref = Some(credential_ref);
        self.token_refresh_provider = Some(provider);
        self
    }

    pub fn last_synced_at(&self) -> Option<DateTime<Utc>> {
        self.last_synced_at
    }

    /// Build the Gmail API URL for listing threads.
    fn build_list_threads_url(&self, page_token: Option<&str>, limit: Option<usize>) -> String {
        let max_results = limit.unwrap_or(self.config.max_results_per_page);
        let mut url = format!(
            "https://gmail.googleapis.com/gmail/v1/users/me/threads?maxResults={}",
            max_results
        );

        if let Some(query) = &self.config.query_filter {
            url.push_str(&format!("&q={}", query));
        }

        for label in &self.config.label_ids {
            url.push_str(&format!("&labelIds={}", label));
        }

        if let Some(token) = page_token {
            url.push_str(&format!("&pageToken={}", token));
        }

        url
    }

    /// Build the Gmail API URL for getting a single thread.
    fn build_get_thread_url(&self, thread_id: &str) -> String {
        format!(
            "https://gmail.googleapis.com/gmail/v1/users/me/threads/{}?format=metadata",
            thread_id
        )
    }
}

#[async_trait]
impl ExternalConnector for GmailConnector {
    fn service_kind(&self) -> ExternalServiceKind {
        ExternalServiceKind::Gmail
    }

    fn is_authenticated(&self) -> bool {
        !self.credentials.access_token.is_empty() && !self.credentials.is_expired()
    }

    fn credentials(&self) -> Option<&ExternalServiceCredentials> {
        Some(&self.credentials)
    }

    async fn list_resources(
        &self,
        resource_kind: &ExternalResourceKind,
        _page_token: Option<&str>,
        _limit: Option<usize>,
    ) -> Result<ExternalResourceList, ExternalConnectorError> {
        if !self.is_authenticated() {
            return Err(ExternalConnectorError::AuthenticationRequired);
        }

        match resource_kind {
            ExternalResourceKind::EmailThread => Err(ExternalConnectorError::ServiceUnavailable(
                "gmail resource listing requires a host-provided Gmail API adapter".to_string(),
            )),
            _ => Err(ExternalConnectorError::InvalidRequest(format!(
                "Gmail connector does not support resource kind: {}",
                resource_kind
            ))),
        }
    }

    async fn get_resource(
        &self,
        resource_kind: &ExternalResourceKind,
        resource_id: &str,
    ) -> Result<ExternalResourceMetadata, ExternalConnectorError> {
        if !self.is_authenticated() {
            return Err(ExternalConnectorError::AuthenticationRequired);
        }

        match resource_kind {
            ExternalResourceKind::EmailThread => {
                Err(ExternalConnectorError::ServiceUnavailable(format!(
                    "gmail resource retrieval for '{}' requires a host-provided Gmail API adapter",
                    resource_id
                )))
            }
            _ => Err(ExternalConnectorError::InvalidRequest(format!(
                "Gmail connector does not support resource kind: {}",
                resource_kind
            ))),
        }
    }

    async fn refresh_token(&mut self) -> Result<(), ExternalConnectorError> {
        let credential_ref = self
            .credential_ref
            .as_ref()
            .ok_or(ExternalConnectorError::TokenRefreshNotSupported)?;
        let provider = self
            .token_refresh_provider
            .as_ref()
            .ok_or(ExternalConnectorError::TokenRefreshNotSupported)?;

        self.credentials = provider
            .refresh_if_needed(credential_ref, Utc::now())
            .await?;
        Ok(())
    }
}

/// OAuth boundary for Gmail.
pub struct GmailOAuthBoundary {
    pub client_id: String,
    pub client_secret: String,
    pub redirect_uri: String,
    pub scopes: Vec<String>,
}

impl GmailOAuthBoundary {
    pub fn new(
        client_id: impl Into<String>,
        client_secret: impl Into<String>,
        redirect_uri: impl Into<String>,
    ) -> Self {
        Self {
            client_id: client_id.into(),
            client_secret: client_secret.into(),
            redirect_uri: redirect_uri.into(),
            scopes: vec!["https://www.googleapis.com/auth/gmail.readonly".to_string()],
        }
    }

    /// Build the OAuth authorization URL.
    pub fn authorization_url(&self, state: &str) -> String {
        let scopes = self.scopes.join(" ");
        format!(
            "https://accounts.google.com/o/oauth2/v2/auth?client_id={}&redirect_uri={}&response_type=code&scope={}&state={}&access_type=offline",
            self.client_id, self.redirect_uri, scopes, state
        )
    }
}

// ===========================================================================
// Original ConnectorRuntime (Server/Peer connections)
// ===========================================================================

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

#[derive(Debug, thiserror::Error)]
pub enum ConnectorError {
    #[error("server account error: {0}")]
    ServerAccount(#[from] ServerAccountError),
    #[error("identity error: {0}")]
    Identity(#[from] identity_core::IdentityError),
    #[error("server binding requires approval: {0}")]
    BindingRequiresApproval(String),
    #[error("server binding rejected: {0}")]
    BindingRejected(String),
    #[error("binding audit error: {0}")]
    Audit(String),
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

        let binding = runtime
            .bind_account_with_approval(
                "agent-1",
                &server_id,
                BindingApproval::Approved {
                    approved_by: "test".to_string(),
                    reason: "legacy test approval".to_string(),
                },
            )
            .await
            .unwrap();
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

        runtime
            .bind_account_with_approval(
                "agent-1",
                &s1,
                BindingApproval::Approved {
                    approved_by: "test".to_string(),
                    reason: "legacy test approval".to_string(),
                },
            )
            .await
            .unwrap();
        runtime
            .bind_account_with_approval(
                "agent-1",
                &s2,
                BindingApproval::Approved {
                    approved_by: "test".to_string(),
                    reason: "legacy test approval".to_string(),
                },
            )
            .await
            .unwrap();

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

        runtime
            .bind_account_with_approval(
                "agent-1",
                &server_id,
                BindingApproval::Approved {
                    approved_by: "test".to_string(),
                    reason: "legacy test approval".to_string(),
                },
            )
            .await
            .unwrap();
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

    // ---- PR 46: E2E identity lifecycle tests ----

    #[tokio::test]
    async fn e2e_create_local_identity_and_bind_personal_server() {
        let runtime = ConnectorRuntime::with_memory_stores();

        // Create local identity
        let identity = runtime.create_identity("Test User").await.unwrap();
        assert!(!identity.agentos_id.0.is_empty());
        assert!(!identity.device_id.0.is_empty());
        assert_eq!(identity.display_name, "Test User");

        // Register personal server
        let server_id = runtime
            .register_server(
                "https://my-home.example.com",
                ServerKind::PersonalSelfHosted,
                ConnectionPolicy::AllowAll,
                "Home Server",
            )
            .await
            .unwrap();

        // Bind identity to server
        let binding = runtime
            .bind_account_with_approval(
                &identity.agentos_id.0,
                &server_id,
                BindingApproval::Approved {
                    approved_by: "test".to_string(),
                    reason: "legacy test approval".to_string(),
                },
            )
            .await
            .unwrap();
        assert_eq!(binding.agentos_id, identity.agentos_id.0);
        assert_eq!(binding.server_id, server_id);

        // Verify binding exists
        let bindings = runtime.list_bindings(&identity.agentos_id.0).await.unwrap();
        assert_eq!(bindings.len(), 1);

        // Disconnect server keeps local identity
        runtime.disconnect_server(&server_id).await.unwrap();
        let servers = runtime.list_servers().await.unwrap();
        assert!(servers.is_empty());
    }

    #[tokio::test]
    async fn e2e_one_identity_binds_multiple_servers() {
        let runtime = ConnectorRuntime::with_memory_stores();

        // Create identity
        let identity = runtime.create_identity("Multi-Server User").await.unwrap();

        // Register 3 servers of different kinds
        let personal = runtime
            .register_server(
                "https://personal.example.com",
                ServerKind::PersonalSelfHosted,
                ConnectionPolicy::AllowAll,
                "Personal",
            )
            .await
            .unwrap();
        let enterprise = runtime
            .register_server(
                "https://corp.example.com",
                ServerKind::EnterpriseManaged,
                ConnectionPolicy::RequireOwnershipNotice,
                "Corporate",
            )
            .await
            .unwrap();
        let relay = runtime
            .register_server(
                "https://relay.example.com",
                ServerKind::OpenSourceRelay,
                ConnectionPolicy::RequireInviteCode,
                "Community Relay",
            )
            .await
            .unwrap();

        // Bind identity to all three
        runtime
            .bind_account_with_approval(
                &identity.agentos_id.0,
                &personal,
                BindingApproval::Approved {
                    approved_by: "test".to_string(),
                    reason: "legacy test approval".to_string(),
                },
            )
            .await
            .unwrap();
        runtime
            .bind_account_with_approval(
                &identity.agentos_id.0,
                &enterprise,
                BindingApproval::Approved {
                    approved_by: "test".to_string(),
                    reason: "legacy test approval".to_string(),
                },
            )
            .await
            .unwrap();
        runtime
            .bind_account_with_approval(
                &identity.agentos_id.0,
                &relay,
                BindingApproval::Approved {
                    approved_by: "test".to_string(),
                    reason: "legacy test approval".to_string(),
                },
            )
            .await
            .unwrap();

        let bindings = runtime.list_bindings(&identity.agentos_id.0).await.unwrap();
        assert_eq!(bindings.len(), 3);

        // Verify each binding points to the correct server
        let server_ids: Vec<_> = bindings.iter().map(|b| &b.server_id).collect();
        assert!(server_ids.contains(&&personal));
        assert!(server_ids.contains(&&enterprise));
        assert!(server_ids.contains(&&relay));
    }

    #[tokio::test]
    async fn e2e_enterprise_server_requires_ownership_notice_acknowledgment() {
        let runtime = ConnectorRuntime::with_memory_stores();

        // Create identity
        let identity = runtime.create_identity("Enterprise User").await.unwrap();

        // Register enterprise server with ownership notice requirement
        let enterprise_server = runtime
            .register_server(
                "https://enterprise.example.com",
                ServerKind::EnterpriseManaged,
                ConnectionPolicy::RequireOwnershipNotice,
                "Enterprise Server",
            )
            .await
            .unwrap();

        // Check that ownership notice is required
        let required = runtime
            .check_enterprise_notice_required(&enterprise_server)
            .await
            .unwrap();
        assert!(required);

        // Register personal server (no notice required)
        let personal_server = runtime
            .register_server(
                "https://personal.example.com",
                ServerKind::PersonalSelfHosted,
                ConnectionPolicy::AllowAll,
                "Personal",
            )
            .await
            .unwrap();

        let not_required = runtime
            .check_enterprise_notice_required(&personal_server)
            .await
            .unwrap();
        assert!(!not_required);

        // Bind to both servers with explicit approval because newly registered servers are unverified.
        runtime
            .bind_account_with_approval(
                &identity.agentos_id.0,
                &enterprise_server,
                BindingApproval::Approved {
                    approved_by: "user".to_string(),
                    reason: "ownership notice acknowledged".to_string(),
                },
            )
            .await
            .unwrap();
        runtime
            .bind_account_with_approval(
                &identity.agentos_id.0,
                &personal_server,
                BindingApproval::Approved {
                    approved_by: "user".to_string(),
                    reason: "personal server reviewed".to_string(),
                },
            )
            .await
            .unwrap();

        let bindings = runtime.list_bindings(&identity.agentos_id.0).await.unwrap();
        assert_eq!(bindings.len(), 2);
    }

    #[tokio::test]
    async fn bind_account_rejects_unverified_server_without_approval() {
        let runtime = ConnectorRuntime::with_memory_stores();
        let server_id = runtime
            .register_server(
                "https://unverified.example.com",
                ServerKind::OpenSourceRelay,
                ConnectionPolicy::AllowAll,
                "Unverified",
            )
            .await
            .unwrap();

        let result = runtime.bind_account("agent-1", &server_id).await;

        assert!(matches!(
            result,
            Err(ConnectorError::BindingRequiresApproval(_))
        ));
        assert!(runtime.list_bindings("agent-1").await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn bind_account_with_approval_allows_unverified_server() {
        let runtime = ConnectorRuntime::with_memory_stores();
        let server_id = runtime
            .register_server(
                "https://unverified.example.com",
                ServerKind::OpenSourceRelay,
                ConnectionPolicy::AllowAll,
                "Unverified",
            )
            .await
            .unwrap();

        let binding = runtime
            .bind_account_with_approval(
                "agent-1",
                &server_id,
                BindingApproval::Approved {
                    approved_by: "user".to_string(),
                    reason: "manual review".to_string(),
                },
            )
            .await
            .unwrap();

        assert_eq!(binding.server_id, server_id);
        assert_eq!(runtime.list_bindings("agent-1").await.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn bind_account_rejects_blocked_server_even_with_approval() {
        let registry = Arc::new(MemoryServerRegistry::new());
        let account_store = Arc::new(MemoryServerAccountStore::new());
        let runtime = ConnectorRuntime::new(
            Arc::new(identity_core::MemoryIdentityStore::new()),
            registry.clone(),
            account_store,
        );
        let mut server = ServerRegistration {
            id: ServerId::from("blocked-server"),
            endpoint: ServerEndpoint::from("https://blocked.example.com"),
            kind: ServerKind::OpenSourceRelay,
            connection_policy: ConnectionPolicy::AllowAll,
            trust_status: ServerTrustStatus::Blocked,
            display_name: "Blocked".to_string(),
            registered_at: Utc::now(),
        };
        registry.register(&server).await.unwrap();
        server.trust_status = ServerTrustStatus::Trusted;

        let result = runtime
            .bind_account_with_approval(
                "agent-1",
                &ServerId::from("blocked-server"),
                BindingApproval::Approved {
                    approved_by: "user".to_string(),
                    reason: "manual review".to_string(),
                },
            )
            .await;

        assert!(matches!(result, Err(ConnectorError::BindingRejected(_))));
        assert!(runtime.list_bindings("agent-1").await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn bind_account_trusted_server_allows_without_approval() {
        let registry = Arc::new(MemoryServerRegistry::new());
        let runtime = ConnectorRuntime::new(
            Arc::new(identity_core::MemoryIdentityStore::new()),
            registry.clone(),
            Arc::new(MemoryServerAccountStore::new()),
        );
        let server = ServerRegistration {
            id: ServerId::from("trusted-server"),
            endpoint: ServerEndpoint::from("https://trusted.example.com"),
            kind: ServerKind::Official,
            connection_policy: ConnectionPolicy::AllowAll,
            trust_status: ServerTrustStatus::Trusted,
            display_name: "Trusted".to_string(),
            registered_at: Utc::now(),
        };
        registry.register(&server).await.unwrap();

        let binding = runtime
            .bind_account("agent-1", &ServerId::from("trusted-server"))
            .await
            .unwrap();

        assert_eq!(binding.server_id, ServerId::from("trusted-server"));
    }

    #[tokio::test]
    async fn binding_audit_records_requires_approval() {
        let runtime = ConnectorRuntime::with_memory_stores();
        let server_id = runtime
            .register_server(
                "https://unverified.example.com",
                ServerKind::OpenSourceRelay,
                ConnectionPolicy::AllowAll,
                "Unverified",
            )
            .await
            .unwrap();

        let _ = runtime.bind_account("agent-1", &server_id).await;
        let events = runtime.list_binding_audit_events();

        assert_eq!(events.len(), 1);
        assert_eq!(
            events[0].outcome,
            AccountBindingAuditOutcome::RequiresApproval
        );
    }

    #[tokio::test]
    async fn binding_audit_records_allowed_with_approval() {
        let runtime = ConnectorRuntime::with_memory_stores();
        let server_id = runtime
            .register_server(
                "https://unverified.example.com",
                ServerKind::OpenSourceRelay,
                ConnectionPolicy::AllowAll,
                "Unverified",
            )
            .await
            .unwrap();

        runtime
            .bind_account_with_approval(
                "agent-1",
                &server_id,
                BindingApproval::Approved {
                    approved_by: "user".to_string(),
                    reason: "manual review".to_string(),
                },
            )
            .await
            .unwrap();
        let events = runtime.list_binding_audit_events();

        assert_eq!(events.len(), 1);
        assert_eq!(events[0].outcome, AccountBindingAuditOutcome::Allowed);
    }

    #[tokio::test]
    async fn binding_audit_records_rejected_blocked_server() {
        let registry = Arc::new(MemoryServerRegistry::new());
        let runtime = ConnectorRuntime::new(
            Arc::new(identity_core::MemoryIdentityStore::new()),
            registry.clone(),
            Arc::new(MemoryServerAccountStore::new()),
        );
        registry
            .register(&ServerRegistration {
                id: ServerId::from("blocked-server"),
                endpoint: ServerEndpoint::from("https://blocked.example.com"),
                kind: ServerKind::OpenSourceRelay,
                connection_policy: ConnectionPolicy::AllowAll,
                trust_status: ServerTrustStatus::Blocked,
                display_name: "Blocked".to_string(),
                registered_at: Utc::now(),
            })
            .await
            .unwrap();

        let _ = runtime
            .bind_account_with_approval(
                "agent-1",
                &ServerId::from("blocked-server"),
                BindingApproval::Approved {
                    approved_by: "user".to_string(),
                    reason: "manual review".to_string(),
                },
            )
            .await;
        let events = runtime.list_binding_audit_events();

        assert_eq!(events.len(), 1);
        assert_eq!(events[0].outcome, AccountBindingAuditOutcome::Rejected);
    }

    // -----------------------------------------------------------------------
    // PR 146: External Connector Framework
    // -----------------------------------------------------------------------

    #[test]
    fn external_service_kind_display() {
        assert_eq!(ExternalServiceKind::Gmail.to_string(), "gmail");
        assert_eq!(ExternalServiceKind::GitHub.to_string(), "github");
        assert_eq!(ExternalServiceKind::Linear.to_string(), "linear");
        assert_eq!(ExternalServiceKind::Slack.to_string(), "slack");
        assert_eq!(ExternalServiceKind::Notion.to_string(), "notion");
        assert_eq!(ExternalServiceKind::Calendar.to_string(), "calendar");
        assert_eq!(
            ExternalServiceKind::Custom("jira".to_string()).to_string(),
            "jira"
        );
    }

    #[test]
    fn external_service_kind_serialize_as_snake_case() {
        let kind = ExternalServiceKind::Gmail;
        let json = serde_json::to_string(&kind).unwrap();
        assert_eq!(json, "\"gmail\"");

        // Custom variant serializes as {"custom":"value"} due to serde tagged enum
        let kind = ExternalServiceKind::Custom("my_service".to_string());
        let json = serde_json::to_string(&kind).unwrap();
        assert!(json.contains("my_service"));
    }

    #[test]
    fn external_resource_kind_display() {
        assert_eq!(ExternalResourceKind::Email.to_string(), "email");
        assert_eq!(
            ExternalResourceKind::EmailThread.to_string(),
            "email_thread"
        );
        assert_eq!(ExternalResourceKind::Issue.to_string(), "issue");
        assert_eq!(
            ExternalResourceKind::PullRequest.to_string(),
            "pull_request"
        );
    }

    #[test]
    fn external_credentials_roundtrip() {
        let creds = ExternalServiceCredentials::new("test-token")
            .with_refresh_token("refresh-token")
            .with_expiry(Utc::now() + chrono::Duration::hours(1))
            .with_scopes(vec!["read".to_string(), "write".to_string()]);

        let json = serde_json::to_string_pretty(&creds).unwrap();
        let decoded: ExternalServiceCredentials = serde_json::from_str(&json).unwrap();

        assert_eq!(decoded.access_token, "test-token");
        assert_eq!(decoded.refresh_token.as_deref(), Some("refresh-token"));
        assert_eq!(decoded.scopes.len(), 2);
        assert!(!decoded.is_expired());
    }

    #[test]
    fn external_credentials_is_expired() {
        let creds = ExternalServiceCredentials::new("token")
            .with_expiry(Utc::now() - chrono::Duration::hours(1));
        assert!(creds.is_expired());

        let creds = ExternalServiceCredentials::new("token")
            .with_expiry(Utc::now() + chrono::Duration::hours(1));
        assert!(!creds.is_expired());

        let creds = ExternalServiceCredentials::new("token");
        assert!(!creds.is_expired());
    }

    #[test]
    fn external_credentials_roundtrip_through_oauth_token_set() {
        let expires_at = Utc::now() + chrono::Duration::minutes(30);
        let creds = ExternalServiceCredentials::new("access-token")
            .with_refresh_token("refresh-token")
            .with_expiry(expires_at)
            .with_scopes(vec!["gmail.readonly".to_string(), "email".to_string()]);

        let roundtripped =
            ExternalServiceCredentials::from_oauth_token_set(creds.to_oauth_token_set());

        assert_eq!(roundtripped.access_token, "access-token");
        assert_eq!(roundtripped.refresh_token.as_deref(), Some("refresh-token"));
        assert_eq!(roundtripped.expires_at, Some(expires_at));
        assert_eq!(roundtripped.scopes, vec!["email", "gmail.readonly"]);
        assert_eq!(roundtripped.token_type, "Bearer");
    }

    #[tokio::test]
    async fn connector_oauth_refresh_provider_refreshes_store_backed_token() {
        let now = Utc::now();
        let store = Arc::new(identity_core::MemoryCredentialStore::new());
        let credential_ref = OAuthCredentialRef::new(
            identity_core::CredentialId::from("gmail-oauth"),
            "gmail",
            Some("user@example.com".to_string()),
            vec!["gmail.readonly".to_string()],
        );
        let initial = OAuthTokenSet::new(
            SecretValue::new("expired-access"),
            Some(SecretValue::new("refresh-token")),
            "Bearer",
            Some(now - chrono::Duration::minutes(1)),
            vec!["gmail.readonly".to_string()],
        );
        store
            .write(
                initial
                    .to_credential_record(&credential_ref, credential_ref.metadata_label(), now)
                    .unwrap(),
            )
            .await
            .unwrap();
        let refresher = Arc::new(identity_core::FakeOAuthTokenRefresher::new("gmail-access"));
        let provider = ConnectorOAuthTokenRefreshProvider::new(store.clone(), refresher.clone())
            .with_refresh_skew_secs(300);

        let refreshed = provider
            .refresh_if_needed(&credential_ref, now)
            .await
            .unwrap();
        let persisted = store.read(&credential_ref.credential_id).await.unwrap();
        let persisted_token = OAuthTokenSet::from_credential_record(&persisted).unwrap();

        assert_eq!(refresher.refresh_count(), 1);
        assert_eq!(refreshed.access_token, "gmail-access-1");
        assert_eq!(
            persisted_token.access_token.expose_secret(),
            "gmail-access-1"
        );
        assert_eq!(
            persisted_token.refresh_token.unwrap().expose_secret(),
            "refresh-token"
        );
    }

    #[tokio::test]
    async fn gmail_connector_refresh_token_updates_credentials_from_provider() {
        let now = Utc::now();
        let store = Arc::new(identity_core::MemoryCredentialStore::new());
        let credential_ref = OAuthCredentialRef::new(
            identity_core::CredentialId::from("gmail-oauth"),
            "gmail",
            Some("user@example.com".to_string()),
            vec!["gmail.readonly".to_string()],
        );
        let initial = OAuthTokenSet::new(
            SecretValue::new("expired-access"),
            Some(SecretValue::new("refresh-token")),
            "Bearer",
            Some(now - chrono::Duration::minutes(1)),
            vec!["gmail.readonly".to_string()],
        );
        store
            .write(
                initial
                    .to_credential_record(&credential_ref, credential_ref.metadata_label(), now)
                    .unwrap(),
            )
            .await
            .unwrap();
        let provider = Arc::new(ConnectorOAuthTokenRefreshProvider::new(
            store,
            Arc::new(identity_core::FakeOAuthTokenRefresher::new("gmail-access")),
        ));
        let credentials = ExternalServiceCredentials::new("expired-access")
            .with_refresh_token("refresh-token")
            .with_expiry(now - chrono::Duration::minutes(1));
        let mut connector = GmailConnector::with_credentials(credentials)
            .with_token_refresh_provider(credential_ref, provider);

        connector.refresh_token().await.unwrap();

        assert_eq!(
            connector.credentials().unwrap().access_token,
            "gmail-access-1"
        );
        assert!(connector.credentials().unwrap().expires_at.unwrap() > now);
    }

    #[tokio::test]
    async fn gmail_connector_refresh_token_requires_provider() {
        let mut connector = GmailConnector::with_credentials(
            ExternalServiceCredentials::new("expired-access")
                .with_expiry(Utc::now() - chrono::Duration::minutes(1)),
        );

        let result = connector.refresh_token().await;

        assert!(matches!(
            result,
            Err(ExternalConnectorError::TokenRefreshNotSupported)
        ));
    }

    #[test]
    fn external_resource_ref_roundtrip() {
        let resource_ref = ExternalResourceRef::new(
            ExternalServiceKind::Gmail,
            ExternalResourceKind::EmailThread,
            "thread-123",
        )
        .with_url("https://mail.google.com/thread/123")
        .with_title("Important Thread");

        let json = serde_json::to_string_pretty(&resource_ref).unwrap();
        let decoded: ExternalResourceRef = serde_json::from_str(&json).unwrap();

        assert_eq!(decoded.service, ExternalServiceKind::Gmail);
        assert_eq!(decoded.resource_kind, ExternalResourceKind::EmailThread);
        assert_eq!(decoded.resource_id, "thread-123");
        assert_eq!(
            decoded.url.as_deref(),
            Some("https://mail.google.com/thread/123")
        );
        assert_eq!(decoded.title.as_deref(), Some("Important Thread"));
    }

    #[test]
    fn external_connector_id_roundtrip() {
        let id = ExternalConnectorId::from("gmail-connector-1");
        assert_eq!(id.0, "gmail-connector-1");
        assert_eq!(id.to_string(), "gmail-connector-1");

        let id = ExternalConnectorId::from("my-id".to_string());
        assert_eq!(id.0, "my-id");
    }

    #[tokio::test]
    async fn memory_external_connector_registry_register_and_list() {
        let registry = MemoryExternalConnectorRegistry::new();

        let reg = ExternalConnectorRegistration {
            id: ExternalConnectorId::from("gmail-1"),
            service: ExternalServiceKind::Gmail,
            display_name: "My Gmail".to_string(),
            connected_at: Utc::now(),
            last_synced_at: None,
        };

        registry.register(&reg).await.unwrap();

        let all = registry.list().await.unwrap();
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].id.0, "gmail-1");
        assert_eq!(all[0].service, ExternalServiceKind::Gmail);
    }

    #[tokio::test]
    async fn memory_external_connector_registry_list_by_service() {
        let registry = MemoryExternalConnectorRegistry::new();

        registry
            .register(&ExternalConnectorRegistration {
                id: ExternalConnectorId::from("gmail-1"),
                service: ExternalServiceKind::Gmail,
                display_name: "Gmail".to_string(),
                connected_at: Utc::now(),
                last_synced_at: None,
            })
            .await
            .unwrap();

        registry
            .register(&ExternalConnectorRegistration {
                id: ExternalConnectorId::from("github-1"),
                service: ExternalServiceKind::GitHub,
                display_name: "GitHub".to_string(),
                connected_at: Utc::now(),
                last_synced_at: None,
            })
            .await
            .unwrap();

        let gmail_connectors = registry
            .list_by_service(&ExternalServiceKind::Gmail)
            .await
            .unwrap();
        assert_eq!(gmail_connectors.len(), 1);
        assert_eq!(gmail_connectors[0].id.0, "gmail-1");

        let github_connectors = registry
            .list_by_service(&ExternalServiceKind::GitHub)
            .await
            .unwrap();
        assert_eq!(github_connectors.len(), 1);
        assert_eq!(github_connectors[0].id.0, "github-1");
    }

    #[tokio::test]
    async fn memory_external_connector_registry_get_and_unregister() {
        let registry = MemoryExternalConnectorRegistry::new();

        let reg = ExternalConnectorRegistration {
            id: ExternalConnectorId::from("gmail-1"),
            service: ExternalServiceKind::Gmail,
            display_name: "Gmail".to_string(),
            connected_at: Utc::now(),
            last_synced_at: None,
        };

        registry.register(&reg).await.unwrap();

        let retrieved = registry
            .get(&ExternalConnectorId::from("gmail-1"))
            .await
            .unwrap();
        assert_eq!(retrieved.display_name, "Gmail");

        registry
            .unregister(&ExternalConnectorId::from("gmail-1"))
            .await
            .unwrap();

        let result = registry.get(&ExternalConnectorId::from("gmail-1")).await;
        assert!(matches!(
            result,
            Err(ExternalConnectorError::ConnectorNotFound(_))
        ));
    }

    #[test]
    fn external_resource_list_roundtrip() {
        let list = ExternalResourceList {
            resources: vec![ExternalResourceMetadata {
                resource_ref: ExternalResourceRef::new(
                    ExternalServiceKind::GitHub,
                    ExternalResourceKind::Issue,
                    "issue-42",
                ),
                synced_at: Utc::now(),
                etag: Some("abc123".to_string()),
                raw_size_bytes: Some(1024),
            }],
            next_page_token: Some("page-2".to_string()),
            total_count: Some(100),
        };

        let json = serde_json::to_string_pretty(&list).unwrap();
        let decoded: ExternalResourceList = serde_json::from_str(&json).unwrap();

        assert_eq!(decoded.resources.len(), 1);
        assert_eq!(decoded.next_page_token.as_deref(), Some("page-2"));
        assert_eq!(decoded.total_count, Some(100));
    }

    // -----------------------------------------------------------------------
    // PR 147: Gmail Read-only Connector
    // -----------------------------------------------------------------------

    #[test]
    fn gmail_connector_service_kind() {
        let creds = ExternalServiceCredentials::new("test-token");
        let connector = GmailConnector::with_credentials(creds);
        assert_eq!(connector.service_kind(), ExternalServiceKind::Gmail);
    }

    #[test]
    fn gmail_connector_is_authenticated() {
        let creds = ExternalServiceCredentials::new("test-token");
        let connector = GmailConnector::with_credentials(creds);
        assert!(connector.is_authenticated());

        let creds = ExternalServiceCredentials::new("");
        let connector = GmailConnector::with_credentials(creds);
        assert!(!connector.is_authenticated());

        let creds = ExternalServiceCredentials::new("token")
            .with_expiry(Utc::now() - chrono::Duration::hours(1));
        let connector = GmailConnector::with_credentials(creds);
        assert!(!connector.is_authenticated());
    }

    #[test]
    fn gmail_connector_credentials() {
        let creds = ExternalServiceCredentials::new("test-token");
        let connector = GmailConnector::with_credentials(creds);
        assert!(connector.credentials().is_some());
        assert_eq!(connector.credentials().unwrap().access_token, "test-token");
    }

    #[test]
    fn gmail_connector_config_default() {
        let config = GmailConnectorConfig::default();
        assert_eq!(config.max_results_per_page, 20);
        assert!(config.query_filter.is_none());
        assert_eq!(config.label_ids, vec!["INBOX"]);
    }

    #[test]
    fn gmail_connector_build_list_threads_url() {
        let creds = ExternalServiceCredentials::new("token");
        let connector = GmailConnector::with_credentials(creds);

        let url = connector.build_list_threads_url(None, None);
        assert!(url.contains("maxResults=20"));
        assert!(url.contains("labelIds=INBOX"));

        let url = connector.build_list_threads_url(Some("page-token"), Some(10));
        assert!(url.contains("maxResults=10"));
        assert!(url.contains("pageToken=page-token"));
    }

    #[test]
    fn gmail_connector_build_get_thread_url() {
        let creds = ExternalServiceCredentials::new("token");
        let connector = GmailConnector::with_credentials(creds);

        let url = connector.build_get_thread_url("thread-123");
        assert!(url.contains("threads/thread-123"));
        assert!(url.contains("format=metadata"));
    }

    #[tokio::test]
    async fn gmail_connector_list_resources_unauthenticated() {
        let creds = ExternalServiceCredentials::new("");
        let connector = GmailConnector::with_credentials(creds);

        let result = connector
            .list_resources(&ExternalResourceKind::EmailThread, None, None)
            .await;
        assert!(matches!(
            result,
            Err(ExternalConnectorError::AuthenticationRequired)
        ));
    }

    #[tokio::test]
    async fn gmail_connector_list_resources_unsupported_kind() {
        let creds = ExternalServiceCredentials::new("token");
        let connector = GmailConnector::with_credentials(creds);

        let result = connector
            .list_resources(&ExternalResourceKind::Email, None, None)
            .await;
        assert!(matches!(
            result,
            Err(ExternalConnectorError::InvalidRequest(_))
        ));
    }

    #[tokio::test]
    async fn gmail_connector_get_resource_unauthenticated() {
        let creds = ExternalServiceCredentials::new("");
        let connector = GmailConnector::with_credentials(creds);

        let result = connector
            .get_resource(&ExternalResourceKind::EmailThread, "thread-1")
            .await;
        assert!(matches!(
            result,
            Err(ExternalConnectorError::AuthenticationRequired)
        ));
    }

    #[tokio::test]
    async fn gmail_connector_get_resource_requires_host_adapter() {
        let creds = ExternalServiceCredentials::new("token");
        let connector = GmailConnector::with_credentials(creds);

        let result = connector
            .get_resource(&ExternalResourceKind::EmailThread, "thread-1")
            .await;

        assert!(matches!(
            result,
            Err(ExternalConnectorError::ServiceUnavailable(message))
                if message.contains("host-provided Gmail API adapter")
                    && message.contains("thread-1")
        ));
    }

    #[test]
    fn gmail_oauth_boundary_authorization_url() {
        let boundary =
            GmailOAuthBoundary::new("client-id", "client-secret", "https://example.com/callback");
        let url = boundary.authorization_url("my-state");

        assert!(url.contains("client_id=client-id"));
        assert!(url.contains("redirect_uri=https://example.com/callback"));
        assert!(url.contains("scope="));
        assert!(url.contains("state=my-state"));
        assert!(url.contains("access_type=offline"));
    }

    #[test]
    fn gmail_oauth_boundary_scopes() {
        let boundary = GmailOAuthBoundary::new("id", "secret", "https://example.com");
        assert_eq!(boundary.scopes.len(), 1);
        assert_eq!(
            boundary.scopes[0],
            "https://www.googleapis.com/auth/gmail.readonly"
        );
    }

    #[test]
    fn gmail_provider_retry_policy_retries_timeout_and_transient_errors() {
        let policy = GmailProviderRetryPolicy::new(GmailProviderRetryConfig {
            max_attempts: 3,
            base_delay_ms: 100,
            max_delay_ms: 1_000,
            timeout_ms: 5_000,
        });

        let timeout = ExternalConnectorError::ServiceUnavailable("request timeout exceeded".into());
        let transient =
            ExternalConnectorError::ServiceUnavailable("gmail 503 backend error".into());

        assert_eq!(
            policy.classify_error(&timeout),
            GmailProviderErrorClass::Timeout
        );
        assert_eq!(
            policy.decide(&timeout, 1),
            GmailProviderRetryDecision::Retry {
                error_class: GmailProviderErrorClass::Timeout,
                delay_ms: 100,
                retry_after_ms: None,
            }
        );
        assert_eq!(
            policy.decide(&transient, 2),
            GmailProviderRetryDecision::Retry {
                error_class: GmailProviderErrorClass::TransientProvider,
                delay_ms: 200,
                retry_after_ms: None,
            }
        );
    }

    #[test]
    fn gmail_provider_retry_policy_uses_retry_after_for_rate_limits() {
        let policy = GmailProviderRetryPolicy::default();
        let rate_limited = ExternalConnectorError::RateLimited(17);

        assert_eq!(
            policy.classify_error(&rate_limited),
            GmailProviderErrorClass::RateLimited
        );
        assert_eq!(
            policy.decide(&rate_limited, 1),
            GmailProviderRetryDecision::Retry {
                error_class: GmailProviderErrorClass::RateLimited,
                delay_ms: 17_000,
                retry_after_ms: Some(17_000),
            }
        );
    }

    #[test]
    fn gmail_provider_retry_policy_fails_closed_for_auth_and_invalid_request() {
        let policy = GmailProviderRetryPolicy::default();

        assert_eq!(
            policy.decide(&ExternalConnectorError::AuthenticationRequired, 1),
            GmailProviderRetryDecision::DoNotRetry {
                error_class: GmailProviderErrorClass::Authentication,
                reason: "authentication/credential failure is not retryable".to_string(),
            }
        );
        assert_eq!(
            policy.decide(
                &ExternalConnectorError::InvalidRequest("bad query".into()),
                1
            ),
            GmailProviderRetryDecision::DoNotRetry {
                error_class: GmailProviderErrorClass::InvalidRequest,
                reason: "invalid request is not retryable".to_string(),
            }
        );
    }

    #[test]
    fn gmail_provider_retry_policy_exhausts_at_max_attempts() {
        let policy = GmailProviderRetryPolicy::new(GmailProviderRetryConfig {
            max_attempts: 2,
            base_delay_ms: 100,
            max_delay_ms: 1_000,
            timeout_ms: 5_000,
        });
        let timeout = ExternalConnectorError::ServiceUnavailable("timeout".into());

        assert_eq!(
            policy.decide(&timeout, 2),
            GmailProviderRetryDecision::Exhausted {
                error_class: GmailProviderErrorClass::Timeout,
                attempts: 2,
            }
        );
    }

    #[test]
    fn gmail_connector_operation_audit_shape_is_metadata_only() {
        let now = Utc::now();
        let event = ConnectorOperationAuditEvent::result(
            ExternalServiceKind::Gmail,
            "gmail-connector-1",
            Some("user-1"),
            ConnectorOperationKind::GetResource,
            ExternalResourceKind::EmailThread,
            Some("thread-1"),
            ConnectorOperationOutcome::Succeeded,
            None,
            "metadata read completed",
            now,
        );

        assert_eq!(event.service, ExternalServiceKind::Gmail);
        assert_eq!(event.connector_id, "gmail-connector-1");
        assert_eq!(event.account_id.as_deref(), Some("user-1"));
        assert_eq!(event.resource_id.as_deref(), Some("thread-1"));
        assert_eq!(event.payload_redaction, "gmail message content omitted");
        assert_eq!(event.credential_redaction, "oauth token material omitted");

        let serialized = serde_json::to_string(&event).unwrap();
        assert!(!serialized.contains("access-token"));
        assert!(!serialized.contains("refresh-token"));
        assert!(!serialized.contains("message body"));
        assert!(!serialized.contains("snippet text"));
    }

    #[tokio::test]
    async fn gmail_read_records_start_and_result_audit_events() {
        let connector = GmailConnector::with_credentials(ExternalServiceCredentials::new("token"));
        let now = Utc::now();
        let start = ConnectorOperationAuditEvent::started(
            connector.service_kind(),
            "gmail-connector-1",
            Some("user-1"),
            ConnectorOperationKind::ListResources,
            ExternalResourceKind::EmailThread,
            None,
            now,
        );
        let result = connector
            .list_resources(&ExternalResourceKind::EmailThread, None, None)
            .await;
        let completed = ConnectorOperationAuditEvent::result(
            connector.service_kind(),
            "gmail-connector-1",
            Some("user-1"),
            ConnectorOperationKind::ListResources,
            ExternalResourceKind::EmailThread,
            None,
            ConnectorOperationOutcome::Succeeded,
            None,
            "read-only list completed",
            now,
        );

        assert!(matches!(
            result,
            Err(ExternalConnectorError::ServiceUnavailable(message))
                if message.contains("host-provided Gmail API adapter")
        ));
        assert_eq!(start.outcome, ConnectorOperationOutcome::Started);
        assert_eq!(completed.outcome, ConnectorOperationOutcome::Succeeded);
        assert_eq!(completed.resource_kind, ExternalResourceKind::EmailThread);
        assert_eq!(completed.payload_redaction, "gmail message content omitted");
    }

    #[test]
    fn gmail_offboarded_account_access_is_denied_and_audited() {
        let decision = evaluate_connector_account_access(
            ExternalServiceKind::Gmail,
            "gmail-connector-1",
            Some("user-1"),
            ConnectorHostAccountLifecycle::Offboarded,
            ConnectorOperationKind::ListResources,
            ExternalResourceKind::EmailThread,
            None,
            Utc::now(),
        );

        match decision {
            ConnectorAccountAccessDecision::Denied(event) => {
                assert_eq!(event.outcome, ConnectorOperationOutcome::Denied);
                assert_eq!(
                    event.error_class,
                    Some(GmailProviderErrorClass::Authentication)
                );
                assert_eq!(event.reason, "connector account is offboarded");
                assert_eq!(event.payload_redaction, "gmail message content omitted");
                assert_eq!(event.credential_redaction, "oauth token material omitted");
            }
            ConnectorAccountAccessDecision::Allowed => {
                panic!("offboarded Gmail account must fail closed")
            }
        }
    }
}
