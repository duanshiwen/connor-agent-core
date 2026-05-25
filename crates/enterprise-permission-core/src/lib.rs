//! # Enterprise Permission Core
//!
//! Permission types and policy for AgentOS enterprise features.
//!
//! This crate defines the access control model for enterprise resources:
//! - Roles: SuperAdmin, Admin, User
//! - Resources: KnowledgeBase, FileArea, File, Asset, SearchResult, AnswerCacheEvidence
//! - Grants: explicit allow/deny with expiry
//! - Policy: deny-by-default, explicit allow required
//!
//! Design principles:
//! - Deny by default: no access unless explicitly granted
//! - Grants are scoped to (user, role, resource_type, resource_id)
//! - Revoked grants deny access immediately
//! - Enterprise assets follow the same permission model

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fmt;

// ---------------------------------------------------------------------------
// Enterprise Identity
// ---------------------------------------------------------------------------

/// Unique identifier for an enterprise user.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct EnterpriseUserId(pub String);

impl fmt::Display for EnterpriseUserId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl From<String> for EnterpriseUserId {
    fn from(s: String) -> Self {
        Self(s)
    }
}

impl From<&str> for EnterpriseUserId {
    fn from(s: &str) -> Self {
        Self(s.to_string())
    }
}

// ---------------------------------------------------------------------------
// Enterprise Roles
// ---------------------------------------------------------------------------

/// Enterprise role hierarchy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EnterpriseRole {
    User,
    Admin,
    SuperAdmin,
}

impl fmt::Display for EnterpriseRole {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::User => write!(f, "user"),
            Self::Admin => write!(f, "admin"),
            Self::SuperAdmin => write!(f, "super_admin"),
        }
    }
}

// ---------------------------------------------------------------------------
// Resource Types
// ---------------------------------------------------------------------------

/// Types of enterprise resources that can be permission-controlled.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResourceType {
    KnowledgeBase,
    FileArea,
    File,
    Asset,
    SearchResult,
    AnswerCacheEvidence,
}

impl fmt::Display for ResourceType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::KnowledgeBase => write!(f, "knowledge_base"),
            Self::FileArea => write!(f, "file_area"),
            Self::File => write!(f, "file"),
            Self::Asset => write!(f, "asset"),
            Self::SearchResult => write!(f, "search_result"),
            Self::AnswerCacheEvidence => write!(f, "answer_cache_evidence"),
        }
    }
}

/// Unique identifier for a resource.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ResourceId(pub String);

impl fmt::Display for ResourceId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl From<String> for ResourceId {
    fn from(s: String) -> Self {
        Self(s)
    }
}

impl From<&str> for ResourceId {
    fn from(s: &str) -> Self {
        Self(s.to_string())
    }
}

// ---------------------------------------------------------------------------
// Permission Grant
// ---------------------------------------------------------------------------

/// Action that can be granted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PermissionAction {
    Read,
    Write,
    Delete,
    Admin,
}

impl fmt::Display for PermissionAction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Read => write!(f, "read"),
            Self::Write => write!(f, "write"),
            Self::Delete => write!(f, "delete"),
            Self::Admin => write!(f, "admin"),
        }
    }
}

/// A permission grant for a user on a resource.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PermissionGrant {
    pub grant_id: String,
    pub user_id: EnterpriseUserId,
    pub role: EnterpriseRole,
    pub resource_type: ResourceType,
    pub resource_id: ResourceId,
    pub actions: Vec<PermissionAction>,
    pub granted_at: DateTime<Utc>,
    pub expires_at: Option<DateTime<Utc>>,
    pub revoked: bool,
}

impl PermissionGrant {
    /// Check if this grant is currently active.
    pub fn is_active(&self, now: DateTime<Utc>) -> bool {
        if self.revoked {
            return false;
        }
        if let Some(expires) = self.expires_at {
            now < expires
        } else {
            true
        }
    }

    /// Check if this grant allows a specific action.
    pub fn allows(&self, action: &PermissionAction) -> bool {
        self.actions.contains(action)
    }
}

// ---------------------------------------------------------------------------
// Permission Decision
// ---------------------------------------------------------------------------

/// Result of a permission check.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PermissionDecision {
    Allow,
    Deny,
}

