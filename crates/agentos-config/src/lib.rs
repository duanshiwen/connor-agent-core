//! Typed AgentOS configuration parsing, overlays, validation, and redaction.

use std::collections::BTreeMap;
use std::fmt;
use std::path::Path;

use serde::{Deserialize, Serialize};

/// Top-level AgentOS configuration parsed from `agentos.toml`.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct AgentOsConfig {
    pub kernel: KernelConfig,
    pub storage: StorageConfig,
    pub model: ModelConfig,
    pub policy: PolicyConfig,
    pub browser: BrowserConfig,
    pub connectors: BTreeMap<String, ConnectorConfig>,
}

impl Default for AgentOsConfig {
    fn default() -> Self {
        Self {
            kernel: KernelConfig::default(),
            storage: StorageConfig::default(),
            model: ModelConfig::default(),
            policy: PolicyConfig::default(),
            browser: BrowserConfig::default(),
            connectors: BTreeMap::new(),
        }
    }
}

impl fmt::Debug for AgentOsConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.redacted().fmt(f)
    }
}

impl AgentOsConfig {
    /// Parse an AgentOS config from TOML text.
    pub fn from_toml_str(input: &str) -> Result<Self, ConfigError> {
        toml::from_str(input).map_err(|source| ConfigError::ParseToml {
            reason: source.to_string(),
        })
    }

    /// Parse an AgentOS config from a file path.
    pub fn from_file(path: impl AsRef<Path>) -> Result<Self, ConfigError> {
        let path = path.as_ref();
        let input = std::fs::read_to_string(path).map_err(|source| ConfigError::ReadFile {
            path: path.display().to_string(),
            reason: source.to_string(),
        })?;
        Self::from_toml_str(&input)
    }

    /// Apply deterministic environment overrides.
    pub fn apply_env_overlay(mut self, env: &dyn EnvSource) -> Self {
        if let Some(value) = env.var("AGENTOS_PROFILE") {
            self.kernel.profile = Some(value);
        }
        if let Some(value) = env.var("AGENTOS_STORAGE_ROOT") {
            self.storage.root = value;
        }
        if let Some(value) = env.var("AGENTOS_MODEL_DEFAULT_PROVIDER") {
            self.model.default_provider = value;
        }
        if let Some(value) = env.var("AGENTOS_MODEL_DEFAULT_MODEL") {
            self.model.default_model = value;
        }

        let openai = self
            .model
            .providers
            .entry("openai".to_string())
            .or_insert_with(|| ModelProviderConfig {
                provider: "openai".to_string(),
                ..ModelProviderConfig::default()
            });

        if let Some(value) = env.var("OPENAI_ENDPOINT") {
            openai.endpoint = value;
        }
        if let Some(value) = env.var("OPENAI_API_KEY") {
            openai.api_key = Some(value);
        }
        if let Some(value) = env.var("OPENAI_MODEL") {
            openai.model = value;
        }
        if let Some(value) = env.var("OPENAI_TIMEOUT_SECS") {
            if let Ok(timeout_secs) = value.parse::<u64>() {
                openai.timeout_secs = Some(timeout_secs);
            }
        }

        self
    }

