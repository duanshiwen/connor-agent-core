use action_core::ActionId;
use chrono::Utc;
use conversation_core::{
    AgentRunStatus, ConversationActionStatus, ConversationId, MessageContent, MessageId,
    ParticipantId, Visibility,
};
use conversation_kernel::{
    AppendMessageCommand, ApproveActionCommand, DenyActionCommand, RequestAgentRunCommand,
    StartAgentRunCommand,
};
use enterprise_permission_core::{
    EnterpriseRole, EnterpriseUserId, PermissionAction, PermissionDecision, ResourceId,
    ResourceType,
};
use serde::{Deserialize, Serialize};

use crate::{KernelError, KernelRuntime};

pub type HostApiResult<T> = Result<T, HostApiError>;

#[derive(Debug, thiserror::Error)]
pub enum HostApiError {
    #[error("kernel operation failed: {reason}")]
    KernelOperationFailed { reason: String },

    #[error("run not found: {run_id}")]
    RunNotFound { run_id: String },

    #[error("permission store unavailable for permission-aware host request")]
    PermissionStoreUnavailable,

    #[error("permission denied: {actor} cannot {action} {resource_type}:{resource_id}")]
    PermissionDenied {
        actor: String,
        action: String,
        resource_type: String,
        resource_id: String,
    },
}

impl From<anyhow::Error> for HostApiError {
    fn from(value: anyhow::Error) -> Self {
        Self::KernelOperationFailed {
            reason: value.to_string(),
        }
    }
}

impl From<KernelError> for HostApiError {
    fn from(value: KernelError) -> Self {
        Self::KernelOperationFailed {
            reason: value.to_string(),
        }
    }
}

#[derive(Clone)]
pub struct KernelHostApi {
    runtime: KernelRuntime,
}

impl KernelHostApi {
    pub fn new(runtime: KernelRuntime) -> Self {
        Self { runtime }
    }

    pub fn runtime(&self) -> &KernelRuntime {
        &self.runtime
    }

    pub async fn submit_user_message(
        &self,
        request: SubmitUserMessageRequest,
    ) -> HostApiResult<SubmitUserMessageResponse> {
        self.require_permission(
            request.actor_context.as_ref(),
            ResourceType::Conversation,
            ResourceId(request.conversation_id.0.clone()),
            PermissionAction::Write,
        )?;

        let message_id = self
            .runtime
            .services()
            .conversation_kernel
            .append_message(AppendMessageCommand {
                conversation_id: request.conversation_id,
                sender_id: request.user_id,
                content: MessageContent::Text { text: request.text },
                reply_to: None,
                thread_id: None,
                visibility: Visibility::Conversation,
            })
            .await?;

        Ok(SubmitUserMessageResponse { message_id })
    }

    pub async fn start_agent_run(
        &self,
        request: StartAgentRunRequest,
    ) -> HostApiResult<StartAgentRunResponse> {
        self.require_permission(
            request.actor_context.as_ref(),
            ResourceType::Conversation,
            ResourceId(request.conversation_id.0.clone()),
            PermissionAction::Write,
        )?;

        let run_id = self
            .runtime
            .services()
            .conversation_kernel
            .request_agent_run(RequestAgentRunCommand {
                conversation_id: request.conversation_id.clone(),
                trigger_message_id: request.trigger_message_id,
                requested_by: request.requested_by.clone(),
            })
            .await?;

        self.runtime
            .services()
            .conversation_kernel
            .start_agent_run(StartAgentRunCommand {
                conversation_id: request.conversation_id,
                run_id: run_id.clone(),
                started_by: request.requested_by,
            })
            .await?;

        Ok(StartAgentRunResponse {
            run_id,
            status: HostRunStatus::Running,
        })
    }

    pub async fn get_run_status(
        &self,
        conversation_id: ConversationId,
        run_id: String,
    ) -> HostApiResult<HostRunStatusResponse> {
        let state = self
            .runtime
            .services()
            .conversation_kernel
            .load_state(&conversation_id)
            .await?;
        let run = state
            .agent_runs
            .get(&run_id)
            .ok_or_else(|| HostApiError::RunNotFound {
                run_id: run_id.clone(),
            })?;

        Ok(HostRunStatusResponse {
            run_id,
            status: HostRunStatus::from(&run.status),
        })
    }

    pub async fn list_pending_approvals(
        &self,
        conversation_id: ConversationId,
    ) -> HostApiResult<Vec<HostPendingApproval>> {
        let state = self
            .runtime
            .services()
            .conversation_kernel
            .load_state(&conversation_id)
            .await?;

        let mut approvals = state
            .actions
            .values()
            .filter(|action| action.status == ConversationActionStatus::ApprovalRequired)
            .map(|action| HostPendingApproval {
                action_id: action.action_id.clone(),
                action_kind: action.action_kind.0.clone(),
                reason: action.approval_required_reason.clone(),
            })
            .collect::<Vec<_>>();
        approvals.sort_by(|left, right| left.action_id.0.cmp(&right.action_id.0));

        Ok(approvals)
    }

