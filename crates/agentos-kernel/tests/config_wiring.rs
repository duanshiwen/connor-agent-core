use std::collections::BTreeMap;
use std::sync::Arc;

use action_core::{ActionKind, ActionRequest, SideEffectKind};
use agentos_config::AgentOsConfig;
use agentos_kernel::KernelRuntimeBuilder;
use audit_log::{AuditLog, MemoryAuditSink};
use capability_policy::PolicyDecision;
use chrono::Utc;
use conversation_journal::{ConversationJournal, MemoryConversationJournal};
use model_adapter::{ModelAdapter, StaticModelAdapter};

fn config() -> AgentOsConfig {
    AgentOsConfig::from_toml_str(
        r#"
[storage]
root = ".agentos"

[model]
default_provider = "openai"
default_model = "gpt-4o-mini"

[model.providers.openai]
provider = "openai"
endpoint = "https://api.openai.com/v1"
api_key = "sk-kernel-secret"
model = "gpt-4o-mini"

[policy]
mode = "deny"

[actions]
default_policy_mode = "ask"

[browser]
profile = "work"
profile_policy = "enterprise_restricted"
allow_js = false

[connectors.gmail]
enabled = true
endpoint = "https://gmail.googleapis.com"
token = "gmail-kernel-secret"

[connectors.gmail.runtime]
isolation = "network_only"
rate_limit_per_minute = 60
health_check_interval_secs = 300
"#,
    )
    .unwrap()
}

fn runtime_from_config(config: AgentOsConfig) -> agentos_kernel::KernelRuntime {
    let journal: Arc<dyn ConversationJournal> = Arc::new(MemoryConversationJournal::new());
    let model_adapter: Arc<dyn ModelAdapter> = Arc::new(StaticModelAdapter::default());
    let audit_log: Arc<dyn AuditLog> = Arc::new(MemoryAuditSink::new());

    KernelRuntimeBuilder::new()
        .agentos_config(config)
        .conversation_journal(journal)
        .model_adapter(model_adapter)
        .action_registry(Arc::new(action_core::ActionRegistry::new()))
        .audit_log(audit_log)
        .build()
        .unwrap()
}

#[test]
fn builder_derives_capability_policy_from_agentos_config_when_policy_not_injected() {
    let runtime = runtime_from_config(config());
    let request = ActionRequest {
        action_id: action_core::ActionId::from("network-action"),
        action_kind: ActionKind::from("mail.send"),
        input: serde_json::json!({}),
        requested_by: "user-1".to_string(),
        conversation_id: None,
        message_id: None,
        requested_at: Utc::now(),
    };

    let decision = runtime
        .services()
        .capability_policy
        .evaluate(&request, &SideEffectKind::NetworkAccess);

    assert!(matches!(decision, PolicyDecision::Ask { .. }));
}

#[tokio::test]
async fn diagnostics_bundle_uses_stored_redacted_agentos_config_when_no_raw_values_are_given() {
    let runtime = runtime_from_config(config());

    let bundle = runtime.diagnostics_bundle(BTreeMap::new()).await.unwrap();
    let serialized_config = bundle.runtime_config.values.get("agentos_config").unwrap();

    assert!(serialized_config.contains("enterprise_restricted"));
    assert!(serialized_config.contains("network_only"));
    assert!(serialized_config.contains("<redacted>"));
    assert!(!serialized_config.contains("sk-kernel-secret"));
    assert!(!serialized_config.contains("gmail-kernel-secret"));
}
