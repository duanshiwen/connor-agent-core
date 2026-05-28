//! # Model Adapter
//!
//! Text-only model adapter abstractions for AgentOS.
//!
//! This crate provides deterministic request/response types, a typed async
//! executor trait, a small model registry, and a static executor for tests and
//! early runtime work. The [`openai`] module supplies a concrete adapter for
//! any OpenAI-compatible Chat Completions endpoint (DeepSeek, Qwen, vLLM, etc.).

pub mod anthropic;
pub mod openai;
pub mod token_budget;
pub use openai::{OpenAiCompatibleAdapter, OpenAiProviderConfig};

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fmt;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Unique model identifier.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct ModelId(pub String);

impl fmt::Display for ModelId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl From<String> for ModelId {
    fn from(value: String) -> Self {
        Self(value)
    }
}

impl From<&str> for ModelId {
    fn from(value: &str) -> Self {
        Self(value.to_string())
    }
}

/// Model provider identifier.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelProvider {
    OpenAi,
    Anthropic,
    Google,
    Local,
    Test,
    Custom(String),
}

/// Provider-neutral capabilities supported by a model.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelCapabilities {
    pub supports_streaming: bool,
    pub supports_tools: bool,
    pub supports_vision: bool,
    pub supports_json: bool,
    pub max_context_tokens: Option<u32>,
}

impl ModelCapabilities {
    pub fn text_only() -> Self {
        Self {
            supports_streaming: false,
            supports_tools: false,
            supports_vision: false,
            supports_json: false,
            max_context_tokens: None,
        }
    }

    pub fn tool_calling() -> Self {
        Self {
            supports_tools: true,
            supports_json: true,
            ..Self::text_only()
        }
    }

    pub fn streaming(mut self, supported: bool) -> Self {
        self.supports_streaming = supported;
        self
    }

    pub fn vision(mut self, supported: bool) -> Self {
        self.supports_vision = supported;
        self
    }

    pub fn json(mut self, supported: bool) -> Self {
        self.supports_json = supported;
        self
    }

    pub fn max_context_tokens(mut self, tokens: u32) -> Self {
        self.max_context_tokens = Some(tokens);
        self
    }
}

impl Default for ModelCapabilities {
    fn default() -> Self {
        Self::text_only()
    }
}

/// Registry metadata for a model.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelProfile {
    pub id: ModelId,
    pub provider: ModelProvider,
    pub display_name: String,
    pub capabilities: ModelCapabilities,
}

impl ModelProfile {
    pub fn new(
        id: impl Into<ModelId>,
        provider: ModelProvider,
        display_name: impl Into<String>,
        capabilities: ModelCapabilities,
    ) -> Self {
        Self {
            id: id.into(),
            provider,
            display_name: display_name.into(),
            capabilities,
        }
    }

    pub fn test_profile(id: impl Into<ModelId>) -> Self {
        let id = id.into();
        Self {
            display_name: id.to_string(),
            id,
            provider: ModelProvider::Test,
            capabilities: ModelCapabilities::text_only(),
        }
    }

    pub fn supports_streaming(&self) -> bool {
        self.capabilities.supports_streaming
    }

    pub fn supports_tools(&self) -> bool {
        self.capabilities.supports_tools
    }

    pub fn supports_vision(&self) -> bool {
        self.capabilities.supports_vision
    }

    pub fn supports_json(&self) -> bool {
        self.capabilities.supports_json
    }

    pub fn max_context_tokens(&self) -> Option<u32> {
        self.capabilities.max_context_tokens
    }
}

/// Chat role for a text-only model message.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelRole {
    System,
    User,
    Assistant,
    Tool,
}

/// Text-only message sent to a model adapter.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelMessage {
    pub role: ModelRole,
    pub text: String,
}

impl ModelMessage {
    pub fn system(text: impl Into<String>) -> Self {
        Self {
            role: ModelRole::System,
            text: text.into(),
        }
    }

    pub fn user(text: impl Into<String>) -> Self {
        Self {
            role: ModelRole::User,
            text: text.into(),
        }
    }

    pub fn assistant(text: impl Into<String>) -> Self {
        Self {
            role: ModelRole::Assistant,
            text: text.into(),
        }
    }
}

/// Text-only model completion request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelRequest {
    pub model_id: ModelId,
    pub messages: Vec<ModelMessage>,
    pub temperature_millis: Option<u16>,
    pub max_output_tokens: Option<u32>,
    pub metadata: BTreeMap<String, String>,
}

impl ModelRequest {
    pub fn new(model_id: impl Into<ModelId>, messages: Vec<ModelMessage>) -> Self {
        Self {
            model_id: model_id.into(),
            messages,
            temperature_millis: None,
            max_output_tokens: None,
            metadata: BTreeMap::new(),
        }
    }
}

/// Token usage reported by a model adapter.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct ModelUsage {
    pub input_tokens: u32,
    pub output_tokens: u32,
}

impl ModelUsage {
    pub fn total_tokens(&self) -> u32 {
        self.input_tokens + self.output_tokens
    }
}

/// Provider-neutral tool definition exposed to the model.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolDefinition {
    /// Tool name, typically matching an `ActionKind` (e.g. `knowledge.search`).
    pub name: String,
    /// Short human-readable description for the model.
    pub description: String,
    /// JSON Schema describing the tool's input parameters.
    pub input_schema: serde_json::Value,
}

/// A single tool call returned by the model.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolCall {
    /// Provider-assigned call identifier (e.g. `call_abc123`).
    pub id: String,
    /// Tool name requested by the model.
    pub name: String,
    /// Parsed JSON arguments.
    pub arguments: serde_json::Value,
    /// Original raw JSON string from the provider (for debugging / audit).
    pub raw_arguments: String,
}

/// Controls whether the model can or must use tools.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolChoice {
    /// Model decides whether to call a tool.
    Auto,
    /// Model must not call any tool.
    None,
    /// Model must call at least one tool.
    Required,
    /// Model must call the named tool.
    Named(String),
}

/// Unified model output — either text-only or tool calls.
///
/// Replaces the former `ModelResponse` struct. Text-only responses map to
/// `ModelOutput::Text`; responses that include tool/function calls map to
/// `ModelOutput::ToolCalls`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ModelOutput {
    Text {
        text: String,
        usage: Option<ModelUsage>,
    },
    ToolCalls {
        /// Optional text content emitted alongside tool calls.
        content: Option<String>,
        tool_calls: Vec<ToolCall>,
        usage: Option<ModelUsage>,
    },
}

impl ModelOutput {
    /// Returns the text content if this is a text-only output.
    pub fn text(&self) -> Option<&str> {
        match self {
            ModelOutput::Text { text, .. } => Some(text),
            ModelOutput::ToolCalls { content, .. } => content.as_deref(),
        }
    }

    /// Returns usage regardless of output variant.
    pub fn usage(&self) -> Option<&ModelUsage> {
        match self {
            ModelOutput::Text { usage, .. } | ModelOutput::ToolCalls { usage, .. } => {
                usage.as_ref()
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Streaming Abstraction
// ---------------------------------------------------------------------------

/// Provider-neutral stream events emitted by streaming model adapters.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ModelStreamEvent {
    Started { model_id: ModelId },
    TextDelta { delta: String },
    ToolCallDelta(ModelToolCallDelta),
    Usage { usage: ModelUsage },
    Finished { reason: ModelStreamFinishReason },
    Error { message: String },
}

/// Provider-neutral finish reason for model streams.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelStreamFinishReason {
    Stop,
    Length,
    ToolCalls,
    Error,
}

/// Incremental tool call update emitted by streaming providers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelToolCallDelta {
    pub index: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id_delta: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name_delta: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub arguments_delta: Option<String>,
}

impl ModelToolCallDelta {
    pub fn arguments(index: usize, delta: impl Into<String>) -> Self {
        Self {
            index,
            id_delta: None,
            name_delta: None,
            arguments_delta: Some(delta.into()),
        }
    }
}

/// Accumulates provider-neutral stream events into a simple text/usage summary.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct ModelStreamAccumulator {
    pub text: String,
    pub usage: Option<ModelUsage>,
    pub finished: Option<ModelStreamFinishReason>,
    pub errors: Vec<String>,
}

impl ModelStreamAccumulator {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn apply(&mut self, event: &ModelStreamEvent) {
        match event {
            ModelStreamEvent::TextDelta { delta } => self.text.push_str(delta),
            ModelStreamEvent::Usage { usage } => self.usage = Some(usage.clone()),
            ModelStreamEvent::Finished { reason } => self.finished = Some(*reason),
            ModelStreamEvent::Error { message } => self.errors.push(message.clone()),
            ModelStreamEvent::Started { .. } | ModelStreamEvent::ToolCallDelta(_) => {}
        }
    }

    pub fn from_events(events: &[ModelStreamEvent]) -> Self {
        let mut accumulator = Self::new();
        for event in events {
            accumulator.apply(event);
        }
        accumulator
    }
}

