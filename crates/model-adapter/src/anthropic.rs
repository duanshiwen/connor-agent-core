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
    ModelAdapter, ModelAdapterError, ModelOutput, ModelRequest, ModelRole, ModelStreamEvent,
    ModelStreamFinishReason, ModelToolCallDelta, ModelUsage, StreamingModelAdapter, ToolCall,
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
    #[serde(skip_serializing_if = "Option::is_none")]
    stream: Option<bool>,
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

#[derive(Debug, Deserialize)]
struct AnthropicStreamEvent {
    #[serde(rename = "type")]
    event_type: String,
    message: Option<AnthropicStreamMessage>,
    index: Option<usize>,
    content_block: Option<AnthropicStreamContentBlock>,
    delta: Option<AnthropicStreamDelta>,
    usage: Option<AnthropicStreamUsage>,
}

#[derive(Debug, Deserialize)]
struct AnthropicStreamMessage {
    model: String,
    usage: Option<AnthropicStreamUsage>,
}

#[derive(Debug, Deserialize)]
struct AnthropicStreamContentBlock {
    #[serde(rename = "type")]
    block_type: String,
    id: Option<String>,
    name: Option<String>,
}

#[derive(Debug, Deserialize)]
struct AnthropicStreamDelta {
    #[serde(rename = "type")]
    delta_type: Option<String>,
    text: Option<String>,
    partial_json: Option<String>,
    stop_reason: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct AnthropicStreamUsage {
    #[serde(default)]
    input_tokens: u32,
    #[serde(default)]
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
            stream: None,
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

    async fn send_stream_and_parse(
        &self,
        anthropic_request: AnthropicRequest,
        fallback_model: crate::ModelId,
    ) -> Result<Vec<ModelStreamEvent>, ModelAdapterError> {
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
            let body = response.text().await.unwrap_or_default();
            if let Ok(error_body) = serde_json::from_str::<AnthropicErrorResponse>(&body) {
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

            return Err(ModelAdapterError::ExecutorFailed(format!(
                "Anthropic API error ({status}): {body}"
            )));
        }

        let body = response
            .text()
            .await
            .map_err(|e| ModelAdapterError::ExecutorFailed(e.to_string()))?;
        parse_anthropic_sse_events(&body, &fallback_model)
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

fn anthropic_stream_finish_reason(reason: Option<&str>) -> ModelStreamFinishReason {
    match reason {
        Some("end_turn") | Some("stop_sequence") => ModelStreamFinishReason::Stop,
        Some("max_tokens") => ModelStreamFinishReason::Length,
        Some("tool_use") => ModelStreamFinishReason::ToolCalls,
        _ => ModelStreamFinishReason::Error,
    }
}

fn parse_anthropic_sse_events(
    raw: &str,
    fallback_model: &crate::ModelId,
) -> Result<Vec<ModelStreamEvent>, ModelAdapterError> {
    let mut events = Vec::new();
    let mut started = false;
    let mut input_tokens = 0;

    for frame in raw.split("\n\n") {
        let mut data_lines = Vec::new();
        for line in frame.lines() {
            let line = line.trim_end();
            if line.is_empty() || line.starts_with(':') || line.starts_with("event:") {
                continue;
            }
            if let Some(data) = line.strip_prefix("data:") {
                data_lines.push(data.trim_start());
            }
        }

        if data_lines.is_empty() {
            continue;
        }

        let data = data_lines.join("\n");
        let stream_event: AnthropicStreamEvent = serde_json::from_str(&data).map_err(|e| {
            ModelAdapterError::ExecutorFailed(format!("malformed Anthropic SSE JSON: {e}"))
        })?;

        match stream_event.event_type.as_str() {
            "message_start" => {
                let model_id = stream_event
                    .message
                    .as_ref()
                    .map(|message| crate::ModelId::from(message.model.clone()))
                    .unwrap_or_else(|| fallback_model.clone());
                if let Some(usage) = stream_event.message.and_then(|message| message.usage) {
                    input_tokens = usage.input_tokens;
                }
                events.push(ModelStreamEvent::Started { model_id });
                started = true;
            }
            "content_block_start" => {
                if !started {
                    events.push(ModelStreamEvent::Started {
                        model_id: fallback_model.clone(),
                    });
                    started = true;
                }
                if let Some(block) = stream_event.content_block
                    && block.block_type == "tool_use"
                {
                    events.push(ModelStreamEvent::ToolCallDelta(ModelToolCallDelta {
                        index: stream_event.index.unwrap_or(0),
                        id_delta: block.id,
                        name_delta: block.name,
                        arguments_delta: None,
                    }));
                }
            }
            "content_block_delta" => {
                if !started {
                    events.push(ModelStreamEvent::Started {
                        model_id: fallback_model.clone(),
                    });
                    started = true;
                }
                if let Some(delta) = stream_event.delta {
                    match delta.delta_type.as_deref() {
                        Some("text_delta") => {
                            if let Some(text) = delta.text {
                                events.push(ModelStreamEvent::TextDelta { delta: text });
                            }
                        }
                        Some("input_json_delta") => {
                            if let Some(partial_json) = delta.partial_json {
                                events.push(ModelStreamEvent::ToolCallDelta(
                                    ModelToolCallDelta::arguments(
                                        stream_event.index.unwrap_or(0),
                                        partial_json,
                                    ),
                                ));
                            }
                        }
                        _ => {}
                    }
                }
            }
            "message_delta" => {
                if let Some(usage) = stream_event.usage {
                    if usage.input_tokens > 0 {
                        input_tokens = usage.input_tokens;
                    }
                    events.push(ModelStreamEvent::Usage {
                        usage: ModelUsage {
                            input_tokens,
                            output_tokens: usage.output_tokens,
                        },
                    });
                }
                if let Some(reason) = stream_event
                    .delta
                    .as_ref()
                    .and_then(|delta| delta.stop_reason.as_deref())
                {
                    events.push(ModelStreamEvent::Finished {
                        reason: anthropic_stream_finish_reason(Some(reason)),
                    });
                }
            }
            "message_stop" => {
                if !events
                    .iter()
                    .any(|event| matches!(event, ModelStreamEvent::Finished { .. }))
                {
                    events.push(ModelStreamEvent::Finished {
                        reason: ModelStreamFinishReason::Stop,
                    });
                }
            }
            "ping" | "content_block_stop" => {}
            _ => {}
        }
    }

    if !started {
        events.insert(
            0,
            ModelStreamEvent::Started {
                model_id: fallback_model.clone(),
            },
        );
    }

    Ok(events)
}

#[async_trait]
impl ModelAdapter for AnthropicAdapter {
    async fn complete(&self, request: ModelRequest) -> Result<ModelOutput, ModelAdapterError> {
        let anthropic_request = self.convert_request(&request);
        self.send_request(anthropic_request).await
    }
}

#[async_trait]
impl StreamingModelAdapter for AnthropicAdapter {
    async fn stream(
        &self,
        request: ModelRequest,
    ) -> Result<Vec<ModelStreamEvent>, ModelAdapterError> {
        if request.messages.is_empty() {
            return Err(ModelAdapterError::EmptyRequest);
        }

        let fallback_model = request.model_id.clone();
        let mut anthropic_request = self.convert_request(&request);
        anthropic_request.stream = Some(true);
        self.send_stream_and_parse(anthropic_request, fallback_model)
            .await
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
    use crate::{
        ModelId, ModelMessage, ModelStreamAccumulator, ModelStreamEvent, ModelStreamFinishReason,
        ModelToolCallDelta, StreamingModelAdapter,
    };
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
            stream: None,
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
            stream: None,
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
    fn anthropic_stream_parser_emits_started_text_usage_finished() {
        let raw = r#"event: message_start
data: {"type":"message_start","message":{"model":"claude-test","usage":{"input_tokens":7,"output_tokens":0}}}

event: content_block_delta
data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"Hel"}}

event: content_block_delta
data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"lo"}}

event: message_delta
data: {"type":"message_delta","delta":{"stop_reason":"end_turn"},"usage":{"output_tokens":2}}

event: message_stop
data: {"type":"message_stop"}

"#;

        let events = parse_anthropic_sse_events(raw, &ModelId::from("fallback")).unwrap();
        assert_eq!(
            events.first(),
            Some(&ModelStreamEvent::Started {
                model_id: ModelId::from("claude-test")
            })
        );

        let accumulator = ModelStreamAccumulator::from_events(&events);
        assert_eq!(accumulator.text, "Hello");
        assert_eq!(accumulator.usage.unwrap().total_tokens(), 9);
        assert_eq!(accumulator.finished, Some(ModelStreamFinishReason::Stop));
    }

