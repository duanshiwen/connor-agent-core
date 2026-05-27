use agentos_config::{
    AgentOsConfig, AgentOsConfigDocument, BuiltinProfile, CURRENT_CONFIG_VERSION,
    ConfigDiagnosticCode, ConfigError, MapEnvSource,
};

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
profile_policy = "isolated"
allow_js = false

[actions]
default_policy_mode = "inherit"

[actions.per_action."mail.send"]
mode = "ask"

[connectors.gmail]
enabled = true
endpoint = "https://gmail.googleapis.com"
token = "gmail-secret"

[connectors.gmail.runtime]
isolation = "network_only"
rate_limit_per_minute = 60
health_check_interval_secs = 300
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
    assert_eq!(config.browser.profile_policy, "isolated");
    assert_eq!(config.actions.default_policy_mode, "inherit");
    assert_eq!(
        config.actions.per_action.get("mail.send").unwrap().mode,
        "ask"
    );
    let gmail = config.connectors.get("gmail").unwrap();
    assert!(gmail.enabled);
    assert_eq!(gmail.runtime.isolation, "network_only");
    assert_eq!(gmail.runtime.rate_limit_per_minute, Some(60));
    assert_eq!(gmail.runtime.health_check_interval_secs, Some(300));

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
fn invalid_runtime_config_returns_typed_diagnostics() {
    let config = AgentOsConfig::from_toml_str(
        r#"
[storage]
root = ".agentos"

[model]
default_provider = "openai"
default_model = "gpt-4o-mini"

[model.providers.openai]
provider = "openai"
endpoint = "https://api.openai.com/v1"
model = "gpt-4o-mini"

[actions]
default_policy_mode = "sometimes"

[actions.per_action.""]
mode = "prompt"

[browser]
profile_policy = "shared"

[connectors.gmail]
enabled = true

[connectors.gmail.runtime]
isolation = "vm"
rate_limit_per_minute = 0
health_check_interval_secs = 0
"#,
    )
    .unwrap();

    let report = config.validate();
    let codes: Vec<_> = report.diagnostics.iter().map(|d| d.code).collect();

    assert!(codes.contains(&ConfigDiagnosticCode::ActionDefaultPolicyModeInvalid));
    assert!(codes.contains(&ConfigDiagnosticCode::ActionKindEmpty));
    assert!(codes.contains(&ConfigDiagnosticCode::ActionPolicyModeInvalid));
    assert!(codes.contains(&ConfigDiagnosticCode::BrowserProfilePolicyInvalid));
    assert!(codes.contains(&ConfigDiagnosticCode::ConnectorEndpointMissing));
    assert!(codes.contains(&ConfigDiagnosticCode::ConnectorIsolationInvalid));
    assert!(codes.contains(&ConfigDiagnosticCode::ConnectorRateLimitInvalid));
    assert!(codes.contains(&ConfigDiagnosticCode::ConnectorHealthCheckIntervalInvalid));
}

#[test]
fn redacted_debug_does_not_leak_api_key() {
    let config = AgentOsConfig::from_toml_str(sample_config()).unwrap();

    let debug = format!("{config:?}");
    let redacted_debug = format!("{:?}", config.redacted());

    assert!(!debug.contains("sk-test-secret"));
    assert!(!redacted_debug.contains("sk-test-secret"));
    assert!(!redacted_debug.contains("gmail-secret"));
    assert!(debug.contains("<redacted>"));
    assert!(redacted_debug.contains("<redacted>"));
    assert!(redacted_debug.contains("network_only"));
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

fn profile_document() -> &'static str {
    r#"
version = 1

[kernel]
profile = "dev"

[storage]
root = ".agentos"

[model]
default_provider = "openai"
default_model = "gpt-4o-mini"

[model.providers.openai]
provider = "openai"
endpoint = "https://api.openai.com/v1"
api_key = "sk-profile-secret"
model = "gpt-4o-mini"
timeout_secs = 120

[policy]
mode = "ask"

[browser]
profile = "default"
allow_js = false

[profiles.local.storage]
root = ".agentos-local"

[profiles.local.browser]
profile = "local-browser"
profile_policy = "ephemeral"

[profiles.local.actions]
default_policy_mode = "ask"

[profiles.local.connectors.gmail]
enabled = true
endpoint = "https://gmail.googleapis.com"

[profiles.local.connectors.gmail.runtime]
isolation = "network_only"
rate_limit_per_minute = 30

[profiles.dev]
extends = "local"

[profiles.dev.storage]
root = ".agentos-dev"

[profiles.dev.policy]
mode = "allow"

[profiles.dev.model]
default_model = "gpt-4.1-mini"

[profiles.dev.model.providers.openai]
model = "gpt-4.1-mini"
timeout_secs = 60

[profiles.test]
extends = "local"

[profiles.test.storage]
root = ".agentos-test"

[profiles.enterprise.policy]
mode = "deny"
"#
}