impl fmt::Display for PermissionDecision {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Allow => write!(f, "allow"),
            Self::Deny => write!(f, "deny"),
        }
    }
}

impl PermissionDecision {
    pub fn is_allowed(&self) -> bool {
        matches!(self, Self::Allow)
    }
}

// ---------------------------------------------------------------------------
// Enterprise Asset Policy
// ---------------------------------------------------------------------------

/// Policy for enterprise assets (knowledge bases, files, etc.).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EnterpriseAssetPolicy {
    pub resource_type: ResourceType,
    pub resource_id: ResourceId,
    pub default_decision: PermissionDecision,
    pub require_explicit_grant: bool,
}

impl EnterpriseAssetPolicy {
    /// Create a policy that denies by default.
    pub fn deny_by_default(resource_type: ResourceType, resource_id: ResourceId) -> Self {
        Self {
            resource_type,
            resource_id,
            default_decision: PermissionDecision::Deny,
            require_explicit_grant: true,
        }
    }

    /// Create a policy that allows by default (for public resources).
    pub fn allow_by_default(resource_type: ResourceType, resource_id: ResourceId) -> Self {
        Self {
            resource_type,
            resource_id,
            default_decision: PermissionDecision::Allow,
            require_explicit_grant: false,
        }
    }
}

// ---------------------------------------------------------------------------
// Permission Store
// ---------------------------------------------------------------------------

/// In-memory store for permission grants.
#[derive(Debug, Clone, Default)]
pub struct PermissionStore {
    grants: Vec<PermissionGrant>,
}

impl PermissionStore {
    pub fn new() -> Self {
        Self { grants: Vec::new() }
    }

    /// Add a grant to the store.
    pub fn add_grant(&mut self, grant: PermissionGrant) {
        self.grants.push(grant);
    }

    /// Revoke a grant by id.
    pub fn revoke_grant(&mut self, grant_id: &str) -> bool {
        if let Some(grant) = self.grants.iter_mut().find(|g| g.grant_id == grant_id) {
            grant.revoked = true;
            true
        } else {
            false
        }
    }

    /// Check permission for a user on a resource.
    pub fn check(
        &self,
        user_id: &EnterpriseUserId,
        resource_type: &ResourceType,
        resource_id: &ResourceId,
        action: &PermissionAction,
        now: DateTime<Utc>,
    ) -> PermissionDecision {
        // Find matching active grants
        let matching: Vec<&PermissionGrant> = self
            .grants
            .iter()
            .filter(|g| {
                g.user_id == *user_id
                    && g.resource_type == *resource_type
                    && g.resource_id == *resource_id
                    && g.is_active(now)
                    && g.allows(action)
            })
            .collect();

        if matching.is_empty() {
            PermissionDecision::Deny
        } else {
            PermissionDecision::Allow
        }
    }

    /// Check permission with role-based escalation.
    ///
    /// SuperAdmin bypasses all checks. Admin can access admin-level resources.
    pub fn check_with_role(
        &self,
        user_id: &EnterpriseUserId,
        role: EnterpriseRole,
        resource_type: &ResourceType,
        resource_id: &ResourceId,
        action: &PermissionAction,
        now: DateTime<Utc>,
    ) -> PermissionDecision {
        // SuperAdmin bypasses all checks
        if role == EnterpriseRole::SuperAdmin {
            return PermissionDecision::Allow;
        }

        // Admin can read anything
        if role == EnterpriseRole::Admin && *action == PermissionAction::Read {
            return PermissionDecision::Allow;
        }

        // Otherwise check grants
        self.check(user_id, resource_type, resource_id, action, now)
    }
}