    #[test]
    fn anthropic_stream_parser_emits_tool_use_deltas() {
        let raw = r#"event: message_start
data: {"type":"message_start","message":{"model":"claude-test","usage":{"input_tokens":5,"output_tokens":0}}}

event: content_block_start
data: {"type":"content_block_start","index":1,"content_block":{"type":"tool_use","id":"toolu_123","name":"knowledge_search"}}

event: content_block_delta
data: {"type":"content_block_delta","index":1,"delta":{"type":"input_json_delta","partial_json":"{\"query\":"}}

event: content_block_delta
data: {"type":"content_block_delta","index":1,"delta":{"type":"input_json_delta","partial_json":"\"AgentOS\"}"}}

event: message_delta
data: {"type":"message_delta","delta":{"stop_reason":"tool_use"},"usage":{"output_tokens":8}}

"#;

        let events = parse_anthropic_sse_events(raw, &ModelId::from("fallback")).unwrap();
        assert!(
            events.contains(&ModelStreamEvent::ToolCallDelta(ModelToolCallDelta {
                index: 1,
                id_delta: Some("toolu_123".to_string()),
                name_delta: Some("knowledge_search".to_string()),
                arguments_delta: None,
            }))
        );
        assert!(events.contains(&ModelStreamEvent::ToolCallDelta(
            ModelToolCallDelta::arguments(1, "{\"query\":")
        )));
        assert!(events.contains(&ModelStreamEvent::Finished {
            reason: ModelStreamFinishReason::ToolCalls
        }));
    }

