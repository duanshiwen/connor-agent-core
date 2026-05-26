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
    Conversation,
    Action,
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
            Self::Conversation => write!(f, "conversation"),
            Self::Action => write!(f, "action"),
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

    /// Get all grants for a user.
    pub fn get_grants_for_user(&self, user_id: &EnterpriseUserId) -> Vec<&PermissionGrant> {
        self.grants
            .iter()
            .filter(|g| g.user_id == *user_id)
            .collect()
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

// ===========================================================================
// Organization/Team/Group Permission Model
// ===========================================================================

/// Unique identifier for an organization.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct OrganizationId(pub String);

impl fmt::Display for OrganizationId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl From<String> for OrganizationId {
    fn from(s: String) -> Self {
        Self(s)
    }
}

impl From<&str> for OrganizationId {
    fn from(s: &str) -> Self {
        Self(s.to_string())
    }
}

/// Unique identifier for a team.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TeamId(pub String);

impl fmt::Display for TeamId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl From<String> for TeamId {
    fn from(s: String) -> Self {
        Self(s)
    }
}

impl From<&str> for TeamId {
    fn from(s: &str) -> Self {
        Self(s.to_string())
    }
}

/// Unique identifier for a group.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct GroupId(pub String);

impl fmt::Display for GroupId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl From<String> for GroupId {
    fn from(s: String) -> Self {
        Self(s)
    }
}

impl From<&str> for GroupId {
    fn from(s: &str) -> Self {
        Self(s.to_string())
    }
}

/// Membership type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum MembershipType {
    /// Member of an organization.
    Organization,
    /// Member of a team.
    Team,
    /// Member of a group.
    Group,
}

impl fmt::Display for MembershipType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            MembershipType::Organization => write!(f, "organization"),
            MembershipType::Team => write!(f, "team"),
            MembershipType::Group => write!(f, "group"),
        }
    }
}

/// A membership record.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Membership {
    pub membership_id: String,
    pub user_id: EnterpriseUserId,
    pub membership_type: MembershipType,
    pub org_id: Option<OrganizationId>,
    pub team_id: Option<TeamId>,
    pub group_id: Option<GroupId>,
    pub role: EnterpriseRole,
    pub joined_at: DateTime<Utc>,
    pub expires_at: Option<DateTime<Utc>>,
}

impl Membership {
    /// Check if membership is active.
    pub fn is_active(&self, now: DateTime<Utc>) -> bool {
        if let Some(expires_at) = self.expires_at {
            now < expires_at
        } else {
            true
        }
    }
}

/// Inherited permission grant.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct InheritedGrant {
    pub grant: PermissionGrant,
    pub inherited_from: InheritanceSource,
}

/// Source of inherited permission.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum InheritanceSource {
    /// Direct grant to user.
    Direct,
    /// Inherited from organization.
    Organization(OrganizationId),
    /// Inherited from team.
    Team(TeamId),
    /// Inherited from group.
    Group(GroupId),
}

impl fmt::Display for InheritanceSource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            InheritanceSource::Direct => write!(f, "direct"),
            InheritanceSource::Organization(id) => write!(f, "org:{}", id),
            InheritanceSource::Team(id) => write!(f, "team:{}", id),
            InheritanceSource::Group(id) => write!(f, "group:{}", id),
        }
    }
}

/// Extended permission store with org/team/group support.
pub struct OrganizationalPermissionStore {
    inner: PermissionStore,
    memberships: Vec<Membership>,
    org_grants: Vec<(OrganizationId, PermissionGrant)>,
    team_grants: Vec<(TeamId, PermissionGrant)>,
    group_grants: Vec<(GroupId, PermissionGrant)>,
}

impl OrganizationalPermissionStore {
    pub fn new() -> Self {
        Self {
            inner: PermissionStore::new(),
            memberships: Vec::new(),
            org_grants: Vec::new(),
            team_grants: Vec::new(),
            group_grants: Vec::new(),
        }
    }

    /// Add a membership.
    pub fn add_membership(&mut self, membership: Membership) {
        self.memberships.push(membership);
    }

    /// Add a grant to an organization.
    pub fn add_org_grant(&mut self, org_id: OrganizationId, grant: PermissionGrant) {
        self.org_grants.push((org_id, grant));
    }

