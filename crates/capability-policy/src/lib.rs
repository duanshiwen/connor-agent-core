//! # Capability Policy
//!
//! Evaluates whether an action is allowed, denied, or requires human approval.
//!
//! ## Decision Flow
//!
//! ```text
//! ActionRequest
//!   → classify side_effect from ActionRegistry
//!   → CapabilityPolicy::evaluate(request, side_effect)
//!   → PolicyDecision::Allow   → execute immediately
//!   → PolicyDecision::Ask     → emit approval event, wait for user
//!   → PolicyDecision::Deny    → block, audit
//! ```

use action_core::{ActionRequest, SideEffectKind};
use chrono::{DateTime, Utc};
use enterprise_permission_core::{
    EnterpriseRole, EnterpriseUserId, PermissionAction, PermissionDecision, PermissionStore,
    ResourceId, ResourceType,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::path::Path;
use thiserror::Error;

// ────────────────────────────────────────────────────────────────────────────
// Policy Decision
// ────────────────────────────────────────────────────────────────────────────

/// The result of a policy evaluation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum PolicyDecision {
    /// Action is allowed to execute immediately.
    Allow,
    /// Action requires human approval before execution.
    Ask { reason: String },
    /// Action is denied and must not execute.
    Deny { reason: String },
}

impl PolicyDecision {
    pub fn is_allowed(&self) -> bool {
        matches!(self, PolicyDecision::Allow)
    }

    pub fn is_ask(&self) -> bool {
        matches!(self, PolicyDecision::Ask { .. })
    }

    pub fn is_denied(&self) -> bool {
        matches!(self, PolicyDecision::Deny { .. })
    }
}

// ────────────────────────────────────────────────────────────────────────────
// Policy Explanation
// ────────────────────────────────────────────────────────────────────────────

/// Identifies why a policy decision was selected.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum PolicyMatchedRule {
    /// A concrete side-effect rule matched the action.
    Rule { side_effect: SideEffectKind },
    /// No rule matched; the policy default was used.
    Default,
    /// Enterprise permission context overrode or confirmed the base decision.
    EnterprisePermission {
        resource_type: ResourceType,
        resource_id: ResourceId,
        action: PermissionAction,
        decision: PermissionDecision,
    },
}

/// Structured explanation for a policy evaluation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PolicyExplanation {
    /// Action kind being evaluated.
    pub action_kind: String,
    /// Side-effect classification used for the decision.
    pub side_effect: SideEffectKind,
    /// Rule decision selected by policy evaluation.
    pub decision: PolicyRuleDecision,
    /// The matched rule, or default fallback when no rule matched.
    pub matched_rule: PolicyMatchedRule,
    /// Deterministic risk summary for logs, approval prompts, and diagnostics.
    pub risk_summary: String,
    /// User-facing reason aligned with the returned `PolicyDecision` reason.
    pub user_facing_reason: String,
}

/// Full policy evaluation result containing both decision and explanation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PolicyEvaluation {
    pub decision: PolicyDecision,
    pub explanation: PolicyExplanation,
}

// ────────────────────────────────────────────────────────────────────────────
// Policy Context
// ────────────────────────────────────────────────────────────────────────────

/// Optional runtime context for context-aware policy evaluation.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PolicyEvaluationContext {
    pub actor: Option<PolicyActorContext>,
    pub session: Option<PolicySessionContext>,
    pub server: Option<PolicyServerContext>,
    pub resource: Option<PolicyResourceContext>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PolicyActorContext {
    pub actor_id: String,
    pub enterprise_user_id: Option<EnterpriseUserId>,
    pub enterprise_role: Option<EnterpriseRole>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PolicySessionContext {
    pub conversation_id: Option<String>,
    pub session_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PolicyServerContext {
    pub server_id: Option<String>,
    pub provider: Option<String>,
    pub domain: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PolicyResourceContext {
    pub resource_type: ResourceType,
    pub resource_id: ResourceId,
    pub permission_action: PermissionAction,
}

// ────────────────────────────────────────────────────────────────────────────
// Policy Rule
// ────────────────────────────────────────────────────────────────────────────

/// A single policy rule that maps side effects to decisions.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PolicyRule {
    /// Side effect kind this rule applies to.
    pub side_effect: SideEffectKind,
    /// Decision for this side effect level.
    pub decision: PolicyRuleDecision,
}

/// The decision a policy rule produces.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PolicyRuleDecision {
    Allow,
    Ask,
    Deny,
}

// ────────────────────────────────────────────────────────────────────────────
// Policy File
// ────────────────────────────────────────────────────────────────────────────

pub const CURRENT_POLICY_FILE_VERSION: u32 = 1;

/// A `policy.toml` document for configuring capability policy rules.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct PolicyFileDocument {
    pub version: Option<u32>,
    pub default_decision: PolicyRuleDecision,
    pub side_effect_rules: Vec<PolicyRule>,
    pub action_kind_rules: Vec<ActionKindPolicyRule>,
    pub provider_domain_rules: Vec<ProviderDomainPolicyRule>,
}