#[test]
fn profile_override_is_deterministic() {
    let document = AgentOsConfigDocument::from_toml_str(profile_document()).unwrap();

    let resolved = document.resolve_profile("dev").unwrap();
    let provider = resolved.model.providers.get("openai").unwrap();

    assert_eq!(resolved.storage.root, ".agentos-dev");
    assert_eq!(resolved.policy.mode, "allow");
    assert_eq!(resolved.browser.profile.as_deref(), Some("local-browser"));
    assert_eq!(resolved.browser.profile_policy, "ephemeral");
    assert_eq!(resolved.actions.default_policy_mode, "ask");
    assert_eq!(
        resolved.connectors.get("gmail").unwrap().runtime.isolation,
        "network_only"
    );
    assert_eq!(resolved.model.default_model, "gpt-4.1-mini");
    assert_eq!(provider.endpoint, "https://api.openai.com/v1");
    assert_eq!(provider.api_key.as_deref(), Some("sk-profile-secret"));
    assert_eq!(provider.model, "gpt-4.1-mini");
    assert_eq!(provider.timeout_secs, Some(60));

    let resolved_again = document.resolve_profile("dev").unwrap();
    assert_eq!(resolved_again, resolved);
}

#[test]
fn resolve_selected_profile_uses_kernel_profile() {
    let document = AgentOsConfigDocument::from_toml_str(profile_document()).unwrap();

    let resolved = document.resolve_selected_profile().unwrap();

    assert_eq!(resolved.kernel.profile.as_deref(), Some("dev"));
    assert_eq!(resolved.storage.root, ".agentos-dev");
}

#[test]
fn profile_inheritance_chain_applies_parent_before_child() {
    let document = AgentOsConfigDocument::from_toml_str(profile_document()).unwrap();

    let resolved = document.resolve_profile("test").unwrap();

    assert_eq!(resolved.storage.root, ".agentos-test");
    assert_eq!(resolved.browser.profile.as_deref(), Some("local-browser"));
    assert_eq!(resolved.policy.mode, "ask");
}

#[test]
fn profile_cycle_returns_typed_error() {
    let document = AgentOsConfigDocument::from_toml_str(
        r#"
[storage]
root = ".agentos"

[model]
default_provider = "openai"
default_model = "gpt-4o-mini"

[model.providers.openai]
provider = "openai"
endpoint = "https://api.openai.com/v1"
model = "gpt-4o-mini"

[profiles.a]
extends = "b"

[profiles.b]
extends = "a"
"#,
    )
    .unwrap();

    let error = document.resolve_profile("a").unwrap_err();

    match error {
        ConfigError::ProfileCycle { profile_chain } => {
            assert_eq!(profile_chain, vec!["a", "b", "a"]);
        }
        other => panic!("expected profile cycle error, got {other:?}"),
    }
}

#[test]
fn missing_profile_returns_typed_error() {
    let document = AgentOsConfigDocument::from_toml_str(profile_document()).unwrap();

    let error = document.resolve_profile("missing").unwrap_err();

    match error {
        ConfigError::ProfileNotFound { profile } => assert_eq!(profile, "missing"),
        other => panic!("expected missing profile error, got {other:?}"),
    }
}

#[test]
fn builtin_profile_names_are_stable() {
    assert_eq!(BuiltinProfile::Dev.as_str(), "dev");
    assert_eq!(BuiltinProfile::Local.as_str(), "local");
    assert_eq!(BuiltinProfile::Enterprise.as_str(), "enterprise");
    assert_eq!(BuiltinProfile::Test.as_str(), "test");
}

