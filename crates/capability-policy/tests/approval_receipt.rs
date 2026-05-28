use action_core::{ActionId, ActionKind, ActionRequest, SideEffectKind};
use capability_policy::{
    ApprovalReceipt, ApprovalRequestId, ApprovalReusePolicy, ApprovalSideEffectSummary,
    ApprovalValidationError, KernelApprovalDecision, KernelApprovalRequest,
    validate_approval_receipt,
};
use chrono::{Duration, Utc};
use serde_json::json;

fn action_request() -> ActionRequest {
    ActionRequest {
        action_id: ActionId::from("action-1"),
        action_kind: ActionKind::from("mail.send"),
        input: json!({ "to": "user@example.com", "subject": "Hello" }),
        requested_by: "human-1".to_string(),
        conversation_id: Some("conversation-1".to_string()),
        message_id: Some("message-1".to_string()),
        requested_at: Utc::now(),
    }
}

fn approval_request(request: &ActionRequest) -> KernelApprovalRequest {
    KernelApprovalRequest::from_action_request(
        ApprovalRequestId::from("approval-1"),
        request,
        ApprovalSideEffectSummary::for_side_effect(
            SideEffectKind::ExternalSystemMutation,
            "Send an email",
        ),
    )
}

#[test]
fn approval_receipt_validates_original_action_payload() {
    let action = action_request();
    let approval = approval_request(&action);
    let receipt = ApprovalReceipt::issue(&approval, KernelApprovalDecision::approved("user", "ok"));

    assert!(validate_approval_receipt(&receipt, &action, Utc::now()).is_ok());
}

#[test]
fn approval_receipt_rejects_tampered_payload() {
    let action = action_request();
    let approval = approval_request(&action);
    let receipt = ApprovalReceipt::issue(&approval, KernelApprovalDecision::approved("user", "ok"));
    let mut tampered = action.clone();
    tampered.input = json!({ "to": "attacker@example.com", "subject": "Hello" });

    let err = validate_approval_receipt(&receipt, &tampered, Utc::now()).unwrap_err();
    assert_eq!(err, ApprovalValidationError::PayloadHashMismatch);
}

#[test]
fn denied_approval_receipt_cannot_execute() {
    let action = action_request();
    let approval = approval_request(&action);
    let receipt = ApprovalReceipt::issue(&approval, KernelApprovalDecision::denied("user", "no"));

    let err = validate_approval_receipt(&receipt, &action, Utc::now()).unwrap_err();
    assert!(matches!(err, ApprovalValidationError::Denied { .. }));
}

#[test]
fn one_time_receipt_cannot_be_reused_after_consumption() {
    let action = action_request();
    let approval = approval_request(&action).reuse_policy(ApprovalReusePolicy::OneTime);
    let receipt =
        ApprovalReceipt::issue(&approval, KernelApprovalDecision::approved("user", "ok")).consume();

    let err = validate_approval_receipt(&receipt, &action, Utc::now()).unwrap_err();
    assert_eq!(err, ApprovalValidationError::AlreadyConsumed);
}

#[test]
fn expired_receipt_cannot_execute() {
    let action = action_request();
    let approval = approval_request(&action).expires_at(Utc::now() - Duration::seconds(1));
    let receipt = ApprovalReceipt::issue(&approval, KernelApprovalDecision::approved("user", "ok"));

    let err = validate_approval_receipt(&receipt, &action, Utc::now()).unwrap_err();
    assert!(matches!(err, ApprovalValidationError::Expired { .. }));
}