    /// Validate config and return typed diagnostics rather than panicking.
    pub fn validate(&self) -> ConfigValidationReport {
        let mut diagnostics = Vec::new();

        if self.storage.root.trim().is_empty() {
            diagnostics.push(ConfigDiagnostic::error(
                ConfigDiagnosticCode::StorageRootEmpty,
                "storage.root",
                "storage.root must not be empty",
            ));
        }

        if self.model.default_provider.trim().is_empty() {
            diagnostics.push(ConfigDiagnostic::error(
                ConfigDiagnosticCode::DefaultProviderEmpty,
                "model.default_provider",
                "model.default_provider must not be empty",
            ));
        } else if !self
            .model
            .providers
            .contains_key(&self.model.default_provider)
        {
            diagnostics.push(ConfigDiagnostic::error(
                ConfigDiagnosticCode::DefaultProviderMissing,
                "model.default_provider",
                format!(
                    "default provider '{}' is not present in model.providers",
                    self.model.default_provider
                ),
            ));
        }

        if self.model.default_model.trim().is_empty() {
            diagnostics.push(ConfigDiagnostic::error(
                ConfigDiagnosticCode::DefaultModelEmpty,
                "model.default_model",
                "model.default_model must not be empty",
            ));
        }

        for (provider_key, provider) in &self.model.providers {
            let prefix = format!("model.providers.{provider_key}");
            if provider.endpoint.trim().is_empty() {
                diagnostics.push(ConfigDiagnostic::error(
                    ConfigDiagnosticCode::ProviderEndpointEmpty,
                    format!("{prefix}.endpoint"),
                    "provider endpoint must not be empty",
                ));
            }
            if provider.model.trim().is_empty() {
                diagnostics.push(ConfigDiagnostic::error(
                    ConfigDiagnosticCode::ProviderModelEmpty,
                    format!("{prefix}.model"),
                    "provider model must not be empty",
                ));
            }
            if provider.timeout_secs == Some(0) {
                diagnostics.push(ConfigDiagnostic::error(
                    ConfigDiagnosticCode::ProviderTimeoutInvalid,
                    format!("{prefix}.timeout_secs"),
                    "provider timeout_secs must be greater than zero",
                ));
            }
        }

        if !matches!(self.policy.mode.as_str(), "allow" | "ask" | "deny") {
            diagnostics.push(ConfigDiagnostic::error(
                ConfigDiagnosticCode::PolicyModeInvalid,
                "policy.mode",
                "policy.mode must be one of: allow, ask, deny",
            ));
        }

        ConfigValidationReport { diagnostics }
    }

