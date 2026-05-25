//! Anthropic Messages API Adapter
//!
//! Implements `ModelAdapter` for the Anthropic Messages API (Claude).
//!
//! Reference: https://docs.anthropic.com/claude/reference/messages-post
//!
//! This adapter:
//! - Uses the Messages API (not the legacy Completions API)
//! - Supports text-only completions and client-side tool use
//! - Configures from environment variables
//! - Maps Anthropic-specific errors to `ModelAdapterError`

use crate::{
    ModelAdapter, ModelAdapterError, ModelOutput, ModelRequest, ModelRole, ModelUsage, ToolCall,
    ToolCallingModelAdapter, ToolChoice, ToolDefinition,
};
use async_trait::async_trait;
use reqwest::Client;
use serde::{Deserialize, Serialize};

/// Anthropic API configuration.
#[derive(Debug, Clone)]
pub struct AnthropicConfig {
    pub api_key: String,
    pub base_url: String,
    pub default_model: String,
    pub api_version: String,
}

impl AnthropicConfig {
    /// Create config from environment variables.
    ///
    /// Expected env vars:
    /// - `ANTHROPIC_API_KEY` (required)
    /// - `ANTHROPIC_BASE_URL` (optional, defaults to https://api.anthropic.com)
    /// - `ANTHROPIC_MODEL` (optional, defaults to claude-3-5-sonnet-20241022)
    pub fn from_env() -> Result<Self, ModelAdapterError> {
        let api_key = std::env::var("ANTHROPIC_API_KEY")
            .map_err(|_| ModelAdapterError::ConfigError("ANTHROPIC_API_KEY not set".to_string()))?;

        let base_url = std::env::var("ANTHROPIC_BASE_URL")
            .unwrap_or_else(|_| "https://api.anthropic.com".to_string());

        let default_model = std::env::var("ANTHROPIC_MODEL")
            .unwrap_or_else(|_| "claude-3-5-sonnet-20241022".to_string());

        Ok(Self {
            api_key,
            base_url,
            default_model,
            api_version: "2023-06-01".to_string(),
        })
    }

    fn messages_url(&self) -> String {
        format!("{}/v1/messages", self.base_url)
    }
}

/// Anthropic Messages API request format.
#[derive(Debug, Serialize)]
#[allow(dead_code)]
struct AnthropicRequest {
    model: String,
    max_tokens: u32,
    messages: Vec<AnthropicMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    system: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f64>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    tools: Vec<AnthropicTool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_choice: Option<AnthropicToolChoice>,
}

/// Anthropic message format.
#[derive(Debug, Serialize, Deserialize)]
struct AnthropicMessage {
    role: String,
    content: AnthropicMessageContent,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(untagged)]
enum AnthropicMessageContent {
    Text(String),
    Blocks(Vec<AnthropicMessageBlock>),
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum AnthropicMessageBlock {
    Text {
        text: String,
    },
    ToolResult {
        tool_use_id: String,
        content: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        is_error: Option<bool>,
    },
}

#[derive(Debug, Serialize)]
struct AnthropicTool {
    name: String,
    description: String,
    input_schema: serde_json::Value,
}

#[derive(Debug, Serialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case")]
enum AnthropicToolChoice {
    Auto,
    Any,
    Tool { name: String },
    None,
}

/// Anthropic Messages API response format.
#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct AnthropicResponse {
    id: String,
    model: String,
    content: Vec<AnthropicContent>,
    usage: AnthropicUsage,
    stop_reason: Option<String>,
}

/// Anthropic content block.
#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct AnthropicContent {
    #[serde(rename = "type")]
    content_type: String,
    text: Option<String>,
    id: Option<String>,
    name: Option<String>,
    input: Option<serde_json::Value>,
}

/// Anthropic usage stats.
#[derive(Debug, Deserialize)]
struct AnthropicUsage {
    input_tokens: u32,
    output_tokens: u32,
}

/// Anthropic error response.
#[derive(Debug, Deserialize)]
struct AnthropicErrorResponse {
    error: AnthropicError,
}

/// Anthropic error details.
#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct AnthropicError {
    #[serde(rename = "type")]
    error_type: String,
    message: String,
}

/// Anthropic Messages API adapter.
pub struct AnthropicAdapter {
    config: AnthropicConfig,
    http_client: Client,
}

