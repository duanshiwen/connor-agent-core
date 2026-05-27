//! Commercial client substrate facade for AgentOS.
//!
//! This crate is the narrow integration surface that a native desktop/mobile
//! client should depend on. It wraps the lower-level kernel/runtime crates with
//! typed commands, UI-safe events, projection models, and conservative safety
//! defaults.

use std::sync::{Arc, Mutex};

use action_core::{ActionId, ActionRegistry, ActionRequest, ActionSchema, SideEffectKind};
use agentos_kernel::{
    HostActionDecisionRequest, HostApiError, HostApiErrorResponse, HostPendingApproval,
    HostRunStatus, KernelHostApi, KernelRuntime, KernelRuntimeBuilder, StartAgentRunRequest,
    SubmitUserMessageRequest,
};
use audit_log::MemoryAuditSink;
use capability_policy::{CapabilityPolicy, PolicyRule, PolicyRuleDecision};
use chrono::{DateTime, Utc};
use conversation_core::{
    ConversationId, ConversationKind, MessageId, Participant, ParticipantId, ParticipantKind,
};
use conversation_journal::MemoryConversationJournal;
use conversation_kernel::CreateConversationCommand;
use model_adapter::FakeModelAdapter;
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Stable client profile identifier supplied by the host product.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ClientProfileId(pub String);

impl From<&str> for ClientProfileId {
    fn from(value: &str) -> Self {
        Self(value.to_string())
    }
}

/// Stable client workspace identifier supplied by the host product.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ClientWorkspaceId(pub String);

impl From<&str> for ClientWorkspaceId {
    fn from(value: &str) -> Self {
        Self(value.to_string())
    }
}

/// Runtime status surfaced to the client shell.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ClientRuntimeStatus {
    Ready,
    Recovering,
    ShuttingDown,
}

/// High-level client commands accepted by the substrate facade.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ClientCommand {
    CreateConversation {
        title: Option<String>,
        user_id: ParticipantId,
        agent_id: ParticipantId,
    },
    SubmitUserMessage {
        conversation_id: ConversationId,
        user_id: ParticipantId,
        text: String,
    },
    StartAgentRun {
        conversation_id: ConversationId,
        trigger_message_id: MessageId,
        requested_by: ParticipantId,
    },
    ApproveAction {
        conversation_id: ConversationId,
        action_id: ActionId,
        decided_by: ParticipantId,
    },
    DenyAction {
        conversation_id: ConversationId,
        action_id: ActionId,
        decided_by: ParticipantId,
        reason: Option<String>,
    },
}

/// Command result returned to host UI code.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ClientCommandResult {
    ConversationCreated {
        conversation_id: ConversationId,
    },
    UserMessageSubmitted {
        message_id: MessageId,
    },
    AgentRunStarted {
        run_id: String,
        status: HostRunStatus,
    },
    ActionApproved {
        action_id: ActionId,
    },
    ActionDenied {
        action_id: ActionId,
    },
}

/// UI-safe event emitted by the substrate.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ClientEvent {
    RuntimeStatusChanged { status: ClientRuntimeStatus },
    ConversationCreated { summary: ClientConversationSummary },
    TimelineItemAppended { item: ClientTimelineItem },
    AgentRunChanged { summary: ClientRunSummary },
    ApprovalRequested { card: ClientApprovalCard },
    ErrorRaised { banner: ClientErrorBanner },
}

/// Conversation row projection for a client sidebar/list.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClientConversationSummary {
    pub conversation_id: ConversationId,
    pub title: Option<String>,
    pub participant_count: usize,
    pub updated_at: DateTime<Utc>,
}

/// Timeline projection for client rendering.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClientTimelineItem {
    pub conversation_id: ConversationId,
    pub item_id: String,
    pub actor_id: Option<ParticipantId>,
    pub text: String,
    pub created_at: DateTime<Utc>,
}

/// Agent run row/card projection.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClientRunSummary {
    pub conversation_id: ConversationId,
    pub run_id: String,
    pub status: HostRunStatus,
}

/// Human approval card projection.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClientApprovalCard {
    pub conversation_id: ConversationId,
    pub action_id: ActionId,
    pub title: String,
    pub risk_level: ClientRiskLevel,
    pub affected_resource: Option<String>,
    pub reversible: bool,
    pub reason: Option<String>,
}

