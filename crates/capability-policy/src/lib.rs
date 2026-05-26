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
