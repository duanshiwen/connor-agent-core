//! # Server Search Core
//!
//! Authorized search with permission filtering for AgentOS enterprise.
//!
//! This crate provides the search layer that respects enterprise permissions:
//! - SearchRequest / SearchResult types
//! - SourceBoundary: marks which enterprise resource a result comes from
//! - AuthorizedSearchExecutor: filters results by user permissions
//! - Permission-aware result filtering
//!
//! Design principles:
//! - No permission = not in results
//! - Unauthorized content never enters Agent context
//! - Unauthorized content never appears as Answer Cache evidence
//! - File references are filtered by the same permission model

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use enterprise_permission_core::{
    CacheStatus, CachedPermissionDecision, EnterpriseUserId, PermissionAction, PermissionDecision,
    PermissionStore, ResourceId, ResourceType, ServerBackedPermissionStore,
};
use serde::{Deserialize, Serialize};
use std::fmt;

// ---------------------------------------------------------------------------
// Search Request
// ---------------------------------------------------------------------------

/// A search query with user context.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SearchRequest {
    pub query: String,
    pub user_id: EnterpriseUserId,
    pub limit: Option<usize>,
    pub resource_types: Option<Vec<ResourceType>>,
}

// ---------------------------------------------------------------------------
// Source Boundary
// ---------------------------------------------------------------------------

/// Identifies the enterprise resource a search result comes from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceBoundary {
    pub resource_type: ResourceType,
    pub resource_id: String,
    pub source_name: String,
}

// ---------------------------------------------------------------------------
// Search Result
// ---------------------------------------------------------------------------

/// A single search result with source attribution.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SearchResult {
    pub id: String,
    pub title: String,
    pub snippet: String,
    pub source: SourceBoundary,
    pub score: f64,
    pub timestamp: Option<DateTime<Utc>>,
}

// ---------------------------------------------------------------------------
// Search Response
// ---------------------------------------------------------------------------

/// Response from a search operation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SearchResponse {
    pub results: Vec<SearchResult>,
    pub total_found: usize,
    pub filtered_count: usize,
    pub query: String,
}

// ---------------------------------------------------------------------------
// Search Executor trait
// ---------------------------------------------------------------------------

/// Trait for search backends.
#[async_trait]
pub trait SearchExecutor: Send + Sync + fmt::Debug {
    /// Execute a search and return raw results (before permission filtering).
    async fn search_raw(&self, request: &SearchRequest) -> Result<Vec<SearchResult>, SearchError>;
}

// ---------------------------------------------------------------------------
// Authorized Search Executor
// ---------------------------------------------------------------------------

/// Wraps a search executor and filters results by enterprise permissions.
#[derive(Debug)]
pub struct AuthorizedSearchExecutor<E: SearchExecutor> {
    inner: E,
    permission_store: PermissionStore,
}

impl<E: SearchExecutor> AuthorizedSearchExecutor<E> {
    pub fn new(inner: E, permission_store: PermissionStore) -> Self {
        Self {
            inner,
            permission_store,
        }
    }

    /// Execute a search with permission filtering.
    ///
    /// Results from unauthorized sources are removed.
    pub async fn search(&self, request: &SearchRequest) -> Result<SearchResponse, SearchError> {
        let raw_results = self.inner.search_raw(request).await?;
        let total_found = raw_results.len();

        // Filter by permissions
        let now = Utc::now();
        let filtered: Vec<SearchResult> = raw_results
            .into_iter()
            .filter(|result| {
                let resource_type = &result.source.resource_type;
                let resource_id =
                    enterprise_permission_core::ResourceId(result.source.resource_id.clone());

                let decision = self.permission_store.check(
                    &request.user_id,
                    resource_type,
                    &resource_id,
                    &PermissionAction::Read,
                    now,
                );

                decision.is_allowed()
            })
            .collect();

        let filtered_count = total_found - filtered.len();

        Ok(SearchResponse {
            results: filtered,
            total_found,
            filtered_count,
            query: request.query.clone(),
        })
    }
}