    /// Return a redacted representation safe for diagnostics and logging.
    pub fn redacted(&self) -> RedactedAgentOsConfig {
        RedactedAgentOsConfig {
            kernel: self.kernel.clone(),
            storage: self.storage.clone(),
            model: self.model.redacted(),
            policy: self.policy.clone(),
            browser: self.browser.clone(),
            connectors: self
                .connectors
                .iter()
                .map(|(key, value)| (key.clone(), value.redacted()))
                .collect(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct KernelConfig {
    pub profile: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct StorageConfig {
    pub root: String,
}

impl Default for StorageConfig {
    fn default() -> Self {
        Self {
            root: ".agentos".to_string(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ModelConfig {
    pub default_provider: String,
    pub default_model: String,
    pub providers: BTreeMap<String, ModelProviderConfig>,
}

impl Default for ModelConfig {
    fn default() -> Self {
        Self {
            default_provider: "fake".to_string(),
            default_model: "fake/default".to_string(),
            providers: BTreeMap::new(),
        }
    }
}

impl ModelConfig {
    fn redacted(&self) -> RedactedModelConfig {
        RedactedModelConfig {
            default_provider: self.default_provider.clone(),
            default_model: self.default_model.clone(),
            providers: self
                .providers
                .iter()
                .map(|(key, value)| (key.clone(), value.redacted()))
                .collect(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct ModelProviderConfig {
    pub provider: String,
    pub endpoint: String,
    pub api_key: Option<String>,
    pub model: String,
    pub timeout_secs: Option<u64>,
}

impl ModelProviderConfig {
    fn redacted(&self) -> RedactedModelProviderConfig {
        RedactedModelProviderConfig {
            provider: self.provider.clone(),
            endpoint: self.endpoint.clone(),
            api_key: self.api_key.as_ref().map(|_| "<redacted>".to_string()),
            model: self.model.clone(),
            timeout_secs: self.timeout_secs,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct PolicyConfig {
    pub mode: String,
}

impl Default for PolicyConfig {
    fn default() -> Self {
        Self {
            mode: "ask".to_string(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct BrowserConfig {
    pub profile: Option<String>,
    pub allow_js: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct ConnectorConfig {
    pub enabled: bool,
    pub endpoint: Option<String>,
    pub token: Option<String>,
}

impl ConnectorConfig {
    fn redacted(&self) -> RedactedConnectorConfig {
        RedactedConnectorConfig {
            enabled: self.enabled,
            endpoint: self.endpoint.clone(),
            token: self.token.as_ref().map(|_| "<redacted>".to_string()),
        }
    }
}

/// Environment source abstraction for deterministic tests and process env overlays.
pub trait EnvSource {
    fn var(&self, key: &str) -> Option<String>;
}

#[derive(Debug, Clone, Copy, Default)]
pub struct ProcessEnv;

impl EnvSource for ProcessEnv {
    fn var(&self, key: &str) -> Option<String> {
        std::env::var(key).ok()
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MapEnvSource {
    values: BTreeMap<String, String>,
}

impl MapEnvSource {
    pub fn new(values: BTreeMap<String, String>) -> Self {
        Self { values }
    }

    pub fn from_pairs<const N: usize>(pairs: [(&str, &str); N]) -> Self {
        let values = pairs
            .into_iter()
            .map(|(key, value)| (key.to_string(), value.to_string()))
            .collect();
        Self { values }
    }
}

impl EnvSource for MapEnvSource {
    fn var(&self, key: &str) -> Option<String> {
        self.values.get(key).cloned()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConfigValidationReport {
    pub diagnostics: Vec<ConfigDiagnostic>,
}

impl ConfigValidationReport {
    pub fn is_valid(&self) -> bool {
        !self
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.severity == ConfigDiagnosticSeverity::Error)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConfigDiagnostic {
    pub severity: ConfigDiagnosticSeverity,
    pub code: ConfigDiagnosticCode,
    pub path: String,
    pub message: String,
}

impl ConfigDiagnostic {
    pub fn error(
        code: ConfigDiagnosticCode,
        path: impl Into<String>,
        message: impl Into<String>,
    ) -> Self {
        Self {
            severity: ConfigDiagnosticSeverity::Error,
            code,
            path: path.into(),
            message: message.into(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConfigDiagnosticSeverity {
    Error,
    Warning,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConfigDiagnosticCode {
    StorageRootEmpty,
    DefaultProviderEmpty,
    DefaultProviderMissing,
    DefaultModelEmpty,
    ProviderEndpointEmpty,
    ProviderModelEmpty,
    ProviderTimeoutInvalid,
    PolicyModeInvalid,
}

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("failed to read config file {path}: {reason}")]
    ReadFile { path: String, reason: String },

    #[error("failed to parse agentos.toml: {reason}")]
    ParseToml { reason: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RedactedAgentOsConfig {
    pub kernel: KernelConfig,
    pub storage: StorageConfig,
    pub model: RedactedModelConfig,
    pub policy: PolicyConfig,
    pub browser: BrowserConfig,
    pub connectors: BTreeMap<String, RedactedConnectorConfig>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RedactedModelConfig {
    pub default_provider: String,
    pub default_model: String,
    pub providers: BTreeMap<String, RedactedModelProviderConfig>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RedactedModelProviderConfig {
    pub provider: String,
    pub endpoint: String,
    pub api_key: Option<String>,
    pub model: String,
    pub timeout_secs: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RedactedConnectorConfig {
    pub enabled: bool,
    pub endpoint: Option<String>,
    pub token: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_is_constructible() {
        let config = AgentOsConfig::default();
        assert_eq!(config.storage.root, ".agentos");
        assert_eq!(config.policy.mode, "ask");
    }

    #[test]
    fn redacts_connector_token() {
        let mut config = AgentOsConfig::default();
        config.connectors.insert(
            "github".to_string(),
            ConnectorConfig {
                enabled: true,
                endpoint: Some("https://api.github.com".to_string()),
                token: Some("ghp-secret".to_string()),
            },
        );

        let debug = format!("{config:?}");

        assert!(!debug.contains("ghp-secret"));
        assert!(debug.contains("<redacted>"));
    }
}
