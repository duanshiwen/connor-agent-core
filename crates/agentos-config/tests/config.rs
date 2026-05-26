use agentos_config::{AgentOsConfig, ConfigDiagnosticCode, MapEnvSource};

fn sample_config() -> &'static str {
    r#"
[kernel]
profile = "local"

[storage]
root = ".agentos"

[model]
default_provider = "openai"
default_model = "gpt-4o-mini"

[model.providers.openai]
provider = "openai"
endpoint = "https://api.openai.com/v1"
api_key = "sk-test-secret"
model = "gpt-4o-mini"
timeout_secs = 120

[policy]
mode = "ask"

[browser]
profile = "default"
allow_js = false
"#
}

#[test]
fn parses_agentos_toml() {
    let config = AgentOsConfig::from_toml_str(sample_config()).unwrap();

    assert_eq!(config.kernel.profile.as_deref(), Some("local"));
    assert_eq!(config.storage.root, ".agentos");
    assert_eq!(config.model.default_provider, "openai");
    assert_eq!(config.model.default_model, "gpt-4o-mini");
    assert_eq!(config.policy.mode, "ask");
    assert_eq!(config.browser.profile.as_deref(), Some("default"));

    let provider = config.model.providers.get("openai").unwrap();
    assert_eq!(provider.endpoint, "https://api.openai.com/v1");
    assert_eq!(provider.api_key.as_deref(), Some("sk-test-secret"));
    assert_eq!(provider.timeout_secs, Some(120));
}

#[test]
fn env_overlay_overrides_file_values() {
    let config = AgentOsConfig::from_toml_str(sample_config()).unwrap();
    let env = MapEnvSource::from_pairs([
        ("AGENTOS_PROFILE", "ci"),
        ("AGENTOS_STORAGE_ROOT", "/tmp/agentos"),
        ("AGENTOS_MODEL_DEFAULT_PROVIDER", "openai"),
        ("AGENTOS_MODEL_DEFAULT_MODEL", "gpt-4.1-mini"),
        ("OPENAI_ENDPOINT", "https://api.example.com/v1"),
        ("OPENAI_API_KEY", "sk-env-secret"),
        ("OPENAI_MODEL", "gpt-4.1-mini"),
        ("OPENAI_TIMEOUT_SECS", "60"),
    ]);

    let overlaid = config.apply_env_overlay(&env);
    let provider = overlaid.model.providers.get("openai").unwrap();

    assert_eq!(overlaid.kernel.profile.as_deref(), Some("ci"));
    assert_eq!(overlaid.storage.root, "/tmp/agentos");
    assert_eq!(overlaid.model.default_model, "gpt-4.1-mini");
    assert_eq!(provider.endpoint, "https://api.example.com/v1");
    assert_eq!(provider.api_key.as_deref(), Some("sk-env-secret"));
    assert_eq!(provider.model, "gpt-4.1-mini");
    assert_eq!(provider.timeout_secs, Some(60));
}

#[test]
fn invalid_config_returns_typed_diagnostics() {
    let config = AgentOsConfig::from_toml_str(
        r#"
[storage]
root = ""

[model]
default_provider = "missing"
default_model = ""

[model.providers.openai]
provider = "openai"
endpoint = ""
model = ""
timeout_secs = 0

[policy]
mode = "invalid"
"#,
    )
    .unwrap();

    let report = config.validate();
    assert!(!report.is_valid());

    let codes: Vec<_> = report.diagnostics.iter().map(|d| d.code).collect();
    assert!(codes.contains(&ConfigDiagnosticCode::StorageRootEmpty));
    assert!(codes.contains(&ConfigDiagnosticCode::DefaultProviderMissing));
    assert!(codes.contains(&ConfigDiagnosticCode::DefaultModelEmpty));
    assert!(codes.contains(&ConfigDiagnosticCode::ProviderEndpointEmpty));
    assert!(codes.contains(&ConfigDiagnosticCode::ProviderModelEmpty));
    assert!(codes.contains(&ConfigDiagnosticCode::ProviderTimeoutInvalid));
    assert!(codes.contains(&ConfigDiagnosticCode::PolicyModeInvalid));
}

#[test]
fn redacted_debug_does_not_leak_api_key() {
    let config = AgentOsConfig::from_toml_str(sample_config()).unwrap();

    let debug = format!("{config:?}");
    let redacted_debug = format!("{:?}", config.redacted());

    assert!(!debug.contains("sk-test-secret"));
    assert!(!redacted_debug.contains("sk-test-secret"));
    assert!(debug.contains("<redacted>"));
    assert!(redacted_debug.contains("<redacted>"));
}

#[test]
fn missing_default_provider_is_reported() {
    let config = AgentOsConfig::from_toml_str(
        r#"
[storage]
root = ".agentos"

[model]
default_provider = "anthropic"
default_model = "claude"

[model.providers.openai]
provider = "openai"
endpoint = "https://api.openai.com/v1"
model = "gpt-4o-mini"

[policy]
mode = "ask"
"#,
    )
    .unwrap();

    let report = config.validate();

    assert!(
        report
            .diagnostics
            .iter()
            .any(|d| d.code == ConfigDiagnosticCode::DefaultProviderMissing)
    );
}

#[test]
fn invalid_policy_mode_is_reported() {
    let config = AgentOsConfig::from_toml_str(sample_config()).unwrap();
    let mut config = config;
    config.policy.mode = "prompt".to_string();

    let report = config.validate();

    assert!(
        report
            .diagnostics
            .iter()
            .any(|d| d.code == ConfigDiagnosticCode::PolicyModeInvalid)
    );
}

#[test]
fn parses_from_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("agentos.toml");
    std::fs::write(&path, sample_config()).unwrap();

    let config = AgentOsConfig::from_file(&path).unwrap();

    assert_eq!(config.model.default_provider, "openai");
}