impl AnthropicAdapter {
    /// Create a new Anthropic adapter with the given config.
    pub fn new(config: AnthropicConfig) -> Self {
        Self {
            config,
            http_client: Client::new(),
        }
    }

    /// Create a new Anthropic adapter from environment variables.
    pub fn from_env() -> Result<Self, ModelAdapterError> {
        let config = AnthropicConfig::from_env()?;
        Ok(Self::new(config))
    }

    /// Convert internal ModelRequest to Anthropic format.
    fn convert_request(&self, request: &ModelRequest) -> AnthropicRequest {
        self.convert_request_with_tools(request, vec![], None)
    }

    fn convert_request_with_tools(
        &self,
        request: &ModelRequest,
        tools: Vec<ToolDefinition>,
        tool_choice: Option<ToolChoice>,
    ) -> AnthropicRequest {
        let model_id = if request.model_id.0.is_empty() {
            self.config.default_model.clone()
        } else {
            request.model_id.0.clone()
        };

        // Extract system message if present
        let system = request
            .messages
            .iter()
            .find(|m| matches!(m.role, ModelRole::System))
            .map(|m| m.text.clone());

        // Convert messages (skip system messages)
        let messages: Vec<AnthropicMessage> = request
            .messages
            .iter()
            .filter(|m| !matches!(m.role, ModelRole::System))
            .map(|m| AnthropicMessage {
                role: match m.role {
                    ModelRole::User => "user".to_string(),
                    ModelRole::Assistant => "assistant".to_string(),
                    ModelRole::System => "user".to_string(), // shouldn't happen due to filter
                    ModelRole::Tool => "user".to_string(),
                },
                content: Self::convert_message_content(m.role.clone(), &m.text),
            })
            .collect();

        let max_tokens = request.max_output_tokens.unwrap_or(4096);

        let temperature = request.temperature_millis.map(|t| t as f64 / 1000.0);
        let anthropic_tools = tools
            .into_iter()
            .map(|tool| AnthropicTool {
                name: Self::sanitize_tool_name(&tool.name),
                description: tool.description,
                input_schema: tool.input_schema,
            })
            .collect::<Vec<_>>();
        let anthropic_tool_choice = tool_choice.map(Self::convert_tool_choice);

        AnthropicRequest {
            model: model_id,
            max_tokens,
            messages,
            system,
            temperature,
            tools: anthropic_tools,
            tool_choice: anthropic_tool_choice,
        }
    }

    fn convert_message_content(role: ModelRole, text: &str) -> AnthropicMessageContent {
        if role == ModelRole::Tool {
            let (tool_use_id, content, is_error) = Self::parse_tool_result_text(text);
            AnthropicMessageContent::Blocks(vec![AnthropicMessageBlock::ToolResult {
                tool_use_id,
                content,
                is_error,
            }])
        } else {
            AnthropicMessageContent::Text(text.to_string())
        }
    }

    fn parse_tool_result_text(text: &str) -> (String, String, Option<bool>) {
        if let Ok(value) = serde_json::from_str::<serde_json::Value>(text)
            && let Some(tool_use_id) = value.get("tool_use_id").and_then(|v| v.as_str())
        {
            let content = value
                .get("content")
                .and_then(|v| v.as_str())
                .map(ToString::to_string)
                .unwrap_or_else(|| text.to_string());
            let is_error = value.get("is_error").and_then(|v| v.as_bool());
            return (tool_use_id.to_string(), content, is_error);
        }

        // Compatibility fallback for the current provider-neutral text-only
        // ModelMessage shape. AgentToolLoop can still pass a useful tool result
        // as plain text; callers that need exact Anthropic correlation should
        // encode {"tool_use_id":"...","content":"..."} in the text.
        ("toolu_unknown".to_string(), text.to_string(), None)
    }

    fn convert_tool_choice(choice: ToolChoice) -> AnthropicToolChoice {
        match choice {
            ToolChoice::Auto => AnthropicToolChoice::Auto,
            ToolChoice::None => AnthropicToolChoice::None,
            ToolChoice::Required => AnthropicToolChoice::Any,
            ToolChoice::Named(name) => AnthropicToolChoice::Tool {
                name: Self::sanitize_tool_name(&name),
            },
        }
    }