/// Typed model adapter errors.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ModelAdapterError {
    #[error("empty model request")]
    EmptyRequest,

    #[error("model not found: {0}")]
    ModelNotFound(ModelId),

    #[error("model capability unsupported: {model_id} does not support {capability}")]
    UnsupportedModelCapability {
        model_id: ModelId,
        capability: &'static str,
    },

    #[error("duplicate model id: {0}")]
    DuplicateModelId(ModelId),

    #[error("default model not set")]
    DefaultModelNotSet,

    #[error("executor failed: {0}")]
    ExecutorFailed(String),

    #[error("malformed tool call arguments: {0}")]
    MalformedToolCallArguments(String),

    #[error("malformed structured output: {0}")]
    MalformedStructuredOutput(String),

    #[error("structured output schema violation: {0}")]
    StructuredOutputSchemaViolation(String),

    #[error("structured output repair failed: {0}")]
    StructuredOutputRepairFailed(String),

    #[error("missing tool call function name")]
    MissingToolCallName,

    #[error("missing tool call id")]
    MissingToolCallId,

    #[error("config error: {0}")]
    ConfigError(String),

    #[error("HTTP error: {0}")]
    HttpError(String),

    #[error("auth error: {0}")]
    AuthError(String),

    #[error("rate limit exceeded")]
    RateLimitExceeded,

    #[error("empty response")]
    EmptyResponse,
}

/// Structured output format requested from a model.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StructuredOutputFormat {
    pub name: String,
    pub schema: serde_json::Value,
    pub strict: bool,
}

impl StructuredOutputFormat {
    pub fn json_schema(name: impl Into<String>, schema: serde_json::Value) -> Self {
        Self {
            name: name.into(),
            schema,
            strict: true,
        }
    }
}

/// Structured output validation result.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StructuredOutput {
    pub value: serde_json::Value,
}

/// Optional hook that can repair invalid structured output before validation fails.
pub trait StructuredOutputRepairPolicy: Send + Sync {
    fn repair(
        &self,
        raw: &str,
        format: &StructuredOutputFormat,
        error: &ModelAdapterError,
    ) -> Option<String>;
}

/// No-op repair policy: invalid structured output remains invalid.
#[derive(Debug, Clone, Default)]
pub struct NoopStructuredOutputRepairPolicy;

impl StructuredOutputRepairPolicy for NoopStructuredOutputRepairPolicy {
    fn repair(
        &self,
        _raw: &str,
        _format: &StructuredOutputFormat,
        _error: &ModelAdapterError,
    ) -> Option<String> {
        None
    }
}

/// Deterministic structured output validator for JSON object schemas.
pub struct StructuredOutputValidator<'a> {
    format: &'a StructuredOutputFormat,
    repair_policy: Option<&'a dyn StructuredOutputRepairPolicy>,
}

impl<'a> StructuredOutputValidator<'a> {
    pub fn new(format: &'a StructuredOutputFormat) -> Self {
        Self {
            format,
            repair_policy: None,
        }
    }

    pub fn with_repair_policy(
        format: &'a StructuredOutputFormat,
        repair_policy: &'a dyn StructuredOutputRepairPolicy,
    ) -> Self {
        Self {
            format,
            repair_policy: Some(repair_policy),
        }
    }

    pub fn validate(&self, raw: &str) -> Result<StructuredOutput, ModelAdapterError> {
        match self.validate_once(raw) {
            Ok(output) => Ok(output),
            Err(error) => {
                let Some(repair_policy) = self.repair_policy else {
                    return Err(error);
                };
                let Some(repaired) = repair_policy.repair(raw, self.format, &error) else {
                    return Err(error);
                };
                self.validate_once(&repaired).map_err(|repair_error| {
                    ModelAdapterError::StructuredOutputRepairFailed(repair_error.to_string())
                })
            }
        }
    }

    fn validate_once(&self, raw: &str) -> Result<StructuredOutput, ModelAdapterError> {
        let value: serde_json::Value = serde_json::from_str(raw).map_err(|error| {
            ModelAdapterError::MalformedStructuredOutput(format!("{}: {error}", self.format.name))
        })?;
        validate_json_schema_subset(&value, &self.format.schema)
            .map_err(ModelAdapterError::StructuredOutputSchemaViolation)?;
        Ok(StructuredOutput { value })
    }
}

fn validate_json_schema_subset(
    value: &serde_json::Value,
    schema: &serde_json::Value,
) -> Result<(), String> {
    if let Some(schema_type) = schema.get("type").and_then(serde_json::Value::as_str) {
        validate_json_type(value, schema_type, "$")?;
    }

    if let Some(required) = schema.get("required").and_then(serde_json::Value::as_array) {
        let object = value
            .as_object()
            .ok_or_else(|| "$: expected object for required properties".to_string())?;
        for key in required.iter().filter_map(serde_json::Value::as_str) {
            if !object.contains_key(key) {
                return Err(format!("$: missing required property `{key}`"));
            }
        }
    }

    if let Some(properties) = schema
        .get("properties")
        .and_then(serde_json::Value::as_object)
    {
        let object = value
            .as_object()
            .ok_or_else(|| "$: expected object for properties".to_string())?;
        for (key, property_schema) in properties {
            if let Some(property_value) = object.get(key)
                && let Some(property_type) = property_schema
                    .get("type")
                    .and_then(serde_json::Value::as_str)
            {
                validate_json_type(property_value, property_type, &format!("$.{key}"))?;
            }
        }
    }

    Ok(())
}

fn validate_json_type(value: &serde_json::Value, expected: &str, path: &str) -> Result<(), String> {
    let valid = match expected {
        "object" => value.is_object(),
        "array" => value.is_array(),
        "string" => value.is_string(),
        "number" => value.is_number(),
        "integer" => value.as_i64().is_some() || value.as_u64().is_some(),
        "boolean" => value.is_boolean(),
        "null" => value.is_null(),
        _ => return Ok(()),
    };
    if valid {
        Ok(())
    } else {
        Err(format!("{path}: expected {expected}"))
    }
}

/// Retry/backoff configuration for model calls.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelRetryConfig {
    pub initial_delay: Duration,
    pub multiplier: u32,
    pub max_delay: Duration,
    /// Maximum total attempts, including the initial attempt.
    pub max_attempts: u32,
}

impl Default for ModelRetryConfig {
    fn default() -> Self {
        Self {
            initial_delay: Duration::from_millis(250),
            multiplier: 2,
            max_delay: Duration::from_secs(10),
            max_attempts: 3,
        }
    }
}

/// Provider-neutral model error classes used by retry policy decisions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelRetryErrorClass {
    RateLimited,
    Timeout,
    TransientNetwork,
    ServerError,
    Auth,
    Validation,
    PermissionDenied,
    EmptyRequest,
    EmptyResponse,
    Unknown,
}

impl ModelRetryErrorClass {
    pub fn is_retryable(self) -> bool {
        matches!(
            self,
            Self::RateLimited | Self::Timeout | Self::TransientNetwork | Self::ServerError
        )
    }
}

/// Deterministic retry policy decision for one model call attempt.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ModelRetryDecision {
    Retry {
        error_class: ModelRetryErrorClass,
        attempt: u32,
        delay: Duration,
    },
    DoNotRetry {
        error_class: ModelRetryErrorClass,
        attempt: u32,
        reason: String,
    },
    Exhausted {
        error_class: ModelRetryErrorClass,
        attempt: u32,
    },
}

/// Classify model adapter errors and calculate retry backoff.
pub trait ModelRetryPolicy: Send + Sync {
    fn classify_error(&self, error: &ModelAdapterError) -> ModelRetryErrorClass;
    fn backoff_delay(&self, attempt: u32) -> Option<Duration>;

    fn decide(&self, error: &ModelAdapterError, attempt: u32) -> ModelRetryDecision {
        let error_class = self.classify_error(error);
        if !error_class.is_retryable() {
            return ModelRetryDecision::DoNotRetry {
                error_class,
                attempt,
                reason: format!("{error_class:?} is not retryable"),
            };
        }

        match self.backoff_delay(attempt) {
            Some(delay) => ModelRetryDecision::Retry {
                error_class,
                attempt,
                delay,
            },
            None => ModelRetryDecision::Exhausted {
                error_class,
                attempt,
            },
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DefaultModelRetryPolicy {
    config: ModelRetryConfig,
}

impl DefaultModelRetryPolicy {
    pub fn new(config: ModelRetryConfig) -> Self {
        Self { config }
    }

    pub fn config(&self) -> &ModelRetryConfig {
        &self.config
    }
}

impl Default for DefaultModelRetryPolicy {
    fn default() -> Self {
        Self::new(ModelRetryConfig::default())
    }
}

impl ModelRetryPolicy for DefaultModelRetryPolicy {
    fn classify_error(&self, error: &ModelAdapterError) -> ModelRetryErrorClass {
        classify_model_adapter_error(error)
    }

    fn backoff_delay(&self, attempt: u32) -> Option<Duration> {
        if attempt == 0 || attempt >= self.config.max_attempts {
            return None;
        }
        let exponent = attempt.saturating_sub(1);
        let factor = self.config.multiplier.saturating_pow(exponent);
        let delay = self.config.initial_delay.saturating_mul(factor);
        Some(delay.min(self.config.max_delay))
    }
}

pub fn classify_model_adapter_error(error: &ModelAdapterError) -> ModelRetryErrorClass {
    match error {
        ModelAdapterError::RateLimitExceeded => ModelRetryErrorClass::RateLimited,
        ModelAdapterError::AuthError(_) => ModelRetryErrorClass::Auth,
        ModelAdapterError::EmptyRequest => ModelRetryErrorClass::EmptyRequest,
        ModelAdapterError::EmptyResponse => ModelRetryErrorClass::EmptyResponse,
        ModelAdapterError::MalformedToolCallArguments(_)
        | ModelAdapterError::MalformedStructuredOutput(_)
        | ModelAdapterError::StructuredOutputSchemaViolation(_)
        | ModelAdapterError::StructuredOutputRepairFailed(_)
        | ModelAdapterError::MissingToolCallName
        | ModelAdapterError::MissingToolCallId => ModelRetryErrorClass::Validation,
        ModelAdapterError::ConfigError(_)
        | ModelAdapterError::ModelNotFound(_)
        | ModelAdapterError::UnsupportedModelCapability { .. }
        | ModelAdapterError::DuplicateModelId(_)
        | ModelAdapterError::DefaultModelNotSet => ModelRetryErrorClass::Validation,
        ModelAdapterError::HttpError(message) | ModelAdapterError::ExecutorFailed(message) => {
            classify_model_error_message(message)
        }
    }
}

/// Circuit breaker configuration for per-provider model calls.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelCircuitBreakerConfig {
    pub failure_threshold: u32,
    pub cooldown: Duration,
}

impl Default for ModelCircuitBreakerConfig {
    fn default() -> Self {
        Self {
            failure_threshold: 3,
            cooldown: Duration::from_secs(30),
        }
    }
}

/// Circuit breaker health state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelCircuitBreakerState {
    Closed,
    Open,
    HalfOpen,
}

/// Snapshot of model circuit breaker health for diagnostics.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelCircuitBreakerHealth {
    pub provider: String,
    pub state: ModelCircuitBreakerState,
    pub consecutive_failures: u32,
    pub failure_threshold: u32,
    pub cooldown: Duration,
}