impl Default for PolicyFileDocument {
    fn default() -> Self {
        Self {
            version: None,
            default_decision: PolicyRuleDecision::Deny,
            side_effect_rules: Vec::new(),
            action_kind_rules: Vec::new(),
            provider_domain_rules: Vec::new(),
        }
    }
}

impl PolicyFileDocument {
    /// Parse a `policy.toml` document from TOML text.
    pub fn from_toml_str(input: &str) -> Result<Self, PolicyError> {
        toml::from_str(input).map_err(|source| PolicyError::ParseToml {
            reason: source.to_string(),
        })
    }

    /// Parse a `policy.toml` document from disk.
    pub fn from_file(path: impl AsRef<Path>) -> Result<Self, PolicyError> {
        let path = path.as_ref();
        let input = std::fs::read_to_string(path).map_err(|source| PolicyError::ReadFile {
            path: path.display().to_string(),
            reason: source.to_string(),
        })?;
        Self::from_toml_str(&input)
    }

    /// Validate the parsed policy document and return typed diagnostics.
    pub fn validate(&self) -> PolicyValidationReport {
        let mut diagnostics = Vec::new();

        if let Some(version) = self.version {
            if version > CURRENT_POLICY_FILE_VERSION {
                diagnostics.push(PolicyDiagnostic::error(
                    PolicyDiagnosticCode::UnsupportedPolicyVersion,
                    "version",
                    format!(
                        "policy file version {version} is newer than supported version {CURRENT_POLICY_FILE_VERSION}"
                    ),
                ));
            }
        }

        let mut side_effects = BTreeSet::new();
        for rule in &self.side_effect_rules {
            if !side_effects.insert(rule.side_effect.clone()) {
                diagnostics.push(PolicyDiagnostic::error(
                    PolicyDiagnosticCode::DuplicateSideEffectRule,
                    "side_effect_rules",
                    format!("duplicate side effect rule: {:?}", rule.side_effect),
                ));
            }
        }

        let mut action_kinds = BTreeSet::new();
        for rule in &self.action_kind_rules {
            if rule.action_kind.trim().is_empty() {
                diagnostics.push(PolicyDiagnostic::error(
                    PolicyDiagnosticCode::EmptyActionKind,
                    "action_kind_rules.action_kind",
                    "action_kind must not be empty",
                ));
            } else if !action_kinds.insert(rule.action_kind.clone()) {
                diagnostics.push(PolicyDiagnostic::error(
                    PolicyDiagnosticCode::DuplicateActionKindRule,
                    "action_kind_rules.action_kind",
                    format!("duplicate action kind rule: {}", rule.action_kind),
                ));
            }
        }

        let mut provider_domains = BTreeSet::new();
        for rule in &self.provider_domain_rules {
            if rule.provider.trim().is_empty() {
                diagnostics.push(PolicyDiagnostic::error(
                    PolicyDiagnosticCode::EmptyProvider,
                    "provider_domain_rules.provider",
                    "provider must not be empty",
                ));
            }
            if rule.domain.trim().is_empty() {
                diagnostics.push(PolicyDiagnostic::error(
                    PolicyDiagnosticCode::EmptyDomain,
                    "provider_domain_rules.domain",
                    "domain must not be empty",
                ));
            }
            if !rule.provider.trim().is_empty()
                && !rule.domain.trim().is_empty()
                && !provider_domains.insert((rule.provider.clone(), rule.domain.clone()))
            {
                diagnostics.push(PolicyDiagnostic::error(
                    PolicyDiagnosticCode::DuplicateProviderDomainRule,
                    "provider_domain_rules",
                    format!(
                        "duplicate provider/domain rule: {}/{}",
                        rule.provider, rule.domain
                    ),
                ));
            }
        }

        PolicyValidationReport { diagnostics }
    }

