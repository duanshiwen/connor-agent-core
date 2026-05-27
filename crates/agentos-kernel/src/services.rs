use std::sync::{Arc, Mutex};

use action_core::{ActionExecutor, ActionRegistry};
use action_runtime::{
    ActionRuntime, ActionRuntimeOutcome, ArtifactResolver, ExecuteApprovedActionRequest,
    ProcessActionRequest,
};
use agentos_config::RedactedAgentOsConfig;
use agentos_storage::AgentOsStorage;
use audit_log::AuditLog;
use capability_policy::CapabilityPolicy;
use conversation_kernel::ConversationKernel;
use enterprise_permission_core::PermissionStore;
use model_adapter::ModelAdapter;

use crate::{PolicyProviderRegistry, StorageProviderRegistry};

/// Kernel-owned action runtime service.
///
/// This service keeps action runtime composition inside `agentos-kernel` instead
/// of requiring hosts to manually assemble `ActionRuntime<'_>` with borrowed
/// conversation, policy, executor, audit, and registry dependencies.
pub struct KernelActionRuntime {
    conversation_kernel: Arc<ConversationKernel>,
    action_registry: Arc<ActionRegistry>,
    capability_policy: Arc<CapabilityPolicy>,
    action_executor: Arc<dyn ActionExecutor>,
    audit_log: Arc<dyn AuditLog>,
    artifact_resolver: Option<Arc<dyn ArtifactResolver>>,
}

impl KernelActionRuntime {
    pub fn new(
        conversation_kernel: Arc<ConversationKernel>,
        action_registry: Arc<ActionRegistry>,
        capability_policy: Arc<CapabilityPolicy>,
        action_executor: Arc<dyn ActionExecutor>,
        audit_log: Arc<dyn AuditLog>,
        artifact_resolver: Option<Arc<dyn ArtifactResolver>>,
    ) -> Self {
        Self {
            conversation_kernel,
            action_registry,
            capability_policy,
            action_executor,
            audit_log,
            artifact_resolver,
        }
    }

    pub async fn process(
        &self,
        request: ProcessActionRequest<'_>,
    ) -> anyhow::Result<ActionRuntimeOutcome> {
        self.borrowed_runtime().process(request).await
    }

    pub async fn execute_approved(
        &self,
        request: ExecuteApprovedActionRequest<'_>,
    ) -> anyhow::Result<ActionRuntimeOutcome> {
        self.borrowed_runtime().execute_approved(request).await
    }

    fn borrowed_runtime(&self) -> ActionRuntime<'_> {
        ActionRuntime {
            kernel: self.conversation_kernel.as_ref(),
            registry: self.action_registry.as_ref(),
            policy: self.capability_policy.as_ref(),
            executor: self.action_executor.as_ref(),
            audit_log: self.audit_log.as_ref(),
            artifact_resolver: self.artifact_resolver.as_deref(),
        }
    }
}

#[derive(Clone)]
pub struct KernelServices {
    pub conversation_kernel: Arc<ConversationKernel>,
    pub model_adapter: Arc<dyn ModelAdapter>,
    pub action_registry: Arc<ActionRegistry>,
    pub capability_policy: Arc<CapabilityPolicy>,
    pub audit_log: Arc<dyn AuditLog>,
    pub action_runtime: Option<Arc<KernelActionRuntime>>,
    pub permission_store: Option<Arc<Mutex<PermissionStore>>>,
    pub storage: Option<Arc<AgentOsStorage>>,
    pub storage_provider_registry: Option<Arc<StorageProviderRegistry>>,
    pub policy_provider_registry: Option<Arc<PolicyProviderRegistry>>,
    pub runtime_config: Option<Arc<RedactedAgentOsConfig>>,
}