#[derive(Debug)]
struct ModelCircuitBreakerInner {
    state: ModelCircuitBreakerState,
    consecutive_failures: u32,
    opened_at: Option<Instant>,
}

/// Per-provider model circuit breaker.
#[derive(Debug)]
pub struct ModelCircuitBreaker {
    provider: String,
    config: ModelCircuitBreakerConfig,
    inner: Mutex<ModelCircuitBreakerInner>,
}

impl ModelCircuitBreaker {
    pub fn new(provider: impl Into<String>, config: ModelCircuitBreakerConfig) -> Self {
        Self {
            provider: provider.into(),
            config,
            inner: Mutex::new(ModelCircuitBreakerInner {
                state: ModelCircuitBreakerState::Closed,
                consecutive_failures: 0,
                opened_at: None,
            }),
        }
    }

    pub fn provider(&self) -> &str {
        &self.provider
    }

    pub fn before_call(&self) -> Result<(), ModelAdapterError> {
        let mut inner = self.inner.lock().expect("model circuit breaker poisoned");
        if inner.state == ModelCircuitBreakerState::Open {
            let elapsed = inner.opened_at.map(|opened_at| opened_at.elapsed());
            if elapsed.is_some_and(|elapsed| elapsed >= self.config.cooldown) {
                inner.state = ModelCircuitBreakerState::HalfOpen;
                return Ok(());
            }
            return Err(ModelAdapterError::ExecutorFailed(format!(
                "model circuit breaker open for provider {}",
                self.provider
            )));
        }
        Ok(())
    }

    pub fn record_success(&self) {
        let mut inner = self.inner.lock().expect("model circuit breaker poisoned");
        inner.state = ModelCircuitBreakerState::Closed;
        inner.consecutive_failures = 0;
        inner.opened_at = None;
    }

    pub fn record_error(&self, error: &ModelAdapterError) {
        let error_class = classify_model_adapter_error(error);
        if !Self::trips_on(error_class) {
            return;
        }

        let mut inner = self.inner.lock().expect("model circuit breaker poisoned");
        inner.consecutive_failures = inner.consecutive_failures.saturating_add(1);
        if inner.consecutive_failures >= self.config.failure_threshold {
            inner.state = ModelCircuitBreakerState::Open;
            inner.opened_at = Some(Instant::now());
        }
    }

    pub fn health(&self) -> ModelCircuitBreakerHealth {
        let inner = self.inner.lock().expect("model circuit breaker poisoned");
        ModelCircuitBreakerHealth {
            provider: self.provider.clone(),
            state: inner.state,
            consecutive_failures: inner.consecutive_failures,
            failure_threshold: self.config.failure_threshold,
            cooldown: self.config.cooldown,
        }
    }

    fn trips_on(error_class: ModelRetryErrorClass) -> bool {
        matches!(
            error_class,
            ModelRetryErrorClass::RateLimited
                | ModelRetryErrorClass::Timeout
                | ModelRetryErrorClass::TransientNetwork
                | ModelRetryErrorClass::ServerError
        )
    }
}

impl Default for ModelCircuitBreaker {
    fn default() -> Self {
        Self::new("default", ModelCircuitBreakerConfig::default())
    }
}

pub fn classify_model_error_message(message: &str) -> ModelRetryErrorClass {
    let normalized = message.to_ascii_lowercase();
    if normalized.contains("429")
        || normalized.contains("rate limit")
        || normalized.contains("too many requests")
    {
        return ModelRetryErrorClass::RateLimited;
    }
    if normalized.contains("timeout")
        || normalized.contains("timed out")
        || normalized.contains("deadline")
    {
        return ModelRetryErrorClass::Timeout;
    }
    if normalized.contains("401")
        || normalized.contains("403")
        || normalized.contains("unauthorized")
        || normalized.contains("forbidden")
        || normalized.contains("permission denied")
    {
        return ModelRetryErrorClass::PermissionDenied;
    }
    if normalized.contains("400")
        || normalized.contains("bad request")
        || normalized.contains("invalid request")
        || normalized.contains("validation")
    {
        return ModelRetryErrorClass::Validation;
    }
    if normalized.contains("500") || normalized.contains("server error") {
        return ModelRetryErrorClass::ServerError;
    }
    if normalized.contains("network")
        || normalized.contains("connection reset")
        || normalized.contains("connection refused")
        || normalized.contains("temporarily unavailable")
        || normalized.contains("502")
        || normalized.contains("503")
        || normalized.contains("504")
    {
        return ModelRetryErrorClass::TransientNetwork;
    }
    ModelRetryErrorClass::Unknown
}

/// Model call operation recorded by observability wrappers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelCallOperation {
    Complete,
    Stream,
    ToolCall,
}

/// Outcome kind recorded by model traces.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelTraceOutcome {
    Success,
    Error,
}

/// Redacted model call trace safe for logs and audit surfaces.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelCallTrace {
    pub operation: ModelCallOperation,
    pub model_id: ModelId,
    pub latency_ms: u64,
    pub usage: Option<ModelUsage>,
    pub outcome: ModelTraceOutcome,
    pub error_class: Option<ModelRetryErrorClass>,
    pub metadata: BTreeMap<String, String>,
}

/// Sink for model call observability events.
pub trait ModelTraceSink: Send + Sync {
    fn record(&self, trace: ModelCallTrace);
}

/// In-memory trace sink for tests and local diagnostics.
#[derive(Debug, Default)]
pub struct MemoryModelTraceSink {
    traces: Mutex<Vec<ModelCallTrace>>,
}

impl MemoryModelTraceSink {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn traces(&self) -> Vec<ModelCallTrace> {
        self.traces.lock().unwrap().clone()
    }
}

impl ModelTraceSink for MemoryModelTraceSink {
    fn record(&self, trace: ModelCallTrace) {
        self.traces.lock().unwrap().push(trace);
    }
}

fn duration_to_millis(duration: Duration) -> u64 {
    duration.as_millis().try_into().unwrap_or(u64::MAX)
}

fn redacted_model_metadata(metadata: &BTreeMap<String, String>) -> BTreeMap<String, String> {
    metadata
        .iter()
        .map(|(key, value)| {
            if is_sensitive_metadata_key(key) {
                (key.clone(), "<redacted>".to_string())
            } else {
                (key.clone(), value.clone())
            }
        })
        .collect()
}

fn is_sensitive_metadata_key(key: &str) -> bool {
    let normalized = key.to_ascii_lowercase();
    normalized.contains("api_key")
        || normalized.contains("apikey")
        || normalized.contains("authorization")
        || normalized.contains("auth")
        || normalized.contains("bearer")
        || normalized.contains("credential")
        || normalized.contains("password")
        || normalized.contains("secret")
        || normalized.contains("token")
}

/// Async model adapter trait — text-only completion.
#[async_trait]
pub trait ModelAdapter: Send + Sync {
    async fn complete(&self, request: ModelRequest) -> Result<ModelOutput, ModelAdapterError>;
}

/// Extended adapter that supports provider-neutral streaming events.
///
/// The v1 abstraction returns a deterministic event vector so providers can
/// share event semantics before transport-specific streaming parsers are wired
/// in PR113/PR114.
#[async_trait]
pub trait StreamingModelAdapter: ModelAdapter {
    async fn stream(
        &self,
        request: ModelRequest,
    ) -> Result<Vec<ModelStreamEvent>, ModelAdapterError>;
}

/// Adapter wrapper that records redacted model call traces.
pub struct ObservedModelAdapter<A> {
    inner: A,
    sink: Arc<dyn ModelTraceSink>,
}

impl<A> ObservedModelAdapter<A> {
    pub fn new(inner: A, sink: Arc<dyn ModelTraceSink>) -> Self {
        Self { inner, sink }
    }

    pub fn inner(&self) -> &A {
        &self.inner
    }
}

impl<A> fmt::Debug for ObservedModelAdapter<A>
where
    A: fmt::Debug,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ObservedModelAdapter")
            .field("inner", &self.inner)
            .finish_non_exhaustive()
    }
}