    fn sanitize_tool_name(name: &str) -> String {
        name.chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
                    c
                } else {
                    '_'
                }
            })
            .take(64)
            .collect()
    }

    /// Parse Anthropic response to internal format.
    fn parse_response(
        &self,
        response: AnthropicResponse,
    ) -> Result<ModelOutput, ModelAdapterError> {
        let text = response
            .content
            .iter()
            .filter_map(|c| c.text.as_deref())
            .collect::<Vec<_>>()
            .join("");

        let usage = ModelUsage {
            input_tokens: response.usage.input_tokens,
            output_tokens: response.usage.output_tokens,
        };

        let tool_calls = response
            .content
            .iter()
            .filter(|c| c.content_type == "tool_use")
            .map(|c| {
                let id = c.id.clone().ok_or(ModelAdapterError::MissingToolCallId)?;
                let name = c
                    .name
                    .clone()
                    .ok_or(ModelAdapterError::MissingToolCallName)?;
                let arguments = c.input.clone().unwrap_or_else(|| serde_json::json!({}));
                let raw_arguments = serde_json::to_string(&arguments)
                    .map_err(|e| ModelAdapterError::MalformedToolCallArguments(e.to_string()))?;
                Ok(ToolCall {
                    id,
                    name,
                    arguments,
                    raw_arguments,
                })
            })
            .collect::<Result<Vec<_>, ModelAdapterError>>()?;

        if !tool_calls.is_empty() {
            return Ok(ModelOutput::ToolCalls {
                content: if text.is_empty() { None } else { Some(text) },
                tool_calls,
                usage: Some(usage),
            });
        }

        if text.is_empty() {
            return Err(ModelAdapterError::EmptyResponse);
        }

        Ok(ModelOutput::Text {
            text,
            usage: Some(usage),
        })
    }

    async fn send_request(
        &self,
        anthropic_request: AnthropicRequest,
    ) -> Result<ModelOutput, ModelAdapterError> {
        let response = self
            .http_client
            .post(self.config.messages_url())
            .header("x-api-key", &self.config.api_key)
            .header("anthropic-version", &self.config.api_version)
            .header("content-type", "application/json")
            .json(&anthropic_request)
            .send()
            .await
            .map_err(|e| ModelAdapterError::HttpError(e.to_string()))?;

        let status = response.status();
        if !status.is_success() {
            let error_body: AnthropicErrorResponse = response
                .json()
                .await
                .map_err(|e| ModelAdapterError::ExecutorFailed(e.to_string()))?;

            return Err(match status.as_u16() {
                401 => ModelAdapterError::AuthError(error_body.error.message),
                429 => ModelAdapterError::RateLimitExceeded,
                500..=599 => ModelAdapterError::ExecutorFailed(format!(
                    "Anthropic server error: {}",
                    error_body.error.message
                )),
                _ => ModelAdapterError::ExecutorFailed(format!(
                    "Anthropic API error ({}): {}",
                    status, error_body.error.message
                )),
            });
        }

        let anthropic_response: AnthropicResponse = response
            .json()
            .await
            .map_err(|e| ModelAdapterError::ExecutorFailed(e.to_string()))?;

        self.parse_response(anthropic_response)
    }
}

#[async_trait]
impl ModelAdapter for AnthropicAdapter {
    async fn complete(&self, request: ModelRequest) -> Result<ModelOutput, ModelAdapterError> {
        let anthropic_request = self.convert_request(&request);
        self.send_request(anthropic_request).await
    }
}

#[async_trait]
impl ToolCallingModelAdapter for AnthropicAdapter {
    async fn complete_with_tools(
        &self,
        request: ModelRequest,
        tools: Vec<ToolDefinition>,
        tool_choice: ToolChoice,
    ) -> Result<ModelOutput, ModelAdapterError> {
        let anthropic_request = self.convert_request_with_tools(&request, tools, Some(tool_choice));
        self.send_request(anthropic_request).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ModelId, ModelMessage};
    use std::collections::BTreeMap;

    // ── Config Tests ──

    #[test]
    fn config_from_env_missing_key() {
        unsafe {
            std::env::remove_var("ANTHROPIC_API_KEY");
        }
        let result = AnthropicConfig::from_env();
        assert!(result.is_err());
    }