impl ClientApprovalCard {
    pub fn from_pending(conversation_id: ConversationId, approval: HostPendingApproval) -> Self {
        let risk_level = ClientRiskLevel::from_action_kind(&approval.action_kind);
        Self {
            conversation_id,
            title: format!("Approve {}", approval.action_kind),
            action_id: approval.action_id,
            risk_level,
            affected_resource: Some(approval.action_kind),
            reversible: risk_level.is_reversible_by_default(),
            reason: approval.reason,
        }
    }
}

/// UI-safe error banner projection.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClientErrorBanner {
    pub code: String,
    pub message: String,
    pub user_actionable: bool,
}

impl From<&HostApiError> for ClientErrorBanner {
    fn from(value: &HostApiError) -> Self {
        let response = HostApiErrorResponse::from(value);
        Self {
            code: response.code,
            message: response.message,
            user_actionable: matches!(
                response.category,
                agentos_kernel::KernelErrorCategory::UserActionable
            ),
        }
    }
}

/// Client-visible risk level for approval UI.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ClientRiskLevel {
    Low,
    Medium,
    High,
    Critical,
}

impl ClientRiskLevel {
    fn from_side_effect(side_effect: &SideEffectKind) -> Self {
        match side_effect {
            SideEffectKind::None | SideEffectKind::ReadOnly => Self::Low,
            SideEffectKind::RuntimeStateMutation | SideEffectKind::UiSideEffect => Self::Medium,
            SideEffectKind::FileSystemMutation | SideEffectKind::NetworkAccess => Self::High,
            SideEffectKind::ExternalSystemMutation
            | SideEffectKind::DeviceControl
            | SideEffectKind::SensitiveProfileMutation => Self::Critical,
        }
    }

    fn from_action_kind(action_kind: &str) -> Self {
        if action_kind.starts_with("browser.")
            || action_kind.contains("send")
            || action_kind.contains("delete")
        {
            Self::Critical
        } else if action_kind.contains("write") || action_kind.contains("create") {
            Self::High
        } else {
            Self::Medium
        }
    }

    fn is_reversible_by_default(self) -> bool {
        matches!(self, Self::Low | Self::Medium)
    }
}

/// Commercial-client safety posture.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClientSafetyProfile {
    pub id: String,
    pub browser_broad_automation_enabled: bool,
    pub connector_write_enabled: bool,
    pub irreversible_actions_require_approval: bool,
    pub external_mutations_require_approval: bool,
}

impl ClientSafetyProfile {
    pub fn personal_local_default() -> Self {
        Self {
            id: "personal_local_default".to_string(),
            browser_broad_automation_enabled: false,
            connector_write_enabled: false,
            irreversible_actions_require_approval: true,
            external_mutations_require_approval: true,
        }
    }

    pub fn enterprise_managed_default() -> Self {
        Self {
            id: "enterprise_managed_default".to_string(),
            browser_broad_automation_enabled: false,
            connector_write_enabled: false,
            irreversible_actions_require_approval: true,
            external_mutations_require_approval: true,
        }
    }

    pub fn to_capability_policy(&self) -> CapabilityPolicy {
        let mut rules = vec![
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
                side_effect: SideEffectKind::UiSideEffect,
                decision: PolicyRuleDecision::Ask,
            },
            PolicyRule {
                side_effect: SideEffectKind::FileSystemMutation,
                decision: PolicyRuleDecision::Ask,
            },
            PolicyRule {
                side_effect: SideEffectKind::NetworkAccess,
                decision: PolicyRuleDecision::Ask,
            },
            PolicyRule {
                side_effect: SideEffectKind::ExternalSystemMutation,
                decision: PolicyRuleDecision::Ask,
            },
            PolicyRule {
                side_effect: SideEffectKind::DeviceControl,
                decision: PolicyRuleDecision::Deny,
            },
            PolicyRule {
                side_effect: SideEffectKind::SensitiveProfileMutation,
                decision: PolicyRuleDecision::Ask,
            },
        ];

        if !self.external_mutations_require_approval {
            rules.push(PolicyRule {
                side_effect: SideEffectKind::ExternalSystemMutation,
                decision: PolicyRuleDecision::Allow,
            });
        }

