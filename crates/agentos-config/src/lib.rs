//! Typed AgentOS configuration parsing, overlays, validation, and redaction.

use std::collections::BTreeMap;
use std::fmt;
use std::path::Path;

use serde::{Deserialize, Serialize};

pub const CURRENT_CONFIG_VERSION: u32 = 1;

/// Built-in profile names recognized by AgentOS conventions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BuiltinProfile {
    Dev,
    Local,
    Enterprise,
    Test,
}

impl BuiltinProfile {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Dev => "dev",
            Self::Local => "local",
            Self::Enterprise => "enterprise",
            Self::Test => "test",
        }
    }
}

/// A full `agentos.toml` document, including optional profiles and version metadata.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct AgentOsConfigDocument {
    pub version: Option<u32>,
    #[serde(flatten)]
    pub config: AgentOsConfig,
    pub profiles: BTreeMap<String, AgentOsProfile>,
}

impl AgentOsConfigDocument {
    /// Parse a full AgentOS config document, including profile definitions.
    pub fn from_toml_str(input: &str) -> Result<Self, ConfigError> {
        toml::from_str(input).map_err(|source| ConfigError::ParseToml {
            reason: source.to_string(),
        })
    }

    /// Parse a full AgentOS config document from a file path.
    pub fn from_file(path: impl AsRef<Path>) -> Result<Self, ConfigError> {
        let path = path.as_ref();
        let input = std::fs::read_to_string(path).map_err(|source| ConfigError::ReadFile {
            path: path.display().to_string(),
            reason: source.to_string(),
        })?;
        Self::from_toml_str(&input)
    }

    /// Resolve the profile selected by `kernel.profile`, or return the base config when unset.
    pub fn resolve_selected_profile(&self) -> Result<AgentOsConfig, ConfigError> {
        match self.config.kernel.profile.as_deref() {
            Some(profile) if !profile.trim().is_empty() => self.resolve_profile(profile),
            _ => Ok(self.config.clone()),
        }
    }

    /// Resolve a named profile using deterministic parent-before-child inheritance.
    pub fn resolve_profile(&self, profile: &str) -> Result<AgentOsConfig, ConfigError> {
        let chain = self.resolve_profile_chain(profile, &mut Vec::new())?;
        let mut resolved = self.config.clone();
        for profile_name in chain {
            let profile =
                self.profiles
                    .get(&profile_name)
                    .ok_or_else(|| ConfigError::ProfileNotFound {
                        profile: profile_name.clone(),
                    })?;
            profile.apply_to(&mut resolved);
        }
        Ok(resolved)
    }

    fn resolve_profile_chain(
        &self,
        profile: &str,
        visiting: &mut Vec<String>,
    ) -> Result<Vec<String>, ConfigError> {
        if let Some(index) = visiting.iter().position(|item| item == profile) {
            let mut cycle = visiting[index..].to_vec();
            cycle.push(profile.to_string());
            return Err(ConfigError::ProfileCycle {
                profile_chain: cycle,
            });
        }

        let profile_config =
            self.profiles
                .get(profile)
                .ok_or_else(|| ConfigError::ProfileNotFound {
                    profile: profile.to_string(),
                })?;

        visiting.push(profile.to_string());
        let mut chain = if let Some(parent) = profile_config.extends.as_deref() {
            self.resolve_profile_chain(parent, visiting)?
        } else {
            Vec::new()
        };
        visiting.pop();
        chain.push(profile.to_string());
        Ok(chain)
    }

    /// Migrate the config document to the current schema version.
    ///
    /// PR97 only provides a no-op migration skeleton for version 1.
    pub fn migrate_to_current(mut self) -> Result<(Self, ConfigMigrationReport), ConfigError> {
        if let Some(version) = self.version
            && version > CURRENT_CONFIG_VERSION
        {
            return Err(ConfigError::UnsupportedConfigVersion {
                version,
                current: CURRENT_CONFIG_VERSION,
            });
        }

        let from_version = self.version;
        self.version = Some(CURRENT_CONFIG_VERSION);
        Ok((
            self,
            ConfigMigrationReport {
                from_version,
                to_version: CURRENT_CONFIG_VERSION,
                steps: Vec::new(),
            },
        ))
    }
}