    #[test]
    fn config_messages_url() {
        let config = AnthropicConfig {
            api_key: "test".to_string(),
            base_url: "https://api.anthropic.com".to_string(),
            default_model: "claude-3".to_string(),
            api_version: "2023-06-01".to_string(),
        };
        assert_eq!(
            config.messages_url(),
            "https://api.anthropic.com/v1/messages"
        );
    }

    // ── Request Conversion Tests ──

    #[test]
    fn convert_request_basic() {
        let config = AnthropicConfig {
            api_key: "test".to_string(),
            base_url: "https://api.anthropic.com".to_string(),
            default_model: "claude-3".to_string(),
            api_version: "2023-06-01".to_string(),
        };
        let adapter = AnthropicAdapter::new(config);

        let request = ModelRequest {
            model_id: ModelId::from("claude-3"),
            messages: vec![ModelMessage {
                role: ModelRole::User,
                text: "Hello".to_string(),
            }],
            max_output_tokens: Some(100),
            temperature_millis: Some(500),
            metadata: BTreeMap::new(),
        };

        let anthropic_request = adapter.convert_request(&request);
        assert_eq!(anthropic_request.model, "claude-3");
        assert_eq!(anthropic_request.max_tokens, 100);
        assert_eq!(anthropic_request.messages.len(), 1);
        assert!(anthropic_request.system.is_none());
        assert!(anthropic_request.tools.is_empty());
        assert!(anthropic_request.tool_choice.is_none());
    }

    #[test]
    fn convert_request_with_system() {
        let config = AnthropicConfig {
            api_key: "test".to_string(),
            base_url: "https://api.anthropic.com".to_string(),
            default_model: "claude-3".to_string(),
            api_version: "2023-06-01".to_string(),
        };
        let adapter = AnthropicAdapter::new(config);

        let request = ModelRequest {
            model_id: ModelId::from("claude-3"),
            messages: vec![
                ModelMessage {
                    role: ModelRole::System,
                    text: "You are a helpful assistant".to_string(),
                },
                ModelMessage {
                    role: ModelRole::User,
                    text: "Hello".to_string(),
                },
            ],
            max_output_tokens: None,
            temperature_millis: None,
            metadata: BTreeMap::new(),
        };

        let anthropic_request = adapter.convert_request(&request);
        assert!(anthropic_request.system.is_some());
        assert_eq!(
            anthropic_request.system.unwrap(),
            "You are a helpful assistant"
        );
        assert_eq!(anthropic_request.messages.len(), 1);
    }

    // ── Response Parsing Tests ──

    #[test]
    fn parse_response_basic() {
        let config = AnthropicConfig {
            api_key: "test".to_string(),
            base_url: "https://api.anthropic.com".to_string(),
            default_model: "claude-3".to_string(),
            api_version: "2023-06-01".to_string(),
        };
        let adapter = AnthropicAdapter::new(config);

        let response = AnthropicResponse {
            id: "msg_123".to_string(),
            model: "claude-3".to_string(),
            content: vec![AnthropicContent {
                content_type: "text".to_string(),
                text: Some("Hello!".to_string()),
                id: None,
                name: None,
                input: None,
            }],
            usage: AnthropicUsage {
                input_tokens: 10,
                output_tokens: 5,
            },
            stop_reason: Some("end_turn".to_string()),
        };

        let output = adapter.parse_response(response).unwrap();
        match output {
            ModelOutput::Text { text, usage } => {
                assert_eq!(text, "Hello!");
                let u = usage.unwrap();
                assert_eq!(u.input_tokens, 10);
                assert_eq!(u.output_tokens, 5);
            }
            _ => panic!("expected text output"),
        }
    }

    #[test]
    fn parse_response_empty_content() {
        let config = AnthropicConfig {
            api_key: "test".to_string(),
            base_url: "https://api.anthropic.com".to_string(),
            default_model: "claude-3".to_string(),
            api_version: "2023-06-01".to_string(),
        };
        let adapter = AnthropicAdapter::new(config);

        let response = AnthropicResponse {
            id: "msg_123".to_string(),
            model: "claude-3".to_string(),
            content: vec![],
            usage: AnthropicUsage {
                input_tokens: 10,
                output_tokens: 0,
            },
            stop_reason: Some("end_turn".to_string()),
        };

        let result = adapter.parse_response(response);
        assert!(result.is_err());
    }

