use std::collections::BTreeMap;
use std::sync::Arc;

use action_core::ActionRegistry;
use agentos_config::{AgentOsConfig, ConnectorConfig, ModelProviderConfig};
use agentos_kernel::{KernelRuntime, KernelRuntimeBuilder, KernelRuntimeState};
use agentos_storage::{AgentOsStorage, STORAGE_LAYOUT_VERSION};
use audit_log::{AuditEvent, AuditLog, MemoryAuditSink};
use capability_policy::CapabilityPolicy;
use chrono::Utc;
use conversation_journal::{ConversationJournal, MemoryConversationJournal};
use model_adapter::{FakeModelAdapter, ModelAdapter};

fn runtime() -> KernelRuntime {
    runtime_with_storage(None)
}

fn runtime_with_storage(storage: Option<Arc<AgentOsStorage>>) -> KernelRuntime {
    let journal: Arc<dyn ConversationJournal> = Arc::new(MemoryConversationJournal::new());
    let model_adapter: Arc<dyn ModelAdapter> = Arc::new(FakeModelAdapter::default());
    let audit_log: Arc<dyn AuditLog> = Arc::new(MemoryAuditSink::new());

    let mut builder = KernelRuntimeBuilder::new()
        .conversation_journal(journal)
        .model_adapter(model_adapter)
        .action_registry(Arc::new(ActionRegistry::new()))
        .capability_policy(Arc::new(CapabilityPolicy::default_safe()))
        .audit_log(audit_log);

    if let Some(storage) = storage {
        builder = builder.storage(storage);
    }

    builder.build().unwrap()
}

fn audit_event(action_id: &str) -> AuditEvent {
    AuditEvent {
        audit_id: format!("audit-{action_id}"),
        action_id: action_id.to_string(),
        action_kind: "knowledge.search".to_string(),
        requested_by: "user-1".to_string(),
        approved_by: None,
        input_summary: "query: diagnostics".to_string(),
        side_effect: "read_only".to_string(),
        policy_decision: "allow".to_string(),
        result_status: "completed".to_string(),
        result_summary: Some("ok".to_string()),
        conversation_id: Some("conversation-1".to_string()),
        message_id: Some("message-1".to_string()),
        timestamp: Utc::now(),
    }
}

#[tokio::test]
async fn diagnostics_bundle_redacts_sensitive_config_and_reports_health() {
    let runtime = runtime();
    runtime.start().unwrap();

    let mut config = BTreeMap::new();
    config.insert("OPENAI_API_KEY".to_string(), "sk-secret".to_string());
    config.insert("DATABASE_PASSWORD".to_string(), "password".to_string());
    config.insert("profile".to_string(), "local-dev".to_string());

    let bundle = runtime.diagnostics_bundle(config).await.unwrap();

    assert_eq!(bundle.service_health.state, KernelRuntimeState::Started);
    assert!(bundle.service_health.healthy);
    assert_eq!(
        bundle.runtime_config.values.get("OPENAI_API_KEY"),
        Some(&"<redacted>".to_string())
    );
    assert_eq!(
        bundle.runtime_config.values.get("DATABASE_PASSWORD"),
        Some(&"<redacted>".to_string())
    );
    assert_eq!(
        bundle.runtime_config.values.get("profile"),
        Some(&"local-dev".to_string())
    );
}

#[tokio::test]
async fn diagnostics_bundle_includes_storage_manifest_placeholder_and_audit_summary() {
    let runtime = runtime();
    runtime
        .services()
        .audit_log
        .record(audit_event("action-1"))
        .await
        .unwrap();

    let bundle = runtime.diagnostics_bundle(BTreeMap::new()).await.unwrap();

    assert_eq!(bundle.storage_manifest.status, "not_configured");
    assert_eq!(bundle.recent_audit_summary.total_events, 1);
    assert_eq!(bundle.recent_audit_summary.recent_events.len(), 1);
    assert_eq!(
        bundle.recent_audit_summary.recent_events[0].audit_id,
        "audit-action-1"
    );
    assert_eq!(
        bundle.recent_audit_summary.recent_events[0].policy_decision,
        "allow"
    );
}