        CapabilityPolicy::new(rules, PolicyRuleDecision::Ask)
    }

    pub fn approval_card_for_schema(
        &self,
        conversation_id: ConversationId,
        action_id: ActionId,
        schema: &ActionSchema,
        reason: Option<String>,
    ) -> ClientApprovalCard {
        let risk_level = ClientRiskLevel::from_side_effect(&schema.side_effect);
        ClientApprovalCard {
            conversation_id,
            action_id,
            title: schema.display_name.clone(),
            risk_level,
            affected_resource: Some(schema.kind.0.clone()),
            reversible: risk_level.is_reversible_by_default(),
            reason,
        }
    }
}

/// Local data root selected by the host product.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClientDataRoot {
    pub path: String,
}

/// Profile manifest stored by the host product next to local client data.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClientProfileManifest {
    pub profile_id: ClientProfileId,
    pub schema_version: u32,
    pub created_at: DateTime<Utc>,
    pub last_opened_at: DateTime<Utc>,
}

/// Workspace manifest stored by the host product next to local workspace data.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClientWorkspaceManifest {
    pub workspace_id: ClientWorkspaceId,
    pub schema_version: u32,
    pub storage_layout_version: u32,
    pub dirty_shutdown: bool,
}

/// UI-safe storage health report.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClientStorageHealthReport {
    pub profile_id: ClientProfileId,
    pub workspace_id: ClientWorkspaceId,
    pub healthy: bool,
    pub requires_migration: bool,
    pub dirty_shutdown_recovered: bool,
    pub issues: Vec<String>,
}

/// Backup plan metadata. It intentionally avoids secret material and raw paths.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClientBackupPlan {
    pub include_conversation_journal: bool,
    pub include_artifacts: bool,
    pub include_credentials: bool,
    pub secret_redaction_required: bool,
}

impl Default for ClientBackupPlan {
    fn default() -> Self {
        Self {
            include_conversation_journal: true,
            include_artifacts: true,
            include_credentials: false,
            secret_redaction_required: true,
        }
    }
}

/// Repair plan surfaced to UI before mutating local data.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClientRepairPlan {
    pub requires_user_confirmation: bool,
    pub preserves_original_data: bool,
    pub steps: Vec<String>,
}

/// Migration report emitted after opening or upgrading a workspace.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClientMigrationReport {
    pub from_schema_version: u32,
    pub to_schema_version: u32,
    pub applied: bool,
    pub rollback_available: bool,
}

/// Host-provided credential backend contract.
#[async_trait::async_trait]
pub trait ClientCredentialBackend: Send + Sync {
    async fn write_secret(&self, key: &str, secret: &str) -> Result<(), ClientCredentialError>;
    async fn read_secret(&self, key: &str) -> Result<Option<String>, ClientCredentialError>;
    async fn delete_secret(&self, key: &str) -> Result<(), ClientCredentialError>;
}

/// System credential backend selected by the host application.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SystemCredentialBackendKind {
    MacOsKeychain,
    WindowsCredentialManager,
    LinuxSecretService,
    BackendService,
    InMemoryTest,
}

/// Credential access audit metadata. Never include secret values here.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CredentialAccessAuditEvent {
    pub backend: SystemCredentialBackendKind,
    pub key_hash: String,
    pub operation: CredentialAccessOperation,
    pub secret_material_present: bool,
    pub occurred_at: DateTime<Utc>,
}

/// Credential access operation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CredentialAccessOperation {
    Read,
    Write,
    Delete,
}

/// Secret redaction check report for logs/debug bundles.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SecretRedactionReport {
    pub scanned_fields: usize,
    pub redacted_fields: usize,
    pub secret_material_detected: bool,
}

impl SecretRedactionReport {
    pub fn assert_safe_for_export(&self) -> Result<(), ClientCredentialError> {
        if self.secret_material_detected {
            Err(ClientCredentialError::SecretMaterialDetected)
        } else {
            Ok(())
        }
    }
}

#[derive(Debug, Error)]
pub enum ClientCredentialError {
    #[error("credential backend unavailable")]
    BackendUnavailable,
    #[error("secret material detected in export-safe payload")]
    SecretMaterialDetected,
    #[error("credential operation failed: {reason}")]
    OperationFailed { reason: String },
}

