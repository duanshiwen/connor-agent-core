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
use serde::{Deserialize, Serialize};
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
// Policy Rule
// ────────────────────────────────────────────────────────────────────────────

/// A single policy rule that maps side effects to decisions.
#[derive(Debug, Clone, Serialize, Deserialize)]
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
        _request: &ActionRequest,
        side_effect: &SideEffectKind,
    ) -> PolicyDecision {
        // Find the most specific rule for this side effect kind.
        let rule_decision = self
            .rules
            .iter()
            .find(|r| r.side_effect == *side_effect)
            .map(|r| &r.decision)
            .unwrap_or(&self.default_decision);

        match rule_decision {
            PolicyRuleDecision::Allow => PolicyDecision::Allow,
            PolicyRuleDecision::Ask => PolicyDecision::Ask {
                reason: format!("Action requires approval: side_effect={:?}", side_effect),
            },
            PolicyRuleDecision::Deny => PolicyDecision::Deny {
                reason: format!("Action denied by policy: side_effect={:?}", side_effect),
            },
        }
    }

    /// Evaluate using an action registry to look up the side effect kind.
    pub fn evaluate_with_registry(
        &self,
        request: &ActionRequest,
        registry: &action_core::ActionRegistry,
    ) -> PolicyDecision {
        let side_effect = registry
            .side_effect(&request.action_kind)
            .cloned()
            .unwrap_or(SideEffectKind::NetworkAccess); // Default to cautious.

        self.evaluate(request, &side_effect)
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
