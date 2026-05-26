//! # Surface Core
//!
//! Core domain types for AgentOS surfaces.
//!
//! Surfaces are renderable, attachable views over artifacts and conversation content.
//! They represent UI surfaces such as web views, mail views, document previews,
//! image viewers, knowledge entry editors, approval dialogs, and audit timelines.

use action_core::{ActionId, ActionKind, ActionRequest, SideEffectKind};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::fmt;

/// Unique identifier for a surface.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SurfaceId(pub String);

impl fmt::Display for SurfaceId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl From<String> for SurfaceId {
    fn from(s: String) -> Self {
        Self(s)
    }
}

impl From<&str> for SurfaceId {
    fn from(s: &str) -> Self {
        Self(s.to_string())
    }
}

/// The broad category of surface.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SurfaceKind {
    WebSurface,
    MailSurface,
    DocumentSurface,
    ImageSurface,
    KnowledgeEntrySurface,
    ApprovalSurface,
    AuditSurface,
}

/// The lifecycle status of a surface.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SurfaceLifecycleStatus {
    Attached,
    Updated,
    Closed,
}

/// Hint for how a surface should be rendered.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SurfaceRendererHint {
    Markdown,
    Html,
    PlainText,
    Image,
    Pdf,
    Table,
    Form,
    Timeline,
    Custom(String),
}

/// Metadata describing a surface available to the Assistant or conversation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SurfaceDescriptor {
    pub id: SurfaceId,
    pub kind: SurfaceKind,
    pub title: Option<String>,
    pub renderer_hint: SurfaceRendererHint,
    pub artifact_id: Option<artifact_core::ArtifactId>,
    pub metadata: serde_json::Value,
    pub created_at: DateTime<Utc>,
}

impl SurfaceDescriptor {
    pub fn new(
        id: impl Into<SurfaceId>,
        kind: SurfaceKind,
        renderer_hint: SurfaceRendererHint,
        created_at: DateTime<Utc>,
    ) -> Self {
        Self {
            id: id.into(),
            kind,
            title: None,
            renderer_hint,
            artifact_id: None,
            metadata: serde_json::json!({}),
            created_at,
        }
    }

    pub fn with_title(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }

    pub fn with_artifact_id(mut self, artifact_id: artifact_core::ArtifactId) -> Self {
        self.artifact_id = Some(artifact_id);
        self
    }

    pub fn with_metadata(mut self, metadata: serde_json::Value) -> Self {
        self.metadata = metadata;
        self
    }
}

/// The current state of a surface, including its lifecycle status and last update time.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SurfaceState {
    pub descriptor: SurfaceDescriptor,
    pub status: SurfaceLifecycleStatus,
    pub updated_at: DateTime<Utc>,
}

impl SurfaceState {
    pub fn attached(descriptor: SurfaceDescriptor, created_at: DateTime<Utc>) -> Self {
        Self {
            descriptor,
            status: SurfaceLifecycleStatus::Attached,
            updated_at: created_at,
        }
    }
}

// ────────────────────────────────────────────────────────────────────────────
// Approval Prompt Model
// ────────────────────────────────────────────────────────────────────────────

/// Stable approval card payload suitable for ApprovalSurface metadata/rendering.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ApprovalPromptCard {
    pub approval_id: String,
    pub action_id: ActionId,
    pub action_kind: ActionKind,
    pub action_display_name: String,
    pub conversation_id: Option<String>,
    pub requested_by: String,
    pub reason: String,
    pub side_effect_summary: ApprovalSideEffectSummary,
    pub diff_summary: Option<ApprovalDiffSummary>,
    pub data_exposure_summary: ApprovalDataExposureSummary,
    pub policy_explanation: Option<capability_policy::PolicyExplanation>,
    pub created_at: DateTime<Utc>,
    pub expires_at: Option<DateTime<Utc>>,
}