#[async_trait]
impl<A> ModelAdapter for ObservedModelAdapter<A>
where
    A: ModelAdapter + Send + Sync,
{
    async fn complete(&self, request: ModelRequest) -> Result<ModelOutput, ModelAdapterError> {
        let started = Instant::now();
        let model_id = request.model_id.clone();
        let metadata = redacted_model_metadata(&request.metadata);
        let result = self.inner.complete(request).await;
        let trace = match &result {
            Ok(output) => ModelCallTrace {
                operation: ModelCallOperation::Complete,
                model_id,
                latency_ms: duration_to_millis(started.elapsed()),
                usage: output.usage().cloned(),
                outcome: ModelTraceOutcome::Success,
                error_class: None,
                metadata,
            },
            Err(error) => ModelCallTrace {
                operation: ModelCallOperation::Complete,
                model_id,
                latency_ms: duration_to_millis(started.elapsed()),
                usage: None,
                outcome: ModelTraceOutcome::Error,
                error_class: Some(classify_model_adapter_error(error)),
                metadata,
            },
        };
        self.sink.record(trace);
        result
    }
}

#[async_trait]
impl<A> StreamingModelAdapter for ObservedModelAdapter<A>
where
    A: StreamingModelAdapter + Send + Sync,
{
    async fn stream(
        &self,
        request: ModelRequest,
    ) -> Result<Vec<ModelStreamEvent>, ModelAdapterError> {
        let started = Instant::now();
        let model_id = request.model_id.clone();
        let metadata = redacted_model_metadata(&request.metadata);
        let result = self.inner.stream(request).await;
        let trace = match &result {
            Ok(events) => ModelCallTrace {
                operation: ModelCallOperation::Stream,
                model_id,
                latency_ms: duration_to_millis(started.elapsed()),
                usage: events.iter().find_map(|event| match event {
                    ModelStreamEvent::Usage { usage } => Some(usage.clone()),
                    _ => None,
                }),
                outcome: ModelTraceOutcome::Success,
                error_class: None,
                metadata,
            },
            Err(error) => ModelCallTrace {
                operation: ModelCallOperation::Stream,
                model_id,
                latency_ms: duration_to_millis(started.elapsed()),
                usage: None,
                outcome: ModelTraceOutcome::Error,
                error_class: Some(classify_model_adapter_error(error)),
                metadata,
            },
        };
        self.sink.record(trace);
        result
    }
}

/// Adapter wrapper that guards provider calls with a circuit breaker.
pub struct CircuitBreakingModelAdapter<A> {
    inner: A,
    breaker: Arc<ModelCircuitBreaker>,
}

impl<A> CircuitBreakingModelAdapter<A> {
    pub fn new(inner: A, breaker: ModelCircuitBreaker) -> Self {
        Self {
            inner,
            breaker: Arc::new(breaker),
        }
    }

    pub fn with_shared_breaker(inner: A, breaker: Arc<ModelCircuitBreaker>) -> Self {
        Self { inner, breaker }
    }

    pub fn inner(&self) -> &A {
        &self.inner
    }

    pub fn breaker(&self) -> &Arc<ModelCircuitBreaker> {
        &self.breaker
    }
}

impl<A> fmt::Debug for CircuitBreakingModelAdapter<A>
where
    A: fmt::Debug,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CircuitBreakingModelAdapter")
            .field("inner", &self.inner)
            .field("health", &self.breaker.health())
            .finish()
    }
}

#[async_trait]
impl<A> ModelAdapter for CircuitBreakingModelAdapter<A>
where
    A: ModelAdapter + Send + Sync,
{
    async fn complete(&self, request: ModelRequest) -> Result<ModelOutput, ModelAdapterError> {
        self.breaker.before_call()?;
        match self.inner.complete(request).await {
            Ok(output) => {
                self.breaker.record_success();
                Ok(output)
            }
            Err(error) => {
                self.breaker.record_error(&error);
                Err(error)
            }
        }
    }
}

#[async_trait]
impl<A> StreamingModelAdapter for CircuitBreakingModelAdapter<A>
where
    A: StreamingModelAdapter + Send + Sync,
{
    async fn stream(
        &self,
        request: ModelRequest,
    ) -> Result<Vec<ModelStreamEvent>, ModelAdapterError> {
        self.breaker.before_call()?;
        match self.inner.stream(request).await {
            Ok(events) => {
                self.breaker.record_success();
                Ok(events)
            }
            Err(error) => {
                self.breaker.record_error(&error);
                Err(error)
            }
        }
    }
}

/// Adapter wrapper that retries provider calls according to a deterministic policy.
pub struct RetryingModelAdapter<A> {
    inner: A,
    policy: Arc<dyn ModelRetryPolicy>,
}

impl<A> RetryingModelAdapter<A> {
    pub fn new(inner: A, policy: impl ModelRetryPolicy + 'static) -> Self {
        Self {
            inner,
            policy: Arc::new(policy),
        }
    }

    pub fn inner(&self) -> &A {
        &self.inner
    }
}

impl<A> fmt::Debug for RetryingModelAdapter<A>
where
    A: fmt::Debug,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RetryingModelAdapter")
            .field("inner", &self.inner)
            .finish_non_exhaustive()
    }
}

#[async_trait]
impl<A> ModelAdapter for RetryingModelAdapter<A>
where
    A: ModelAdapter + Send + Sync,
{
    async fn complete(&self, request: ModelRequest) -> Result<ModelOutput, ModelAdapterError> {
        let mut attempt = 1;
        loop {
            match self.inner.complete(request.clone()).await {
                Ok(output) => return Ok(output),
                Err(error) => match self.policy.decide(&error, attempt) {
                    ModelRetryDecision::Retry { delay, .. } => {
                        tokio::time::sleep(delay).await;
                        attempt += 1;
                    }
                    ModelRetryDecision::DoNotRetry { .. }
                    | ModelRetryDecision::Exhausted { .. } => return Err(error),
                },
            }
        }
    }
}

#[async_trait]
impl<A> StreamingModelAdapter for RetryingModelAdapter<A>
where
    A: StreamingModelAdapter + Send + Sync,
{
    async fn stream(
        &self,
        request: ModelRequest,
    ) -> Result<Vec<ModelStreamEvent>, ModelAdapterError> {
        let mut attempt = 1;
        loop {
            match self.inner.stream(request.clone()).await {
                Ok(events) => return Ok(events),
                Err(error) => match self.policy.decide(&error, attempt) {
                    ModelRetryDecision::Retry { delay, .. } => {
                        tokio::time::sleep(delay).await;
                        attempt += 1;
                    }
                    ModelRetryDecision::DoNotRetry { .. }
                    | ModelRetryDecision::Exhausted { .. } => return Err(error),
                },
            }
        }
    }
}

/// Extended adapter that supports tool / function calling.
///
/// Implementors translate provider-neutral [`ToolDefinition`]s into the
/// provider's native wire format and parse tool call responses back into
/// [`ModelOutput::ToolCalls`].
#[async_trait]
pub trait ToolCallingModelAdapter: ModelAdapter {
    async fn complete_with_tools(
        &self,
        request: ModelRequest,
        tools: Vec<ToolDefinition>,
        tool_choice: ToolChoice,
    ) -> Result<ModelOutput, ModelAdapterError>;
}

#[async_trait]
impl<A> ToolCallingModelAdapter for ObservedModelAdapter<A>
where
    A: ToolCallingModelAdapter + Send + Sync,
{
    async fn complete_with_tools(
        &self,
        request: ModelRequest,
        tools: Vec<ToolDefinition>,
        tool_choice: ToolChoice,
    ) -> Result<ModelOutput, ModelAdapterError> {
        let started = Instant::now();
        let model_id = request.model_id.clone();
        let metadata = redacted_model_metadata(&request.metadata);
        let result = self
            .inner
            .complete_with_tools(request, tools, tool_choice)
            .await;
        let trace = match &result {
            Ok(output) => ModelCallTrace {
                operation: ModelCallOperation::ToolCall,
                model_id,
                latency_ms: duration_to_millis(started.elapsed()),
                usage: output.usage().cloned(),
                outcome: ModelTraceOutcome::Success,
                error_class: None,
                metadata,
            },
            Err(error) => ModelCallTrace {
                operation: ModelCallOperation::ToolCall,
                model_id,
                latency_ms: duration_to_millis(started.elapsed()),
                usage: None,
                outcome: ModelTraceOutcome::Error,
                error_class: Some(classify_model_adapter_error(error)),
                metadata,
            },
        };
        self.sink.record(trace);
        result
    }
}

/// Tool-calling adapter wrapper that enforces registry capabilities before calls.
pub struct CapabilityGatedToolAdapter<A> {
    inner: A,
    registry: Arc<ModelRegistry>,
}

impl<A> CapabilityGatedToolAdapter<A> {
    pub fn new(inner: A, registry: Arc<ModelRegistry>) -> Self {
        Self { inner, registry }
    }

    pub fn registry(&self) -> &ModelRegistry {
        self.registry.as_ref()
    }
}

#[async_trait]
impl<A> ModelAdapter for CapabilityGatedToolAdapter<A>
where
    A: ToolCallingModelAdapter + Send + Sync,
{
    async fn complete(&self, request: ModelRequest) -> Result<ModelOutput, ModelAdapterError> {
        self.inner.complete(request).await
    }
}