    /// Add a grant to a team.
    pub fn add_team_grant(&mut self, team_id: TeamId, grant: PermissionGrant) {
        self.team_grants.push((team_id, grant));
    }

    /// Add a grant to a group.
    pub fn add_group_grant(&mut self, group_id: GroupId, grant: PermissionGrant) {
        self.group_grants.push((group_id, grant));
    }

    /// Add a direct grant to a user.
    pub fn add_direct_grant(&mut self, grant: PermissionGrant) {
        self.inner.add_grant(grant);
    }

    /// Get all inherited grants for a user.
    pub fn get_inherited_grants(
        &self,
        user_id: &EnterpriseUserId,
        now: DateTime<Utc>,
    ) -> Vec<InheritedGrant> {
        let mut result = Vec::new();

        // Direct grants
        for grant in self.inner.get_grants_for_user(user_id) {
            if grant.is_active(now) {
                result.push(InheritedGrant {
                    grant: grant.clone(),
                    inherited_from: InheritanceSource::Direct,
                });
            }
        }

        // Organization grants
        for membership in &self.memberships {
            if membership.user_id == *user_id
                && membership.membership_type == MembershipType::Organization
                && let Some(org_id) = &membership.org_id
                && membership.is_active(now)
            {
                for (grant_org_id, grant) in &self.org_grants {
                    if grant_org_id == org_id && grant.is_active(now) {
                        result.push(InheritedGrant {
                            grant: grant.clone(),
                            inherited_from: InheritanceSource::Organization(org_id.clone()),
                        });
                    }
                }
            }
        }

        // Team grants
        for membership in &self.memberships {
            if membership.user_id == *user_id
                && membership.membership_type == MembershipType::Team
                && let Some(team_id) = &membership.team_id
                && membership.is_active(now)
            {
                for (grant_team_id, grant) in &self.team_grants {
                    if grant_team_id == team_id && grant.is_active(now) {
                        result.push(InheritedGrant {
                            grant: grant.clone(),
                            inherited_from: InheritanceSource::Team(team_id.clone()),
                        });
                    }
                }
            }
        }

        // Group grants
        for membership in &self.memberships {
            if membership.user_id == *user_id
                && membership.membership_type == MembershipType::Group
                && let Some(group_id) = &membership.group_id
                && membership.is_active(now)
            {
                for (grant_group_id, grant) in &self.group_grants {
                    if grant_group_id == group_id && grant.is_active(now) {
                        result.push(InheritedGrant {
                            grant: grant.clone(),
                            inherited_from: InheritanceSource::Group(group_id.clone()),
                        });
                    }
                }
            }
        }

        result
    }

    /// Check permission with inheritance.
    pub fn check_with_inheritance(
        &self,
        user_id: &EnterpriseUserId,
        resource_type: &ResourceType,
        resource_id: &ResourceId,
        action: &PermissionAction,
        now: DateTime<Utc>,
    ) -> PermissionDecision {
        let inherited_grants = self.get_inherited_grants(user_id, now);

        for inherited in &inherited_grants {
            if inherited.grant.resource_type == *resource_type
                && inherited.grant.resource_id == *resource_id
                && inherited.grant.allows(action)
            {
                return PermissionDecision::Allow;
            }
        }

        PermissionDecision::Deny
    }
}

impl Default for OrganizationalPermissionStore {
    fn default() -> Self {
        Self::new()
    }
}

// ===========================================================================
// Server-backed Permission Store Boundary
// ===========================================================================

/// Permission data fetched from an enterprise permission server.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ServerPermissionSnapshot {
    pub snapshot_id: String,
    pub fetched_at: DateTime<Utc>,
    pub direct_grants: Vec<PermissionGrant>,
    pub memberships: Vec<Membership>,
    pub org_grants: Vec<(OrganizationId, PermissionGrant)>,
    pub team_grants: Vec<(TeamId, PermissionGrant)>,
    pub group_grants: Vec<(GroupId, PermissionGrant)>,
}

impl ServerPermissionSnapshot {
    fn into_store(self) -> OrganizationalPermissionStore {
        let mut store = OrganizationalPermissionStore::new();

        for grant in self.direct_grants {
            store.add_direct_grant(grant);
        }
        for membership in self.memberships {
            store.add_membership(membership);
        }
        for (org_id, grant) in self.org_grants {
            store.add_org_grant(org_id, grant);
        }
        for (team_id, grant) in self.team_grants {
            store.add_team_grant(team_id, grant);
        }
        for (group_id, grant) in self.group_grants {
            store.add_group_grant(group_id, grant);
        }

        store
    }
}