/// Client release channel selected by host update infrastructure.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ClientReleaseChannel {
    Dev,
    Beta,
    Stable,
    Enterprise,
}

/// Host-renderable update policy.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClientUpdatePolicy {
    pub channel: ClientReleaseChannel,
    pub auto_update_enabled: bool,
    pub staged_rollout_enabled: bool,
    pub rollback_supported: bool,
}

/// Crash report policy with privacy-safe defaults.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClientCrashReportPolicy {
    pub enabled: bool,
    pub include_pii: bool,
    pub include_secret_material: bool,
    pub requires_user_consent: bool,
}

impl Default for ClientCrashReportPolicy {
    fn default() -> Self {
        Self {
            enabled: false,
            include_pii: false,
            include_secret_material: false,
            requires_user_consent: true,
        }
    }
}

/// Diagnostic bundle plan. Host code turns this into concrete files.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClientDiagnosticBundlePlan {
    pub include_logs: bool,
    pub include_audit_metadata: bool,
    pub include_config: bool,
    pub include_credentials: bool,
    pub secret_scan_required: bool,
    pub expiration_hours: u32,
}

impl Default for ClientDiagnosticBundlePlan {
    fn default() -> Self {
        Self {
            include_logs: true,
            include_audit_metadata: true,
            include_config: true,
            include_credentials: false,
            secret_scan_required: true,
            expiration_hours: 72,
        }
    }
}

/// Telemetry consent state owned by the host product.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClientTelemetryConsent {
    pub telemetry_enabled: bool,
    pub crash_reports_enabled: bool,
    pub product_analytics_enabled: bool,
}

/// Builder for [`ClientSubstrate`].
pub struct ClientSubstrateBuilder {
    profile_id: ClientProfileId,
    workspace_id: ClientWorkspaceId,
    safety_profile: ClientSafetyProfile,
    action_registry: ActionRegistry,
}

impl Default for ClientSubstrateBuilder {
    fn default() -> Self {
        Self {
            profile_id: ClientProfileId("default".to_string()),
            workspace_id: ClientWorkspaceId("local".to_string()),
            safety_profile: ClientSafetyProfile::personal_local_default(),
            action_registry: ActionRegistry::new(),
        }
    }
}

impl ClientSubstrateBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn profile_id(mut self, profile_id: ClientProfileId) -> Self {
        self.profile_id = profile_id;
        self
    }

    pub fn workspace_id(mut self, workspace_id: ClientWorkspaceId) -> Self {
        self.workspace_id = workspace_id;
        self
    }

    pub fn safety_profile(mut self, safety_profile: ClientSafetyProfile) -> Self {
        self.safety_profile = safety_profile;
        self
    }

    pub fn action_registry(mut self, action_registry: ActionRegistry) -> Self {
        self.action_registry = action_registry;
        self
    }

    pub fn build(self) -> Result<ClientSubstrate, ClientSubstrateError> {
        let action_registry = Arc::new(self.action_registry);
        let runtime = KernelRuntimeBuilder::new()
            .conversation_journal(Arc::new(MemoryConversationJournal::new()))
            .model_adapter(Arc::new(FakeModelAdapter::default()))
            .action_registry(Arc::clone(&action_registry))
            .capability_policy(Arc::new(self.safety_profile.to_capability_policy()))
            .audit_log(Arc::new(MemoryAuditSink::new()))
            .build()
            .map_err(|source| ClientSubstrateError::BuildFailed {
                reason: source.to_string(),
            })?;

        Ok(ClientSubstrate::new(
            self.profile_id,
            self.workspace_id,
            self.safety_profile,
            action_registry,
            runtime,
        ))
    }
}

/// Commercial client facade.
#[derive(Clone)]
pub struct ClientSubstrate {
    profile_id: ClientProfileId,
    workspace_id: ClientWorkspaceId,
    safety_profile: ClientSafetyProfile,
    action_registry: Arc<ActionRegistry>,
    host_api: KernelHostApi,
    events: Arc<Mutex<Vec<ClientEvent>>>,
}

impl ClientSubstrate {
    pub fn builder() -> ClientSubstrateBuilder {
        ClientSubstrateBuilder::new()
    }

