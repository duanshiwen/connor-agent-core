use std::sync::Arc;

use action_core::ActionRegistry;
use audit_log::AuditLog;
use capability_policy::CapabilityPolicy;
use conversation_journal::ConversationJournal;
use conversation_kernel::ConversationKernel;
use model_adapter::ModelAdapter;

use crate::{KernelError, KernelResult, KernelRuntime, KernelServices};

#[derive(Default)]
pub struct KernelRuntimeBuilder {
    conversation_journal: Option<Arc<dyn ConversationJournal>>,
    model_adapter: Option<Arc<dyn ModelAdapter>>,
    action_registry: Option<Arc<ActionRegistry>>,
    capability_policy: Option<Arc<CapabilityPolicy>>,
    audit_log: Option<Arc<dyn AuditLog>>,
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
        };

        Ok(KernelRuntime::new(services))
    }
}
