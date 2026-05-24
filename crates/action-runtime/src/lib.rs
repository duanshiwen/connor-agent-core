//! # Action Runtime
//!
//! Orchestrates action policy, execution, audit, and conversation lifecycle events.
//!
//! This crate intentionally does not implement concrete Browser / Knowledge / Mail
//! integrations. It provides the safe execution boundary those integrations will
//! use later.

use action_core::{
    ActionExecutor, ActionExecutorError, ActionId, ActionRegistry, ActionRequest, ActionResult,
    SideEffectKind,
};
use anyhow::Result;
use audit_log::{AuditEvent, AuditLog};
use capability_policy::{CapabilityPolicy, PolicyDecision};
use chrono::Utc;
use conversation_core::{ConversationId, ParticipantId};
use conversation_kernel::{
    CompleteActionCommand, ConversationKernel, DenyActionCommand, FailActionCommand,
    RequestActionCommand, RequireActionApprovalCommand, StartActionCommand,
};
use serde::{Deserialize, Serialize};

/// Result of processing an action request through policy and execution.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ActionRuntimeOutcome {
    /// Action was allowed and completed successfully.
    Completed {
        action_id: ActionId,
        result: ActionResult,
    },
    /// Action requires approval before execution.
    ApprovalRequired { action_id: ActionId, reason: String },
    /// Action was denied and not executed.
    Denied { action_id: ActionId, reason: String },
    /// Action was allowed, started, and then failed during execution.
    Failed {
        action_id: ActionId,
        error_message: String,
    },
}

/// Request to process a single action.
pub struct ProcessActionRequest<'a> {
    pub conversation_id: &'a ConversationId,
    pub action_request: ActionRequest,
    pub requested_by: Option<ParticipantId>,
    pub runtime_actor: Option<ParticipantId>,
}

/// Coordinates action policy, execution, audit, and conversation lifecycle events.
pub struct ActionRuntime<'a> {
    pub kernel: &'a ConversationKernel,
    pub registry: &'a ActionRegistry,
    pub policy: &'a CapabilityPolicy,
    pub executor: &'a dyn ActionExecutor,
    pub audit_log: &'a dyn AuditLog,
}

impl<'a> ActionRuntime<'a> {
    /// Process an action request.
    ///
    /// The request is always recorded in the conversation first. Policy then
    /// determines whether the action is executed, waits for approval, or is denied.
    pub async fn process(&self, req: ProcessActionRequest<'_>) -> Result<ActionRuntimeOutcome> {
        let ProcessActionRequest {
            conversation_id,
            action_request,
            requested_by,
            runtime_actor,
        } = req;

        self.kernel
            .request_action(RequestActionCommand {
                conversation_id: conversation_id.clone(),
                action_request: action_request.clone(),
                requested_by,
            })
            .await?;

        let action_id = action_request.action_id.clone();
        let Some(schema) = self.registry.get(&action_request.action_kind) else {
            let reason = format!("action kind not registered: {}", action_request.action_kind);
            self.kernel
                .deny_action(DenyActionCommand {
                    conversation_id: conversation_id.clone(),
                    action_id: action_id.clone(),
                    reason: reason.clone(),
                    denied_by: runtime_actor.clone(),
                })
                .await?;
            self.record_audit(AuditRecordInput {
                request: &action_request,
                side_effect: None,
                policy_decision: "deny",
                result_status: "denied",
                result_summary: Some(&reason),
                approved_by: None,
            })
            .await?;
            return Ok(ActionRuntimeOutcome::Denied { action_id, reason });
        };

        let side_effect = schema.side_effect.clone();
        match self.policy.evaluate(&action_request, &side_effect) {
            PolicyDecision::Allow => {
                self.kernel
                    .start_action(StartActionCommand {
                        conversation_id: conversation_id.clone(),
                        action_id: action_id.clone(),
                        started_by: runtime_actor.clone(),
                    })
                    .await?;

                match self.executor.execute(&action_request).await {
                    Ok(result) => {
                        self.kernel
                            .complete_action(CompleteActionCommand {
                                conversation_id: conversation_id.clone(),
                                action_id: action_id.clone(),
                                result: result.clone(),
                                completed_by: runtime_actor,
                            })
                            .await?;
                        self.record_audit(AuditRecordInput {
                            request: &action_request,
                            side_effect: Some(&side_effect),
                            policy_decision: "allow",
                            result_status: "completed",
                            result_summary: Some(&result.summary),
                            approved_by: None,
                        })
                        .await?;
                        Ok(ActionRuntimeOutcome::Completed { action_id, result })
                    }
                    Err(err) => {
                        let error_message = action_executor_error_message(&err);
                        self.kernel
                            .fail_action(FailActionCommand {
                                conversation_id: conversation_id.clone(),
                                action_id: action_id.clone(),
                                error_message: error_message.clone(),
                                failed_by: runtime_actor,
                            })
                            .await?;
                        self.record_audit(AuditRecordInput {
                            request: &action_request,
                            side_effect: Some(&side_effect),
                            policy_decision: "allow",
                            result_status: "failed",
                            result_summary: Some(&error_message),
                            approved_by: None,
                        })
                        .await?;
                        Ok(ActionRuntimeOutcome::Failed {
                            action_id,
                            error_message,
                        })
                    }
                }
            }
            PolicyDecision::Ask { reason } => {
                self.kernel
                    .require_action_approval(RequireActionApprovalCommand {
                        conversation_id: conversation_id.clone(),
                        action_id: action_id.clone(),
                        reason: reason.clone(),
                        required_by: runtime_actor,
                    })
                    .await?;
                self.record_audit(AuditRecordInput {
                    request: &action_request,
                    side_effect: Some(&side_effect),
                    policy_decision: "ask",
                    result_status: "approval_required",
                    result_summary: Some(&reason),
                    approved_by: None,
                })
                .await?;
                Ok(ActionRuntimeOutcome::ApprovalRequired { action_id, reason })
            }
            PolicyDecision::Deny { reason } => {
                self.kernel
                    .deny_action(DenyActionCommand {
                        conversation_id: conversation_id.clone(),
                        action_id: action_id.clone(),
                        reason: reason.clone(),
                        denied_by: runtime_actor,
                    })
                    .await?;
                self.record_audit(AuditRecordInput {
                    request: &action_request,
                    side_effect: Some(&side_effect),
                    policy_decision: "deny",
                    result_status: "denied",
                    result_summary: Some(&reason),
                    approved_by: None,
                })
                .await?;
                Ok(ActionRuntimeOutcome::Denied { action_id, reason })
            }
        }
    }