    pub fn new(
        profile_id: ClientProfileId,
        workspace_id: ClientWorkspaceId,
        safety_profile: ClientSafetyProfile,
        action_registry: Arc<ActionRegistry>,
        runtime: KernelRuntime,
    ) -> Self {
        let events = Arc::new(Mutex::new(vec![ClientEvent::RuntimeStatusChanged {
            status: ClientRuntimeStatus::Ready,
        }]));
        Self {
            profile_id,
            workspace_id,
            safety_profile,
            action_registry,
            host_api: KernelHostApi::new(runtime),
            events,
        }
    }

    pub fn profile_id(&self) -> &ClientProfileId {
        &self.profile_id
    }

    pub fn workspace_id(&self) -> &ClientWorkspaceId {
        &self.workspace_id
    }

    pub fn safety_profile(&self) -> &ClientSafetyProfile {
        &self.safety_profile
    }

    pub fn storage_health_report(&self) -> ClientStorageHealthReport {
        ClientStorageHealthReport {
            profile_id: self.profile_id.clone(),
            workspace_id: self.workspace_id.clone(),
            healthy: true,
            requires_migration: false,
            dirty_shutdown_recovered: false,
            issues: Vec::new(),
        }
    }

    pub fn default_backup_plan(&self) -> ClientBackupPlan {
        ClientBackupPlan::default()
    }

    pub fn default_repair_plan(&self) -> ClientRepairPlan {
        ClientRepairPlan {
            requires_user_confirmation: true,
            preserves_original_data: true,
            steps: vec![
                "pause client runtime".to_string(),
                "copy current data root".to_string(),
                "verify journal and storage manifests".to_string(),
                "re-open workspace read-only before write access".to_string(),
            ],
        }
    }

    pub fn default_diagnostic_bundle_plan(&self) -> ClientDiagnosticBundlePlan {
        ClientDiagnosticBundlePlan::default()
    }

    pub fn default_crash_report_policy(&self) -> ClientCrashReportPolicy {
        ClientCrashReportPolicy::default()
    }

    pub fn default_telemetry_consent(&self) -> ClientTelemetryConsent {
        ClientTelemetryConsent::default()
    }

    pub fn drain_events(&self) -> Vec<ClientEvent> {
        let mut events = self.events.lock().unwrap();
        std::mem::take(&mut *events)
    }

    pub async fn dispatch(
        &self,
        command: ClientCommand,
    ) -> Result<ClientCommandResult, ClientSubstrateError> {
        match command {
            ClientCommand::CreateConversation {
                title,
                user_id,
                agent_id,
            } => self.create_conversation(title, user_id, agent_id).await,
            ClientCommand::SubmitUserMessage {
                conversation_id,
                user_id,
                text,
            } => {
                self.submit_user_message(conversation_id, user_id, text)
                    .await
            }
            ClientCommand::StartAgentRun {
                conversation_id,
                trigger_message_id,
                requested_by,
            } => {
                self.start_agent_run(conversation_id, trigger_message_id, requested_by)
                    .await
            }
            ClientCommand::ApproveAction {
                conversation_id,
                action_id,
                decided_by,
            } => {
                self.approve_action(conversation_id, action_id, decided_by)
                    .await
            }
            ClientCommand::DenyAction {
                conversation_id,
                action_id,
                decided_by,
                reason,
            } => {
                self.deny_action(conversation_id, action_id, decided_by, reason)
                    .await
            }
        }
    }

    pub async fn create_conversation(
        &self,
        title: Option<String>,
        user_id: ParticipantId,
        agent_id: ParticipantId,
    ) -> Result<ClientCommandResult, ClientSubstrateError> {
        let participants = vec![
            Participant {
                id: user_id.clone(),
                kind: ParticipantKind::Human,
                display_name: user_id.0.clone(),
            },
            Participant {
                id: agent_id.clone(),
                kind: ParticipantKind::Agent,
                display_name: agent_id.0.clone(),
            },
        ];
        let conversation_id = self
            .host_api
            .runtime()
            .services()
            .conversation_kernel
            .create_conversation(CreateConversationCommand {
                kind: ConversationKind::AgentTask,
                title: title.clone(),
                participants,
                actor_id: Some(user_id),
            })
            .await
            .map_err(ClientSubstrateError::from_anyhow)?;

        let state = self
            .host_api
            .runtime()
            .services()
            .conversation_kernel
            .load_state(&conversation_id)
            .await
            .map_err(ClientSubstrateError::from_anyhow)?;
        let session = state
            .session
            .expect("created conversation must have session");
        self.push_event(ClientEvent::ConversationCreated {
            summary: ClientConversationSummary {
                conversation_id: conversation_id.clone(),
                title,
                participant_count: session.participants.len(),
                updated_at: session.updated_at,
            },
        });

        Ok(ClientCommandResult::ConversationCreated { conversation_id })
    }

