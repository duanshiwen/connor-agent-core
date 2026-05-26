use std::sync::{Arc, Mutex};

use action_core::ActionRegistry;
use agentos_storage::AgentOsStorage;
use audit_log::AuditLog;
use capability_policy::CapabilityPolicy;
use conversation_kernel::ConversationKernel;
use enterprise_permission_core::PermissionStore;
use model_adapter::ModelAdapter;

use crate::{PolicyProviderRegistry, StorageProviderRegistry};

#[derive(Clone)]
pub struct KernelServices {
    pub conversation_kernel: Arc<ConversationKernel>,
    pub model_adapter: Arc<dyn ModelAdapter>,
    pub action_registry: Arc<ActionRegistry>,
    pub capability_policy: Arc<CapabilityPolicy>,
    pub audit_log: Arc<dyn AuditLog>,
    pub permission_store: Option<Arc<Mutex<PermissionStore>>>,
    pub storage: Option<Arc<AgentOsStorage>>,
    pub storage_provider_registry: Option<Arc<StorageProviderRegistry>>,
    pub policy_provider_registry: Option<Arc<PolicyProviderRegistry>>,
}