    /// Convert a valid policy file document into a capability policy.
    pub fn into_capability_policy(self) -> Result<CapabilityPolicy, PolicyError> {
        let report = self.validate();
        if !report.is_valid() {
            return Err(PolicyError::ValidationFailed { report });
        }
        Ok(CapabilityPolicy::new(
            self.side_effect_rules,
            self.default_decision,
        ))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActionKindPolicyRule {
    pub action_kind: String,
    pub decision: PolicyRuleDecision,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderDomainPolicyRule {
    pub provider: String,
    pub domain: String,
    pub decision: PolicyRuleDecision,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PolicyValidationReport {
    pub diagnostics: Vec<PolicyDiagnostic>,
}

impl PolicyValidationReport {
    pub fn is_valid(&self) -> bool {
        !self
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.severity == PolicyDiagnosticSeverity::Error)
    }

    pub fn has_error(&self, code: PolicyDiagnosticCode) -> bool {
        self.diagnostics.iter().any(|diagnostic| {
            diagnostic.severity == PolicyDiagnosticSeverity::Error && diagnostic.code == code
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PolicyDiagnostic {
    pub severity: PolicyDiagnosticSeverity,
    pub code: PolicyDiagnosticCode,
    pub path: String,
    pub message: String,
}

impl PolicyDiagnostic {
    pub fn error(
        code: PolicyDiagnosticCode,
        path: impl Into<String>,
        message: impl Into<String>,
    ) -> Self {
        Self {
            severity: PolicyDiagnosticSeverity::Error,
            code,
            path: path.into(),
            message: message.into(),
        }
    }

    pub fn warning(
        code: PolicyDiagnosticCode,
        path: impl Into<String>,
        message: impl Into<String>,
    ) -> Self {
        Self {
            severity: PolicyDiagnosticSeverity::Warning,
            code,
            path: path.into(),
            message: message.into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PolicyDiagnosticSeverity {
    Error,
    Warning,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PolicyDiagnosticCode {
    UnsupportedPolicyVersion,
    DuplicateSideEffectRule,
    DuplicateActionKindRule,
    DuplicateProviderDomainRule,
    EmptyActionKind,
    EmptyProvider,
    EmptyDomain,
}

// ────────────────────────────────────────────────────────────────────────────
// Capability Policy
// ────────────────────────────────────────────────────────────────────────────

/// The capability policy engine.
///
/// Evaluates actions against a set of rules to determine if they're allowed.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapabilityPolicy {
    rules: Vec<PolicyRule>,
    /// Default decision when no rule matches.
    default_decision: PolicyRuleDecision,
}

impl CapabilityPolicy {
    /// Create a capability policy from `policy.toml` text.
    pub fn from_policy_toml_str(input: &str) -> Result<Self, PolicyError> {
        PolicyFileDocument::from_toml_str(input)?.into_capability_policy()
    }

    /// Create a capability policy from a `policy.toml` file.
    pub fn from_policy_file(path: impl AsRef<Path>) -> Result<Self, PolicyError> {
        PolicyFileDocument::from_file(path)?.into_capability_policy()
    }

    /// Create a new policy with the given rules.
    pub fn new(rules: Vec<PolicyRule>, default: PolicyRuleDecision) -> Self {
        Self {
            rules,
            default_decision: default,
        }
    }

    /// Create a default safe policy:
    /// - None/ReadOnly → Allow
    /// - RuntimeStateMutation/NetworkAccess/UiSideEffect → Ask
    /// - FileSystem/ExternalSystem/Device/SensitiveProfile → Deny
    pub fn default_safe() -> Self {
        Self::new(
            vec![
                PolicyRule {
                    side_effect: SideEffectKind::None,
                    decision: PolicyRuleDecision::Allow,
                },
                PolicyRule {
                    side_effect: SideEffectKind::ReadOnly,
                    decision: PolicyRuleDecision::Allow,
                },
                PolicyRule {
                    side_effect: SideEffectKind::RuntimeStateMutation,
                    decision: PolicyRuleDecision::Ask,
                },
                PolicyRule {
                    side_effect: SideEffectKind::NetworkAccess,
                    decision: PolicyRuleDecision::Ask,
                },
                PolicyRule {
                    side_effect: SideEffectKind::UiSideEffect,
                    decision: PolicyRuleDecision::Ask,
                },
                PolicyRule {
                    side_effect: SideEffectKind::FileSystemMutation,
                    decision: PolicyRuleDecision::Deny,
                },
                PolicyRule {
                    side_effect: SideEffectKind::ExternalSystemMutation,
                    decision: PolicyRuleDecision::Deny,
                },
                PolicyRule {
                    side_effect: SideEffectKind::DeviceControl,
                    decision: PolicyRuleDecision::Deny,
                },
                PolicyRule {
                    side_effect: SideEffectKind::SensitiveProfileMutation,
                    decision: PolicyRuleDecision::Deny,
                },
            ],
            PolicyRuleDecision::Deny,
        )
    }

    /// Evaluate an action request against the policy.
    pub fn evaluate(
        &self,
        request: &ActionRequest,
        side_effect: &SideEffectKind,
    ) -> PolicyDecision {
        self.evaluate_with_explanation(request, side_effect)
            .decision
    }

    /// Evaluate an action request and return a structured explanation.
    pub fn evaluate_with_explanation(
        &self,
        request: &ActionRequest,
        side_effect: &SideEffectKind,
    ) -> PolicyEvaluation {
        // Find the most specific rule for this side effect kind.
        let matched_rule = self.rules.iter().find(|r| r.side_effect == *side_effect);
        let (rule_decision, matched_rule) = matched_rule
            .map(|rule| {
                (
                    rule.decision.clone(),
                    PolicyMatchedRule::Rule {
                        side_effect: rule.side_effect.clone(),
                    },
                )
            })
            .unwrap_or_else(|| (self.default_decision.clone(), PolicyMatchedRule::Default));

        let user_facing_reason = user_facing_reason(&rule_decision, side_effect);
        let decision = match rule_decision {
            PolicyRuleDecision::Allow => PolicyDecision::Allow,
            PolicyRuleDecision::Ask => PolicyDecision::Ask {
                reason: user_facing_reason.clone(),
            },
            PolicyRuleDecision::Deny => PolicyDecision::Deny {
                reason: user_facing_reason.clone(),
            },
        };

        PolicyEvaluation {
            decision,
            explanation: PolicyExplanation {
                action_kind: request.action_kind.to_string(),
                side_effect: side_effect.clone(),
                decision: rule_decision,
                matched_rule,
                risk_summary: risk_summary(side_effect),
                user_facing_reason,
            },
        }
    }

    /// Evaluate using an action registry to look up the side effect kind.
    pub fn evaluate_with_registry(
        &self,
        request: &ActionRequest,
        registry: &action_core::ActionRegistry,
    ) -> PolicyDecision {
        self.evaluate_with_registry_explanation(request, registry)
            .decision
    }

    /// Evaluate using an action registry and return a structured explanation.
    pub fn evaluate_with_registry_explanation(
        &self,
        request: &ActionRequest,
        registry: &action_core::ActionRegistry,
    ) -> PolicyEvaluation {
        let side_effect = registry
            .side_effect(&request.action_kind)
            .cloned()
            .unwrap_or(SideEffectKind::NetworkAccess); // Default to cautious.

        self.evaluate_with_explanation(request, &side_effect)
    }

    /// Evaluate an action with runtime context such as actor, session, server, and resource.
    pub fn evaluate_with_context(
        &self,
        request: &ActionRequest,
        side_effect: &SideEffectKind,
        context: &PolicyEvaluationContext,
        permission_store: Option<&PermissionStore>,
        now: DateTime<Utc>,
    ) -> PolicyEvaluation {
        let base = self.evaluate_with_explanation(request, side_effect);
        self.apply_context(base, context, permission_store, now)
    }

    /// Evaluate using an action registry and runtime context.
    pub fn evaluate_with_registry_and_context(
        &self,
        request: &ActionRequest,
        registry: &action_core::ActionRegistry,
        context: &PolicyEvaluationContext,
        permission_store: Option<&PermissionStore>,
        now: DateTime<Utc>,
    ) -> PolicyEvaluation {
        let side_effect = registry
            .side_effect(&request.action_kind)
            .cloned()
            .unwrap_or(SideEffectKind::NetworkAccess); // Default to cautious.

        self.evaluate_with_context(request, &side_effect, context, permission_store, now)
    }

    fn apply_context(
        &self,
        base: PolicyEvaluation,
        context: &PolicyEvaluationContext,
        permission_store: Option<&PermissionStore>,
        now: DateTime<Utc>,
    ) -> PolicyEvaluation {
        if base.decision.is_denied() {
            return base;
        }

        let Some(resource) = context.resource.as_ref() else {
            return base;
        };

        let permission_decision = match (
            context
                .actor
                .as_ref()
                .and_then(|actor| actor.enterprise_user_id.as_ref()),
            permission_store,
        ) {
            (Some(user_id), Some(store)) => {
                let role = context
                    .actor
                    .as_ref()
                    .and_then(|actor| actor.enterprise_role)
                    .unwrap_or(EnterpriseRole::User);
                store.check_with_role(
                    user_id,
                    role,
                    &resource.resource_type,
                    &resource.resource_id,
                    &resource.permission_action,
                    now,
                )
            }
            _ => PermissionDecision::Deny,
        };

        match permission_decision {
            PermissionDecision::Allow => base,
            PermissionDecision::Deny => enterprise_permission_denied(base, resource),
        }
    }
}

fn enterprise_permission_denied(
    mut base: PolicyEvaluation,
    resource: &PolicyResourceContext,
) -> PolicyEvaluation {
    let reason = format!(
        "Action denied by enterprise permission: resource={}:{} action={}",
        resource.resource_type, resource.resource_id, resource.permission_action
    );
    base.decision = PolicyDecision::Deny {
        reason: reason.clone(),
    };
    base.explanation.decision = PolicyRuleDecision::Deny;
    base.explanation.matched_rule = PolicyMatchedRule::EnterprisePermission {
        resource_type: resource.resource_type.clone(),
        resource_id: resource.resource_id.clone(),
        action: resource.permission_action,
        decision: PermissionDecision::Deny,
    };
    base.explanation.risk_summary =
        "High risk: enterprise resource access denied by permission policy.".to_string();
    base.explanation.user_facing_reason = reason;
    base
}

fn user_facing_reason(decision: &PolicyRuleDecision, side_effect: &SideEffectKind) -> String {
    match decision {
        PolicyRuleDecision::Allow => {
            format!("Action is allowed by policy: side_effect={:?}", side_effect)
        }
        PolicyRuleDecision::Ask => {
            format!("Action requires approval: side_effect={:?}", side_effect)
        }
        PolicyRuleDecision::Deny => {
            format!("Action denied by policy: side_effect={:?}", side_effect)
        }
    }
}

fn risk_summary(side_effect: &SideEffectKind) -> String {
    match side_effect {
        SideEffectKind::None => "Low risk: action has no external side effects.".to_string(),
        SideEffectKind::ReadOnly => {
            "Low risk: action reads data without mutating external state.".to_string()
        }
        SideEffectKind::RuntimeStateMutation => {
            "approval recommended: action mutates runtime state.".to_string()
        }
        SideEffectKind::NetworkAccess => {
            "approval recommended: action may access the network.".to_string()
        }
        SideEffectKind::UiSideEffect => {
            "approval recommended: action may change user-visible UI state.".to_string()
        }
        SideEffectKind::FileSystemMutation => {
            "High risk: action may modify local filesystem data.".to_string()
        }
        SideEffectKind::ExternalSystemMutation => {
            "High risk: action may mutate an external system.".to_string()
        }
        SideEffectKind::DeviceControl => {
            "High risk: action may control local device hardware.".to_string()
        }
        SideEffectKind::SensitiveProfileMutation => {
            "High risk: action may modify sensitive user profile data.".to_string()
        }
    }
}

// ────────────────────────────────────────────────────────────────────────────
// Errors
// ────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Error)]
pub enum PolicyError {
    #[error("action denied: {reason}")]
    Denied { reason: String },
    #[error("approval required: {reason}")]
    ApprovalRequired { reason: String },
    #[error("failed to parse policy toml: {reason}")]
    ParseToml { reason: String },
    #[error("failed to read policy file {path}: {reason}")]
    ReadFile { path: String, reason: String },
    #[error("policy validation failed: {report:?}")]
    ValidationFailed { report: PolicyValidationReport },
}

// ────────────────────────────────────────────────────────────────────────────
// Tests
// ────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use action_core::{ActionId, ActionKind};
    use chrono::Utc;

    fn test_request(kind: &str) -> ActionRequest {
        ActionRequest {
            action_id: ActionId::from("action-001"),
            action_kind: ActionKind::from(kind),
            input: serde_json::json!({}),
            requested_by: "u1".to_string(),
            conversation_id: None,
            message_id: None,
            requested_at: Utc::now(),
        }
    }

    #[test]
    fn policy_decision_serde_roundtrip() {
        let decisions = vec![
            PolicyDecision::Allow,
            PolicyDecision::Ask {
                reason: "needs approval".to_string(),
            },
            PolicyDecision::Deny {
                reason: "not allowed".to_string(),
            },
        ];
        for decision in decisions {
            let json = serde_json::to_string(&decision).unwrap();
            let decoded: PolicyDecision = serde_json::from_str(&json).unwrap();
            assert_eq!(
                serde_json::to_string(&decoded).unwrap(),
                serde_json::to_string(&decision).unwrap()
            );
        }
    }

    #[test]
    fn policy_decision_helpers() {
        assert!(PolicyDecision::Allow.is_allowed());
        assert!(!PolicyDecision::Allow.is_ask());
        assert!(!PolicyDecision::Allow.is_denied());

        let ask = PolicyDecision::Ask {
            reason: "test".to_string(),
        };
        assert!(!ask.is_allowed());
        assert!(ask.is_ask());
        assert!(!ask.is_denied());

        let deny = PolicyDecision::Deny {
            reason: "test".to_string(),
        };
        assert!(!deny.is_allowed());
        assert!(!deny.is_ask());
        assert!(deny.is_denied());
    }

    #[test]
    fn read_only_action_auto_allows() {
        let policy = CapabilityPolicy::default_safe();
        let request = test_request("knowledge.search");
        let decision = policy.evaluate(&request, &SideEffectKind::ReadOnly);
        assert!(decision.is_allowed());
    }

    #[test]
    fn write_action_requires_approval() {
        let policy = CapabilityPolicy::default_safe();
        let request = test_request("knowledge.save_entry");
        let decision = policy.evaluate(&request, &SideEffectKind::RuntimeStateMutation);
        assert!(decision.is_ask());
    }

    #[test]
    fn dangerous_action_is_denied() {
        let policy = CapabilityPolicy::default_safe();
        let request = test_request("mail.send");
        let decision = policy.evaluate(&request, &SideEffectKind::ExternalSystemMutation);
        assert!(decision.is_denied());
    }

    #[test]
    fn custom_policy_overrides_default() {
        // Custom policy: allow everything.
        let policy = CapabilityPolicy::new(
            vec![PolicyRule {
                side_effect: SideEffectKind::ExternalSystemMutation,
                decision: PolicyRuleDecision::Allow,
            }],
            PolicyRuleDecision::Allow,
        );
        let request = test_request("mail.send");
        let decision = policy.evaluate(&request, &SideEffectKind::ExternalSystemMutation);
        assert!(decision.is_allowed());
    }

    #[test]
    fn evaluate_with_registry() {
        let policy = CapabilityPolicy::default_safe();
        let mut registry = action_core::ActionRegistry::new();
        registry
            .register(action_core::ActionSchema {
                kind: ActionKind::from("knowledge.search"),
                display_name: "Search".to_string(),
                description: "Search KB".to_string(),
                side_effect: SideEffectKind::ReadOnly,
                input_schema: None,
                output_schema: None,
            })
            .unwrap();

        let request = test_request("knowledge.search");
        let decision = policy.evaluate_with_registry(&request, &registry);
        assert!(decision.is_allowed());
    }

    #[test]
    fn policy_explanation_records_matched_rule() {
        let policy = CapabilityPolicy::default_safe();
        let request = test_request("knowledge.search");

        let evaluation = policy.evaluate_with_explanation(&request, &SideEffectKind::ReadOnly);

        assert_eq!(evaluation.decision, PolicyDecision::Allow);
        assert_eq!(
            evaluation.explanation.matched_rule,
            PolicyMatchedRule::Rule {
                side_effect: SideEffectKind::ReadOnly
            }
        );
        assert_eq!(evaluation.explanation.action_kind, "knowledge.search");
        assert_eq!(evaluation.explanation.side_effect, SideEffectKind::ReadOnly);
        assert_eq!(evaluation.explanation.decision, PolicyRuleDecision::Allow);
    }

    #[test]
    fn policy_explanation_records_default_fallback() {
        let policy = CapabilityPolicy::new(vec![], PolicyRuleDecision::Deny);
        let request = test_request("mail.send");

        let evaluation =
            policy.evaluate_with_explanation(&request, &SideEffectKind::ExternalSystemMutation);

        assert!(evaluation.decision.is_denied());
        assert_eq!(
            evaluation.explanation.matched_rule,
            PolicyMatchedRule::Default
        );
        assert_eq!(
            evaluation.explanation.side_effect,
            SideEffectKind::ExternalSystemMutation
        );
        assert_eq!(evaluation.explanation.decision, PolicyRuleDecision::Deny);
    }

    #[test]
    fn policy_explanation_has_user_facing_reason_for_approval() {
        let policy = CapabilityPolicy::default_safe();
        let request = test_request("knowledge.save_entry");

        let evaluation =
            policy.evaluate_with_explanation(&request, &SideEffectKind::RuntimeStateMutation);

        let PolicyDecision::Ask { reason } = &evaluation.decision else {
            panic!("expected approval decision");
        };
        assert_eq!(evaluation.explanation.user_facing_reason, *reason);
        assert!(evaluation.explanation.risk_summary.contains("approval"));
    }

    #[test]
    fn registry_explanation_defaults_unknown_action_to_network_access() {
        let policy = CapabilityPolicy::default_safe();
        let registry = action_core::ActionRegistry::new();
        let request = test_request("unknown.action");

        let evaluation = policy.evaluate_with_registry_explanation(&request, &registry);

        assert!(evaluation.decision.is_ask());
        assert_eq!(
            evaluation.explanation.side_effect,
            SideEffectKind::NetworkAccess
        );
        assert_eq!(
            evaluation.explanation.matched_rule,
            PolicyMatchedRule::Rule {
                side_effect: SideEffectKind::NetworkAccess
            }
        );
    }

    #[test]
    fn policy_evaluation_serde_roundtrip() {
        let policy = CapabilityPolicy::default_safe();
        let request = test_request("mail.send");
        let evaluation =
            policy.evaluate_with_explanation(&request, &SideEffectKind::ExternalSystemMutation);

        let json = serde_json::to_string(&evaluation).unwrap();
        let decoded: PolicyEvaluation = serde_json::from_str(&json).unwrap();

        assert_eq!(decoded, evaluation);
    }

    #[test]
    fn policy_file_loads_side_effect_rules() {
        let document = PolicyFileDocument::from_toml_str(
            r#"
version = 1
default_decision = "deny"

[[side_effect_rules]]
side_effect = "read_only"
decision = "allow"

[[side_effect_rules]]
side_effect = "network_access"
decision = "ask"
"#,
        )
        .unwrap();

        assert_eq!(document.version, Some(1));
        assert_eq!(document.default_decision, PolicyRuleDecision::Deny);
        assert_eq!(document.side_effect_rules.len(), 2);
        assert!(document.validate().is_valid());
    }

    #[test]
    fn policy_file_rejects_duplicate_side_effect_rules() {
        let document = PolicyFileDocument::from_toml_str(
            r#"
default_decision = "deny"

[[side_effect_rules]]
side_effect = "read_only"
decision = "allow"

[[side_effect_rules]]
side_effect = "read_only"
decision = "deny"
"#,
        )
        .unwrap();

        let report = document.validate();

        assert!(!report.is_valid());
        assert!(report.has_error(PolicyDiagnosticCode::DuplicateSideEffectRule));
    }

    #[test]
    fn policy_file_validates_action_kind_rules() {
        let document = PolicyFileDocument::from_toml_str(
            r#"
default_decision = "deny"

[[action_kind_rules]]
action_kind = "knowledge.search"
decision = "allow"

[[action_kind_rules]]
action_kind = ""
decision = "deny"
"#,
        )
        .unwrap();

        let report = document.validate();

        assert!(!report.is_valid());
        assert!(report.has_error(PolicyDiagnosticCode::EmptyActionKind));
    }

    #[test]
    fn policy_file_validates_provider_domain_rules() {
        let document = PolicyFileDocument::from_toml_str(
            r#"
default_decision = "ask"

[[provider_domain_rules]]
provider = "openai"
domain = "api.openai.com"
decision = "ask"

[[provider_domain_rules]]
provider = "openai"
domain = "api.openai.com"
decision = "deny"

[[provider_domain_rules]]
provider = ""
domain = ""
decision = "deny"
"#,
        )
        .unwrap();

        let report = document.validate();

        assert!(!report.is_valid());
        assert!(report.has_error(PolicyDiagnosticCode::DuplicateProviderDomainRule));
        assert!(report.has_error(PolicyDiagnosticCode::EmptyProvider));
        assert!(report.has_error(PolicyDiagnosticCode::EmptyDomain));
    }

    #[test]
    fn capability_policy_can_load_from_policy_toml() {
        let policy = CapabilityPolicy::from_policy_toml_str(
            r#"
default_decision = "deny"

[[side_effect_rules]]
side_effect = "read_only"
decision = "allow"

[[side_effect_rules]]
side_effect = "runtime_state_mutation"
decision = "ask"
"#,
        )
        .unwrap();

        assert!(
            policy
                .evaluate(&test_request("knowledge.search"), &SideEffectKind::ReadOnly)
                .is_allowed()
        );
        assert!(
            policy
                .evaluate(
                    &test_request("knowledge.save_entry"),
                    &SideEffectKind::RuntimeStateMutation
                )
                .is_ask()
        );
        assert!(
            policy
                .evaluate(
                    &test_request("mail.send"),
                    &SideEffectKind::ExternalSystemMutation
                )
                .is_denied()
        );
    }

    #[test]
    fn policy_file_from_file_reads_toml() {
        let dir = std::env::temp_dir().join(format!(
            "agentos-policy-test-{}",
            chrono::Utc::now().timestamp_nanos_opt().unwrap()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("policy.toml");
        std::fs::write(
            &path,
            r#"
default_decision = "deny"

[[side_effect_rules]]
side_effect = "read_only"
decision = "allow"
"#,
        )
        .unwrap();

        let policy = CapabilityPolicy::from_policy_file(&path).unwrap();

        assert!(
            policy
                .evaluate(&test_request("knowledge.search"), &SideEffectKind::ReadOnly)
                .is_allowed()
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }

    fn enterprise_ts() -> chrono::DateTime<chrono::Utc> {
        "2026-05-26T08:00:00Z".parse().unwrap()
    }

    fn enterprise_resource_context() -> PolicyResourceContext {
        PolicyResourceContext {
            resource_type: enterprise_permission_core::ResourceType::KnowledgeBase,
            resource_id: enterprise_permission_core::ResourceId::from("kb-main"),
            permission_action: enterprise_permission_core::PermissionAction::Read,
        }
    }

    fn enterprise_actor_context(
        role: enterprise_permission_core::EnterpriseRole,
    ) -> PolicyActorContext {
        PolicyActorContext {
            actor_id: "actor-001".to_string(),
            enterprise_user_id: Some(enterprise_permission_core::EnterpriseUserId::from(
                "user-001",
            )),
            enterprise_role: Some(role),
        }
    }

    fn permission_store_with_read_grant() -> enterprise_permission_core::PermissionStore {
        let mut store = enterprise_permission_core::PermissionStore::new();
        store.add_grant(enterprise_permission_core::PermissionGrant {
            grant_id: "grant-001".to_string(),
            user_id: enterprise_permission_core::EnterpriseUserId::from("user-001"),
            role: enterprise_permission_core::EnterpriseRole::User,
            resource_type: enterprise_permission_core::ResourceType::KnowledgeBase,
            resource_id: enterprise_permission_core::ResourceId::from("kb-main"),
            actions: vec![enterprise_permission_core::PermissionAction::Read],
            granted_at: enterprise_ts(),
            expires_at: None,
            revoked: false,
        });
        store
    }

    #[test]
    fn context_policy_allows_enterprise_resource_with_grant() {
        let policy = CapabilityPolicy::default_safe();
        let store = permission_store_with_read_grant();
        let context = PolicyEvaluationContext {
            actor: Some(enterprise_actor_context(
                enterprise_permission_core::EnterpriseRole::User,
            )),
            resource: Some(enterprise_resource_context()),
            ..PolicyEvaluationContext::default()
        };

        let evaluation = policy.evaluate_with_context(
            &test_request("knowledge.search"),
            &SideEffectKind::ReadOnly,
            &context,
            Some(&store),
            enterprise_ts(),
        );

        assert!(evaluation.decision.is_allowed());
    }

    #[test]
    fn context_policy_denies_enterprise_resource_without_grant() {
        let policy = CapabilityPolicy::default_safe();
        let store = enterprise_permission_core::PermissionStore::new();
        let context = PolicyEvaluationContext {
            actor: Some(enterprise_actor_context(
                enterprise_permission_core::EnterpriseRole::User,
            )),
            resource: Some(enterprise_resource_context()),
            ..PolicyEvaluationContext::default()
        };

        let evaluation = policy.evaluate_with_context(
            &test_request("knowledge.search"),
            &SideEffectKind::ReadOnly,
            &context,
            Some(&store),
            enterprise_ts(),
        );

        assert!(evaluation.decision.is_denied());
        assert!(matches!(
            evaluation.explanation.matched_rule,
            PolicyMatchedRule::EnterprisePermission { .. }
        ));
    }

    #[test]
    fn context_policy_denies_enterprise_resource_without_actor() {
        let policy = CapabilityPolicy::default_safe();
        let store = permission_store_with_read_grant();
        let context = PolicyEvaluationContext {
            resource: Some(enterprise_resource_context()),
            ..PolicyEvaluationContext::default()
        };

        let evaluation = policy.evaluate_with_context(
            &test_request("knowledge.search"),
            &SideEffectKind::ReadOnly,
            &context,
            Some(&store),
            enterprise_ts(),
        );

        assert!(evaluation.decision.is_denied());
    }

    #[test]
    fn context_policy_preserves_base_deny() {
        let policy = CapabilityPolicy::default_safe();
        let store = permission_store_with_read_grant();
        let context = PolicyEvaluationContext {
            actor: Some(enterprise_actor_context(
                enterprise_permission_core::EnterpriseRole::User,
            )),
            resource: Some(enterprise_resource_context()),
            ..PolicyEvaluationContext::default()
        };

        let evaluation = policy.evaluate_with_context(
            &test_request("mail.send"),
            &SideEffectKind::ExternalSystemMutation,
            &context,
            Some(&store),
            enterprise_ts(),
        );

        assert!(evaluation.decision.is_denied());
        assert_eq!(
            evaluation.explanation.matched_rule,
            PolicyMatchedRule::Rule {
                side_effect: SideEffectKind::ExternalSystemMutation
            }
        );
    }

    #[test]
    fn registry_context_policy_uses_registry_side_effect() {
        let policy = CapabilityPolicy::default_safe();
        let store = permission_store_with_read_grant();
        let context = PolicyEvaluationContext {
            actor: Some(enterprise_actor_context(
                enterprise_permission_core::EnterpriseRole::User,
            )),
            resource: Some(enterprise_resource_context()),
            ..PolicyEvaluationContext::default()
        };
        let mut registry = action_core::ActionRegistry::new();
        registry
            .register(action_core::ActionSchema {
                kind: ActionKind::from("knowledge.search"),
                display_name: "Search".to_string(),
                description: "Search KB".to_string(),
                side_effect: SideEffectKind::ReadOnly,
                input_schema: None,
                output_schema: None,
            })
            .unwrap();

        let evaluation = policy.evaluate_with_registry_and_context(
            &test_request("knowledge.search"),
            &registry,
            &context,
            Some(&store),
            enterprise_ts(),
        );

        assert!(evaluation.decision.is_allowed());
        assert_eq!(evaluation.explanation.side_effect, SideEffectKind::ReadOnly);
    }

    #[test]
    fn context_policy_super_admin_bypasses_resource_grant() {
        let policy = CapabilityPolicy::default_safe();
        let store = enterprise_permission_core::PermissionStore::new();
        let context = PolicyEvaluationContext {
            actor: Some(enterprise_actor_context(
                enterprise_permission_core::EnterpriseRole::SuperAdmin,
            )),
            resource: Some(enterprise_resource_context()),
            ..PolicyEvaluationContext::default()
        };

        let evaluation = policy.evaluate_with_context(
            &test_request("knowledge.search"),
            &SideEffectKind::ReadOnly,
            &context,
            Some(&store),
            enterprise_ts(),
        );

        assert!(evaluation.decision.is_allowed());
    }

    #[test]
    fn default_safe_policy_rules() {
        let policy = CapabilityPolicy::default_safe();

        // Verify all side effect kinds have expected decisions.
        assert!(
            policy
                .evaluate(&test_request("x"), &SideEffectKind::None)
                .is_allowed()
        );
        assert!(
            policy
                .evaluate(&test_request("x"), &SideEffectKind::ReadOnly)
                .is_allowed()
        );
        assert!(
            policy
                .evaluate(&test_request("x"), &SideEffectKind::RuntimeStateMutation)
                .is_ask()
        );
        assert!(
            policy
                .evaluate(&test_request("x"), &SideEffectKind::NetworkAccess)
                .is_ask()
        );
        assert!(
            policy
                .evaluate(&test_request("x"), &SideEffectKind::UiSideEffect)
                .is_ask()
        );
        assert!(
            policy
                .evaluate(&test_request("x"), &SideEffectKind::FileSystemMutation)
                .is_denied()
        );
        assert!(
            policy
                .evaluate(&test_request("x"), &SideEffectKind::ExternalSystemMutation)
                .is_denied()
        );
        assert!(
            policy
                .evaluate(&test_request("x"), &SideEffectKind::DeviceControl)
                .is_denied()
        );
        assert!(
            policy
                .evaluate(
                    &test_request("x"),
                    &SideEffectKind::SensitiveProfileMutation
                )
                .is_denied()
        );
    }
}