#[test]
fn migration_skeleton_sets_current_version() {
    let document = AgentOsConfigDocument::from_toml_str(sample_config()).unwrap();
    assert_eq!(document.version, None);

    let (migrated, report) = document.migrate_to_current().unwrap();

    assert_eq!(migrated.version, Some(CURRENT_CONFIG_VERSION));
    assert_eq!(report.from_version, None);
    assert_eq!(report.to_version, CURRENT_CONFIG_VERSION);
    assert!(report.steps.is_empty());
}

#[test]
fn unsupported_future_config_version_returns_error() {
    let document = AgentOsConfigDocument::from_toml_str(
        r#"
version = 99

[storage]
root = ".agentos"
"#,
    )
    .unwrap();

    let error = document.migrate_to_current().unwrap_err();

    match error {
        ConfigError::UnsupportedConfigVersion { version, current } => {
            assert_eq!(version, 99);
            assert_eq!(current, CURRENT_CONFIG_VERSION);
        }
        other => panic!("expected unsupported version error, got {other:?}"),
    }
}

#[test]
fn production_identity_rejects_fake_crypto() {
    let mut config = AgentOsConfig::from_toml_str(sample_config()).unwrap();
    config.identity.mode = "production".to_string();
    config.identity.crypto_provider = "fake".to_string();

    let report = config.validate();

    assert!(
        report
            .diagnostics
            .iter()
            .any(|d| d.code == ConfigDiagnosticCode::FakeCryptoForbiddenInProduction)
    );
}

#[test]
fn enterprise_production_profile_rejects_fake_crypto() {
    let document = AgentOsConfigDocument::from_toml_str(
        r#"
[kernel]
profile = "enterprise"

[storage]
root = ".agentos"

[model]
default_provider = "openai"
default_model = "gpt-4o-mini"

[model.providers.openai]
provider = "openai"
endpoint = "https://api.openai.com/v1"
model = "gpt-4o-mini"

[profiles.enterprise.identity]
mode = "production"
crypto_provider = "fake"
"#,
    )
    .unwrap();

    let resolved = document.resolve_selected_profile().unwrap();
    let report = resolved.validate();

    assert!(
        report
            .diagnostics
            .iter()
            .any(|d| d.code == ConfigDiagnosticCode::FakeCryptoForbiddenInProduction)
    );
}

#[test]
fn local_production_profile_rejects_fake_crypto() {
    let document = AgentOsConfigDocument::from_toml_str(
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
model = "gpt-4o-mini"

[profiles.local.identity]
mode = "production"
crypto_provider = "fake"
"#,
    )
    .unwrap();

    let resolved = document.resolve_selected_profile().unwrap();
    let report = resolved.validate();

    assert!(
        report
            .diagnostics
            .iter()
            .any(|d| d.code == ConfigDiagnosticCode::FakeCryptoForbiddenInProduction)
    );
}

#[test]
fn production_identity_allows_ed25519() {
    let mut config = AgentOsConfig::from_toml_str(sample_config()).unwrap();
    config.identity.mode = "production".to_string();
    config.identity.crypto_provider = "ed25519".to_string();

    let report = config.validate();

    assert!(
        !report
            .diagnostics
            .iter()
            .any(|d| d.code == ConfigDiagnosticCode::FakeCryptoForbiddenInProduction)
    );
}

#[test]
fn development_identity_allows_fake_crypto() {
    let config = AgentOsConfig::from_toml_str(sample_config()).unwrap();

    let report = config.validate();

    assert_eq!(config.identity.mode, "development");
    assert_eq!(config.identity.crypto_provider, "fake");
    assert!(
        !report
            .diagnostics
            .iter()
            .any(|d| d.code == ConfigDiagnosticCode::FakeCryptoForbiddenInProduction)
    );
}

#[test]
fn identity_profile_patch_merges_deterministically() {
    let document = AgentOsConfigDocument::from_toml_str(
        r#"
[kernel]
profile = "child"

[storage]
root = ".agentos"

[model]
default_provider = "openai"
default_model = "gpt-4o-mini"

[model.providers.openai]
provider = "openai"
endpoint = "https://api.openai.com/v1"
model = "gpt-4o-mini"

[profiles.parent.identity]
mode = "production"

[profiles.child]
extends = "parent"

[profiles.child.identity]
crypto_provider = "ed25519"
"#,
    )
    .unwrap();

    let resolved = document.resolve_selected_profile().unwrap();

    assert_eq!(resolved.identity.mode, "production");
    assert_eq!(resolved.identity.crypto_provider, "ed25519");
    assert!(resolved.validate().is_valid());
}