/// Freshness state for a server-backed permission cache.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CacheStatus {
    Fresh,
    Stale,
    Expired,
}

impl fmt::Display for CacheStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Fresh => write!(f, "fresh"),
            Self::Stale => write!(f, "stale"),
            Self::Expired => write!(f, "expired"),
        }
    }
}

/// Cache freshness policy for server-backed permissions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CachePolicy {
    pub fresh_for: chrono::Duration,
    pub stale_for: chrono::Duration,
}

impl Default for CachePolicy {
    fn default() -> Self {
        Self {
            fresh_for: chrono::Duration::minutes(5),
            stale_for: chrono::Duration::minutes(30),
        }
    }
}

/// Result of refreshing a permission cache from a remote provider.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CacheRefreshReport {
    pub status: CacheStatus,
    pub snapshot_id: Option<String>,
    pub explanation: String,
}

/// Permission decision plus cache freshness explanation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CachedPermissionDecision {
    pub decision: PermissionDecision,
    pub cache_status: CacheStatus,
    pub explanation: String,
}

/// Boundary for server-backed enterprise permission providers.
pub trait RemotePermissionProvider: Clone {
    fn fetch_snapshot(
        &self,
        now: DateTime<Utc>,
    ) -> Result<ServerPermissionSnapshot, PermissionProviderError>;
}

/// Error returned by a remote permission provider.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PermissionProviderError {
    #[error("provider unavailable: {0}")]
    Unavailable(String),
    #[error("provider returned invalid snapshot: {0}")]
    InvalidSnapshot(String),
}

/// In-memory remote permission provider used by tests and host fakes.
#[derive(Debug, Clone)]
pub struct MemoryRemotePermissionProvider {
    snapshot: ServerPermissionSnapshot,
    available: bool,
}

impl MemoryRemotePermissionProvider {
    pub fn with_snapshot(snapshot: ServerPermissionSnapshot) -> Self {
        Self {
            snapshot,
            available: true,
        }
    }

    pub fn set_snapshot(&mut self, snapshot: ServerPermissionSnapshot) {
        self.snapshot = snapshot;
    }

    pub fn set_available(&mut self, available: bool) {
        self.available = available;
    }
}

impl RemotePermissionProvider for MemoryRemotePermissionProvider {
    fn fetch_snapshot(
        &self,
        _now: DateTime<Utc>,
    ) -> Result<ServerPermissionSnapshot, PermissionProviderError> {
        if self.available {
            Ok(self.snapshot.clone())
        } else {
            Err(PermissionProviderError::Unavailable(
                "provider unavailable".to_string(),
            ))
        }
    }
}

/// Server-backed permission store with a local cache and explicit offline behavior.
pub struct ServerBackedPermissionStore<P: RemotePermissionProvider> {
    provider: P,
    policy: CachePolicy,
    cached_snapshot_id: Option<String>,
    cached_fetched_at: Option<DateTime<Utc>>,
    cached_store: Option<OrganizationalPermissionStore>,
    last_status: CacheStatus,
    last_explanation: String,
}

impl<P: RemotePermissionProvider> ServerBackedPermissionStore<P> {
    pub fn new(provider: P, policy: CachePolicy) -> Self {
        Self {
            provider,
            policy,
            cached_snapshot_id: None,
            cached_fetched_at: None,
            cached_store: None,
            last_status: CacheStatus::Expired,
            last_explanation: "cache not populated".to_string(),
        }
    }

    pub fn set_provider(&mut self, provider: P) {
        self.provider = provider;
    }

    pub fn cached_snapshot_id(&self) -> Option<&str> {
        self.cached_snapshot_id.as_deref()
    }

