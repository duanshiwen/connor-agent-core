use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};

use crate::{KernelError, KernelResult, KernelServices};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KernelRuntimeState {
    New,
    Initialized,
    Started,
    Recovering,
    ShuttingDown,
    Shutdown,
}

impl KernelRuntimeState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::New => "new",
            Self::Initialized => "initialized",
            Self::Started => "started",
            Self::Recovering => "recovering",
            Self::ShuttingDown => "shutting_down",
            Self::Shutdown => "shutdown",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KernelHealthReport {
    pub state: KernelRuntimeState,
    pub healthy: bool,
    pub conversation_kernel_available: bool,
    pub model_adapter_available: bool,
    pub action_registry_available: bool,
    pub capability_policy_available: bool,
    pub audit_log_available: bool,
    pub permission_store_available: bool,
}

#[derive(Clone)]
pub struct KernelRuntime {
    services: KernelServices,
    state: Arc<Mutex<KernelRuntimeState>>,
}

impl KernelRuntime {
    pub(crate) fn new(services: KernelServices) -> Self {
        Self {
            services,
            state: Arc::new(Mutex::new(KernelRuntimeState::New)),
        }
    }

    pub fn services(&self) -> &KernelServices {
        &self.services
    }

    pub fn state(&self) -> KernelRuntimeState {
        *self.state.lock().expect("kernel runtime state poisoned")
    }

    pub fn init(&self) -> KernelResult<()> {
        let mut state = self.state.lock().expect("kernel runtime state poisoned");
        match *state {
            KernelRuntimeState::New => {
                *state = KernelRuntimeState::Initialized;
                Ok(())
            }
            KernelRuntimeState::Initialized | KernelRuntimeState::Started => Ok(()),
            KernelRuntimeState::Recovering => {
                *state = KernelRuntimeState::Initialized;
                Ok(())
            }
            KernelRuntimeState::ShuttingDown | KernelRuntimeState::Shutdown => {
                Err(KernelError::InvalidLifecycleTransition {
                    from: state.as_str(),
                    to: KernelRuntimeState::Initialized.as_str(),
                })
            }
        }
    }

    pub fn start(&self) -> KernelResult<()> {
        let mut state = self.state.lock().expect("kernel runtime state poisoned");
        match *state {
            KernelRuntimeState::New | KernelRuntimeState::Initialized => {
                *state = KernelRuntimeState::Started;
                Ok(())
            }
            KernelRuntimeState::Started => Ok(()),
            KernelRuntimeState::Recovering => {
                *state = KernelRuntimeState::Started;
                Ok(())
            }
            KernelRuntimeState::ShuttingDown | KernelRuntimeState::Shutdown => {
                Err(KernelError::InvalidLifecycleTransition {
                    from: state.as_str(),
                    to: KernelRuntimeState::Started.as_str(),
                })
            }
        }
    }

    pub fn shutdown(&self) -> KernelResult<()> {
        let mut state = self.state.lock().expect("kernel runtime state poisoned");
        match *state {
            KernelRuntimeState::Shutdown => Ok(()),
            KernelRuntimeState::New
            | KernelRuntimeState::Initialized
            | KernelRuntimeState::Started
            | KernelRuntimeState::Recovering => {
                *state = KernelRuntimeState::ShuttingDown;
                *state = KernelRuntimeState::Shutdown;
                Ok(())
            }
            KernelRuntimeState::ShuttingDown => {
                *state = KernelRuntimeState::Shutdown;
                Ok(())
            }
        }
    }

    pub fn recover(&self) -> KernelResult<()> {
        let mut state = self.state.lock().expect("kernel runtime state poisoned");
        match *state {
            KernelRuntimeState::New
            | KernelRuntimeState::Initialized
            | KernelRuntimeState::Started
            | KernelRuntimeState::Recovering => {
                *state = KernelRuntimeState::Recovering;
                *state = KernelRuntimeState::Initialized;
                Ok(())
            }
            KernelRuntimeState::ShuttingDown | KernelRuntimeState::Shutdown => {
                Err(KernelError::InvalidLifecycleTransition {
                    from: state.as_str(),
                    to: KernelRuntimeState::Recovering.as_str(),
                })
            }
        }
    }

    pub async fn diagnostics_bundle(
        &self,
        runtime_config: BTreeMap<String, String>,
    ) -> KernelResult<crate::KernelDiagnosticsBundle> {
        if runtime_config.is_empty()
            && let Some(config) = self.services.runtime_config.as_ref()
        {
            let runtime_config = crate::RedactedRuntimeConfig::from_redacted_agentos_config(
                config.as_ref().clone(),
            )?;
            return crate::diagnostics::build_diagnostics_bundle_from_redacted_config(
                runtime_config,
                self.health_check(),
                self.services.audit_log.as_ref(),
                self.services.storage.as_deref(),
            )
            .await;
        }

        crate::diagnostics::build_diagnostics_bundle(
            runtime_config,
            self.health_check(),
            self.services.audit_log.as_ref(),
            self.services.storage.as_deref(),
        )
        .await
    }

    pub async fn diagnostics_bundle_for_config(
        &self,
        config: &agentos_config::AgentOsConfig,
    ) -> KernelResult<crate::KernelDiagnosticsBundle> {
        let runtime_config = crate::RedactedRuntimeConfig::from_agentos_config(config)?;
        crate::diagnostics::build_diagnostics_bundle_from_redacted_config(
            runtime_config,
            self.health_check(),
            self.services.audit_log.as_ref(),
            self.services.storage.as_deref(),
        )
        .await
    }

    pub fn health_check(&self) -> KernelHealthReport {
        let state = self.state();
        let conversation_kernel_available =
            Arc::strong_count(&self.services.conversation_kernel) > 0;
        let model_adapter_available = Arc::strong_count(&self.services.model_adapter) > 0;
        let action_registry_available = Arc::strong_count(&self.services.action_registry) > 0;
        let capability_policy_available = Arc::strong_count(&self.services.capability_policy) > 0;
        let audit_log_available = Arc::strong_count(&self.services.audit_log) > 0;
        let permission_store_available = self.services.permission_store.is_some();
        let healthy = state != KernelRuntimeState::Recovering
            && state != KernelRuntimeState::ShuttingDown
            && state != KernelRuntimeState::Shutdown
            && conversation_kernel_available
            && model_adapter_available
            && action_registry_available
            && capability_policy_available
            && audit_log_available;

        KernelHealthReport {
            state,
            healthy,
            conversation_kernel_available,
            model_adapter_available,
            action_registry_available,
            capability_policy_available,
            audit_log_available,
            permission_store_available,
        }
    }
}