// ---------------------------------------------------------------------------
// Error
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PermissionError {
    #[error("grant not found: {0}")]
    GrantNotFound(String),
    #[error("access denied: {user} cannot {action} {resource_type}:{resource_id}")]
    AccessDenied {
        user: String,
        action: String,
        resource_type: String,
        resource_id: String,
    },
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

    fn user_a() -> EnterpriseUserId {
        EnterpriseUserId::from("user-001")
    }

    fn make_grant(grant_id: &str, revoked: bool) -> PermissionGrant {
        PermissionGrant {
            grant_id: grant_id.to_string(),
            user_id: user_a(),
            role: EnterpriseRole::User,
            resource_type: ResourceType::KnowledgeBase,
            resource_id: ResourceId::from("kb-main"),
            actions: vec![PermissionAction::Read],
            granted_at: ts(),
            expires_at: None,
            revoked,
        }
    }

    // ---- Type roundtrips ----

    #[test]
    fn enterprise_user_id_roundtrips() {
        let id = EnterpriseUserId::from("user-001");
        let json = serde_json::to_string(&id).unwrap();
        let decoded: EnterpriseUserId = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded, id);
        assert_eq!(id.to_string(), "user-001");
    }

    #[test]
    fn enterprise_role_serde() {
        assert_eq!(
            serde_json::to_string(&EnterpriseRole::SuperAdmin).unwrap(),
            "\"super_admin\""
        );
        let decoded: EnterpriseRole = serde_json::from_str("\"admin\"").unwrap();
        assert_eq!(decoded, EnterpriseRole::Admin);
    }

    #[test]
    fn enterprise_role_ordering() {
        assert!(EnterpriseRole::SuperAdmin > EnterpriseRole::Admin);
        assert!(EnterpriseRole::Admin > EnterpriseRole::User);
    }

    #[test]
    fn resource_type_serde() {
        assert_eq!(
            serde_json::to_string(&ResourceType::KnowledgeBase).unwrap(),
            "\"knowledge_base\""
        );
    }

    #[test]
    fn permission_action_serde() {
        assert_eq!(
            serde_json::to_string(&PermissionAction::Read).unwrap(),
            "\"read\""
        );
    }

    // ---- PermissionGrant ----

    #[test]
    fn grant_roundtrips() {
        let grant = make_grant("grant-1", false);
        let json = serde_json::to_string_pretty(&grant).unwrap();
        let decoded: PermissionGrant = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded.grant_id, "grant-1");
        assert!(!decoded.revoked);
    }

    #[test]
    fn grant_active_when_not_revoked() {
        let grant = make_grant("grant-1", false);
        assert!(grant.is_active(ts()));
    }

    #[test]
    fn grant_inactive_when_revoked() {
        let grant = make_grant("grant-1", true);
        assert!(!grant.is_active(ts()));
    }

    #[test]
    fn grant_inactive_when_expired() {
        let mut grant = make_grant("grant-1", false);
        grant.expires_at = Some(ts() - chrono::Duration::hours(1));
        assert!(!grant.is_active(ts()));
    }

    #[test]
    fn grant_active_before_expiry() {
        let mut grant = make_grant("grant-1", false);
        grant.expires_at = Some(ts() + chrono::Duration::hours(1));
        assert!(grant.is_active(ts()));
    }

    #[test]
    fn grant_allows_matching_action() {
        let grant = make_grant("grant-1", false);
        assert!(grant.allows(&PermissionAction::Read));
        assert!(!grant.allows(&PermissionAction::Write));
    }

    // ---- PermissionDecision ----

    #[test]
    fn decision_display() {
        assert_eq!(PermissionDecision::Allow.to_string(), "allow");
        assert_eq!(PermissionDecision::Deny.to_string(), "deny");
    }

    #[test]
    fn decision_is_allowed() {
        assert!(PermissionDecision::Allow.is_allowed());
        assert!(!PermissionDecision::Deny.is_allowed());
    }

    // ---- EnterpriseAssetPolicy ----

    #[test]
    fn policy_deny_by_default() {
        let policy = EnterpriseAssetPolicy::deny_by_default(
            ResourceType::KnowledgeBase,
            ResourceId::from("kb-1"),
        );
        assert_eq!(policy.default_decision, PermissionDecision::Deny);
        assert!(policy.require_explicit_grant);
    }

    #[test]
    fn policy_allow_by_default() {
        let policy = EnterpriseAssetPolicy::allow_by_default(
            ResourceType::KnowledgeBase,
            ResourceId::from("kb-public"),
        );
        assert_eq!(policy.default_decision, PermissionDecision::Allow);
        assert!(!policy.require_explicit_grant);
    }

    // ---- PermissionStore ----

    #[test]
    fn store_deny_by_default() {
        let store = PermissionStore::new();
        let decision = store.check(
            &user_a(),
            &ResourceType::KnowledgeBase,
            &ResourceId::from("kb-1"),
            &PermissionAction::Read,
            ts(),
        );
        assert_eq!(decision, PermissionDecision::Deny);
    }

    #[test]
    fn store_allow_with_grant() {
        let mut store = PermissionStore::new();
        store.add_grant(make_grant("grant-1", false));

        let decision = store.check(
            &user_a(),
            &ResourceType::KnowledgeBase,
            &ResourceId::from("kb-main"),
            &PermissionAction::Read,
            ts(),
        );
        assert_eq!(decision, PermissionDecision::Allow);
    }

    #[test]
    fn store_deny_revoked_grant() {
        let mut store = PermissionStore::new();
        store.add_grant(make_grant("grant-1", true));

        let decision = store.check(
            &user_a(),
            &ResourceType::KnowledgeBase,
            &ResourceId::from("kb-main"),
            &PermissionAction::Read,
            ts(),
        );
        assert_eq!(decision, PermissionDecision::Deny);
    }

    #[test]
    fn store_revoke_grant() {
        let mut store = PermissionStore::new();
        store.add_grant(make_grant("grant-1", false));

        assert!(store.revoke_grant("grant-1"));

        let decision = store.check(
            &user_a(),
            &ResourceType::KnowledgeBase,
            &ResourceId::from("kb-main"),
            &PermissionAction::Read,
            ts(),
        );
        assert_eq!(decision, PermissionDecision::Deny);
    }

    #[test]
    fn store_revoke_nonexistent_grant() {
        let mut store = PermissionStore::new();
        assert!(!store.revoke_grant("nonexistent"));
    }

    #[test]
    fn store_deny_wrong_resource() {
        let mut store = PermissionStore::new();
        store.add_grant(make_grant("grant-1", false));

        let decision = store.check(
            &user_a(),
            &ResourceType::KnowledgeBase,
            &ResourceId::from("kb-other"),
            &PermissionAction::Read,
            ts(),
        );
        assert_eq!(decision, PermissionDecision::Deny);
    }

    #[test]
    fn store_deny_wrong_action() {
        let mut store = PermissionStore::new();
        store.add_grant(make_grant("grant-1", false));

        let decision = store.check(
            &user_a(),
            &ResourceType::KnowledgeBase,
            &ResourceId::from("kb-main"),
            &PermissionAction::Write,
            ts(),
        );
        assert_eq!(decision, PermissionDecision::Deny);
    }

    // ---- Role-based checks ----

    #[test]
    fn superadmin_bypasses_all_checks() {
        let store = PermissionStore::new();
        let decision = store.check_with_role(
            &user_a(),
            EnterpriseRole::SuperAdmin,
            &ResourceType::KnowledgeBase,
            &ResourceId::from("kb-secret"),
            &PermissionAction::Admin,
            ts(),
        );
        assert_eq!(decision, PermissionDecision::Allow);
    }

    #[test]
    fn admin_can_read_anything() {
        let store = PermissionStore::new();
        let decision = store.check_with_role(
            &user_a(),
            EnterpriseRole::Admin,
            &ResourceType::KnowledgeBase,
            &ResourceId::from("kb-secret"),
            &PermissionAction::Read,
            ts(),
        );
        assert_eq!(decision, PermissionDecision::Allow);
    }

    #[test]
    fn admin_cannot_write_without_grant() {
        let store = PermissionStore::new();
        let decision = store.check_with_role(
            &user_a(),
            EnterpriseRole::Admin,
            &ResourceType::KnowledgeBase,
            &ResourceId::from("kb-secret"),
            &PermissionAction::Write,
            ts(),
        );
        assert_eq!(decision, PermissionDecision::Deny);
    }

    #[test]
    fn user_needs_grant() {
        let store = PermissionStore::new();
        let decision = store.check_with_role(
            &user_a(),
            EnterpriseRole::User,
            &ResourceType::KnowledgeBase,
            &ResourceId::from("kb-1"),
            &PermissionAction::Read,
            ts(),
        );
        assert_eq!(decision, PermissionDecision::Deny);
    }
}