    async fn record_audit(&self, input: AuditRecordInput<'_>) -> Result<()> {
        self.audit_log.record(build_audit_event(input)).await
    }
}

struct AuditRecordInput<'a> {
    request: &'a ActionRequest,
    side_effect: Option<&'a SideEffectKind>,
    policy_decision: &'a str,
    result_status: &'a str,
    result_summary: Option<&'a str>,
    approved_by: Option<&'a ParticipantId>,
}

fn build_audit_event(input: AuditRecordInput<'_>) -> AuditEvent {
    AuditEvent {
        audit_id: format!("audit-{}-{}", input.request.action_id, input.result_status),
        action_id: input.request.action_id.to_string(),
        action_kind: input.request.action_kind.to_string(),
        requested_by: input.request.requested_by.clone(),
        approved_by: input.approved_by.map(ToString::to_string),
        input_summary: summarize_input(&input.request.input),
        side_effect: input
            .side_effect
            .map(|side_effect| format!("{side_effect:?}"))
            .unwrap_or_else(|| "unknown".to_string()),
        policy_decision: input.policy_decision.to_string(),
        result_status: input.result_status.to_string(),
        result_summary: input.result_summary.map(ToString::to_string),
        conversation_id: input.request.conversation_id.clone(),
        message_id: input.request.message_id.clone(),
        timestamp: Utc::now(),
    }
}

fn summarize_input(input: &serde_json::Value) -> String {
    let text = input.to_string();
    const MAX_LEN: usize = 200;
    if text.len() <= MAX_LEN {
        text
    } else {
        format!("{}…", &text[..MAX_LEN])
    }
}

fn action_executor_error_message(err: &ActionExecutorError) -> String {
    err.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use action_core::{ActionKind, ActionSchema, SideEffectKind};

    #[test]
    fn summarizes_short_input() {
        assert_eq!(
            summarize_input(&serde_json::json!({"q":"x"})),
            "{\"q\":\"x\"}"
        );
    }

    #[test]
    fn audit_event_contains_core_fields() {
        let request = ActionRequest {
            action_id: ActionId::from("action-1"),
            action_kind: ActionKind::from("knowledge.search"),
            input: serde_json::json!({"query":"agent os"}),
            requested_by: "user-1".to_string(),
            conversation_id: Some("conv-1".to_string()),
            message_id: Some("msg-1".to_string()),
            requested_at: Utc::now(),
        };

        let event = build_audit_event(AuditRecordInput {
            request: &request,
            side_effect: Some(&SideEffectKind::ReadOnly),
            policy_decision: "allow",
            result_status: "completed",
            result_summary: Some("ok"),
            approved_by: None,
        });

        assert_eq!(event.action_id, "action-1");
        assert_eq!(event.action_kind, "knowledge.search");
        assert_eq!(event.policy_decision, "allow");
        assert_eq!(event.result_status, "completed");
    }

    #[test]
    fn registry_lookup_smoke_test() {
        let mut registry = ActionRegistry::new();
        registry
            .register(ActionSchema {
                kind: ActionKind::from("knowledge.search"),
                display_name: "Search".to_string(),
                description: "Search knowledge".to_string(),
                side_effect: SideEffectKind::ReadOnly,
                input_schema: None,
                output_schema: None,
            })
            .unwrap();
        assert!(
            registry
                .get(&ActionKind::from("knowledge.search"))
                .is_some()
        );
    }
}