    pub async fn submit_user_message(
        &self,
        conversation_id: ConversationId,
        user_id: ParticipantId,
        text: String,
    ) -> Result<ClientCommandResult, ClientSubstrateError> {
        let response = self
            .host_api
            .submit_user_message(SubmitUserMessageRequest {
                conversation_id: conversation_id.clone(),
                user_id: user_id.clone(),
                text: text.clone(),
                actor_context: None,
            })
            .await
            .map_err(ClientSubstrateError::from_host_api)?;

        self.push_event(ClientEvent::TimelineItemAppended {
            item: ClientTimelineItem {
                conversation_id,
                item_id: response.message_id.0.clone(),
                actor_id: Some(user_id),
                text,
                created_at: Utc::now(),
            },
        });

        Ok(ClientCommandResult::UserMessageSubmitted {
            message_id: response.message_id,
        })
    }

    pub async fn start_agent_run(
        &self,
        conversation_id: ConversationId,
        trigger_message_id: MessageId,
        requested_by: ParticipantId,
    ) -> Result<ClientCommandResult, ClientSubstrateError> {
        let response = self
            .host_api
            .start_agent_run(StartAgentRunRequest {
                conversation_id: conversation_id.clone(),
                trigger_message_id,
                requested_by,
                actor_context: None,
            })
            .await
            .map_err(ClientSubstrateError::from_host_api)?;

        self.push_event(ClientEvent::AgentRunChanged {
            summary: ClientRunSummary {
                conversation_id,
                run_id: response.run_id.clone(),
                status: response.status.clone(),
            },
        });

        Ok(ClientCommandResult::AgentRunStarted {
            run_id: response.run_id,
            status: response.status,
        })
    }

    pub async fn pending_approval_cards(
        &self,
        conversation_id: ConversationId,
    ) -> Result<Vec<ClientApprovalCard>, ClientSubstrateError> {
        let approvals = self
            .host_api
            .list_pending_approvals(conversation_id.clone())
            .await
            .map_err(ClientSubstrateError::from_host_api)?;
        Ok(approvals
            .into_iter()
            .map(|approval| ClientApprovalCard::from_pending(conversation_id.clone(), approval))
            .collect())
    }

    pub fn approval_card_for_request(
        &self,
        conversation_id: ConversationId,
        request: &ActionRequest,
        reason: Option<String>,
    ) -> ClientApprovalCard {
        if let Some(schema) = self.action_registry.get(&request.action_kind) {
            self.safety_profile.approval_card_for_schema(
                conversation_id,
                request.action_id.clone(),
                schema,
                reason,
            )
        } else {
            ClientApprovalCard {
                conversation_id,
                action_id: request.action_id.clone(),
                title: format!("Approve {}", request.action_kind.0),
                risk_level: ClientRiskLevel::from_action_kind(&request.action_kind.0),
                affected_resource: Some(request.action_kind.0.clone()),
                reversible: false,
                reason,
            }
        }
    }

    pub async fn approve_action(
        &self,
        conversation_id: ConversationId,
        action_id: ActionId,
        decided_by: ParticipantId,
    ) -> Result<ClientCommandResult, ClientSubstrateError> {
        self.host_api
            .approve_action(HostActionDecisionRequest {
                conversation_id,
                action_id: action_id.clone(),
                decided_by,
                reason: None,
                actor_context: None,
            })
            .await
            .map_err(ClientSubstrateError::from_host_api)?;
        Ok(ClientCommandResult::ActionApproved { action_id })
    }

