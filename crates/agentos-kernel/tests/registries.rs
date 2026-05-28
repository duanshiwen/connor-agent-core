use std::sync::Arc;

use action_core::{ActionExecutor, ActionExecutorError, ActionRequest, ActionResult};
use agentos_kernel::{
    ActionExecutorRegistry, ConnectorRegistry, ConnectorService, KernelError,
    ModelProviderRegistry, PolicyProviderRegistry, PolicyProviderService, RepositoryRegistry,
    RepositoryService, StorageProviderRegistry, StorageProviderService,
};
use async_trait::async_trait;
use model_adapter::{ModelAdapter, StaticModelAdapter};

#[derive(Debug)]
struct DummyActionExecutor;

#[async_trait]
impl ActionExecutor for DummyActionExecutor {
    async fn execute(&self, _request: &ActionRequest) -> Result<ActionResult, ActionExecutorError> {
        Err(ActionExecutorError::ExecutionFailed(
            "dummy executor should not be called".to_string(),
        ))
    }
}

#[derive(Debug)]
struct DummyRepository;

impl RepositoryService for DummyRepository {}

#[derive(Debug)]
struct DummyConnector;

impl ConnectorService for DummyConnector {}

#[derive(Debug)]
struct DummyStorageProvider;

impl StorageProviderService for DummyStorageProvider {}

#[derive(Debug)]
struct DummyPolicyProvider;

impl PolicyProviderService for DummyPolicyProvider {}

fn assert_service_not_found<T>(
    result: Result<T, KernelError>,
    registry: &'static str,
    service_id: &str,
) {
    match result {
        Err(err) => assert_eq!(
            err,
            KernelError::ServiceNotFound {
                registry,
                service_id: service_id.to_string(),
            }
        ),
        Ok(_) => panic!("expected service not found for {registry}:{service_id}"),
    }
}

#[test]
fn model_provider_registry_returns_registered_provider() {
    let mut registry = ModelProviderRegistry::new();
    registry.register("test", Arc::new(StaticModelAdapter::default()));

    let provider: Arc<dyn ModelAdapter> = registry.get("test").unwrap();

    assert!(Arc::strong_count(&provider) >= 2);
    assert!(registry.contains("test"));
    assert_eq!(registry.len(), 1);
}

#[test]
fn model_provider_registry_unknown_provider_returns_typed_error() {
    let registry = ModelProviderRegistry::new();

    assert_service_not_found(registry.get("missing"), "model_provider", "missing");
}

#[test]
fn action_executor_registry_returns_registered_executor() {
    let mut registry = ActionExecutorRegistry::new();
    registry.register("browser.click", Arc::new(DummyActionExecutor));

    let executor: Arc<dyn ActionExecutor> = registry.get("browser.click").unwrap();

    assert!(Arc::strong_count(&executor) >= 2);
    assert!(registry.contains("browser.click"));
    assert_eq!(registry.len(), 1);
}

#[test]
fn action_executor_registry_unknown_executor_returns_typed_error() {
    let registry = ActionExecutorRegistry::new();

    assert_service_not_found(
        registry.get("missing.action"),
        "action_executor",
        "missing.action",
    );
}

#[test]
fn repository_registry_skeleton_returns_registered_repository() {
    let mut registry = RepositoryRegistry::new();
    registry.register("knowledge", Arc::new(DummyRepository));

    let repository: Arc<dyn RepositoryService> = registry.get("knowledge").unwrap();

    assert!(Arc::strong_count(&repository) >= 2);
    assert!(registry.contains("knowledge"));
    assert_eq!(registry.len(), 1);
}

#[test]
fn repository_registry_unknown_repository_returns_typed_error() {
    let registry = RepositoryRegistry::new();

    assert_service_not_found(registry.get("missing"), "repository", "missing");
}

#[test]
fn connector_registry_skeleton_returns_registered_connector() {
    let mut registry = ConnectorRegistry::new();
    registry.register("gmail", Arc::new(DummyConnector));

    let connector: Arc<dyn ConnectorService> = registry.get("gmail").unwrap();

    assert!(Arc::strong_count(&connector) >= 2);
    assert!(registry.contains("gmail"));
    assert_eq!(registry.len(), 1);
}

#[test]
fn connector_registry_unknown_connector_returns_typed_error() {
    let registry = ConnectorRegistry::new();

    assert_service_not_found(registry.get("missing"), "connector", "missing");
}

#[test]
fn storage_provider_registry_returns_registered_provider() {
    let mut registry = StorageProviderRegistry::new();
    registry.register("local", Arc::new(DummyStorageProvider));

    let provider: Arc<dyn StorageProviderService> = registry.get("local").unwrap();

    assert!(Arc::strong_count(&provider) >= 2);
    assert!(registry.contains("local"));
    assert_eq!(registry.len(), 1);
}

#[test]
fn storage_provider_registry_unknown_provider_returns_typed_error() {
    let registry = StorageProviderRegistry::new();

    assert_service_not_found(registry.get("missing"), "storage_provider", "missing");
}

#[test]
fn policy_provider_registry_returns_registered_provider() {
    let mut registry = PolicyProviderRegistry::new();
    registry.register("default", Arc::new(DummyPolicyProvider));

    let provider: Arc<dyn PolicyProviderService> = registry.get("default").unwrap();

    assert!(Arc::strong_count(&provider) >= 2);
    assert!(registry.contains("default"));
    assert_eq!(registry.len(), 1);
}

#[test]
fn policy_provider_registry_unknown_provider_returns_typed_error() {
    let registry = PolicyProviderRegistry::new();

    assert_service_not_found(registry.get("missing"), "policy_provider", "missing");
}
