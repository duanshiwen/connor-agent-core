use std::collections::BTreeMap;
use std::sync::Arc;

use action_core::ActionRegistry;
use agentos_kernel::{KernelRuntime, KernelRuntimeBuilder, KernelRuntimeState};
use audit_log::{AuditEvent, AuditLog, MemoryAuditSink};
use capability_policy::CapabilityPolicy;
use chrono::Utc;
use conversation_journal::{ConversationJournal, MemoryConversationJournal};
use model_adapter::{FakeModelAdapter, ModelAdapter};

fn runtime() -> KernelRuntime {
    let journal: Arc<dyn ConversationJournal> = Arc::new(MemoryConversationJournal::new());
    let model_adapter: Arc<dyn ModelAdapter> = Arc::new(FakeModelAdapter::default());
    let audit_log: Arc<dyn AuditLog> = Arc::new(MemoryAuditSink::new());

    KernelRuntimeBuilder::new()
        .conversation_journal(journal)
        .model_adapter(model_adapter)
        .action_registry(Arc::new(ActionRegistry::new()))
        .capability_policy(Arc::new(CapabilityPolicy::default_safe()))
        .audit_log(audit_log)
        .build()
        .unwrap()
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
    assert!(json.contains("\"recent_audit_summary\""));
    assert!(json.contains("\"api_token\": \"<redacted>\""));
    assert!(!json.contains("token-value"));
}