    #[test]
    fn parse_response_multiple_content_blocks() {
        let config = AnthropicConfig {
            api_key: "test".to_string(),
            base_url: "https://api.anthropic.com".to_string(),
            default_model: "claude-3".to_string(),
            api_version: "2023-06-01".to_string(),
        };
        let adapter = AnthropicAdapter::new(config);

        let response = AnthropicResponse {
            id: "msg_123".to_string(),
            model: "claude-3".to_string(),
            content: vec![
                AnthropicContent {
                    content_type: "text".to_string(),
                    text: Some("Hello ".to_string()),
                    id: None,
                    name: None,
                    input: None,
                },
                AnthropicContent {
                    content_type: "text".to_string(),
                    text: Some("world!".to_string()),
                    id: None,
                    name: None,
                    input: None,
                },
            ],
            usage: AnthropicUsage {
                input_tokens: 10,
                output_tokens: 10,
            },
            stop_reason: Some("end_turn".to_string()),
        };

        let output = adapter.parse_response(response).unwrap();
        match output {
            ModelOutput::Text { text, .. } => {
                assert_eq!(text, "Hello world!");
            }
            _ => panic!("expected text output"),
        }
    }

    // ── Serialization Tests ──

    #[test]
    fn anthropic_request_serialization() {
        let request = AnthropicRequest {
            model: "claude-3".to_string(),
            max_tokens: 1024,
            messages: vec![AnthropicMessage {
                role: "user".to_string(),
                content: AnthropicMessageContent::Text("Hello".to_string()),
            }],
            system: Some("You are helpful".to_string()),
            temperature: Some(0.7),
            tools: vec![],
            tool_choice: None,
        };

        let json = serde_json::to_string(&request).unwrap();
        assert!(json.contains("\"model\":\"claude-3\""));
        assert!(json.contains("\"max_tokens\":1024"));
        assert!(json.contains("\"system\":\"You are helpful\""));
    }

    #[test]
    fn anthropic_request_serialization_no_optional() {
        let request = AnthropicRequest {
            model: "claude-3".to_string(),
            max_tokens: 1024,
            messages: vec![AnthropicMessage {
                role: "user".to_string(),
                content: AnthropicMessageContent::Text("Hello".to_string()),
            }],
            system: None,
            temperature: None,
            tools: vec![],
            tool_choice: None,
        };

        let json = serde_json::to_string(&request).unwrap();
        assert!(!json.contains("\"system\""));
        assert!(!json.contains("\"temperature\""));
    }

