//! # Model Adapter
//!
//! Text-only model adapter abstractions for AgentOS.
//!
//! This crate provides deterministic request/response types, a typed async
//! executor trait, a small model registry, and a fake executor for tests and
//! early runtime work. The [`openai`] module supplies a concrete adapter for
//! any OpenAI-compatible Chat Completions endpoint (DeepSeek, Qwen, vLLM, etc.).

pub mod openai;
pub use openai::{OpenAiCompatibleAdapter, OpenAiProviderConfig};

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fmt;

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
    Fake,
    Custom(String),
}

/// Registry metadata for a model.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelProfile {
    pub id: ModelId,
    pub provider: ModelProvider,
    pub display_name: String,
    pub supports_streaming: bool,
    pub supports_tools: bool,
    pub max_context_tokens: Option<u32>,
}

impl ModelProfile {
    pub fn fake(id: impl Into<ModelId>) -> Self {
        let id = id.into();
        Self {
            display_name: id.to_string(),
            id,
            provider: ModelProvider::Fake,
            supports_streaming: false,
            supports_tools: false,
            max_context_tokens: None,
        }
    }
}

/// Chat role for a text-only model message.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelRole {
    System,
    User,
    Assistant,
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

/// Typed model adapter errors.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ModelAdapterError {
    #[error("empty model request")]
    EmptyRequest,

    #[error("model not found: {0}")]
    ModelNotFound(ModelId),

    #[error("duplicate model id: {0}")]
    DuplicateModelId(ModelId),

    #[error("default model not set")]
    DefaultModelNotSet,

    #[error("executor failed: {0}")]
    ExecutorFailed(String),

    #[error("malformed tool call arguments: {0}")]
    MalformedToolCallArguments(String),

    #[error("missing tool call function name")]
    MissingToolCallName,

    #[error("missing tool call id")]
    MissingToolCallId,
}

/// Async model adapter trait — text-only completion.
#[async_trait]
pub trait ModelAdapter: Send + Sync {
    async fn complete(&self, request: ModelRequest) -> Result<ModelOutput, ModelAdapterError>;
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

/// Deterministic fake adapter for tests and early runtime integration.
#[derive(Debug, Clone)]
pub struct FakeModelAdapter {
    response_prefix: String,
}

impl Default for FakeModelAdapter {
    fn default() -> Self {
        Self {
            response_prefix: "Fake model response".to_string(),
        }
    }
}

impl FakeModelAdapter {
    pub fn new(response_prefix: impl Into<String>) -> Self {
        Self {
            response_prefix: response_prefix.into(),
        }
    }
}

#[async_trait]
impl ModelAdapter for FakeModelAdapter {
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

fn count_words(text: &str) -> u32 {
    text.split_whitespace().count() as u32
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

    #[test]
    fn model_id_display_and_serde_roundtrip() {
        let id = ModelId::from("fake/default");
        assert_eq!(id.to_string(), "fake/default");

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
            "fake/default",
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
    async fn fake_adapter_returns_deterministic_response() {
        let adapter = FakeModelAdapter::default();
        let request = ModelRequest::new(
            "fake/default",
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
                    "Fake model response for fake/default with 2 message(s): Summarize this text"
                );
                assert!(usage.is_some());
            }
            _ => panic!("expected text output"),
        }
    }

    #[tokio::test]
    async fn fake_adapter_rejects_empty_request() {
        let adapter = FakeModelAdapter::default();
        let request = ModelRequest::new("fake/default", vec![]);

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
            .register(ModelProfile::fake("fake/default"))
            .unwrap();
        registry.set_default("fake/default").unwrap();

        let default = registry.default_model().unwrap();

        assert_eq!(default.id, ModelId::from("fake/default"));
        assert_eq!(default.provider, ModelProvider::Fake);
    }

    #[test]
    fn model_registry_rejects_duplicate_model_id() {
        let mut registry = ModelRegistry::new();
        registry
            .register(ModelProfile::fake("fake/default"))
            .unwrap();

        let error = registry
            .register(ModelProfile::fake("fake/default"))
            .unwrap_err();

        assert_eq!(
            error,
            ModelAdapterError::DuplicateModelId(ModelId::from("fake/default"))
        );
    }

    #[test]
    fn model_registry_rejects_missing_default_model() {
        let mut registry = ModelRegistry::new();
        registry
            .register(ModelProfile::fake("fake/default"))
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