impl ApprovalPromptCard {
    #[allow(clippy::too_many_arguments)]
    pub fn from_action_request(
        approval_id: impl Into<String>,
        request: &ActionRequest,
        action_display_name: impl Into<String>,
        side_effect: SideEffectKind,
        reason: impl Into<String>,
        diff_summary: Option<ApprovalDiffSummary>,
        policy_explanation: Option<capability_policy::PolicyExplanation>,
        created_at: DateTime<Utc>,
        expires_at: Option<DateTime<Utc>>,
    ) -> Self {
        let side_effect_summary = ApprovalSideEffectSummary::from_side_effect(&side_effect);
        let data_exposure_summary =
            ApprovalDataExposureSummary::from_action_input(&side_effect, &request.input);

        Self {
            approval_id: approval_id.into(),
            action_id: request.action_id.clone(),
            action_kind: request.action_kind.clone(),
            action_display_name: action_display_name.into(),
            conversation_id: request.conversation_id.clone(),
            requested_by: request.requested_by.clone(),
            reason: reason.into(),
            side_effect_summary,
            diff_summary,
            data_exposure_summary,
            policy_explanation,
            created_at,
            expires_at,
        }
    }
}

/// Human-facing risk level for an approval prompt.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalRiskLevel {
    Low,
    Medium,
    High,
}

/// Human-facing side-effect summary for approval UI.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApprovalSideEffectSummary {
    pub side_effect: SideEffectKind,
    pub risk_level: ApprovalRiskLevel,
    pub summary: String,
}

impl ApprovalSideEffectSummary {
    pub fn from_side_effect(side_effect: &SideEffectKind) -> Self {
        let (risk_level, summary) = match side_effect {
            SideEffectKind::None => (
                ApprovalRiskLevel::Low,
                "Low risk: no external side effects.",
            ),
            SideEffectKind::ReadOnly => (ApprovalRiskLevel::Low, "Low risk: read-only access."),
            SideEffectKind::RuntimeStateMutation => (
                ApprovalRiskLevel::Medium,
                "Medium risk: modifies local runtime state.",
            ),
            SideEffectKind::FileSystemMutation => (
                ApprovalRiskLevel::Medium,
                "Medium risk: modifies the local filesystem.",
            ),
            SideEffectKind::NetworkAccess => (
                ApprovalRiskLevel::Medium,
                "Medium risk: accesses the network.",
            ),
            SideEffectKind::ExternalSystemMutation => (
                ApprovalRiskLevel::High,
                "High risk: modifies an external system.",
            ),
            SideEffectKind::UiSideEffect => (
                ApprovalRiskLevel::Medium,
                "Medium risk: changes user-visible UI state.",
            ),
            SideEffectKind::DeviceControl => (
                ApprovalRiskLevel::High,
                "High risk: controls local device capabilities.",
            ),
            SideEffectKind::SensitiveProfileMutation => (
                ApprovalRiskLevel::High,
                "High risk: modifies sensitive profile data.",
            ),
        };

        Self {
            side_effect: side_effect.clone(),
            risk_level,
            summary: summary.to_string(),
        }
    }
}

/// Optional diff summary for approvals that mutate content/state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApprovalDiffSummary {
    pub before_label: Option<String>,
    pub after_label: Option<String>,
    pub summary: String,
    pub changed_fields: Vec<String>,
}

/// Human-facing summary of data exposure for approval UI.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApprovalDataExposureSummary {
    pub exposes_external_data: bool,
    pub exposes_sensitive_profile: bool,
    pub destinations: Vec<String>,
    pub summary: String,
}

impl ApprovalDataExposureSummary {
    pub fn from_action_input(side_effect: &SideEffectKind, input: &serde_json::Value) -> Self {
        let destinations = extract_destinations(input);
        let exposes_external_data = matches!(
            side_effect,
            SideEffectKind::NetworkAccess | SideEffectKind::ExternalSystemMutation
        );
        let exposes_sensitive_profile =
            matches!(side_effect, SideEffectKind::SensitiveProfileMutation);
        let summary = data_exposure_summary_text(
            exposes_external_data,
            exposes_sensitive_profile,
            destinations.len(),
        );

        Self {
            exposes_external_data,
            exposes_sensitive_profile,
            destinations,
            summary,
        }
    }
}

