use std::sync::{Arc, Mutex};

use action_core::ActionRegistry;
use agentos_storage::AgentOsStorage;
use audit_log::AuditLog;
use capability_policy::CapabilityPolicy;
use conversation_journal::ConversationJournal;
use conversation_kernel::ConversationKernel;
use enterprise_permission_core::PermissionStore;
use model_adapter::ModelAdapter;

use crate::{
    KernelError, KernelResult, KernelRuntime, KernelServices, PolicyProviderRegistry,
    StorageProviderRegistry,
};

#[derive(Default)]
pub struct KernelRuntimeBuilder {
    conversation_journal: Option<Arc<dyn ConversationJournal>>,
    model_adapter: Option<Arc<dyn ModelAdapter>>,
    action_registry: Option<Arc<ActionRegistry>>,
    capability_policy: Option<Arc<CapabilityPolicy>>,
    audit_log: Option<Arc<dyn AuditLog>>,
    permission_store: Option<Arc<Mutex<PermissionStore>>>,
    storage: Option<Arc<AgentOsStorage>>,
    storage_provider_registry: Option<Arc<StorageProviderRegistry>>,
    policy_provider_registry: Option<Arc<PolicyProviderRegistry>>,
}

impl KernelRuntimeBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn conversation_journal(mut self, journal: Arc<dyn ConversationJournal>) -> Self {
        self.conversation_journal = Some(journal);
        self
    }

    pub fn model_adapter(mut self, adapter: Arc<dyn ModelAdapter>) -> Self {
        self.model_adapter = Some(adapter);
        self
    }

    pub fn action_registry(mut self, registry: Arc<ActionRegistry>) -> Self {
        self.action_registry = Some(registry);
        self
    }

    pub fn capability_policy(mut self, policy: Arc<CapabilityPolicy>) -> Self {
        self.capability_policy = Some(policy);
        self
    }

    pub fn audit_log(mut self, audit_log: Arc<dyn AuditLog>) -> Self {
        self.audit_log = Some(audit_log);
        self
    }

    pub fn permission_store(mut self, permission_store: Arc<Mutex<PermissionStore>>) -> Self {
        self.permission_store = Some(permission_store);
        self
    }

    pub fn storage(mut self, storage: Arc<AgentOsStorage>) -> Self {
        self.storage = Some(storage);
        self
    }

    pub fn storage_provider_registry(mut self, registry: Arc<StorageProviderRegistry>) -> Self {
        self.storage_provider_registry = Some(registry);
        self
    }

    pub fn policy_provider_registry(mut self, registry: Arc<PolicyProviderRegistry>) -> Self {
        self.policy_provider_registry = Some(registry);
        self
    }

    pub fn enterprise_permission_store(
        self,
        permission_store: Arc<Mutex<PermissionStore>>,
    ) -> Self {
        self.permission_store(permission_store)
    }

    pub fn permission_store_value(mut self, permission_store: PermissionStore) -> Self {
        self.permission_store = Some(Arc::new(Mutex::new(permission_store)));
        self
    }

    pub fn build(self) -> KernelResult<KernelRuntime> {
        let conversation_journal =
            self.conversation_journal
                .ok_or(KernelError::MissingService {
                    service: "conversation_journal",
                })?;
        let model_adapter = self.model_adapter.ok_or(KernelError::MissingService {
            service: "model_adapter",
        })?;
        let action_registry = self.action_registry.ok_or(KernelError::MissingService {
            service: "action_registry",
        })?;
        let capability_policy = self.capability_policy.ok_or(KernelError::MissingService {
            service: "capability_policy",
        })?;
        let audit_log = self.audit_log.ok_or(KernelError::MissingService {
            service: "audit_log",
        })?;

        let services = KernelServices {
            conversation_kernel: Arc::new(ConversationKernel::new(conversation_journal)),
            model_adapter,
            action_registry,
            capability_policy,
            audit_log,
            permission_store: self.permission_store,
            storage: self.storage,
            storage_provider_registry: self.storage_provider_registry,
            policy_provider_registry: self.policy_provider_registry,
        };

        Ok(KernelRuntime::new(services))
    }
}