    pub async fn deny_action(
        &self,
        conversation_id: ConversationId,
        action_id: ActionId,
        decided_by: ParticipantId,
        reason: Option<String>,
    ) -> Result<ClientCommandResult, ClientSubstrateError> {
        self.host_api
            .deny_action(HostActionDecisionRequest {
                conversation_id,
                action_id: action_id.clone(),
                decided_by,
                reason,
                actor_context: None,
            })
            .await
            .map_err(ClientSubstrateError::from_host_api)?;
        Ok(ClientCommandResult::ActionDenied { action_id })
    }

    fn push_event(&self, event: ClientEvent) {
        self.events.lock().unwrap().push(event);
    }
}

/// Error type suitable for host UI boundaries.
#[derive(Debug, Error)]
pub enum ClientSubstrateError {
    #[error("client substrate build failed: {reason}")]
    BuildFailed { reason: String },
    #[error("client command failed: {banner:?}")]
    CommandFailed { banner: ClientErrorBanner },
}

impl ClientSubstrateError {
    fn from_host_api(error: HostApiError) -> Self {
        Self::CommandFailed {
            banner: ClientErrorBanner::from(&error),
        }
    }

    fn from_anyhow(error: anyhow::Error) -> Self {
        Self::BuildFailed {
            reason: error.to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use action_core::ActionKind;
    use capability_policy::PolicyDecision;
    use chrono::Utc;

    fn user() -> ParticipantId {
        ParticipantId::from("user-1")
    }

    fn agent() -> ParticipantId {
        ParticipantId::from("agent-1")
    }

    #[tokio::test]
    async fn client_substrate_can_create_conversation_and_start_run() {
        let substrate = ClientSubstrate::builder().build().unwrap();
        let conversation = substrate
            .dispatch(ClientCommand::CreateConversation {
                title: Some("Commercial client".to_string()),
                user_id: user(),
                agent_id: agent(),
            })
            .await
            .unwrap();
        let conversation_id = match conversation {
            ClientCommandResult::ConversationCreated { conversation_id } => conversation_id,
            other => panic!("unexpected result: {other:?}"),
        };

        let message = substrate
            .dispatch(ClientCommand::SubmitUserMessage {
                conversation_id: conversation_id.clone(),
                user_id: user(),
                text: "hello".to_string(),
            })
            .await
            .unwrap();
        let message_id = match message {
            ClientCommandResult::UserMessageSubmitted { message_id } => message_id,
            other => panic!("unexpected result: {other:?}"),
        };

        let run = substrate
            .dispatch(ClientCommand::StartAgentRun {
                conversation_id: conversation_id.clone(),
                trigger_message_id: message_id,
                requested_by: user(),
            })
            .await
            .unwrap();

        assert!(matches!(
            run,
            ClientCommandResult::AgentRunStarted {
                status: HostRunStatus::Running,
                ..
            }
        ));
    }

    #[tokio::test]
    async fn client_substrate_emits_typed_events_for_ui() {
        let substrate = ClientSubstrate::builder().build().unwrap();
        let result = substrate
            .create_conversation(Some("Events".to_string()), user(), agent())
            .await
            .unwrap();
        let conversation_id = match result {
            ClientCommandResult::ConversationCreated { conversation_id } => conversation_id,
            other => panic!("unexpected result: {other:?}"),
        };
        substrate
            .submit_user_message(conversation_id, user(), "event text".to_string())
            .await
            .unwrap();

        let events = substrate.drain_events();
        assert!(matches!(
            events.first(),
            Some(ClientEvent::RuntimeStatusChanged {
                status: ClientRuntimeStatus::Ready
            })
        ));
        assert!(events.iter().any(|event| matches!(
            event,
            ClientEvent::ConversationCreated { summary }
                if summary.title.as_deref() == Some("Events")
        )));
        assert!(events.iter().any(|event| matches!(
            event,
            ClientEvent::TimelineItemAppended { item } if item.text == "event text"
        )));
    }

    #[test]
    fn client_safety_profile_blocks_high_risk_defaults() {
        let profile = ClientSafetyProfile::personal_local_default();
        assert!(!profile.browser_broad_automation_enabled);
        assert!(!profile.connector_write_enabled);
        assert!(profile.irreversible_actions_require_approval);
        assert!(profile.external_mutations_require_approval);

        let policy = profile.to_capability_policy();
        let request = ActionRequest {
            action_id: ActionId::from("action-policy"),
            action_kind: ActionKind::from("test.action"),
            input: serde_json::json!({}),
            requested_by: "agent-1".to_string(),
            conversation_id: None,
            message_id: None,
            requested_at: Utc::now(),
        };
        assert_eq!(
            policy.evaluate(&request, &SideEffectKind::ReadOnly),
            PolicyDecision::Allow
        );
        assert!(matches!(
            policy.evaluate(&request, &SideEffectKind::ExternalSystemMutation),
            PolicyDecision::Ask { .. }
        ));
        assert!(matches!(
            policy.evaluate(&request, &SideEffectKind::DeviceControl),
            PolicyDecision::Deny { .. }
        ));
    }

    #[test]
    fn data_lifecycle_contract_is_ui_safe() {
        let substrate = ClientSubstrate::builder()
            .profile_id(ClientProfileId::from("profile-a"))
            .workspace_id(ClientWorkspaceId::from("workspace-a"))
            .build()
            .unwrap();

        let health = substrate.storage_health_report();
        assert!(health.healthy);
        assert!(!health.requires_migration);
        assert!(health.issues.is_empty());

        let backup = substrate.default_backup_plan();
        assert!(backup.include_conversation_journal);
        assert!(!backup.include_credentials);
        assert!(backup.secret_redaction_required);

        let repair = substrate.default_repair_plan();
        assert!(repair.requires_user_confirmation);
        assert!(repair.preserves_original_data);
        assert!(!repair.steps.is_empty());
    }

    #[test]
    fn credential_and_diagnostic_contracts_are_secret_safe_by_default() {
        let audit = CredentialAccessAuditEvent {
            backend: SystemCredentialBackendKind::MacOsKeychain,
            key_hash: "hash-only".to_string(),
            operation: CredentialAccessOperation::Read,
            secret_material_present: false,
            occurred_at: Utc::now(),
        };
        assert!(!audit.secret_material_present);

        let redaction = SecretRedactionReport {
            scanned_fields: 3,
            redacted_fields: 1,
            secret_material_detected: false,
        };
        redaction.assert_safe_for_export().unwrap();

        let substrate = ClientSubstrate::builder().build().unwrap();
        let diagnostics = substrate.default_diagnostic_bundle_plan();
        assert!(!diagnostics.include_credentials);
        assert!(diagnostics.secret_scan_required);

        let crash = substrate.default_crash_report_policy();
        assert!(!crash.enabled);
        assert!(!crash.include_pii);
        assert!(!crash.include_secret_material);
        assert!(crash.requires_user_consent);

        let telemetry = substrate.default_telemetry_consent();
        assert!(!telemetry.telemetry_enabled);
        assert!(!telemetry.crash_reports_enabled);
        assert!(!telemetry.product_analytics_enabled);
    }

    #[test]
    fn approval_card_contains_user_understandable_metadata() {
        let mut registry = ActionRegistry::new();
        registry
            .register(ActionSchema {
                kind: ActionKind::from("mail.send"),
                display_name: "Send email".to_string(),
                description: "Sends an outbound email".to_string(),
                side_effect: SideEffectKind::ExternalSystemMutation,
                input_schema: None,
                output_schema: None,
            })
            .unwrap();
        let substrate = ClientSubstrate::builder()
            .action_registry(registry)
            .build()
            .unwrap();
        let request = ActionRequest {
            action_id: ActionId::from("action-1"),
            action_kind: ActionKind::from("mail.send"),
            input: serde_json::json!({"to":"redacted@example.com"}),
            requested_by: "agent-1".to_string(),
            conversation_id: Some("conv-1".to_string()),
            message_id: None,
            requested_at: Utc::now(),
        };

        let card = substrate.approval_card_for_request(
            ConversationId::from("conv-1"),
            &request,
            Some("External email send requires approval".to_string()),
        );

        assert_eq!(card.title, "Send email");
        assert_eq!(card.risk_level, ClientRiskLevel::Critical);
        assert!(!card.reversible);
        assert_eq!(card.affected_resource.as_deref(), Some("mail.send"));
        assert_eq!(
            card.reason.as_deref(),
            Some("External email send requires approval")
        );
    }
}