fn data_exposure_summary_text(
    exposes_external_data: bool,
    exposes_sensitive_profile: bool,
    destination_count: usize,
) -> String {
    if exposes_sensitive_profile {
        "May expose or modify sensitive profile data.".to_string()
    } else if exposes_external_data && destination_count > 0 {
        format!("May expose data to {destination_count} destination(s).")
    } else if exposes_external_data {
        "May expose data to an external destination.".to_string()
    } else {
        "No external data exposure detected.".to_string()
    }
}

fn extract_destinations(value: &serde_json::Value) -> Vec<String> {
    let mut ordered = Vec::new();
    let mut seen = BTreeSet::new();
    for key in [
        "recipient",
        "recipients",
        "to",
        "email",
        "url",
        "endpoint",
        "domain",
    ] {
        collect_destination_key(value, key, &mut seen, &mut ordered);
    }
    ordered
}

fn collect_destination_key(
    value: &serde_json::Value,
    target_key: &str,
    seen: &mut BTreeSet<String>,
    ordered: &mut Vec<String>,
) {
    match value {
        serde_json::Value::Object(map) => {
            for (key, child) in map {
                if key == target_key {
                    collect_destination_value(child, seen, ordered);
                }
                collect_destination_key(child, target_key, seen, ordered);
            }
        }
        serde_json::Value::Array(values) => {
            for child in values {
                collect_destination_key(child, target_key, seen, ordered);
            }
        }
        _ => {}
    }
}

fn collect_destination_value(
    value: &serde_json::Value,
    seen: &mut BTreeSet<String>,
    ordered: &mut Vec<String>,
) {
    match value {
        serde_json::Value::String(destination) => {
            if seen.insert(destination.clone()) {
                ordered.push(destination.clone());
            }
        }
        serde_json::Value::Array(values) => {
            for child in values {
                collect_destination_value(child, seen, ordered);
            }
        }
        _ => {}
    }
}

/// Errors from surface operations.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SurfaceRegistryError {
    #[error("duplicate surface id: {0}")]
    DuplicateSurfaceId(SurfaceId),
}

/// In-memory surface registry for tests and early runtime flows.
#[derive(Debug, Clone, Default)]
pub struct SurfaceRegistry {
    surfaces: std::collections::HashMap<SurfaceId, SurfaceState>,
}

