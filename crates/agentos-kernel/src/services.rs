use std::sync::Arc;

use action_core::ActionRegistry;
use audit_log::AuditLog;
use capability_policy::CapabilityPolicy;
use conversation_kernel::ConversationKernel;
use model_adapter::ModelAdapter;

#[derive(Clone)]
pub struct KernelServices {
    pub conversation_kernel: Arc<ConversationKernel>,
    pub model_adapter: Arc<dyn ModelAdapter>,
    pub action_registry: Arc<ActionRegistry>,
    pub capability_policy: Arc<CapabilityPolicy>,
    pub audit_log: Arc<dyn AuditLog>,
}