#[async_trait]
impl<A> ToolCallingModelAdapter for CapabilityGatedToolAdapter<A>
where
    A: ToolCallingModelAdapter + Send + Sync,
{
    async fn complete_with_tools(
        &self,
        request: ModelRequest,
        tools: Vec<ToolDefinition>,
        tool_choice: ToolChoice,
    ) -> Result<ModelOutput, ModelAdapterError> {
        if !tools.is_empty() || matches!(tool_choice, ToolChoice::Required | ToolChoice::Named(_)) {
            self.registry.require_tools(&request.model_id)?;
        }
        self.inner
            .complete_with_tools(request, tools, tool_choice)
            .await
    }
}

/// Deterministic static adapter for tests and early runtime integration.
#[derive(Debug, Clone)]
pub struct StaticModelAdapter {
    response_prefix: String,
}

impl Default for StaticModelAdapter {
    fn default() -> Self {
        Self {
            response_prefix: "Static model response".to_string(),
        }
    }
}

impl StaticModelAdapter {
    pub fn new(response_prefix: impl Into<String>) -> Self {
        Self {
            response_prefix: response_prefix.into(),
        }
    }
}

#[async_trait]
impl ModelAdapter for StaticModelAdapter {
    async fn complete(&self, request: ModelRequest) -> Result<ModelOutput, ModelAdapterError> {
        if request.messages.is_empty() {
            return Err(ModelAdapterError::EmptyRequest);
        }

        let last_user_text = request
            .messages
            .iter()
            .rev()
            .find(|message| message.role == ModelRole::User)
            .map(|message| message.text.as_str())
            .unwrap_or("");

        Ok(ModelOutput::Text {
            text: format!(
                "{} for {} with {} message(s): {}",
                self.response_prefix,
                request.model_id,
                request.messages.len(),
                last_user_text
            ),
            usage: Some(ModelUsage {
                input_tokens: request
                    .messages
                    .iter()
                    .map(|message| count_words(&message.text))
                    .sum(),
                output_tokens: count_words(&self.response_prefix) + count_words(last_user_text),
            }),
        })
    }
}

/// Deterministic test-only streaming adapter for tests and early runtime work.
#[derive(Debug, Clone, Default)]
pub struct StaticStreamingModelAdapter {
    inner: StaticModelAdapter,
}

impl StaticStreamingModelAdapter {
    pub fn new(response_prefix: impl Into<String>) -> Self {
        Self {
            inner: StaticModelAdapter::new(response_prefix),
        }
    }
}

#[async_trait]
impl ModelAdapter for StaticStreamingModelAdapter {
    async fn complete(&self, request: ModelRequest) -> Result<ModelOutput, ModelAdapterError> {
        self.inner.complete(request).await
    }
}

#[async_trait]
impl StreamingModelAdapter for StaticStreamingModelAdapter {
    async fn stream(
        &self,
        request: ModelRequest,
    ) -> Result<Vec<ModelStreamEvent>, ModelAdapterError> {
        let model_id = request.model_id.clone();
        let output = self.complete(request).await?;
        let mut events = vec![ModelStreamEvent::Started { model_id }];

        match output {
            ModelOutput::Text { text, usage } => {
                events.extend(
                    text_delta_chunks(&text)
                        .into_iter()
                        .map(|delta| ModelStreamEvent::TextDelta { delta }),
                );
                if let Some(usage) = usage {
                    events.push(ModelStreamEvent::Usage { usage });
                }
                events.push(ModelStreamEvent::Finished {
                    reason: ModelStreamFinishReason::Stop,
                });
            }
            ModelOutput::ToolCalls {
                content,
                tool_calls,
                usage,
            } => {
                if let Some(content) = content {
                    events.extend(
                        text_delta_chunks(&content)
                            .into_iter()
                            .map(|delta| ModelStreamEvent::TextDelta { delta }),
                    );
                }
                for (index, call) in tool_calls.into_iter().enumerate() {
                    events.push(ModelStreamEvent::ToolCallDelta(ModelToolCallDelta {
                        index,
                        id_delta: Some(call.id),
                        name_delta: Some(call.name),
                        arguments_delta: Some(call.raw_arguments),
                    }));
                }
                if let Some(usage) = usage {
                    events.push(ModelStreamEvent::Usage { usage });
                }
                events.push(ModelStreamEvent::Finished {
                    reason: ModelStreamFinishReason::ToolCalls,
                });
            }
        }

        Ok(events)
    }
}

fn count_words(text: &str) -> u32 {
    text.split_whitespace().count() as u32
}

fn text_delta_chunks(text: &str) -> Vec<String> {
    if text.is_empty() {
        return Vec::new();
    }

    let mut chunks = Vec::new();
    let mut current = String::new();
    for token in text.split_inclusive(' ') {
        current.push_str(token);
        if current.len() >= 16 {
            chunks.push(std::mem::take(&mut current));
        }
    }
    if !current.is_empty() {
        chunks.push(current);
    }
    chunks
}

/// Deterministic model registry.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ModelRegistry {
    models: BTreeMap<ModelId, ModelProfile>,
    default_model_id: Option<ModelId>,
}