// ---------------------------------------------------------------------------
// Enterprise Search Backend Boundary
// ---------------------------------------------------------------------------

/// Enterprise search request with organization scope.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EnterpriseSearchRequest {
    pub query: String,
    pub user_id: EnterpriseUserId,
    pub organization_id: Option<String>,
    pub limit: Option<usize>,
    pub resource_types: Option<Vec<ResourceType>>,
}

/// Raw backend response before permission filtering.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EnterpriseSearchBackendResponse {
    pub results: Vec<SearchResult>,
    pub backend_total_found: usize,
}

/// Enterprise search response after permission filtering.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EnterpriseSearchResponse {
    pub results: Vec<SearchResult>,
    pub total_found: usize,
    pub filtered_count: usize,
    pub query: String,
    pub cache_status: Option<CacheStatus>,
    pub policy_warnings: Vec<String>,
}

/// Policy for safe enterprise search response shaping.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct EnterpriseSearchPolicy {
    pub redact_unauthorized_metadata: bool,
}

impl Default for EnterpriseSearchPolicy {
    fn default() -> Self {
        Self {
            redact_unauthorized_metadata: true,
        }
    }
}

/// Trait for enterprise search backends.
#[async_trait]
pub trait EnterpriseSearchBackend: Send + Sync + fmt::Debug {
    async fn search_enterprise(
        &self,
        request: &EnterpriseSearchRequest,
    ) -> Result<EnterpriseSearchBackendResponse, SearchError>;
}

/// Permission source used by enterprise search filtering.
pub trait EnterpriseSearchPermissionSource {
    fn check_search_permission(
        &self,
        user_id: &EnterpriseUserId,
        resource_type: &ResourceType,
        resource_id: &ResourceId,
        now: DateTime<Utc>,
    ) -> CachedPermissionDecision;
}

impl EnterpriseSearchPermissionSource for PermissionStore {
    fn check_search_permission(
        &self,
        user_id: &EnterpriseUserId,
        resource_type: &ResourceType,
        resource_id: &ResourceId,
        now: DateTime<Utc>,
    ) -> CachedPermissionDecision {
        CachedPermissionDecision {
            decision: self.check(
                user_id,
                resource_type,
                resource_id,
                &PermissionAction::Read,
                now,
            ),
            cache_status: CacheStatus::Fresh,
            explanation: "local permission store".to_string(),
        }
    }
}

impl<P> EnterpriseSearchPermissionSource for ServerBackedPermissionStore<P>
where
    P: enterprise_permission_core::RemotePermissionProvider,
{
    fn check_search_permission(
        &self,
        user_id: &EnterpriseUserId,
        resource_type: &ResourceType,
        resource_id: &ResourceId,
        now: DateTime<Utc>,
    ) -> CachedPermissionDecision {
        self.check_with_inheritance(
            user_id,
            resource_type,
            resource_id,
            &PermissionAction::Read,
            now,
        )
    }
}

/// Enterprise search executor that filters backend results by enterprise permissions.
#[derive(Debug)]
pub struct EnterpriseSearchExecutor<B, P> {
    backend: B,
    permission_source: P,
    policy: EnterpriseSearchPolicy,
}