    pub async fn approve_action(&self, request: HostActionDecisionRequest) -> HostApiResult<()> {
        self.require_permission(
            request.actor_context.as_ref(),
            ResourceType::Conversation,
            ResourceId(request.conversation_id.0.clone()),
            PermissionAction::Admin,
        )?;

        self.runtime
            .services()
            .conversation_kernel
            .approve_action(ApproveActionCommand {
                conversation_id: request.conversation_id,
                action_id: request.action_id,
                approved_by: request.decided_by,
            })
            .await?;
        Ok(())
    }

    pub async fn deny_action(&self, request: HostActionDecisionRequest) -> HostApiResult<()> {
        self.require_permission(
            request.actor_context.as_ref(),
            ResourceType::Conversation,
            ResourceId(request.conversation_id.0.clone()),
            PermissionAction::Admin,
        )?;

        self.runtime
            .services()
            .conversation_kernel
            .deny_action(DenyActionCommand {
                conversation_id: request.conversation_id,
                action_id: request.action_id,
                reason: request
                    .reason
                    .unwrap_or_else(|| "denied by host".to_string()),
                denied_by: Some(request.decided_by),
            })
            .await?;
        Ok(())
    }

    pub async fn shutdown(&self) -> HostApiResult<()> {
        self.runtime.shutdown()?;
        Ok(())
    }

    fn require_permission(
        &self,
        actor_context: Option<&HostActorContext>,
        resource_type: ResourceType,
        resource_id: ResourceId,
        action: PermissionAction,
    ) -> HostApiResult<()> {
        let Some(actor_context) = actor_context else {
            return Ok(());
        };
        let Some(permission_store) = self.runtime.services().permission_store.as_ref() else {
            return Err(HostApiError::PermissionStoreUnavailable);
        };

        let decision = permission_store.lock().unwrap().check_with_role(
            &actor_context.enterprise_user_id,
            actor_context.role,
            &resource_type,
            &resource_id,
            &action,
            Utc::now(),
        );
        if decision == PermissionDecision::Allow {
            Ok(())
        } else {
            Err(HostApiError::PermissionDenied {
                actor: actor_context.enterprise_user_id.to_string(),
                action: action.to_string(),
                resource_type: resource_type.to_string(),
                resource_id: resource_id.to_string(),
            })
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HostActorContext {
    pub user_id: ParticipantId,
    pub enterprise_user_id: EnterpriseUserId,
    pub role: EnterpriseRole,
}

impl HostActorContext {
    pub fn user(user_id: ParticipantId, enterprise_user_id: EnterpriseUserId) -> Self {
        Self {
            user_id,
            enterprise_user_id,
            role: EnterpriseRole::User,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HostPermissionResource {
    pub resource_type: ResourceType,
    pub resource_id: ResourceId,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SubmitUserMessageRequest {
    pub conversation_id: ConversationId,
    pub user_id: ParticipantId,
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub actor_context: Option<HostActorContext>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SubmitUserMessageResponse {
    pub message_id: MessageId,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StartAgentRunRequest {
    pub conversation_id: ConversationId,
    pub trigger_message_id: MessageId,
    pub requested_by: ParticipantId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub actor_context: Option<HostActorContext>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StartAgentRunResponse {
    pub run_id: String,
    pub status: HostRunStatus,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HostRunStatusResponse {
    pub run_id: String,
    pub status: HostRunStatus,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HostRunStatus {
    Queued,
    Running,
    WaitingForApproval,
    Completed,
    Failed,
    Cancelled,
    TimedOut,
}

impl From<&AgentRunStatus> for HostRunStatus {
    fn from(value: &AgentRunStatus) -> Self {
        match value {
            AgentRunStatus::Requested => Self::Queued,
            AgentRunStatus::Started => Self::Running,
            AgentRunStatus::Completed => Self::Completed,
            AgentRunStatus::Failed => Self::Failed,
            AgentRunStatus::Cancelled => Self::Cancelled,
            AgentRunStatus::TimedOut => Self::TimedOut,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HostPendingApproval {
    pub action_id: ActionId,
    pub action_kind: String,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HostActionDecisionRequest {
    pub conversation_id: ConversationId,
    pub action_id: ActionId,
    pub decided_by: ParticipantId,
    pub reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub actor_context: Option<HostActorContext>,
}
