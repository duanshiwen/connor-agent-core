use std::collections::BTreeMap;
use std::sync::Arc;

use action_core::ActionExecutor;
use model_adapter::ModelAdapter;

use crate::{KernelError, KernelResult};

pub trait RepositoryService: Send + Sync {}

pub trait ConnectorService: Send + Sync {}

pub trait StorageProviderService: Send + Sync {}

pub trait PolicyProviderService: Send + Sync {}

#[derive(Default)]
pub struct ModelProviderRegistry {
    providers: BTreeMap<String, Arc<dyn ModelAdapter>>,
}

impl ModelProviderRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(&mut self, service_id: impl Into<String>, provider: Arc<dyn ModelAdapter>) {
        self.providers.insert(service_id.into(), provider);
    }

    pub fn get(&self, service_id: &str) -> KernelResult<Arc<dyn ModelAdapter>> {
        self.providers
            .get(service_id)
            .cloned()
            .ok_or_else(|| KernelError::ServiceNotFound {
                registry: "model_provider",
                service_id: service_id.to_string(),
            })
    }

    pub fn contains(&self, service_id: &str) -> bool {
        self.providers.contains_key(service_id)
    }

    pub fn len(&self) -> usize {
        self.providers.len()
    }

    pub fn is_empty(&self) -> bool {
        self.providers.is_empty()
    }
}

#[derive(Default)]
pub struct ActionExecutorRegistry {
    executors: BTreeMap<String, Arc<dyn ActionExecutor>>,
}

impl ActionExecutorRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(&mut self, service_id: impl Into<String>, executor: Arc<dyn ActionExecutor>) {
        self.executors.insert(service_id.into(), executor);
    }

    pub fn get(&self, service_id: &str) -> KernelResult<Arc<dyn ActionExecutor>> {
        self.executors
            .get(service_id)
            .cloned()
            .ok_or_else(|| KernelError::ServiceNotFound {
                registry: "action_executor",
                service_id: service_id.to_string(),
            })
    }

    pub fn contains(&self, service_id: &str) -> bool {
        self.executors.contains_key(service_id)
    }

    pub fn len(&self) -> usize {
        self.executors.len()
    }

    pub fn is_empty(&self) -> bool {
        self.executors.is_empty()
    }
}

#[derive(Default)]
pub struct RepositoryRegistry {
    repositories: BTreeMap<String, Arc<dyn RepositoryService>>,
}

impl RepositoryRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(
        &mut self,
        service_id: impl Into<String>,
        repository: Arc<dyn RepositoryService>,
    ) {
        self.repositories.insert(service_id.into(), repository);
    }

    pub fn get(&self, service_id: &str) -> KernelResult<Arc<dyn RepositoryService>> {
        self.repositories
            .get(service_id)
            .cloned()
            .ok_or_else(|| KernelError::ServiceNotFound {
                registry: "repository",
                service_id: service_id.to_string(),
            })
    }

    pub fn contains(&self, service_id: &str) -> bool {
        self.repositories.contains_key(service_id)
    }

    pub fn len(&self) -> usize {
        self.repositories.len()
    }

    pub fn is_empty(&self) -> bool {
        self.repositories.is_empty()
    }
}

#[derive(Default)]
pub struct ConnectorRegistry {
    connectors: BTreeMap<String, Arc<dyn ConnectorService>>,
}

impl ConnectorRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(
        &mut self,
        service_id: impl Into<String>,
        connector: Arc<dyn ConnectorService>,
    ) {
        self.connectors.insert(service_id.into(), connector);
    }

    pub fn get(&self, service_id: &str) -> KernelResult<Arc<dyn ConnectorService>> {
        self.connectors
            .get(service_id)
            .cloned()
            .ok_or_else(|| KernelError::ServiceNotFound {
                registry: "connector",
                service_id: service_id.to_string(),
            })
    }

    pub fn contains(&self, service_id: &str) -> bool {
        self.connectors.contains_key(service_id)
    }

    pub fn len(&self) -> usize {
        self.connectors.len()
    }

    pub fn is_empty(&self) -> bool {
        self.connectors.is_empty()
    }
}

#[derive(Default)]
pub struct StorageProviderRegistry {
    providers: BTreeMap<String, Arc<dyn StorageProviderService>>,
}

impl StorageProviderRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(
        &mut self,
        service_id: impl Into<String>,
        provider: Arc<dyn StorageProviderService>,
    ) {
        self.providers.insert(service_id.into(), provider);
    }

    pub fn get(&self, service_id: &str) -> KernelResult<Arc<dyn StorageProviderService>> {
        self.providers
            .get(service_id)
            .cloned()
            .ok_or_else(|| KernelError::ServiceNotFound {
                registry: "storage_provider",
                service_id: service_id.to_string(),
            })
    }

    pub fn contains(&self, service_id: &str) -> bool {
        self.providers.contains_key(service_id)
    }

    pub fn len(&self) -> usize {
        self.providers.len()
    }

    pub fn is_empty(&self) -> bool {
        self.providers.is_empty()
    }
}

#[derive(Default)]
pub struct PolicyProviderRegistry {
    providers: BTreeMap<String, Arc<dyn PolicyProviderService>>,
}

impl PolicyProviderRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(
        &mut self,
        service_id: impl Into<String>,
        provider: Arc<dyn PolicyProviderService>,
    ) {
        self.providers.insert(service_id.into(), provider);
    }

    pub fn get(&self, service_id: &str) -> KernelResult<Arc<dyn PolicyProviderService>> {
        self.providers
            .get(service_id)
            .cloned()
            .ok_or_else(|| KernelError::ServiceNotFound {
                registry: "policy_provider",
                service_id: service_id.to_string(),
            })
    }

    pub fn contains(&self, service_id: &str) -> bool {
        self.providers.contains_key(service_id)
    }

    pub fn len(&self) -> usize {
        self.providers.len()
    }

    pub fn is_empty(&self) -> bool {
        self.providers.is_empty()
    }
}