impl<B, P> EnterpriseSearchExecutor<B, P>
where
    B: EnterpriseSearchBackend,
    P: EnterpriseSearchPermissionSource,
{
    pub fn new(backend: B, permission_source: P, policy: EnterpriseSearchPolicy) -> Self {
        Self {
            backend,
            permission_source,
            policy,
        }
    }

    pub async fn search(
        &self,
        request: &EnterpriseSearchRequest,
    ) -> Result<EnterpriseSearchResponse, SearchError> {
        self.search_at(request, Utc::now()).await
    }

    pub async fn search_at(
        &self,
        request: &EnterpriseSearchRequest,
        now: DateTime<Utc>,
    ) -> Result<EnterpriseSearchResponse, SearchError> {
        let backend_response = self.backend.search_enterprise(request).await?;
        let backend_total_found = backend_response.backend_total_found;
        let mut results = Vec::new();
        let mut filtered_count = 0;
        let mut cache_status = None;
        let mut policy_warnings = Vec::new();

        for result in backend_response.results {
            let resource_id = ResourceId(result.source.resource_id.clone());
            let decision = self.permission_source.check_search_permission(
                &request.user_id,
                &result.source.resource_type,
                &resource_id,
                now,
            );
            cache_status = Some(most_conservative_cache_status(
                cache_status,
                decision.cache_status,
            ));
            if decision.cache_status == CacheStatus::Stale {
                push_unique_warning(
                    &mut policy_warnings,
                    format!("permission cache is stale: {}", decision.explanation),
                );
            }

            if decision.decision == PermissionDecision::Allow {
                results.push(result);
            } else if !self.policy.redact_unauthorized_metadata {
                filtered_count += 1;
            }
        }

        let total_found = if self.policy.redact_unauthorized_metadata {
            results.len()
        } else {
            backend_total_found
        };
        if self.policy.redact_unauthorized_metadata {
            filtered_count = 0;
        }

        Ok(EnterpriseSearchResponse {
            results,
            total_found,
            filtered_count,
            query: request.query.clone(),
            cache_status,
            policy_warnings,
        })
    }
}

fn most_conservative_cache_status(current: Option<CacheStatus>, next: CacheStatus) -> CacheStatus {
    match (current, next) {
        (Some(CacheStatus::Expired), _) | (_, CacheStatus::Expired) => CacheStatus::Expired,
        (Some(CacheStatus::Stale), _) | (_, CacheStatus::Stale) => CacheStatus::Stale,
        _ => CacheStatus::Fresh,
    }
}

fn push_unique_warning(warnings: &mut Vec<String>, warning: String) {
    if !warnings.iter().any(|existing| existing == &warning) {
        warnings.push(warning);
    }
}

// ---------------------------------------------------------------------------
// Answer Cache Evidence Filter
// ---------------------------------------------------------------------------