    pub fn refresh(&mut self, now: DateTime<Utc>) -> Result<CacheRefreshReport, PermissionError> {
        match self.provider.fetch_snapshot(now) {
            Ok(snapshot) => {
                let snapshot_id = snapshot.snapshot_id.clone();
                let fetched_at = snapshot.fetched_at;
                self.cached_store = Some(snapshot.into_store());
                self.cached_snapshot_id = Some(snapshot_id.clone());
                self.cached_fetched_at = Some(fetched_at);
                self.last_status = self.cache_status_at(now);
                self.last_explanation = format!(
                    "permission snapshot {} refreshed from provider",
                    snapshot_id
                );
                Ok(CacheRefreshReport {
                    status: self.last_status,
                    snapshot_id: Some(snapshot_id),
                    explanation: self.last_explanation.clone(),
                })
            }
            Err(error) => {
                self.last_status = self.cache_status_at(now);
                self.last_explanation = match self.last_status {
                    CacheStatus::Fresh | CacheStatus::Stale => {
                        format!(
                            "provider unavailable; using {} permission cache: {}",
                            self.last_status, error
                        )
                    }
                    CacheStatus::Expired => {
                        format!("provider unavailable; permission cache expired: {}", error)
                    }
                };
                Ok(CacheRefreshReport {
                    status: self.last_status,
                    snapshot_id: self.cached_snapshot_id.clone(),
                    explanation: self.last_explanation.clone(),
                })
            }
        }
    }

    pub fn check_with_inheritance(
        &self,
        user_id: &EnterpriseUserId,
        resource_type: &ResourceType,
        resource_id: &ResourceId,
        action: &PermissionAction,
        now: DateTime<Utc>,
    ) -> CachedPermissionDecision {
        let status = self.cache_status_at(now);

        if status == CacheStatus::Expired {
            return CachedPermissionDecision {
                decision: PermissionDecision::Deny,
                cache_status: status,
                explanation: "permission cache expired; deny by default".to_string(),
            };
        }

        let decision = self
            .cached_store
            .as_ref()
            .map(|store| {
                store.check_with_inheritance(user_id, resource_type, resource_id, action, now)
            })
            .unwrap_or(PermissionDecision::Deny);

        CachedPermissionDecision {
            decision,
            cache_status: status,
            explanation: self.last_explanation.clone(),
        }
    }