    #[test]
    fn anthropic_stream_parser_rejects_malformed_json() {
        let raw = "event: message_start\ndata: {not-json}\n\n";
        let err = parse_anthropic_sse_events(raw, &ModelId::from("fallback")).unwrap_err();
        match err {
            ModelAdapterError::ExecutorFailed(message) => {
                assert!(
                    message.contains("malformed Anthropic SSE JSON"),
                    "unexpected: {message}"
                );
            }
            other => panic!("expected ExecutorFailed, got: {other:?}"),
        }
    }

    #[tokio::test]
    async fn anthropic_streaming_adapter_posts_stream_true_and_parses_sse() {
        use wiremock::matchers::{body_partial_json, method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let mock_server = MockServer::start().await;
        let sse_body = r#"event: message_start
data: {"type":"message_start","message":{"model":"claude-test","usage":{"input_tokens":7,"output_tokens":0}}}

event: content_block_delta
data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"Hel"}}

event: content_block_delta
data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"lo"}}

event: message_delta
data: {"type":"message_delta","delta":{"stop_reason":"end_turn"},"usage":{"output_tokens":2}}

event: message_stop
data: {"type":"message_stop"}

"#;

        Mock::given(method("POST"))
            .and(path("/v1/messages"))
            .and(body_partial_json(serde_json::json!({"stream": true})))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("content-type", "text/event-stream")
                    .set_body_string(sse_body),
            )
            .expect(1)
            .mount(&mock_server)
            .await;

        let config = AnthropicConfig {
            api_key: "test".to_string(),
            base_url: mock_server.uri(),
            default_model: "claude-test".to_string(),
            api_version: "2023-06-01".to_string(),
        };
        let adapter = AnthropicAdapter::new(config);
        let request = ModelRequest::new("claude-test", vec![ModelMessage::user("Say hello")]);

        let events = adapter.stream(request).await.unwrap();
        let accumulator = ModelStreamAccumulator::from_events(&events);

        assert_eq!(accumulator.text, "Hello");
        assert_eq!(accumulator.usage.unwrap().total_tokens(), 9);
        assert_eq!(accumulator.finished, Some(ModelStreamFinishReason::Stop));
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
