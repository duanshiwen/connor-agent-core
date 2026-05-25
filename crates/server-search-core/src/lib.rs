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
    EnterpriseUserId, PermissionAction, PermissionStore, ResourceType,
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
}