    fn cache_status_at(&self, now: DateTime<Utc>) -> CacheStatus {
        let Some(fetched_at) = self.cached_fetched_at else {
            return CacheStatus::Expired;
        };
        let age = now - fetched_at;
        if age <= self.policy.fresh_for {
            CacheStatus::Fresh
        } else if age <= self.policy.fresh_for + self.policy.stale_for {
            CacheStatus::Stale
        } else {
            CacheStatus::Expired
        }
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

    // -----------------------------------------------------------------------
    // PR 155: Org/team/group permission model
    // -----------------------------------------------------------------------

    #[test]
    fn organization_id_display() {
        let id = OrganizationId::from("org-1");
        assert_eq!(id.to_string(), "org-1");
    }

    #[test]
    fn team_id_display() {
        let id = TeamId::from("team-1");
        assert_eq!(id.to_string(), "team-1");
    }

    #[test]
    fn group_id_display() {
        let id = GroupId::from("group-1");
        assert_eq!(id.to_string(), "group-1");
    }

    #[test]
    fn membership_type_display() {
        assert_eq!(MembershipType::Organization.to_string(), "organization");
        assert_eq!(MembershipType::Team.to_string(), "team");
        assert_eq!(MembershipType::Group.to_string(), "group");
    }

    #[test]
    fn membership_is_active() {
        let membership = Membership {
            membership_id: "m-1".to_string(),
            user_id: EnterpriseUserId::from("user-1"),
            membership_type: MembershipType::Organization,
            org_id: Some(OrganizationId::from("org-1")),
            team_id: None,
            group_id: None,
            role: EnterpriseRole::User,
            joined_at: ts(),
            expires_at: None,
        };

        assert!(membership.is_active(ts()));
    }

    #[test]
    fn membership_is_expired() {
        let membership = Membership {
            membership_id: "m-1".to_string(),
            user_id: EnterpriseUserId::from("user-1"),
            membership_type: MembershipType::Organization,
            org_id: Some(OrganizationId::from("org-1")),
            team_id: None,
            group_id: None,
            role: EnterpriseRole::User,
            joined_at: ts(),
            expires_at: Some(ts() - chrono::Duration::hours(1)),
        };

        assert!(!membership.is_active(ts()));
    }

    #[test]
    fn inheritance_source_display() {
        assert_eq!(InheritanceSource::Direct.to_string(), "direct");
        assert_eq!(
            InheritanceSource::Organization(OrganizationId::from("org-1")).to_string(),
            "org:org-1"
        );
        assert_eq!(
            InheritanceSource::Team(TeamId::from("team-1")).to_string(),
            "team:team-1"
        );
        assert_eq!(
            InheritanceSource::Group(GroupId::from("group-1")).to_string(),
            "group:group-1"
        );
    }

    #[test]
    fn org_permission_inheritance() {
        let mut store = OrganizationalPermissionStore::new();

        // Add user to organization
        store.add_membership(Membership {
            membership_id: "m-1".to_string(),
            user_id: EnterpriseUserId::from("user-1"),
            membership_type: MembershipType::Organization,
            org_id: Some(OrganizationId::from("org-1")),
            team_id: None,
            group_id: None,
            role: EnterpriseRole::User,
            joined_at: ts(),
            expires_at: None,
        });

        // Add grant to organization
        store.add_org_grant(
            OrganizationId::from("org-1"),
            PermissionGrant {
                grant_id: "g-1".to_string(),
                user_id: EnterpriseUserId::from("user-1"),
                role: EnterpriseRole::User,
                resource_type: ResourceType::KnowledgeBase,
                resource_id: ResourceId::from("kb-1"),
                actions: vec![PermissionAction::Read],
                granted_at: ts(),
                expires_at: None,
                revoked: false,
            },
        );

        // Check inherited permission
        let inherited = store.get_inherited_grants(&EnterpriseUserId::from("user-1"), ts());
        assert_eq!(inherited.len(), 1);
        assert_eq!(
            inherited[0].inherited_from,
            InheritanceSource::Organization(OrganizationId::from("org-1"))
        );
    }

    #[test]
    fn team_permission_inheritance() {
        let mut store = OrganizationalPermissionStore::new();

        // Add user to team
        store.add_membership(Membership {
            membership_id: "m-1".to_string(),
            user_id: EnterpriseUserId::from("user-1"),
            membership_type: MembershipType::Team,
            org_id: None,
            team_id: Some(TeamId::from("team-1")),
            group_id: None,
            role: EnterpriseRole::User,
            joined_at: ts(),
            expires_at: None,
        });

        // Add grant to team
        store.add_team_grant(
            TeamId::from("team-1"),
            PermissionGrant {
                grant_id: "g-1".to_string(),
                user_id: EnterpriseUserId::from("user-1"),
                role: EnterpriseRole::User,
                resource_type: ResourceType::KnowledgeBase,
                resource_id: ResourceId::from("kb-1"),
                actions: vec![PermissionAction::Read],
                granted_at: ts(),
                expires_at: None,
                revoked: false,
            },
        );

        // Check inherited permission
        let inherited = store.get_inherited_grants(&EnterpriseUserId::from("user-1"), ts());
        assert_eq!(inherited.len(), 1);
        assert_eq!(
            inherited[0].inherited_from,
            InheritanceSource::Team(TeamId::from("team-1"))
        );
    }

    #[test]
    fn group_permission_inheritance() {
        let mut store = OrganizationalPermissionStore::new();

        // Add user to group
        store.add_membership(Membership {
            membership_id: "m-1".to_string(),
            user_id: EnterpriseUserId::from("user-1"),
            membership_type: MembershipType::Group,
            org_id: None,
            team_id: None,
            group_id: Some(GroupId::from("group-1")),
            role: EnterpriseRole::User,
            joined_at: ts(),
            expires_at: None,
        });

        // Add grant to group
        store.add_group_grant(
            GroupId::from("group-1"),
            PermissionGrant {
                grant_id: "g-1".to_string(),
                user_id: EnterpriseUserId::from("user-1"),
                role: EnterpriseRole::User,
                resource_type: ResourceType::KnowledgeBase,
                resource_id: ResourceId::from("kb-1"),
                actions: vec![PermissionAction::Read],
                granted_at: ts(),
                expires_at: None,
                revoked: false,
            },
        );

        // Check inherited permission
        let inherited = store.get_inherited_grants(&EnterpriseUserId::from("user-1"), ts());
        assert_eq!(inherited.len(), 1);
        assert_eq!(
            inherited[0].inherited_from,
            InheritanceSource::Group(GroupId::from("group-1"))
        );
    }

    #[test]
    fn check_with_inheritance() {
        let mut store = OrganizationalPermissionStore::new();

        // Add user to organization
        store.add_membership(Membership {
            membership_id: "m-1".to_string(),
            user_id: EnterpriseUserId::from("user-1"),
            membership_type: MembershipType::Organization,
            org_id: Some(OrganizationId::from("org-1")),
            team_id: None,
            group_id: None,
            role: EnterpriseRole::User,
            joined_at: ts(),
            expires_at: None,
        });

        // Add grant to organization
        store.add_org_grant(
            OrganizationId::from("org-1"),
            PermissionGrant {
                grant_id: "g-1".to_string(),
                user_id: EnterpriseUserId::from("user-1"),
                role: EnterpriseRole::User,
                resource_type: ResourceType::KnowledgeBase,
                resource_id: ResourceId::from("kb-1"),
                actions: vec![PermissionAction::Read],
                granted_at: ts(),
                expires_at: None,
                revoked: false,
            },
        );

        // Check permission with inheritance
        let decision = store.check_with_inheritance(
            &EnterpriseUserId::from("user-1"),
            &ResourceType::KnowledgeBase,
            &ResourceId::from("kb-1"),
            &PermissionAction::Read,
            ts(),
        );

        assert_eq!(decision, PermissionDecision::Allow);
    }

    #[test]
    fn check_with_inheritance_denied() {
        let mut store = OrganizationalPermissionStore::new();

        // Add user to organization
        store.add_membership(Membership {
            membership_id: "m-1".to_string(),
            user_id: EnterpriseUserId::from("user-1"),
            membership_type: MembershipType::Organization,
            org_id: Some(OrganizationId::from("org-1")),
            team_id: None,
            group_id: None,
            role: EnterpriseRole::User,
            joined_at: ts(),
            expires_at: None,
        });

        // Add grant to organization (but not for the resource we'll check)
        store.add_org_grant(
            OrganizationId::from("org-1"),
            PermissionGrant {
                grant_id: "g-1".to_string(),
                user_id: EnterpriseUserId::from("user-1"),
                role: EnterpriseRole::User,
                resource_type: ResourceType::KnowledgeBase,
                resource_id: ResourceId::from("kb-1"),
                actions: vec![PermissionAction::Read],
                granted_at: ts(),
                expires_at: None,
                revoked: false,
            },
        );

        // Check permission for different resource
        let decision = store.check_with_inheritance(
            &EnterpriseUserId::from("user-1"),
            &ResourceType::KnowledgeBase,
            &ResourceId::from("kb-2"),
            &PermissionAction::Read,
            ts(),
        );

        assert_eq!(decision, PermissionDecision::Deny);
    }

    #[test]
    fn server_backed_store_refreshes_remote_snapshot_for_inherited_check() {
        let now = ts();
        let snapshot = ServerPermissionSnapshot {
            snapshot_id: "snap-1".to_string(),
            fetched_at: now,
            direct_grants: vec![],
            memberships: vec![Membership {
                membership_id: "m-1".to_string(),
                user_id: EnterpriseUserId::from("user-1"),
                membership_type: MembershipType::Organization,
                org_id: Some(OrganizationId::from("org-1")),
                team_id: None,
                group_id: None,
                role: EnterpriseRole::User,
                joined_at: now,
                expires_at: None,
            }],
            org_grants: vec![(
                OrganizationId::from("org-1"),
                PermissionGrant {
                    grant_id: "g-1".to_string(),
                    user_id: EnterpriseUserId::from("user-1"),
                    role: EnterpriseRole::User,
                    resource_type: ResourceType::KnowledgeBase,
                    resource_id: ResourceId::from("kb-1"),
                    actions: vec![PermissionAction::Read],
                    granted_at: now,
                    expires_at: None,
                    revoked: false,
                },
            )],
            team_grants: vec![],
            group_grants: vec![],
        };
        let provider = MemoryRemotePermissionProvider::with_snapshot(snapshot);
        let mut store = ServerBackedPermissionStore::new(provider, CachePolicy::default());

        let refresh = store.refresh(now).unwrap();
        assert_eq!(refresh.status, CacheStatus::Fresh);

        let decision = store.check_with_inheritance(
            &EnterpriseUserId::from("user-1"),
            &ResourceType::KnowledgeBase,
            &ResourceId::from("kb-1"),
            &PermissionAction::Read,
            now,
        );

        assert_eq!(decision.decision, PermissionDecision::Allow);
        assert_eq!(decision.cache_status, CacheStatus::Fresh);
    }

    #[test]
    fn server_backed_store_uses_stale_cache_when_provider_unavailable() {
        let now = ts();
        let stale_time = now + chrono::Duration::minutes(10);
        let mut provider =
            MemoryRemotePermissionProvider::with_snapshot(ServerPermissionSnapshot {
                snapshot_id: "snap-1".to_string(),
                fetched_at: now,
                direct_grants: vec![make_grant("g-1", false)],
                memberships: vec![],
                org_grants: vec![],
                team_grants: vec![],
                group_grants: vec![],
            });
        let policy = CachePolicy {
            fresh_for: chrono::Duration::minutes(5),
            stale_for: chrono::Duration::minutes(30),
        };
        let mut store = ServerBackedPermissionStore::new(provider.clone(), policy);
        store.refresh(now).unwrap();

        provider.set_available(false);
        store.set_provider(provider);
        let refresh = store.refresh(stale_time).unwrap();
        assert_eq!(refresh.status, CacheStatus::Stale);
        assert!(refresh.explanation.contains("provider unavailable"));

        let decision = store.check_with_inheritance(
            &user_a(),
            &ResourceType::KnowledgeBase,
            &ResourceId::from("kb-main"),
            &PermissionAction::Read,
            stale_time,
        );

        assert_eq!(decision.decision, PermissionDecision::Allow);
        assert_eq!(decision.cache_status, CacheStatus::Stale);
    }

    #[test]
    fn server_backed_store_denies_when_cache_expired_and_provider_unavailable() {
        let now = ts();
        let expired_time = now + chrono::Duration::minutes(40);
        let mut provider =
            MemoryRemotePermissionProvider::with_snapshot(ServerPermissionSnapshot {
                snapshot_id: "snap-1".to_string(),
                fetched_at: now,
                direct_grants: vec![make_grant("g-1", false)],
                memberships: vec![],
                org_grants: vec![],
                team_grants: vec![],
                group_grants: vec![],
            });
        let policy = CachePolicy {
            fresh_for: chrono::Duration::minutes(5),
            stale_for: chrono::Duration::minutes(30),
        };
        let mut store = ServerBackedPermissionStore::new(provider.clone(), policy);
        store.refresh(now).unwrap();

        provider.set_available(false);
        store.set_provider(provider);
        let refresh = store.refresh(expired_time).unwrap();
        assert_eq!(refresh.status, CacheStatus::Expired);

        let decision = store.check_with_inheritance(
            &user_a(),
            &ResourceType::KnowledgeBase,
            &ResourceId::from("kb-main"),
            &PermissionAction::Read,
            expired_time,
        );

        assert_eq!(decision.decision, PermissionDecision::Deny);
        assert_eq!(decision.cache_status, CacheStatus::Expired);
    }

    #[test]
    fn server_backed_store_refresh_overwrites_revoked_cached_grant() {
        let now = ts();
        let mut provider =
            MemoryRemotePermissionProvider::with_snapshot(ServerPermissionSnapshot {
                snapshot_id: "snap-1".to_string(),
                fetched_at: now,
                direct_grants: vec![make_grant("g-1", false)],
                memberships: vec![],
                org_grants: vec![],
                team_grants: vec![],
                group_grants: vec![],
            });
        let mut store = ServerBackedPermissionStore::new(provider.clone(), CachePolicy::default());
        store.refresh(now).unwrap();

        provider.set_snapshot(ServerPermissionSnapshot {
            snapshot_id: "snap-2".to_string(),
            fetched_at: now + chrono::Duration::minutes(1),
            direct_grants: vec![make_grant("g-1", true)],
            memberships: vec![],
            org_grants: vec![],
            team_grants: vec![],
            group_grants: vec![],
        });
        store.set_provider(provider);
        store.refresh(now + chrono::Duration::minutes(1)).unwrap();

        let decision = store.check_with_inheritance(
            &user_a(),
            &ResourceType::KnowledgeBase,
            &ResourceId::from("kb-main"),
            &PermissionAction::Read,
            now + chrono::Duration::minutes(1),
        );

        assert_eq!(decision.decision, PermissionDecision::Deny);
        assert_eq!(store.cached_snapshot_id(), Some("snap-2"));
    }
}
