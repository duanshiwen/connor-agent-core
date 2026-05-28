use action_core::{ActionId, ActionKind, ActionRequest, SideEffectKind};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ApprovalRequestId(pub String);

impl From<&str> for ApprovalRequestId {
    fn from(value: &str) -> Self {
        Self(value.to_string())
    }
}

impl From<String> for ApprovalRequestId {
    fn from(value: String) -> Self {
        Self(value)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActionPayloadHash(pub String);

impl ActionPayloadHash {
    pub fn for_request(request: &ActionRequest) -> Self {
        let mut hasher = DefaultHasher::new();
        request.action_id.0.hash(&mut hasher);
        request.action_kind.0.hash(&mut hasher);
        canonical_json(&request.input).hash(&mut hasher);
        request.requested_by.hash(&mut hasher);
        request.conversation_id.hash(&mut hasher);
        request.message_id.hash(&mut hasher);
        Self(format!("agentos-hash-v1:{:016x}", hasher.finish()))
    }
}

fn canonical_json(value: &serde_json::Value) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| "null".to_string())
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalReusePolicy {
    OneTime,
    ReusableUntilExpiry,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApprovalSideEffectSummary {
    pub side_effect: SideEffectKind,
    pub risk_level: String,
    pub affected_resources: Vec<String>,
    pub external_destinations: Vec<String>,
    pub data_exposure: Vec<String>,
    pub reversible: bool,
    pub user_visible_description: String,
}

impl ApprovalSideEffectSummary {
    pub fn for_side_effect(side_effect: SideEffectKind, description: impl Into<String>) -> Self {
        let risk_level = match side_effect {
            SideEffectKind::None => "none",
            SideEffectKind::ReadOnly => "low",
            SideEffectKind::RuntimeStateMutation | SideEffectKind::UiSideEffect => "medium",
            SideEffectKind::FileSystemMutation
            | SideEffectKind::NetworkAccess
            | SideEffectKind::ExternalSystemMutation
            | SideEffectKind::DeviceControl
            | SideEffectKind::SensitiveProfileMutation => "high",
        }
        .to_string();
        Self {
            side_effect,
            risk_level,
            affected_resources: Vec::new(),
            external_destinations: Vec::new(),
            data_exposure: Vec::new(),
            reversible: false,
            user_visible_description: description.into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KernelApprovalRequest {
    pub approval_request_id: ApprovalRequestId,
    pub action_id: ActionId,
    pub action_kind: ActionKind,
    pub action_payload_hash: ActionPayloadHash,
    pub requested_by: String,
    pub requested_at: DateTime<Utc>,
    pub expires_at: Option<DateTime<Utc>>,
    pub reuse_policy: ApprovalReusePolicy,
    pub side_effect_summary: ApprovalSideEffectSummary,
}

impl KernelApprovalRequest {
    pub fn from_action_request(
        approval_request_id: ApprovalRequestId,
        request: &ActionRequest,
        side_effect_summary: ApprovalSideEffectSummary,
    ) -> Self {
        Self {
            approval_request_id,
            action_id: request.action_id.clone(),
            action_kind: request.action_kind.clone(),
            action_payload_hash: ActionPayloadHash::for_request(request),
            requested_by: request.requested_by.clone(),
            requested_at: Utc::now(),
            expires_at: None,
            reuse_policy: ApprovalReusePolicy::OneTime,
            side_effect_summary,
        }
    }

    pub fn expires_at(mut self, expires_at: DateTime<Utc>) -> Self {
        self.expires_at = Some(expires_at);
        self
    }

    pub fn reuse_policy(mut self, reuse_policy: ApprovalReusePolicy) -> Self {
        self.reuse_policy = reuse_policy;
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KernelApprovalDecisionKind {
    Approved,
    Denied,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KernelApprovalDecision {
    pub kind: KernelApprovalDecisionKind,
    pub decided_by: String,
    pub reason: String,
    pub decided_at: DateTime<Utc>,
}

impl KernelApprovalDecision {
    pub fn approved(decided_by: impl Into<String>, reason: impl Into<String>) -> Self {
        Self {
            kind: KernelApprovalDecisionKind::Approved,
            decided_by: decided_by.into(),
            reason: reason.into(),
            decided_at: Utc::now(),
        }
    }

    pub fn denied(decided_by: impl Into<String>, reason: impl Into<String>) -> Self {
        Self {
            kind: KernelApprovalDecisionKind::Denied,
            decided_by: decided_by.into(),
            reason: reason.into(),
            decided_at: Utc::now(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApprovalReceipt {
    pub approval_request_id: ApprovalRequestId,
    pub action_id: ActionId,
    pub action_payload_hash: ActionPayloadHash,
    pub decision: KernelApprovalDecision,
    pub issued_at: DateTime<Utc>,
    pub expires_at: Option<DateTime<Utc>>,
    pub reuse_policy: ApprovalReusePolicy,
    pub consumed_at: Option<DateTime<Utc>>,
}

impl ApprovalReceipt {
    pub fn issue(request: &KernelApprovalRequest, decision: KernelApprovalDecision) -> Self {
        Self {
            approval_request_id: request.approval_request_id.clone(),
            action_id: request.action_id.clone(),
            action_payload_hash: request.action_payload_hash.clone(),
            decision,
            issued_at: Utc::now(),
            expires_at: request.expires_at,
            reuse_policy: request.reuse_policy.clone(),
            consumed_at: None,
        }
    }

    pub fn consume(mut self) -> Self {
        self.consumed_at = Some(Utc::now());
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ApprovalValidationResult {
    Valid,
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum ApprovalValidationError {
    #[error("approval receipt was denied: {reason}")]
    Denied { reason: String },
    #[error("approval receipt expired at {expires_at}")]
    Expired { expires_at: DateTime<Utc> },
    #[error("approval receipt already consumed")]
    AlreadyConsumed,
    #[error("approval receipt action id mismatch")]
    ActionIdMismatch,
    #[error("approval receipt payload hash mismatch")]
    PayloadHashMismatch,
}

pub fn validate_approval_receipt(
    receipt: &ApprovalReceipt,
    request: &ActionRequest,
    now: DateTime<Utc>,
) -> Result<ApprovalValidationResult, ApprovalValidationError> {
    if receipt.decision.kind == KernelApprovalDecisionKind::Denied {
        return Err(ApprovalValidationError::Denied {
            reason: receipt.decision.reason.clone(),
        });
    }
    if let Some(expires_at) = receipt.expires_at
        && now > expires_at
    {
        return Err(ApprovalValidationError::Expired { expires_at });
    }
    if receipt.consumed_at.is_some() && receipt.reuse_policy == ApprovalReusePolicy::OneTime {
        return Err(ApprovalValidationError::AlreadyConsumed);
    }
    if receipt.action_id != request.action_id {
        return Err(ApprovalValidationError::ActionIdMismatch);
    }
    if receipt.action_payload_hash != ActionPayloadHash::for_request(request) {
        return Err(ApprovalValidationError::PayloadHashMismatch);
    }
    Ok(ApprovalValidationResult::Valid)
}