/// Filter evidence items by permission.
///
/// Removes evidence from unauthorized sources before including in Answer Cache.
pub fn filter_evidence_by_permission(
    evidence: Vec<SearchResult>,
    user_id: &EnterpriseUserId,
    permission_store: &PermissionStore,
    now: DateTime<Utc>,
) -> Vec<SearchResult> {
    evidence
        .into_iter()
        .filter(|result| {
            let resource_id =
                enterprise_permission_core::ResourceId(result.source.resource_id.clone());

            let decision = permission_store.check(
                user_id,
                &result.source.resource_type,
                &resource_id,
                &PermissionAction::Read,
                now,
            );

            decision.is_allowed()
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Error
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SearchError {
    #[error("search backend error: {0}")]
    BackendError(String),
    #[error("permission check failed: {0}")]
    PermissionError(String),
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use enterprise_permission_core::{EnterpriseRole, PermissionGrant, PermissionStore};

    fn ts() -> DateTime<Utc> {
        "2026-05-25T12:00:00Z".parse().unwrap()
    }

    fn user_a() -> EnterpriseUserId {
        EnterpriseUserId::from("user-001")
    }

    fn user_b() -> EnterpriseUserId {
        EnterpriseUserId::from("user-002")
    }

    fn make_result(id: &str, resource_id: &str) -> SearchResult {
        SearchResult {
            id: id.to_string(),
            title: format!("Result {}", id),
            snippet: "Some content".to_string(),
            source: SourceBoundary {
                resource_type: ResourceType::KnowledgeBase,
                resource_id: resource_id.to_string(),
                source_name: "KB Main".to_string(),
            },
            score: 0.95,
            timestamp: Some(ts()),
        }
    }

    fn setup_permission_store() -> PermissionStore {
        let mut store = PermissionStore::new();
        // User A can read kb-1
        store.add_grant(PermissionGrant {
            grant_id: "grant-1".to_string(),
            user_id: user_a(),
            role: EnterpriseRole::User,
            resource_type: ResourceType::KnowledgeBase,
            resource_id: enterprise_permission_core::ResourceId::from("kb-1"),
            actions: vec![PermissionAction::Read],
            granted_at: ts(),
            expires_at: None,
            revoked: false,
        });
        store
    }

    // ---- Type roundtrips ----

    #[test]
    fn search_request_roundtrips() {
        let req = SearchRequest {
            query: "test query".to_string(),
            user_id: user_a(),
            limit: Some(10),
            resource_types: Some(vec![ResourceType::KnowledgeBase]),
        };
        let json = serde_json::to_string(&req).unwrap();
        let decoded: SearchRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded.query, "test query");
    }

    #[test]
    fn source_boundary_roundtrips() {
        let boundary = SourceBoundary {
            resource_type: ResourceType::FileArea,
            resource_id: "files-1".to_string(),
            source_name: "Documents".to_string(),
        };
        let json = serde_json::to_string(&boundary).unwrap();
        let decoded: SourceBoundary = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded.resource_id, "files-1");
    }

    #[test]
    fn search_result_roundtrips() {
        let result = make_result("r1", "kb-1");
        let json = serde_json::to_string_pretty(&result).unwrap();
        let decoded: SearchResult = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded.id, "r1");
    }

    #[test]
    fn search_response_roundtrips() {
        let response = SearchResponse {
            results: vec![make_result("r1", "kb-1")],
            total_found: 2,
            filtered_count: 1,
            query: "test".to_string(),
        };
        let json = serde_json::to_string(&response).unwrap();
        let decoded: SearchResponse = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded.total_found, 2);
        assert_eq!(decoded.filtered_count, 1);
    }

    // ---- AuthorizedSearchExecutor tests ----

    #[derive(Debug)]
    struct FakeSearchExecutor {
        results: Vec<SearchResult>,
    }

    impl FakeSearchExecutor {
        fn new(results: Vec<SearchResult>) -> Self {
            Self { results }
        }
    }

    #[async_trait]
    impl SearchExecutor for FakeSearchExecutor {
        async fn search_raw(
            &self,
            _request: &SearchRequest,
        ) -> Result<Vec<SearchResult>, SearchError> {
            Ok(self.results.clone())
        }
    }

    #[tokio::test]
    async fn authorized_search_filters_unauthorized() {
        let results = vec![
            make_result("r1", "kb-1"),
            make_result("r2", "kb-2"), // User A has no access to kb-2
        ];

        let executor = FakeSearchExecutor::new(results);
        let store = setup_permission_store();
        let authorized = AuthorizedSearchExecutor::new(executor, store);

        let request = SearchRequest {
            query: "test".to_string(),
            user_id: user_a(),
            limit: None,
            resource_types: None,
        };

        let response = authorized.search(&request).await.unwrap();

        assert_eq!(response.total_found, 2);
        assert_eq!(response.filtered_count, 1);
        assert_eq!(response.results.len(), 1);
        assert_eq!(response.results[0].id, "r1");
    }

    #[tokio::test]
    async fn authorized_search_allows_authorized() {
        let results = vec![make_result("r1", "kb-1")];

        let executor = FakeSearchExecutor::new(results);
        let store = setup_permission_store();
        let authorized = AuthorizedSearchExecutor::new(executor, store);

        let request = SearchRequest {
            query: "test".to_string(),
            user_id: user_a(),
            limit: None,
            resource_types: None,
        };

        let response = authorized.search(&request).await.unwrap();

        assert_eq!(response.total_found, 1);
        assert_eq!(response.filtered_count, 0);
        assert_eq!(response.results.len(), 1);
    }

    #[tokio::test]
    async fn unauthorized_user_gets_no_results() {
        let results = vec![make_result("r1", "kb-1")];

        let executor = FakeSearchExecutor::new(results);
        let store = setup_permission_store();
        let authorized = AuthorizedSearchExecutor::new(executor, store);

        let request = SearchRequest {
            query: "test".to_string(),
            user_id: user_b(), // No grants for user B
            limit: None,
            resource_types: None,
        };

        let response = authorized.search(&request).await.unwrap();

        assert_eq!(response.total_found, 1);
        assert_eq!(response.filtered_count, 1);
        assert!(response.results.is_empty());
    }

    // ---- Evidence filter tests ----

    #[test]
    fn filter_evidence_removes_unauthorized() {
        let evidence = vec![make_result("e1", "kb-1"), make_result("e2", "kb-2")];

        let store = setup_permission_store();
        let filtered = filter_evidence_by_permission(evidence, &user_a(), &store, ts());

        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].id, "e1");
    }

    #[test]
    fn filter_evidence_empty_for_unauthorized_user() {
        let evidence = vec![make_result("e1", "kb-1")];

        let store = setup_permission_store();
        let filtered = filter_evidence_by_permission(evidence, &user_b(), &store, ts());

        assert!(filtered.is_empty());
    }

    #[test]
    fn filter_evidence_all_authorized() {
        let mut store = PermissionStore::new();
        store.add_grant(PermissionGrant {
            grant_id: "grant-1".to_string(),
            user_id: user_a(),
            role: EnterpriseRole::User,
            resource_type: ResourceType::KnowledgeBase,
            resource_id: enterprise_permission_core::ResourceId::from("kb-1"),
            actions: vec![PermissionAction::Read],
            granted_at: ts(),
            expires_at: None,
            revoked: false,
        });
        store.add_grant(PermissionGrant {
            grant_id: "grant-2".to_string(),
            user_id: user_a(),
            role: EnterpriseRole::User,
            resource_type: ResourceType::KnowledgeBase,
            resource_id: enterprise_permission_core::ResourceId::from("kb-2"),
            actions: vec![PermissionAction::Read],
            granted_at: ts(),
            expires_at: None,
            revoked: false,
        });

        let evidence = vec![make_result("e1", "kb-1"), make_result("e2", "kb-2")];

        let filtered = filter_evidence_by_permission(evidence, &user_a(), &store, ts());

        assert_eq!(filtered.len(), 2);
    }

    // ---- Enterprise search backend boundary tests ----

    #[derive(Debug)]
    struct FakeEnterpriseSearchBackend {
        results: Vec<SearchResult>,
    }

    #[async_trait]
    impl EnterpriseSearchBackend for FakeEnterpriseSearchBackend {
        async fn search_enterprise(
            &self,
            request: &EnterpriseSearchRequest,
        ) -> Result<EnterpriseSearchBackendResponse, SearchError> {
            assert_eq!(request.query, "secret");
            assert_eq!(request.user_id, user_a());
            assert_eq!(request.organization_id.as_deref(), Some("org-1"));
            Ok(EnterpriseSearchBackendResponse {
                results: self.results.clone(),
                backend_total_found: self.results.len(),
            })
        }
    }

    #[tokio::test]
    async fn enterprise_search_redacts_unauthorized_metadata() {
        let backend = FakeEnterpriseSearchBackend {
            results: vec![
                make_result("r1", "kb-1"),
                SearchResult {
                    id: "secret-r2".to_string(),
                    title: "Secret Result".to_string(),
                    snippet: "Do not leak this snippet".to_string(),
                    source: SourceBoundary {
                        resource_type: ResourceType::KnowledgeBase,
                        resource_id: "secret-kb-2".to_string(),
                        source_name: "Secret KB".to_string(),
                    },
                    score: 0.99,
                    timestamp: Some(ts()),
                },
            ],
        };
        let policy = EnterpriseSearchPolicy {
            redact_unauthorized_metadata: true,
        };
        let executor = EnterpriseSearchExecutor::new(backend, setup_permission_store(), policy);

        let request = EnterpriseSearchRequest {
            query: "secret".to_string(),
            user_id: user_a(),
            organization_id: Some("org-1".to_string()),
            limit: None,
            resource_types: Some(vec![ResourceType::KnowledgeBase]),
        };

        let response = executor.search(&request).await.unwrap();

        assert_eq!(response.results.len(), 1);
        assert_eq!(response.results[0].id, "r1");
        assert_eq!(response.total_found, 1);
        assert_eq!(response.filtered_count, 0);
        assert!(response.policy_warnings.is_empty());
        let serialized = serde_json::to_string(&response).unwrap();
        assert!(!serialized.contains("secret-r2"));
        assert!(!serialized.contains("Secret KB"));
        assert!(!serialized.contains("Do not leak this snippet"));
        assert!(!serialized.contains("secret-kb-2"));
    }

    #[tokio::test]
    async fn enterprise_search_can_report_filtered_count_when_not_redacting() {
        let backend = FakeEnterpriseSearchBackend {
            results: vec![make_result("r1", "kb-1"), make_result("r2", "kb-2")],
        };
        let policy = EnterpriseSearchPolicy {
            redact_unauthorized_metadata: false,
        };
        let executor = EnterpriseSearchExecutor::new(backend, setup_permission_store(), policy);

        let request = EnterpriseSearchRequest {
            query: "secret".to_string(),
            user_id: user_a(),
            organization_id: Some("org-1".to_string()),
            limit: None,
            resource_types: None,
        };

        let response = executor.search(&request).await.unwrap();

        assert_eq!(response.total_found, 2);
        assert_eq!(response.filtered_count, 1);
        assert_eq!(response.results.len(), 1);
        assert_eq!(response.results[0].id, "r1");
    }

    #[tokio::test]
    async fn enterprise_search_warns_when_permission_cache_is_stale() {
        let backend = FakeEnterpriseSearchBackend {
            results: vec![make_result("r1", "kb-1")],
        };
        let mut provider =
            enterprise_permission_core::MemoryRemotePermissionProvider::with_snapshot(
                enterprise_permission_core::ServerPermissionSnapshot {
                    snapshot_id: "snap-1".to_string(),
                    fetched_at: ts(),
                    direct_grants: vec![PermissionGrant {
                        grant_id: "grant-1".to_string(),
                        user_id: user_a(),
                        role: EnterpriseRole::User,
                        resource_type: ResourceType::KnowledgeBase,
                        resource_id: enterprise_permission_core::ResourceId::from("kb-1"),
                        actions: vec![PermissionAction::Read],
                        granted_at: ts(),
                        expires_at: None,
                        revoked: false,
                    }],
                    memberships: vec![],
                    org_grants: vec![],
                    team_grants: vec![],
                    group_grants: vec![],
                },
            );
        let mut permission_store = enterprise_permission_core::ServerBackedPermissionStore::new(
            provider.clone(),
            enterprise_permission_core::CachePolicy {
                fresh_for: chrono::Duration::minutes(5),
                stale_for: chrono::Duration::minutes(30),
            },
        );
        permission_store.refresh(ts()).unwrap();
        provider.set_available(false);
        permission_store.set_provider(provider);
        permission_store
            .refresh(ts() + chrono::Duration::minutes(10))
            .unwrap();

        let executor = EnterpriseSearchExecutor::new(
            backend,
            permission_store,
            EnterpriseSearchPolicy::default(),
        );
        let request = EnterpriseSearchRequest {
            query: "secret".to_string(),
            user_id: user_a(),
            organization_id: Some("org-1".to_string()),
            limit: None,
            resource_types: None,
        };

        let response = executor
            .search_at(&request, ts() + chrono::Duration::minutes(10))
            .await
            .unwrap();

        assert_eq!(response.results.len(), 1);
        assert_eq!(
            response.cache_status,
            Some(enterprise_permission_core::CacheStatus::Stale)
        );
        assert_eq!(response.policy_warnings.len(), 1);
        assert!(response.policy_warnings[0].contains("stale"));
    }
}
