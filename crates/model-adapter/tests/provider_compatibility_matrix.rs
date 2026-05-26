//! Provider compatibility matrix tests.
//!
//! Default tests are fully mocked and require no network access. Real provider smoke
//! tests are metadata-gated by environment variables and ignored by default.

use model_adapter::{
    ModelCapabilities, ModelId, ModelProfile, ModelProvider, ModelRegistry, OpenAiProviderConfig,
    anthropic::AnthropicConfig,
};

#[derive(Debug, Clone, PartialEq, Eq)]
struct ProviderCompatibilityCase {
    provider: &'static str,
    model_id: &'static str,
    supports_complete: bool,
    supports_tools: bool,
    supports_streaming: bool,
    supports_json: bool,
    supports_vision: bool,
    max_context_tokens: Option<u32>,
    real_smoke_env: Option<&'static str>,
}

fn compatibility_matrix() -> Vec<ProviderCompatibilityCase> {
    vec![
        ProviderCompatibilityCase {
            provider: "openai_compatible",
            model_id: "openai-compatible/test-chat",
            supports_complete: true,
            supports_tools: true,
            supports_streaming: true,
            supports_json: true,
            supports_vision: false,
            max_context_tokens: Some(128_000),
            real_smoke_env: Some("AGENTOS_OPENAI_COMPAT_SMOKE_URL"),
        },
        ProviderCompatibilityCase {
            provider: "anthropic",
            model_id: "anthropic/claude-test",
            supports_complete: true,
            supports_tools: true,
            supports_streaming: true,
            supports_json: true,
            supports_vision: false,
            max_context_tokens: Some(200_000),
            real_smoke_env: Some("AGENTOS_ANTHROPIC_SMOKE_URL"),
        },
        ProviderCompatibilityCase {
            provider: "local",
            model_id: "local/text-only",
            supports_complete: true,
            supports_tools: false,
            supports_streaming: false,
            supports_json: false,
            supports_vision: false,
            max_context_tokens: Some(32_768),
            real_smoke_env: None,
        },
    ]
}

fn provider_for(case: &ProviderCompatibilityCase) -> ModelProvider {
    match case.provider {
        "openai_compatible" => ModelProvider::OpenAi,
        "anthropic" => ModelProvider::Anthropic,
        "local" => ModelProvider::Local,
        other => ModelProvider::Custom(other.to_string()),
    }
}

fn capabilities_for(case: &ProviderCompatibilityCase) -> ModelCapabilities {
    let mut capabilities = ModelCapabilities::text_only()
        .streaming(case.supports_streaming)
        .vision(case.supports_vision)
        .json(case.supports_json);
    capabilities.supports_tools = case.supports_tools;
    if let Some(max_context_tokens) = case.max_context_tokens {
        capabilities = capabilities.max_context_tokens(max_context_tokens);
    }
    capabilities
}

fn registry_from_matrix() -> ModelRegistry {
    let mut registry = ModelRegistry::new();
    for case in compatibility_matrix() {
        registry
            .register(ModelProfile::new(
                case.model_id,
                provider_for(&case),
                case.model_id,
                capabilities_for(&case),
            ))
            .unwrap();
    }
    registry
}

#[test]
fn mock_compatibility_matrix_registers_all_provider_profiles() {
    let matrix = compatibility_matrix();
    let registry = registry_from_matrix();

    assert_eq!(registry.len(), matrix.len());
    for case in matrix {
        let profile = registry.get(&ModelId::from(case.model_id)).unwrap();
        assert_eq!(profile.id, ModelId::from(case.model_id));
        assert_eq!(profile.supports_tools(), case.supports_tools);
        assert_eq!(profile.supports_streaming(), case.supports_streaming);
        assert_eq!(profile.supports_json(), case.supports_json);
        assert_eq!(profile.supports_vision(), case.supports_vision);
        assert_eq!(profile.max_context_tokens(), case.max_context_tokens);
    }
}

#[test]
fn mock_compatibility_matrix_enforces_tool_capabilities() {
    let registry = registry_from_matrix();

    for case in compatibility_matrix() {
        let model_id = ModelId::from(case.model_id);
        let result = registry.require_tools(&model_id);
        assert_eq!(result.is_ok(), case.supports_tools, "{}", case.model_id);
    }
}

#[test]
fn mock_compatibility_matrix_declares_real_smoke_env_gates() {
    let env_gated: Vec<_> = compatibility_matrix()
        .into_iter()
        .filter(|case| case.real_smoke_env.is_some())
        .collect();

    assert_eq!(env_gated.len(), 2);
    assert!(env_gated.iter().any(|case| {
        case.provider == "openai_compatible"
            && case.real_smoke_env == Some("AGENTOS_OPENAI_COMPAT_SMOKE_URL")
    }));
    assert!(env_gated.iter().any(|case| {
        case.provider == "anthropic" && case.real_smoke_env == Some("AGENTOS_ANTHROPIC_SMOKE_URL")
    }));
}

#[test]
fn provider_configs_can_be_built_from_mock_matrix() {
    let openai = OpenAiProviderConfig::new("http://127.0.0.1:1", "test-key", "test-model");
    assert_eq!(openai.endpoint, "http://127.0.0.1:1");
    assert_eq!(openai.model, "test-model");

    let anthropic = AnthropicConfig {
        api_key: "test-key".to_string(),
        base_url: "http://127.0.0.1:1".to_string(),
        default_model: "claude-test".to_string(),
        api_version: "2023-06-01".to_string(),
    };
    assert_eq!(anthropic.base_url, "http://127.0.0.1:1");
    assert_eq!(anthropic.default_model, "claude-test");
}

#[tokio::test]
#[ignore = "real provider smoke test; enable manually with AGENTOS_OPENAI_COMPAT_SMOKE_URL"]
async fn real_openai_compatible_smoke_is_env_gated() {
    let smoke_url = std::env::var("AGENTOS_OPENAI_COMPAT_SMOKE_URL")
        .expect("AGENTOS_OPENAI_COMPAT_SMOKE_URL must be set for real smoke tests");
    let api_key = std::env::var("AGENTOS_OPENAI_COMPAT_SMOKE_API_KEY")
        .expect("AGENTOS_OPENAI_COMPAT_SMOKE_API_KEY must be set for real smoke tests");
    let model = std::env::var("AGENTOS_OPENAI_COMPAT_SMOKE_MODEL")
        .unwrap_or_else(|_| "test-model".to_string());

    let config = OpenAiProviderConfig::new(smoke_url, api_key, model);
    assert!(!config.endpoint.is_empty());
    assert!(!config.api_key.is_empty());
}

#[tokio::test]
#[ignore = "real provider smoke test; enable manually with AGENTOS_ANTHROPIC_SMOKE_URL"]
async fn real_anthropic_smoke_is_env_gated() {
    let base_url = std::env::var("AGENTOS_ANTHROPIC_SMOKE_URL")
        .expect("AGENTOS_ANTHROPIC_SMOKE_URL must be set for real smoke tests");
    let api_key = std::env::var("AGENTOS_ANTHROPIC_SMOKE_API_KEY")
        .expect("AGENTOS_ANTHROPIC_SMOKE_API_KEY must be set for real smoke tests");
    let default_model = std::env::var("AGENTOS_ANTHROPIC_SMOKE_MODEL")
        .unwrap_or_else(|_| "claude-test".to_string());

    let config = AnthropicConfig {
        api_key,
        base_url,
        default_model,
        api_version: "2023-06-01".to_string(),
    };
    assert!(!config.base_url.is_empty());
    assert!(!config.api_key.is_empty());
}