impl SurfaceRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(&mut self, state: SurfaceState) -> Result<(), SurfaceRegistryError> {
        if self.surfaces.contains_key(&state.descriptor.id) {
            return Err(SurfaceRegistryError::DuplicateSurfaceId(
                state.descriptor.id.clone(),
            ));
        }
        self.surfaces.insert(state.descriptor.id.clone(), state);
        Ok(())
    }

    pub fn get(&self, id: &SurfaceId) -> Option<&SurfaceState> {
        self.surfaces.get(id)
    }

    pub fn contains(&self, id: &SurfaceId) -> bool {
        self.surfaces.contains_key(id)
    }

    pub fn len(&self) -> usize {
        self.surfaces.len()
    }

    pub fn is_empty(&self) -> bool {
        self.surfaces.is_empty()
    }

    pub fn iter(&self) -> impl Iterator<Item = &SurfaceState> {
        self.surfaces.values()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use action_core::{ActionId, ActionKind, ActionRequest, SideEffectKind};

    fn approval_ts() -> DateTime<Utc> {
        "2026-05-26T08:00:00Z".parse().unwrap()
    }

    fn approval_action_request(input: serde_json::Value) -> ActionRequest {
        ActionRequest {
            action_id: ActionId::from("action-approval-1"),
            action_kind: ActionKind::from("mail.send"),
            input,
            requested_by: "agent-1".to_string(),
            conversation_id: Some("conversation-1".to_string()),
            message_id: Some("message-1".to_string()),
            requested_at: approval_ts(),
        }
    }

    fn web_surface_descriptor() -> SurfaceDescriptor {
        SurfaceDescriptor::new(
            "surface-web-1",
            SurfaceKind::WebSurface,
            SurfaceRendererHint::Html,
            "2026-05-24T12:00:00Z".parse().unwrap(),
        )
        .with_title("Example Page")
        .with_artifact_id(artifact_core::ArtifactId::from("artifact-web-1"))
    }

    fn knowledge_surface_descriptor() -> SurfaceDescriptor {
        SurfaceDescriptor::new(
            "surface-knowledge-1",
            SurfaceKind::KnowledgeEntrySurface,
            SurfaceRendererHint::Markdown,
            "2026-05-24T12:00:00Z".parse().unwrap(),
        )
        .with_title("Knowledge Entry")
    }

    #[test]
    fn surface_id_roundtrips() {
        let id = SurfaceId::from("surface-1");
        assert_eq!(id.to_string(), "surface-1");

        let json = serde_json::to_string(&id).unwrap();
        let decoded: SurfaceId = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded, id);
    }

    #[test]
    fn surface_kind_serializes_as_snake_case() {
        let json = serde_json::to_string(&SurfaceKind::KnowledgeEntrySurface).unwrap();
        assert_eq!(json, "\"knowledge_entry_surface\"");

        let decoded: SurfaceKind = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded, SurfaceKind::KnowledgeEntrySurface);
    }

    #[test]
    fn surface_lifecycle_status_serializes_as_snake_case() {
        let json = serde_json::to_string(&SurfaceLifecycleStatus::Closed).unwrap();
        assert_eq!(json, "\"closed\"");

        let decoded: SurfaceLifecycleStatus = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded, SurfaceLifecycleStatus::Closed);
    }

    #[test]
    fn surface_renderer_hint_roundtrips() {
        let hints = vec![
            SurfaceRendererHint::Markdown,
            SurfaceRendererHint::Html,
            SurfaceRendererHint::PlainText,
            SurfaceRendererHint::Image,
            SurfaceRendererHint::Pdf,
            SurfaceRendererHint::Table,
            SurfaceRendererHint::Form,
            SurfaceRendererHint::Timeline,
            SurfaceRendererHint::Custom("custom_layout".to_string()),
        ];

        for hint in hints {
            let json = serde_json::to_string(&hint).unwrap();
            let decoded: SurfaceRendererHint = serde_json::from_str(&json).unwrap();
            assert_eq!(decoded, hint);
        }
    }

    #[test]
    fn surface_descriptor_roundtrips() {
        let descriptor = web_surface_descriptor();
        let json = serde_json::to_string_pretty(&descriptor).unwrap();
        let decoded: SurfaceDescriptor = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded, descriptor);
    }

    #[test]
    fn surface_state_roundtrips() {
        let descriptor = knowledge_surface_descriptor();
        let created_at: DateTime<Utc> = "2026-05-24T12:00:00Z".parse().unwrap();
        let state = SurfaceState::attached(descriptor, created_at);

        let json = serde_json::to_string_pretty(&state).unwrap();
        let decoded: SurfaceState = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded, state);
    }

    #[test]
    fn surface_descriptor_can_reference_artifact_id() {
        let descriptor = web_surface_descriptor();
        assert!(descriptor.artifact_id.is_some());
        assert_eq!(
            descriptor.artifact_id.unwrap(),
            artifact_core::ArtifactId::from("artifact-web-1")
        );
    }

    #[test]
    fn surface_descriptor_without_artifact_id_defaults_to_none() {
        let descriptor = knowledge_surface_descriptor();
        assert!(descriptor.artifact_id.is_none());
    }

    #[test]
    fn surface_registry_register_and_get() {
        let mut registry = SurfaceRegistry::new();
        let descriptor = web_surface_descriptor();
        let created_at: DateTime<Utc> = "2026-05-24T12:00:00Z".parse().unwrap();
        let state = SurfaceState::attached(descriptor, created_at);

        registry.register(state.clone()).unwrap();

        assert_eq!(registry.len(), 1);
        assert!(registry.contains(&SurfaceId::from("surface-web-1")));
        assert_eq!(
            registry.get(&SurfaceId::from("surface-web-1")),
            Some(&state)
        );
    }

    #[test]
    fn surface_registry_duplicate_id_fails() {
        let mut registry = SurfaceRegistry::new();
        let descriptor = web_surface_descriptor();
        let created_at: DateTime<Utc> = "2026-05-24T12:00:00Z".parse().unwrap();
        let state = SurfaceState::attached(descriptor, created_at);

        registry.register(state.clone()).unwrap();

        let err = registry.register(state).unwrap_err();
        assert_eq!(
            err,
            SurfaceRegistryError::DuplicateSurfaceId(SurfaceId::from("surface-web-1"))
        );
    }

    #[test]
    fn surface_registry_returns_none_for_missing_surface() {
        let registry = SurfaceRegistry::new();
        assert_eq!(registry.get(&SurfaceId::from("missing")), None);
    }

    #[test]
    fn surface_state_is_attached_lifecycle() {
        let descriptor = knowledge_surface_descriptor();
        let created_at: DateTime<Utc> = "2026-05-24T12:00:00Z".parse().unwrap();
        let state = SurfaceState::attached(descriptor, created_at);

        assert_eq!(state.status, SurfaceLifecycleStatus::Attached);
        assert_eq!(state.updated_at, created_at);
    }

    #[test]
    fn approval_side_effect_summary_maps_risk_levels() {
        assert_eq!(
            ApprovalSideEffectSummary::from_side_effect(&SideEffectKind::ReadOnly).risk_level,
            ApprovalRiskLevel::Low
        );
        assert_eq!(
            ApprovalSideEffectSummary::from_side_effect(&SideEffectKind::NetworkAccess).risk_level,
            ApprovalRiskLevel::Medium
        );
        assert_eq!(
            ApprovalSideEffectSummary::from_side_effect(&SideEffectKind::ExternalSystemMutation)
                .risk_level,
            ApprovalRiskLevel::High
        );
        assert_eq!(
            ApprovalSideEffectSummary::from_side_effect(&SideEffectKind::SensitiveProfileMutation)
                .risk_level,
            ApprovalRiskLevel::High
        );
    }

    #[test]
    fn approval_prompt_card_from_action_request_contains_action_and_reason() {
        let request = approval_action_request(serde_json::json!({"to": "ops@example.com"}));
        let card = ApprovalPromptCard::from_action_request(
            "approval-1",
            &request,
            "Send Mail",
            SideEffectKind::ExternalSystemMutation,
            "External email send requires approval",
            None,
            None,
            approval_ts(),
            Some("2026-05-26T09:00:00Z".parse().unwrap()),
        );

        assert_eq!(card.approval_id, "approval-1");
        assert_eq!(card.action_id, ActionId::from("action-approval-1"));
        assert_eq!(card.action_kind, ActionKind::from("mail.send"));
        assert_eq!(card.action_display_name, "Send Mail");
        assert_eq!(card.conversation_id.as_deref(), Some("conversation-1"));
        assert_eq!(card.requested_by, "agent-1");
        assert_eq!(card.reason, "External email send requires approval");
        assert_eq!(card.created_at, approval_ts());
        assert!(card.expires_at.is_some());
    }

    #[test]
    fn approval_prompt_card_extracts_destinations_from_input() {
        let request = approval_action_request(serde_json::json!({
            "url": "https://api.example.com/v1/messages",
            "recipient": "ops@example.com",
            "payload": {"subject": "Deploy"}
        }));
        let card = ApprovalPromptCard::from_action_request(
            "approval-1",
            &request,
            "Send Mail",
            SideEffectKind::ExternalSystemMutation,
            "External send requires approval",
            None,
            None,
            approval_ts(),
            None,
        );

        assert!(card.data_exposure_summary.exposes_external_data);
        assert_eq!(
            card.data_exposure_summary.destinations,
            vec![
                "ops@example.com".to_string(),
                "https://api.example.com/v1/messages".to_string()
            ]
        );
    }

    #[test]
    fn approval_prompt_card_marks_sensitive_profile_exposure() {
        let request = approval_action_request(serde_json::json!({
            "profile_field": "home_address",
            "value": "Hangzhou"
        }));
        let card = ApprovalPromptCard::from_action_request(
            "approval-1",
            &request,
            "Update Profile",
            SideEffectKind::SensitiveProfileMutation,
            "Profile change requires approval",
            None,
            None,
            approval_ts(),
            None,
        );

        assert!(card.data_exposure_summary.exposes_sensitive_profile);
        assert_eq!(card.side_effect_summary.risk_level, ApprovalRiskLevel::High);
    }

    #[test]
    fn approval_prompt_card_serde_snapshot_is_deterministic() {
        let request = approval_action_request(serde_json::json!({
            "url": "https://api.example.com/v1/messages",
            "recipient": "ops@example.com"
        }));
        let diff = ApprovalDiffSummary {
            before_label: Some("draft".to_string()),
            after_label: Some("sent".to_string()),
            summary: "Sends one email to operations.".to_string(),
            changed_fields: vec!["status".to_string()],
        };
        let card = ApprovalPromptCard::from_action_request(
            "approval-1",
            &request,
            "Send Mail",
            SideEffectKind::ExternalSystemMutation,
            "External send requires approval",
            Some(diff),
            None,
            approval_ts(),
            Some("2026-05-26T09:00:00Z".parse().unwrap()),
        );

        let json = serde_json::to_string_pretty(&card).unwrap();
        assert_eq!(
            json,
            r#"{
  "approval_id": "approval-1",
  "action_id": "action-approval-1",
  "action_kind": "mail.send",
  "action_display_name": "Send Mail",
  "conversation_id": "conversation-1",
  "requested_by": "agent-1",
  "reason": "External send requires approval",
  "side_effect_summary": {
    "side_effect": "external_system_mutation",
    "risk_level": "high",
    "summary": "High risk: modifies an external system."
  },
  "diff_summary": {
    "before_label": "draft",
    "after_label": "sent",
    "summary": "Sends one email to operations.",
    "changed_fields": [
      "status"
    ]
  },
  "data_exposure_summary": {
    "exposes_external_data": true,
    "exposes_sensitive_profile": false,
    "destinations": [
      "ops@example.com",
      "https://api.example.com/v1/messages"
    ],
    "summary": "May expose data to 2 destination(s)."
  },
  "policy_explanation": null,
  "created_at": "2026-05-26T08:00:00Z",
  "expires_at": "2026-05-26T09:00:00Z"
}"#
        );
    }

    #[test]
    fn approval_surface_descriptor_can_embed_prompt_card_metadata() {
        let request = approval_action_request(serde_json::json!({"recipient": "ops@example.com"}));
        let card = ApprovalPromptCard::from_action_request(
            "approval-1",
            &request,
            "Send Mail",
            SideEffectKind::ExternalSystemMutation,
            "External send requires approval",
            None,
            None,
            approval_ts(),
            None,
        );
        let descriptor = SurfaceDescriptor::new(
            "surface-approval-1",
            SurfaceKind::ApprovalSurface,
            SurfaceRendererHint::Form,
            approval_ts(),
        )
        .with_title("Approve Send Mail")
        .with_metadata(serde_json::to_value(&card).unwrap());

        assert_eq!(descriptor.kind, SurfaceKind::ApprovalSurface);
        assert_eq!(descriptor.renderer_hint, SurfaceRendererHint::Form);
        assert_eq!(descriptor.metadata["approval_id"], "approval-1");
        assert_eq!(
            descriptor.metadata["side_effect_summary"]["risk_level"],
            "high"
        );
    }
}