/// Top-level AgentOS configuration parsed from `agentos.toml`.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct AgentOsConfig {
    pub kernel: KernelConfig,
    pub storage: StorageConfig,
    pub model: ModelConfig,
    pub policy: PolicyConfig,
    pub identity: IdentityConfig,
    pub browser: BrowserConfig,
    pub connectors: BTreeMap<String, ConnectorConfig>,
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
        if let Some(value) = env.var("OPENAI_TIMEOUT_SECS")
            && let Ok(timeout_secs) = value.parse::<u64>()
        {
            openai.timeout_secs = Some(timeout_secs);
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

        if !matches!(self.identity.mode.as_str(), "development" | "production") {
            diagnostics.push(ConfigDiagnostic::error(
                ConfigDiagnosticCode::IdentityModeInvalid,
                "identity.mode",
                "identity.mode must be one of: development, production",
            ));
        }

        if !matches!(self.identity.crypto_provider.as_str(), "fake" | "ed25519") {
            diagnostics.push(ConfigDiagnostic::error(
                ConfigDiagnosticCode::IdentityCryptoProviderInvalid,
                "identity.crypto_provider",
                "identity.crypto_provider must be one of: fake, ed25519",
            ));
        }

        if self.identity.mode == "production" && self.identity.crypto_provider == "fake" {
            diagnostics.push(ConfigDiagnostic::error(
                ConfigDiagnosticCode::FakeCryptoForbiddenInProduction,
                "identity.crypto_provider",
                "fake crypto is forbidden when identity.mode is production",
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
            identity: self.identity.clone(),
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct IdentityConfig {
    pub mode: String,
    pub crypto_provider: String,
}

impl Default for IdentityConfig {
    fn default() -> Self {
        Self {
            mode: "development".to_string(),
            crypto_provider: "fake".to_string(),
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

/// Profile override patch. All fields are optional so omitted values inherit deterministically.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct AgentOsProfile {
    pub extends: Option<String>,
    pub kernel: Option<KernelConfigPatch>,
    pub storage: Option<StorageConfigPatch>,
    pub model: Option<ModelConfigPatch>,
    pub policy: Option<PolicyConfigPatch>,
    pub identity: Option<IdentityConfigPatch>,
    pub browser: Option<BrowserConfigPatch>,
    pub connectors: BTreeMap<String, ConnectorConfigPatch>,
}

impl AgentOsProfile {
    fn apply_to(&self, config: &mut AgentOsConfig) {
        if let Some(patch) = &self.kernel {
            patch.apply_to(&mut config.kernel);
        }
        if let Some(patch) = &self.storage {
            patch.apply_to(&mut config.storage);
        }
        if let Some(patch) = &self.model {
            patch.apply_to(&mut config.model);
        }
        if let Some(patch) = &self.policy {
            patch.apply_to(&mut config.policy);
        }
        if let Some(patch) = &self.identity {
            patch.apply_to(&mut config.identity);
        }
        if let Some(patch) = &self.browser {
            patch.apply_to(&mut config.browser);
        }
        for (connector_key, patch) in &self.connectors {
            let connector = config.connectors.entry(connector_key.clone()).or_default();
            patch.apply_to(connector);
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct KernelConfigPatch {
    pub profile: Option<String>,
}

impl KernelConfigPatch {
    fn apply_to(&self, config: &mut KernelConfig) {
        if let Some(value) = &self.profile {
            config.profile = Some(value.clone());
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct StorageConfigPatch {
    pub root: Option<String>,
}

impl StorageConfigPatch {
    fn apply_to(&self, config: &mut StorageConfig) {
        if let Some(value) = &self.root {
            config.root = value.clone();
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct ModelConfigPatch {
    pub default_provider: Option<String>,
    pub default_model: Option<String>,
    pub providers: BTreeMap<String, ModelProviderConfigPatch>,
}

impl ModelConfigPatch {
    fn apply_to(&self, config: &mut ModelConfig) {
        if let Some(value) = &self.default_provider {
            config.default_provider = value.clone();
        }
        if let Some(value) = &self.default_model {
            config.default_model = value.clone();
        }
        for (provider_key, patch) in &self.providers {
            let provider = config.providers.entry(provider_key.clone()).or_default();
            patch.apply_to(provider);
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct ModelProviderConfigPatch {
    pub provider: Option<String>,
    pub endpoint: Option<String>,
    pub api_key: Option<String>,
    pub model: Option<String>,
    pub timeout_secs: Option<u64>,
}

impl ModelProviderConfigPatch {
    fn apply_to(&self, config: &mut ModelProviderConfig) {
        if let Some(value) = &self.provider {
            config.provider = value.clone();
        }
        if let Some(value) = &self.endpoint {
            config.endpoint = value.clone();
        }
        if let Some(value) = &self.api_key {
            config.api_key = Some(value.clone());
        }
        if let Some(value) = &self.model {
            config.model = value.clone();
        }
        if let Some(value) = self.timeout_secs {
            config.timeout_secs = Some(value);
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct PolicyConfigPatch {
    pub mode: Option<String>,
}

impl PolicyConfigPatch {
    fn apply_to(&self, config: &mut PolicyConfig) {
        if let Some(value) = &self.mode {
            config.mode = value.clone();
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct IdentityConfigPatch {
    pub mode: Option<String>,
    pub crypto_provider: Option<String>,
}

impl IdentityConfigPatch {
    fn apply_to(&self, config: &mut IdentityConfig) {
        if let Some(value) = &self.mode {
            config.mode = value.clone();
        }
        if let Some(value) = &self.crypto_provider {
            config.crypto_provider = value.clone();
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct BrowserConfigPatch {
    pub profile: Option<String>,
    pub allow_js: Option<bool>,
}

impl BrowserConfigPatch {
    fn apply_to(&self, config: &mut BrowserConfig) {
        if let Some(value) = &self.profile {
            config.profile = Some(value.clone());
        }
        if let Some(value) = self.allow_js {
            config.allow_js = value;
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct ConnectorConfigPatch {
    pub enabled: Option<bool>,
    pub endpoint: Option<String>,
    pub token: Option<String>,
}

impl ConnectorConfigPatch {
    fn apply_to(&self, config: &mut ConnectorConfig) {
        if let Some(value) = self.enabled {
            config.enabled = value;
        }
        if let Some(value) = &self.endpoint {
            config.endpoint = Some(value.clone());
        }
        if let Some(value) = &self.token {
            config.token = Some(value.clone());
        }
    }
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
    IdentityModeInvalid,
    IdentityCryptoProviderInvalid,
    FakeCryptoForbiddenInProduction,
}

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("failed to read config file {path}: {reason}")]
    ReadFile { path: String, reason: String },

    #[error("failed to parse agentos.toml: {reason}")]
    ParseToml { reason: String },

    #[error("config profile not found: {profile}")]
    ProfileNotFound { profile: String },

    #[error("config profile inheritance cycle: {profile_chain:?}")]
    ProfileCycle { profile_chain: Vec<String> },

    #[error("unsupported config version {version}; current supported version is {current}")]
    UnsupportedConfigVersion { version: u32, current: u32 },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConfigMigrationReport {
    pub from_version: Option<u32>,
    pub to_version: u32,
    pub steps: Vec<ConfigMigrationStep>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConfigMigrationStep {
    pub from_version: u32,
    pub to_version: u32,
    pub description: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RedactedAgentOsConfig {
    pub kernel: KernelConfig,
    pub storage: StorageConfig,
    pub model: RedactedModelConfig,
    pub policy: PolicyConfig,
    pub identity: IdentityConfig,
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