    #[test]
    fn convert_request_with_tools_sanitizes_names_and_sets_choice() {
        let config = AnthropicConfig {
            api_key: "test".to_string(),
            base_url: "https://api.anthropic.com".to_string(),
            default_model: "claude-3".to_string(),
            api_version: "2023-06-01".to_string(),
        };
        let adapter = AnthropicAdapter::new(config);
        let request = ModelRequest::new("claude-3", vec![ModelMessage::user("Search docs")]);
        let tool = ToolDefinition {
            name: "knowledge.search".to_string(),
            description: "Search knowledge entries".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": { "query": { "type": "string" } },
                "required": ["query"]
            }),
        };

        let anthropic_request = adapter.convert_request_with_tools(
            &request,
            vec![tool],
            Some(ToolChoice::Named("knowledge.search".to_string())),
        );

        assert_eq!(anthropic_request.tools.len(), 1);
        assert_eq!(anthropic_request.tools[0].name, "knowledge_search");
        assert_eq!(
            anthropic_request.tool_choice,
            Some(AnthropicToolChoice::Tool {
                name: "knowledge_search".to_string()
            })
        );
    }

    #[test]
    fn tool_choice_required_maps_to_any() {
        assert_eq!(
            AnthropicAdapter::convert_tool_choice(ToolChoice::Required),
            AnthropicToolChoice::Any
        );
        assert_eq!(
            AnthropicAdapter::convert_tool_choice(ToolChoice::Auto),
            AnthropicToolChoice::Auto
        );
        assert_eq!(
            AnthropicAdapter::convert_tool_choice(ToolChoice::None),
            AnthropicToolChoice::None
        );
    }

    #[test]
    fn tool_model_message_serializes_as_tool_result_block() {
        let content = AnthropicAdapter::convert_message_content(
            ModelRole::Tool,
            r#"{"tool_use_id":"toolu_123","content":"15 degrees","is_error":false}"#,
        );

        let json = serde_json::to_value(content).unwrap();
        assert_eq!(json[0]["type"], "tool_result");
        assert_eq!(json[0]["tool_use_id"], "toolu_123");
        assert_eq!(json[0]["content"], "15 degrees");
        assert_eq!(json[0]["is_error"], false);
    }

    #[test]
    fn parse_response_tool_use_block() {
        let config = AnthropicConfig {
            api_key: "test".to_string(),
            base_url: "https://api.anthropic.com".to_string(),
            default_model: "claude-3".to_string(),
            api_version: "2023-06-01".to_string(),
        };
        let adapter = AnthropicAdapter::new(config);
        let response = AnthropicResponse {
            id: "msg_123".to_string(),
            model: "claude-3".to_string(),
            content: vec![
                AnthropicContent {
                    content_type: "text".to_string(),
                    text: Some("I'll search.".to_string()),
                    id: None,
                    name: None,
                    input: None,
                },
                AnthropicContent {
                    content_type: "tool_use".to_string(),
                    text: None,
                    id: Some("toolu_123".to_string()),
                    name: Some("knowledge_search".to_string()),
                    input: Some(serde_json::json!({"query": "AgentOS"})),
                },
            ],
            usage: AnthropicUsage {
                input_tokens: 25,
                output_tokens: 12,
            },
            stop_reason: Some("tool_use".to_string()),
        };

        let output = adapter.parse_response(response).unwrap();
        match output {
            ModelOutput::ToolCalls {
                content,
                tool_calls,
                usage,
            } => {
                assert_eq!(content, Some("I'll search.".to_string()));
                assert_eq!(tool_calls.len(), 1);
                assert_eq!(tool_calls[0].id, "toolu_123");
                assert_eq!(tool_calls[0].name, "knowledge_search");
                assert_eq!(tool_calls[0].arguments["query"], "AgentOS");
                assert_eq!(tool_calls[0].raw_arguments, r#"{"query":"AgentOS"}"#);
                assert_eq!(usage.unwrap().total_tokens(), 37);
            }
            other => panic!("expected tool calls, got {other:?}"),
        }
    }

    #[test]
    fn parse_response_tool_use_missing_id_is_error() {
        let config = AnthropicConfig {
            api_key: "test".to_string(),
            base_url: "https://api.anthropic.com".to_string(),
            default_model: "claude-3".to_string(),
            api_version: "2023-06-01".to_string(),
        };
        let adapter = AnthropicAdapter::new(config);
        let response = AnthropicResponse {
            id: "msg_123".to_string(),
            model: "claude-3".to_string(),
            content: vec![AnthropicContent {
                content_type: "tool_use".to_string(),
                text: None,
                id: None,
                name: Some("knowledge_search".to_string()),
                input: Some(serde_json::json!({"query": "AgentOS"})),
            }],
            usage: AnthropicUsage {
                input_tokens: 25,
                output_tokens: 12,
            },
            stop_reason: Some("tool_use".to_string()),
        };

        assert_eq!(
            adapter.parse_response(response).unwrap_err(),
            ModelAdapterError::MissingToolCallId
        );
    }

    #[test]
    fn anthropic_response_deserialization() {
        let json = r#"{
            "id": "msg_123",
            "model": "claude-3",
            "content": [{"type": "text", "text": "Hello!"}],
            "usage": {"input_tokens": 10, "output_tokens": 5},
            "stop_reason": "end_turn"
        }"#;

        let response: AnthropicResponse = serde_json::from_str(json).unwrap();
        assert_eq!(response.id, "msg_123");
        assert_eq!(response.content.len(), 1);
        assert_eq!(response.content[0].text, Some("Hello!".to_string()));
    }

    #[test]
    fn anthropic_error_deserialization() {
        let json = r#"{
            "error": {
                "type": "authentication_error",
                "message": "Invalid API key"
            }
        }"#;

        let error: AnthropicErrorResponse = serde_json::from_str(json).unwrap();
        assert_eq!(error.error.error_type, "authentication_error");
        assert_eq!(error.error.message, "Invalid API key");
    }
}