#[tokio::test]
async fn diagnostics_bundle_reports_configured_storage_manifest() {
    let temp_dir = tempfile::tempdir().unwrap();
    let storage = Arc::new(AgentOsStorage::init(temp_dir.path()).unwrap());
    let expected_root = storage.root().display().to_string();
    let runtime = runtime_with_storage(Some(storage));

    let bundle = runtime.diagnostics_bundle(BTreeMap::new()).await.unwrap();

    assert_eq!(bundle.storage_manifest.status, "configured");
    assert_eq!(bundle.storage_manifest.storage_root, Some(expected_root));
    assert_eq!(
        bundle.storage_manifest.manifest_version,
        Some(STORAGE_LAYOUT_VERSION)
    );
}

#[tokio::test]
async fn diagnostics_bundle_for_config_redacts_typed_agentos_config() {
    let runtime = runtime();
    let mut config = AgentOsConfig::default();
    config.model.default_provider = "openai".to_string();
    config.model.default_model = "gpt-4.1".to_string();
    config.model.providers.insert(
        "openai".to_string(),
        ModelProviderConfig {
            provider: "openai".to_string(),
            endpoint: "https://api.openai.example/v1".to_string(),
            api_key: Some("sk-typed-secret".to_string()),
            model: "gpt-4.1".to_string(),
            timeout_secs: Some(30),
        },
    );
    config.connectors.insert(
        "github".to_string(),
        ConnectorConfig {
            enabled: true,
            endpoint: Some("https://api.github.example".to_string()),
            token: Some("ghp-typed-secret".to_string()),
            ..ConnectorConfig::default()
        },
    );

    let bundle = runtime
        .diagnostics_bundle_for_config(&config)
        .await
        .unwrap();
    let json = serde_json::to_string_pretty(&bundle).unwrap();

    assert_eq!(
        bundle.runtime_config.values.get("agentos_config_valid"),
        Some(&"true".to_string())
    );
    assert_eq!(
        bundle
            .runtime_config
            .values
            .get("agentos_config_error_count"),
        Some(&"0".to_string())
    );
    assert!(json.contains("<redacted>"));
    assert!(!json.contains("sk-typed-secret"));
    assert!(!json.contains("ghp-typed-secret"));
}

#[tokio::test]
async fn diagnostics_bundle_for_invalid_config_reports_error_count() {
    let runtime = runtime();
    let mut config = AgentOsConfig::default();
    config.model.default_provider = "missing".to_string();

    let bundle = runtime
        .diagnostics_bundle_for_config(&config)
        .await
        .unwrap();

    assert_eq!(
        bundle.runtime_config.values.get("agentos_config_valid"),
        Some(&"false".to_string())
    );
    assert_eq!(
        bundle
            .runtime_config
            .values
            .get("agentos_config_error_count"),
        Some(&"1".to_string())
    );
}

#[tokio::test]
async fn diagnostics_bundle_reports_ok_failure_summary_for_healthy_runtime() {
    let runtime = runtime();
    runtime.start().unwrap();

    let bundle = runtime.diagnostics_bundle(BTreeMap::new()).await.unwrap();

    assert_eq!(bundle.failure_summary.status, "ok");
    assert!(bundle.failure_summary.classifications.is_empty());
}

#[tokio::test]
async fn diagnostics_bundle_classifies_shutdown_runtime_as_unavailable() {
    let runtime = runtime();
    runtime.shutdown().unwrap();

    let bundle = runtime.diagnostics_bundle(BTreeMap::new()).await.unwrap();

    assert_eq!(bundle.failure_summary.status, "unavailable");
    assert!(
        bundle
            .failure_summary
            .classifications
            .iter()
            .any(|classification| classification.code == "kernel_not_running"
                && classification.severity == "error")
    );
}

#[tokio::test]
async fn diagnostics_bundle_has_deterministic_snapshot_shape() {
    let runtime = runtime();
    runtime.init().unwrap();

    let mut config = BTreeMap::new();
    config.insert("api_token".to_string(), "token-value".to_string());
    config.insert("profile".to_string(), "test".to_string());

    let bundle = runtime.diagnostics_bundle(config).await.unwrap();
    let json = serde_json::to_string_pretty(&bundle).unwrap();

    assert!(json.contains("\"runtime_config\""));
    assert!(json.contains("\"service_health\""));
    assert!(json.contains("\"storage_manifest\""));
    assert!(json.contains("\"failure_summary\""));
    assert!(json.contains("\"recent_audit_summary\""));
    assert!(json.contains("\"api_token\": \"<redacted>\""));
    assert!(!json.contains("token-value"));
}