impl ModelRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(&mut self, profile: ModelProfile) -> Result<(), ModelAdapterError> {
        if self.models.contains_key(&profile.id) {
            return Err(ModelAdapterError::DuplicateModelId(profile.id));
        }
        self.models.insert(profile.id.clone(), profile);
        Ok(())
    }

    pub fn set_default(&mut self, model_id: impl Into<ModelId>) -> Result<(), ModelAdapterError> {
        let model_id = model_id.into();
        if !self.models.contains_key(&model_id) {
            return Err(ModelAdapterError::ModelNotFound(model_id));
        }
        self.default_model_id = Some(model_id);
        Ok(())
    }

    pub fn get(&self, model_id: &ModelId) -> Option<&ModelProfile> {
        self.models.get(model_id)
    }

    pub fn require_tools(&self, model_id: &ModelId) -> Result<&ModelProfile, ModelAdapterError> {
        self.require_capability(model_id, "tools", ModelProfile::supports_tools)
    }

    pub fn require_streaming(
        &self,
        model_id: &ModelId,
    ) -> Result<&ModelProfile, ModelAdapterError> {
        self.require_capability(model_id, "streaming", ModelProfile::supports_streaming)
    }

    pub fn require_json(&self, model_id: &ModelId) -> Result<&ModelProfile, ModelAdapterError> {
        self.require_capability(model_id, "json", ModelProfile::supports_json)
    }

    pub fn require_vision(&self, model_id: &ModelId) -> Result<&ModelProfile, ModelAdapterError> {
        self.require_capability(model_id, "vision", ModelProfile::supports_vision)
    }

    fn require_capability(
        &self,
        model_id: &ModelId,
        capability: &'static str,
        predicate: impl Fn(&ModelProfile) -> bool,
    ) -> Result<&ModelProfile, ModelAdapterError> {
        let profile = self
            .models
            .get(model_id)
            .ok_or_else(|| ModelAdapterError::ModelNotFound(model_id.clone()))?;
        if predicate(profile) {
            Ok(profile)
        } else {
            Err(ModelAdapterError::UnsupportedModelCapability {
                model_id: model_id.clone(),
                capability,
            })
        }
    }

    pub fn default_model(&self) -> Result<&ModelProfile, ModelAdapterError> {
        let model_id = self
            .default_model_id
            .as_ref()
            .ok_or(ModelAdapterError::DefaultModelNotSet)?;
        self.models
            .get(model_id)
            .ok_or_else(|| ModelAdapterError::ModelNotFound(model_id.clone()))
    }

    pub fn len(&self) -> usize {
        self.models.len()
    }

    pub fn is_empty(&self) -> bool {
        self.models.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    #[derive(Debug)]
    struct EchoToolAdapter;

    #[async_trait]
    impl ModelAdapter for EchoToolAdapter {
        async fn complete(&self, _request: ModelRequest) -> Result<ModelOutput, ModelAdapterError> {
            Ok(ModelOutput::Text {
                text: "ok".to_string(),
                usage: None,
            })
        }
    }

    #[async_trait]
    impl ToolCallingModelAdapter for EchoToolAdapter {
        async fn complete_with_tools(
            &self,
            _request: ModelRequest,
            _tools: Vec<ToolDefinition>,
            _tool_choice: ToolChoice,
        ) -> Result<ModelOutput, ModelAdapterError> {
            Ok(ModelOutput::ToolCalls {
                content: None,
                tool_calls: vec![],
                usage: None,
            })
        }
    }

    #[derive(Debug)]
    struct FlakyModelAdapter {
        failures_before_success: u32,
        calls: Arc<AtomicU32>,
        error: ModelAdapterError,
    }

    #[async_trait]
    impl ModelAdapter for FlakyModelAdapter {
        async fn complete(&self, _request: ModelRequest) -> Result<ModelOutput, ModelAdapterError> {
            let call = self.calls.fetch_add(1, Ordering::SeqCst) + 1;
            if call <= self.failures_before_success {
                return Err(self.error.clone());
            }
            Ok(ModelOutput::Text {
                text: "ok".to_string(),
                usage: None,
            })
        }
    }

    fn no_delay_retry_policy(max_attempts: u32) -> DefaultModelRetryPolicy {
        DefaultModelRetryPolicy::new(ModelRetryConfig {
            initial_delay: Duration::ZERO,
            multiplier: 2,
            max_delay: Duration::ZERO,
            max_attempts,
        })
    }

    fn task_schema() -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "required": ["title", "priority"],
            "properties": {
                "title": {"type": "string"},
                "priority": {"type": "integer"},
                "done": {"type": "boolean"}
            }
        })
    }

    #[test]
    fn structured_output_validator_accepts_matching_json() {
        let format = StructuredOutputFormat::json_schema("task", task_schema());
        let output = StructuredOutputValidator::new(&format)
            .validate(r#"{"title":"ship","priority":2,"done":false}"#)
            .unwrap();

        assert_eq!(output.value["title"], "ship");
        assert_eq!(output.value["priority"], 2);
    }

    #[test]
    fn structured_output_validator_rejects_malformed_json() {
        let format = StructuredOutputFormat::json_schema("task", task_schema());
        let error = StructuredOutputValidator::new(&format)
            .validate("not json")
            .unwrap_err();

        assert!(matches!(
            error,
            ModelAdapterError::MalformedStructuredOutput(_)
        ));
    }

    #[test]
    fn structured_output_validator_rejects_schema_violation() {
        let format = StructuredOutputFormat::json_schema("task", task_schema());
        let error = StructuredOutputValidator::new(&format)
            .validate(r#"{"title":"ship","priority":"high"}"#)
            .unwrap_err();

        assert!(matches!(
            error,
            ModelAdapterError::StructuredOutputSchemaViolation(_)
        ));
    }

    struct StaticRepairPolicy {
        repaired: String,
    }

    impl StructuredOutputRepairPolicy for StaticRepairPolicy {
        fn repair(
            &self,
            _raw: &str,
            _format: &StructuredOutputFormat,
            _error: &ModelAdapterError,
        ) -> Option<String> {
            Some(self.repaired.clone())
        }
    }

    #[test]
    fn structured_output_validator_uses_repair_policy() {
        let format = StructuredOutputFormat::json_schema("task", task_schema());
        let repair = StaticRepairPolicy {
            repaired: r#"{"title":"ship","priority":1}"#.to_string(),
        };
        let output = StructuredOutputValidator::with_repair_policy(&format, &repair)
            .validate("not json")
            .unwrap();

        assert_eq!(output.value["priority"], 1);
    }

    #[test]
    fn structured_output_validator_returns_typed_repair_failure() {
        let format = StructuredOutputFormat::json_schema("task", task_schema());
        let repair = StaticRepairPolicy {
            repaired: r#"{"title":"ship","priority":"high"}"#.to_string(),
        };
        let error = StructuredOutputValidator::with_repair_policy(&format, &repair)
            .validate("not json")
            .unwrap_err();

        assert!(matches!(
            error,
            ModelAdapterError::StructuredOutputRepairFailed(_)
        ));
    }

    #[test]
    fn retry_classifier_treats_structured_output_errors_as_validation() {
        assert_eq!(
            classify_model_adapter_error(&ModelAdapterError::StructuredOutputSchemaViolation(
                "missing title".to_string()
            )),
            ModelRetryErrorClass::Validation
        );
    }

    #[tokio::test]
    async fn observed_adapter_records_success_trace_with_usage_and_redacted_metadata() {
        let sink = Arc::new(MemoryModelTraceSink::new());
        let adapter = ObservedModelAdapter::new(StaticModelAdapter::default(), sink.clone());
        let mut request = ModelRequest::new("test/default", vec![ModelMessage::user("hello")]);
        request
            .metadata
            .insert("request_id".to_string(), "req-123".to_string());
        request
            .metadata
            .insert("api_key".to_string(), "sk-secret".to_string());

        let output = adapter.complete(request).await.unwrap();

        assert!(output.usage().is_some());
        let traces = sink.traces();
        assert_eq!(traces.len(), 1);
        let trace = &traces[0];
        assert_eq!(trace.operation, ModelCallOperation::Complete);
        assert_eq!(trace.model_id, ModelId::from("test/default"));
        assert_eq!(trace.outcome, ModelTraceOutcome::Success);
        assert!(trace.usage.is_some());
        assert_eq!(trace.metadata["request_id"], "req-123");
        assert_eq!(trace.metadata["api_key"], "<redacted>");
    }

    #[tokio::test]
    async fn observed_adapter_records_error_trace_with_error_class() {
        let sink = Arc::new(MemoryModelTraceSink::new());
        let adapter = ObservedModelAdapter::new(StaticModelAdapter::default(), sink.clone());
        let error = adapter
            .complete(ModelRequest::new("test/default", vec![]))
            .await
            .unwrap_err();

        assert_eq!(error, ModelAdapterError::EmptyRequest);
        let traces = sink.traces();
        assert_eq!(traces[0].outcome, ModelTraceOutcome::Error);
        assert_eq!(
            traces[0].error_class,
            Some(ModelRetryErrorClass::EmptyRequest)
        );
    }

    #[tokio::test]
    async fn observed_streaming_adapter_records_usage_without_prompt_text() {
        let sink = Arc::new(MemoryModelTraceSink::new());
        let adapter =
            ObservedModelAdapter::new(StaticStreamingModelAdapter::default(), sink.clone());
        let mut request =
            ModelRequest::new("test/default", vec![ModelMessage::user("secret prompt")]);
        request
            .metadata
            .insert("auth_token".to_string(), "token-secret".to_string());

        let events = adapter.stream(request).await.unwrap();

        assert!(
            events
                .iter()
                .any(|event| matches!(event, ModelStreamEvent::Usage { .. }))
        );
        let traces = sink.traces();
        assert_eq!(traces.len(), 1);
        assert_eq!(traces[0].operation, ModelCallOperation::Stream);
        assert_eq!(traces[0].outcome, ModelTraceOutcome::Success);
        assert!(traces[0].usage.is_some());
        assert_eq!(traces[0].metadata["auth_token"], "<redacted>");
        let serialized = serde_json::to_string(&traces[0]).unwrap();
        assert!(!serialized.contains("secret prompt"));
        assert!(!serialized.contains("token-secret"));
    }

    #[tokio::test]
    async fn observed_tool_adapter_records_tool_call_trace() {
        let sink = Arc::new(MemoryModelTraceSink::new());
        let adapter = ObservedModelAdapter::new(EchoToolAdapter, sink.clone());
        let output = adapter
            .complete_with_tools(
                ModelRequest::new("fake/tools", vec![ModelMessage::user("search")]),
                vec![ToolDefinition {
                    name: "knowledge.search".to_string(),
                    description: "Search".to_string(),
                    input_schema: serde_json::json!({"type":"object"}),
                }],
                ToolChoice::Auto,
            )
            .await
            .unwrap();

        assert!(matches!(output, ModelOutput::ToolCalls { .. }));
        let traces = sink.traces();
        assert_eq!(traces.len(), 1);
        assert_eq!(traces[0].operation, ModelCallOperation::ToolCall);
        assert_eq!(traces[0].outcome, ModelTraceOutcome::Success);
    }

    #[test]
    fn retry_classifier_marks_429_5xx_timeout_and_validation() {
        assert_eq!(
            classify_model_adapter_error(&ModelAdapterError::RateLimitExceeded),
            ModelRetryErrorClass::RateLimited
        );
        assert_eq!(
            classify_model_adapter_error(&ModelAdapterError::ExecutorFailed(
                "provider returned HTTP 503 temporarily unavailable".to_string()
            )),
            ModelRetryErrorClass::TransientNetwork
        );
        assert_eq!(
            classify_model_adapter_error(&ModelAdapterError::HttpError(
                "request timeout exceeded".to_string()
            )),
            ModelRetryErrorClass::Timeout
        );
        assert_eq!(
            classify_model_adapter_error(&ModelAdapterError::ExecutorFailed(
                "invalid request: bad request 400".to_string()
            )),
            ModelRetryErrorClass::Validation
        );
    }

    #[test]
    fn retry_backoff_is_deterministic_and_capped() {
        let policy = DefaultModelRetryPolicy::new(ModelRetryConfig {
            initial_delay: Duration::from_millis(100),
            multiplier: 2,
            max_delay: Duration::from_millis(250),
            max_attempts: 5,
        });

        assert_eq!(policy.backoff_delay(0), None);
        assert_eq!(policy.backoff_delay(1), Some(Duration::from_millis(100)));
        assert_eq!(policy.backoff_delay(2), Some(Duration::from_millis(200)));
        assert_eq!(policy.backoff_delay(3), Some(Duration::from_millis(250)));
        assert_eq!(policy.backoff_delay(5), None);
    }

    #[test]
    fn circuit_breaker_opens_after_repeated_rate_limits() {
        let breaker = ModelCircuitBreaker::new(
            "openai",
            ModelCircuitBreakerConfig {
                failure_threshold: 2,
                cooldown: Duration::from_secs(60),
            },
        );

        assert_eq!(breaker.health().state, ModelCircuitBreakerState::Closed);
        breaker.record_error(&ModelAdapterError::RateLimitExceeded);
        assert_eq!(breaker.health().state, ModelCircuitBreakerState::Closed);
        breaker.record_error(&ModelAdapterError::RateLimitExceeded);

        let health = breaker.health();
        assert_eq!(health.provider, "openai");
        assert_eq!(health.state, ModelCircuitBreakerState::Open);
        assert_eq!(health.consecutive_failures, 2);
        assert!(breaker.before_call().is_err());
    }

    #[test]
    fn circuit_breaker_ignores_validation_errors_and_resets_on_success() {
        let breaker = ModelCircuitBreaker::new(
            "anthropic",
            ModelCircuitBreakerConfig {
                failure_threshold: 1,
                cooldown: Duration::from_secs(60),
            },
        );

        breaker.record_error(&ModelAdapterError::MalformedToolCallArguments(
            "bad json".to_string(),
        ));
        assert_eq!(breaker.health().state, ModelCircuitBreakerState::Closed);

        breaker.record_error(&ModelAdapterError::RateLimitExceeded);
        assert_eq!(breaker.health().state, ModelCircuitBreakerState::Open);
        breaker.record_success();

        let health = breaker.health();
        assert_eq!(health.state, ModelCircuitBreakerState::Closed);
        assert_eq!(health.consecutive_failures, 0);
        assert!(breaker.before_call().is_ok());
    }

    #[test]
    fn circuit_breaker_allows_half_open_after_cooldown() {
        let breaker = ModelCircuitBreaker::new(
            "local",
            ModelCircuitBreakerConfig {
                failure_threshold: 1,
                cooldown: Duration::ZERO,
            },
        );

        breaker.record_error(&ModelAdapterError::RateLimitExceeded);
        assert_eq!(breaker.health().state, ModelCircuitBreakerState::Open);

        assert!(breaker.before_call().is_ok());
        assert_eq!(breaker.health().state, ModelCircuitBreakerState::HalfOpen);
    }

    #[tokio::test]
    async fn circuit_breaking_adapter_blocks_calls_after_threshold() {
        let calls = Arc::new(AtomicU32::new(0));
        let inner = FlakyModelAdapter {
            failures_before_success: 99,
            calls: Arc::clone(&calls),
            error: ModelAdapterError::RateLimitExceeded,
        };
        let breaker = ModelCircuitBreaker::new(
            "openai",
            ModelCircuitBreakerConfig {
                failure_threshold: 1,
                cooldown: Duration::from_secs(60),
            },
        );
        let adapter = CircuitBreakingModelAdapter::new(inner, breaker);
        let request = ModelRequest::new("test/default", vec![ModelMessage::user("hi")]);

        assert_eq!(
            adapter.complete(request.clone()).await.unwrap_err(),
            ModelAdapterError::RateLimitExceeded
        );
        let err = adapter.complete(request).await.unwrap_err();

        assert!(
            matches!(err, ModelAdapterError::ExecutorFailed(message) if message.contains("circuit breaker open"))
        );
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn retrying_adapter_retries_rate_limit_then_succeeds() {
        let calls = Arc::new(AtomicU32::new(0));
        let inner = FlakyModelAdapter {
            failures_before_success: 2,
            calls: Arc::clone(&calls),
            error: ModelAdapterError::RateLimitExceeded,
        };
        let adapter = RetryingModelAdapter::new(inner, no_delay_retry_policy(3));

        let output = adapter
            .complete(ModelRequest::new(
                "test/default",
                vec![ModelMessage::user("hi")],
            ))
            .await
            .unwrap();

        assert_eq!(output.text(), Some("ok"));
        assert_eq!(calls.load(Ordering::SeqCst), 3);
    }

    #[tokio::test]
    async fn retrying_adapter_does_not_retry_validation_error() {
        let calls = Arc::new(AtomicU32::new(0));
        let inner = FlakyModelAdapter {
            failures_before_success: 1,
            calls: Arc::clone(&calls),
            error: ModelAdapterError::MalformedToolCallArguments("bad json".to_string()),
        };
        let adapter = RetryingModelAdapter::new(inner, no_delay_retry_policy(3));

        let err = adapter
            .complete(ModelRequest::new(
                "test/default",
                vec![ModelMessage::user("hi")],
            ))
            .await
            .unwrap_err();

        assert_eq!(
            err,
            ModelAdapterError::MalformedToolCallArguments("bad json".to_string())
        );
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn retrying_adapter_stops_at_max_attempts() {
        let calls = Arc::new(AtomicU32::new(0));
        let inner = FlakyModelAdapter {
            failures_before_success: 99,
            calls: Arc::clone(&calls),
            error: ModelAdapterError::RateLimitExceeded,
        };
        let adapter = RetryingModelAdapter::new(inner, no_delay_retry_policy(2));

        let err = adapter
            .complete(ModelRequest::new(
                "test/default",
                vec![ModelMessage::user("hi")],
            ))
            .await
            .unwrap_err();

        assert_eq!(err, ModelAdapterError::RateLimitExceeded);
        assert_eq!(calls.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn stream_event_serde_roundtrip() {
        let event = ModelStreamEvent::TextDelta {
            delta: "hello".to_string(),
        };

        let json = serde_json::to_string(&event).unwrap();
        let decoded: ModelStreamEvent = serde_json::from_str(&json).unwrap();

        assert_eq!(decoded, event);
    }

    #[test]
    fn tool_call_delta_serde_roundtrip() {
        let delta = ModelToolCallDelta {
            index: 1,
            id_delta: Some("call_1".to_string()),
            name_delta: Some("knowledge.search".to_string()),
            arguments_delta: Some(r#"{"query":"agent os"}"#.to_string()),
        };

        let json = serde_json::to_string(&delta).unwrap();
        let decoded: ModelToolCallDelta = serde_json::from_str(&json).unwrap();

        assert_eq!(decoded, delta);
    }

    #[test]
    fn stream_finish_reason_serde() {
        assert_eq!(
            serde_json::to_string(&ModelStreamFinishReason::ToolCalls).unwrap(),
            "\"tool_calls\""
        );
        assert_eq!(
            serde_json::from_str::<ModelStreamFinishReason>("\"length\"").unwrap(),
            ModelStreamFinishReason::Length
        );
    }

    #[tokio::test]
    async fn static_streaming_adapter_emits_started_text_usage_finished() {
        let adapter = StaticStreamingModelAdapter::default();
        let request = ModelRequest::new(
            "test/default",
            vec![ModelMessage::user("Summarize this text")],
        );

        let events = adapter.stream(request).await.unwrap();

        assert!(matches!(
            events.first(),
            Some(ModelStreamEvent::Started { model_id }) if model_id == &ModelId::from("test/default")
        ));
        assert!(
            events
                .iter()
                .any(|event| matches!(event, ModelStreamEvent::TextDelta { .. }))
        );
        assert!(
            events
                .iter()
                .any(|event| matches!(event, ModelStreamEvent::Usage { .. }))
        );
        assert_eq!(
            events.last(),
            Some(&ModelStreamEvent::Finished {
                reason: ModelStreamFinishReason::Stop
            })
        );
    }

    #[tokio::test]
    async fn static_streaming_adapter_rejects_empty_request() {
        let adapter = StaticStreamingModelAdapter::default();
        let request = ModelRequest::new("test/default", vec![]);

        let error = adapter.stream(request).await.unwrap_err();

        assert_eq!(error, ModelAdapterError::EmptyRequest);
    }

    #[test]
    fn stream_accumulator_collects_text_and_usage() {
        let events = vec![
            ModelStreamEvent::Started {
                model_id: ModelId::from("test/default"),
            },
            ModelStreamEvent::TextDelta {
                delta: "hello ".to_string(),
            },
            ModelStreamEvent::TextDelta {
                delta: "world".to_string(),
            },
            ModelStreamEvent::Usage {
                usage: ModelUsage {
                    input_tokens: 2,
                    output_tokens: 3,
                },
            },
            ModelStreamEvent::Finished {
                reason: ModelStreamFinishReason::Stop,
            },
        ];

        let accumulator = ModelStreamAccumulator::from_events(&events);

        assert_eq!(accumulator.text, "hello world");
        assert_eq!(accumulator.usage.unwrap().total_tokens(), 5);
        assert_eq!(accumulator.finished, Some(ModelStreamFinishReason::Stop));
        assert!(accumulator.errors.is_empty());
    }

    #[tokio::test]
    async fn streaming_adapter_complete_matches_accumulated_text() {
        let adapter = StaticStreamingModelAdapter::new("Streaming test-only response");
        let request = ModelRequest::new(
            "test/default",
            vec![ModelMessage::user("Summarize this text")],
        );

        let complete = adapter.complete(request.clone()).await.unwrap();
        let events = adapter.stream(request).await.unwrap();
        let accumulator = ModelStreamAccumulator::from_events(&events);

        assert_eq!(complete.text(), Some(accumulator.text.as_str()));
        assert_eq!(complete.usage(), accumulator.usage.as_ref());
    }

    #[test]
    fn model_id_display_and_serde_roundtrip() {
        let id = ModelId::from("test/default");
        assert_eq!(id.to_string(), "test/default");

        let json = serde_json::to_string(&id).unwrap();
        let decoded: ModelId = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded, id);
    }

    #[test]
    fn model_provider_serde_uses_snake_case() {
        let json = serde_json::to_string(&ModelProvider::OpenAi).unwrap();
        assert_eq!(json, "\"open_ai\"");
    }

    #[test]
    fn model_request_serializes_messages_in_order() {
        let request = ModelRequest::new(
            "test/default",
            vec![
                ModelMessage::system("You are concise."),
                ModelMessage::user("Hello"),
                ModelMessage::assistant("Hi"),
                ModelMessage::user("Summarize this"),
            ],
        );

        let json = serde_json::to_string(&request).unwrap();
        let decoded: ModelRequest = serde_json::from_str(&json).unwrap();

        assert_eq!(decoded.messages[0].role, ModelRole::System);
        assert_eq!(decoded.messages[1].role, ModelRole::User);
        assert_eq!(decoded.messages[2].role, ModelRole::Assistant);
        assert_eq!(decoded.messages[3].text, "Summarize this");
    }

    #[tokio::test]
    async fn static_adapter_returns_deterministic_response() {
        let adapter = StaticModelAdapter::default();
        let request = ModelRequest::new(
            "test/default",
            vec![
                ModelMessage::system("You are concise."),
                ModelMessage::user("Summarize this text"),
            ],
        );

        let output = adapter.complete(request).await.unwrap();

        match output {
            ModelOutput::Text { text, usage } => {
                assert_eq!(
                    text,
                    "Static model response for test/default with 2 message(s): Summarize this text"
                );
                assert!(usage.is_some());
            }
            _ => panic!("expected text output"),
        }
    }

    #[tokio::test]
    async fn static_adapter_rejects_empty_request() {
        let adapter = StaticModelAdapter::default();
        let request = ModelRequest::new("test/default", vec![]);

        let error = adapter.complete(request).await.unwrap_err();

        assert_eq!(error, ModelAdapterError::EmptyRequest);
    }

    #[test]
    fn model_usage_accounts_total_tokens() {
        let usage = ModelUsage {
            input_tokens: 10,
            output_tokens: 7,
        };

        assert_eq!(usage.total_tokens(), 17);
    }

    #[test]
    fn model_registry_registers_and_resolves_default_model() {
        let mut registry = ModelRegistry::new();
        registry
            .register(ModelProfile::test_profile("test/default"))
            .unwrap();
        registry.set_default("test/default").unwrap();

        let default = registry.default_model().unwrap();

        assert_eq!(default.id, ModelId::from("test/default"));
        assert_eq!(default.provider, ModelProvider::Test);
    }

    #[test]
    fn model_registry_rejects_duplicate_model_id() {
        let mut registry = ModelRegistry::new();
        registry
            .register(ModelProfile::test_profile("test/default"))
            .unwrap();

        let error = registry
            .register(ModelProfile::test_profile("test/default"))
            .unwrap_err();

        assert_eq!(
            error,
            ModelAdapterError::DuplicateModelId(ModelId::from("test/default"))
        );
    }

    #[test]
    fn model_registry_tracks_provider_neutral_capabilities() {
        let profile = ModelProfile::new(
            "openai/gpt-test",
            ModelProvider::OpenAi,
            "GPT Test",
            ModelCapabilities::tool_calling()
                .streaming(true)
                .vision(true)
                .max_context_tokens(128_000),
        );

        assert!(profile.supports_tools());
        assert!(profile.supports_streaming());
        assert!(profile.supports_vision());
        assert!(profile.supports_json());
        assert_eq!(profile.max_context_tokens(), Some(128_000));
    }

    #[test]
    fn model_registry_requires_capability_before_tool_use() {
        let mut registry = ModelRegistry::new();
        registry
            .register(ModelProfile::test_profile("fake/text-only"))
            .unwrap();
        registry
            .register(ModelProfile::new(
                "fake/tools",
                ModelProvider::Test,
                "Fake Tools",
                ModelCapabilities::tool_calling(),
            ))
            .unwrap();

        let text_only = ModelId::from("fake/text-only");
        let tool_model = ModelId::from("fake/tools");

        assert!(matches!(
            registry.require_tools(&text_only),
            Err(ModelAdapterError::UnsupportedModelCapability {
                capability: "tools",
                ..
            })
        ));
        assert_eq!(registry.require_tools(&tool_model).unwrap().id, tool_model);
    }

    #[tokio::test]
    async fn capability_gated_tool_adapter_blocks_unsupported_model() {
        let mut registry = ModelRegistry::new();
        registry
            .register(ModelProfile::test_profile("fake/text-only"))
            .unwrap();
        let adapter = CapabilityGatedToolAdapter::new(EchoToolAdapter, Arc::new(registry));
        let err = adapter
            .complete_with_tools(
                ModelRequest::new("fake/text-only", vec![ModelMessage::user("search")]),
                vec![ToolDefinition {
                    name: "knowledge.search".to_string(),
                    description: "Search".to_string(),
                    input_schema: serde_json::json!({"type":"object"}),
                }],
                ToolChoice::Auto,
            )
            .await
            .unwrap_err();

        assert!(matches!(
            err,
            ModelAdapterError::UnsupportedModelCapability {
                capability: "tools",
                ..
            }
        ));
    }

    #[tokio::test]
    async fn capability_gated_tool_adapter_allows_supported_model() {
        let mut registry = ModelRegistry::new();
        registry
            .register(ModelProfile::new(
                "fake/tools",
                ModelProvider::Test,
                "Fake Tools",
                ModelCapabilities::tool_calling(),
            ))
            .unwrap();
        let adapter = CapabilityGatedToolAdapter::new(EchoToolAdapter, Arc::new(registry));
        let output = adapter
            .complete_with_tools(
                ModelRequest::new("fake/tools", vec![ModelMessage::user("search")]),
                vec![ToolDefinition {
                    name: "knowledge.search".to_string(),
                    description: "Search".to_string(),
                    input_schema: serde_json::json!({"type":"object"}),
                }],
                ToolChoice::Auto,
            )
            .await
            .unwrap();

        assert!(matches!(output, ModelOutput::ToolCalls { .. }));
    }

    #[test]
    fn model_registry_rejects_missing_default_model() {
        let mut registry = ModelRegistry::new();
        registry
            .register(ModelProfile::test_profile("test/default"))
            .unwrap();

        let error = registry.set_default("missing").unwrap_err();

        assert_eq!(
            error,
            ModelAdapterError::ModelNotFound(ModelId::from("missing"))
        );
    }

    // ---- PR 58: Tool calling type roundtrip tests ----

    #[test]
    fn tool_definition_serde_roundtrip() {
        let def = ToolDefinition {
            name: "knowledge.search".to_string(),
            description: "Search the knowledge base".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "query": { "type": "string" }
                },
                "required": ["query"]
            }),
        };

        let json = serde_json::to_string(&def).unwrap();
        let decoded: ToolDefinition = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded, def);
    }

    #[test]
    fn tool_call_serde_roundtrip() {
        let call = ToolCall {
            id: "call_abc123".to_string(),
            name: "knowledge.search".to_string(),
            arguments: serde_json::json!({"query": "agent os"}),
            raw_arguments: r#"{"query":"agent os"}"#.to_string(),
        };

        let json = serde_json::to_string(&call).unwrap();
        let decoded: ToolCall = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded, call);
    }

    #[test]
    fn tool_choice_serde_variants() {
        let auto = ToolChoice::Auto;
        assert_eq!(serde_json::to_string(&auto).unwrap(), r#""auto""#);

        let none = ToolChoice::None;
        assert_eq!(serde_json::to_string(&none).unwrap(), r#""none""#);

        let required = ToolChoice::Required;
        assert_eq!(serde_json::to_string(&required).unwrap(), r#""required""#);

        let named = ToolChoice::Named("knowledge.search".to_string());
        let named_json = serde_json::to_string(&named).unwrap();
        let named_val: serde_json::Value = serde_json::from_str(&named_json).unwrap();
        assert_eq!(named_val["named"].as_str().unwrap(), "knowledge.search");
    }

    #[test]
    fn model_output_text_serde_roundtrip() {
        let output = ModelOutput::Text {
            text: "Hello!".to_string(),
            usage: Some(ModelUsage {
                input_tokens: 10,
                output_tokens: 5,
            }),
        };

        let json = serde_json::to_string(&output).unwrap();
        let decoded: ModelOutput = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded, output);
    }

    #[test]
    fn model_output_tool_calls_serde_roundtrip() {
        let output = ModelOutput::ToolCalls {
            content: Some("I'll search for you.".to_string()),
            tool_calls: vec![ToolCall {
                id: "call_001".to_string(),
                name: "knowledge.search".to_string(),
                arguments: serde_json::json!({"query": "agent os"}),
                raw_arguments: r#"{"query":"agent os"}"#.to_string(),
            }],
            usage: Some(ModelUsage {
                input_tokens: 50,
                output_tokens: 15,
            }),
        };

        let json = serde_json::to_string(&output).unwrap();
        let decoded: ModelOutput = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded, output);
    }

    #[test]
    fn model_output_text_accessor() {
        let output = ModelOutput::Text {
            text: "Hello".to_string(),
            usage: None,
        };
        assert_eq!(output.text(), Some("Hello"));
        assert!(output.usage().is_none());
    }

    #[test]
    fn model_output_tool_calls_content_accessor() {
        let output = ModelOutput::ToolCalls {
            content: Some("Thinking...".to_string()),
            tool_calls: vec![],
            usage: None,
        };
        assert_eq!(output.text(), Some("Thinking..."));
    }

    #[test]
    fn model_output_tool_calls_none_content_accessor() {
        let output = ModelOutput::ToolCalls {
            content: None,
            tool_calls: vec![],
            usage: None,
        };
        assert_eq!(output.text(), None);
    }

    #[test]
    fn tool_call_empty_arguments_json_object() {
        // Some models return "{}" for no-arg tools
        let call = ToolCall {
            id: "call_empty".to_string(),
            name: "tool.no_args".to_string(),
            arguments: serde_json::json!({}),
            raw_arguments: "{}".to_string(),
        };
        let json = serde_json::to_string(&call).unwrap();
        let decoded: ToolCall = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded.arguments, serde_json::json!({}));
    }
}
