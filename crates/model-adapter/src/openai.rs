//! OpenAI-compatible model adapter.
//!
//! Implements [`ModelAdapter`] for any provider that exposes the OpenAI
//! Chat Completions API format (DeepSeek, Qwen, OpenAI, vLLM, etc.).

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::{
    ModelAdapter, ModelAdapterError, ModelOutput, ModelRequest, ModelRole, ModelUsage, ToolCall,
    ToolCallingModelAdapter, ToolChoice, ToolDefinition,
};

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

/// Configuration for connecting to an OpenAI-compatible endpoint.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OpenAiProviderConfig {
    /// Base URL for the Chat Completions endpoint (e.g. `https://api.deepseek.com/v1`).
    pub endpoint: String,
    /// API key / bearer token.
    pub api_key: String,
    /// Model identifier (e.g. `deepseek-chat`, `qwen-plus`).
    pub model: String,
    /// Optional fixed temperature (0.0–2.0). Overrides per-request conversion.
    pub temperature: Option<f32>,
    /// Optional max output tokens.
    pub max_tokens: Option<u32>,
    /// Request timeout in seconds (default: 120).
    pub timeout_secs: Option<u64>,
}

impl OpenAiProviderConfig {
    pub fn new(
        endpoint: impl Into<String>,
        api_key: impl Into<String>,
        model: impl Into<String>,
    ) -> Self {
        Self {
            endpoint: endpoint.into(),
            api_key: api_key.into(),
            model: model.into(),
            temperature: None,
            max_tokens: None,
            timeout_secs: None,
        }
    }
}

// ---------------------------------------------------------------------------
// Wire types (OpenAI Chat Completions API)
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize)]
struct ChatRequest {
    model: String,
    messages: Vec<ChatMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tools: Option<Vec<ChatTool>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_choice: Option<ChatToolChoice>,
}

#[derive(Debug, Serialize, Deserialize)]
struct ChatMessage {
    role: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_calls: Option<Vec<ChatToolCallResponse>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_call_id: Option<String>,
}

#[derive(Debug, Serialize)]
struct ChatTool {
    #[serde(rename = "type")]
    tool_type: String,
    function: ChatFunction,
}

#[derive(Debug, Serialize)]
struct ChatFunction {
    name: String,
    description: String,
    parameters: serde_json::Value,
}

#[derive(Debug, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum ChatToolChoice {
    Auto,
    None,
    Required,
    Named { function: ChatToolChoiceName },
}

#[derive(Debug, Serialize)]
struct ChatToolChoiceName {
    name: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct ChatToolCallResponse {
    id: Option<String>,
    #[serde(rename = "type")]
    #[allow(dead_code)]
    call_type: Option<String>,
    function: ChatFunctionCall,
}

#[derive(Debug, Serialize, Deserialize)]
struct ChatFunctionCall {
    name: Option<String>,
    arguments: String,
}

#[derive(Debug, Deserialize)]
struct ChatResponse {
    #[allow(dead_code)]
    id: Option<String>,
    choices: Vec<ChatChoice>,
    usage: Option<ChatUsage>,
    #[allow(dead_code)]
    model: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ChatChoice {
    message: ChatMessage,
    #[allow(dead_code)]
    finish_reason: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ChatUsage {
    prompt_tokens: Option<u32>,
    completion_tokens: Option<u32>,
}

#[derive(Debug, Deserialize)]
struct ErrorResponse {
    error: Option<ErrorDetail>,
}

#[derive(Debug, Deserialize)]
struct ErrorDetail {
    message: Option<String>,
}

// ---------------------------------------------------------------------------
// Adapter
// ---------------------------------------------------------------------------

/// A [`ModelAdapter`] implementation that speaks the OpenAI Chat Completions API.
///
/// Works with: DeepSeek, Qwen (DashScope compatible-mode), OpenAI, vLLM,
/// Ollama (with `--api openai`), and any other OpenAI-compatible endpoint.
#[derive(Debug, Clone)]
pub struct OpenAiCompatibleAdapter {
    config: OpenAiProviderConfig,
    client: reqwest::Client,
}

/// Parse a wire `ChatToolCallResponse` into a provider-neutral [`ToolCall`].
fn parse_tool_call(tc: ChatToolCallResponse) -> Result<ToolCall, ModelAdapterError> {
    let id = tc.id.ok_or(ModelAdapterError::MissingToolCallId)?;
    let name = tc
        .function
        .name
        .ok_or(ModelAdapterError::MissingToolCallName)?;
    let arguments: serde_json::Value =
        serde_json::from_str(&tc.function.arguments).map_err(|e| {
            ModelAdapterError::MalformedToolCallArguments(format!(
                "invalid JSON in tool call '{name}': {e}"
            ))
        })?;

    Ok(ToolCall {
        id,
        name,
        arguments,
        raw_arguments: tc.function.arguments,
    })
}

impl OpenAiCompatibleAdapter {
    /// Create a new adapter from configuration.
    pub fn new(config: OpenAiProviderConfig) -> Self {
        let timeout = std::time::Duration::from_secs(config.timeout_secs.unwrap_or(120));
        let client = reqwest::Client::builder()
            .timeout(timeout)
            .build()
            .expect("failed to build reqwest client");

        Self { config, client }
    }

    /// Create a new adapter with an externally-provided [`reqwest::Client`].
    ///
    /// Useful for tests that inject a `wiremock`-backed client.
    pub fn with_client(config: OpenAiProviderConfig, client: reqwest::Client) -> Self {
        Self { config, client }
    }

    #[allow(dead_code)]
    fn chat_completions_url(&self) -> String {
        let base = self.config.endpoint.trim_end_matches('/');
        format!("{base}/chat/completions")
    }

    fn convert_role(role: &ModelRole) -> &'static str {
        match role {
            ModelRole::System => "system",
            ModelRole::User => "user",
            ModelRole::Assistant => "assistant",
            ModelRole::Tool => "tool",
        }
    }

    fn convert_request(&self, request: &ModelRequest) -> ChatRequest {
        let messages: Vec<ChatMessage> = request
            .messages
            .iter()
            .map(|m| ChatMessage {
                role: Self::convert_role(&m.role).to_string(),
                content: Some(m.text.clone()),
                tool_calls: None,
                tool_call_id: None,
            })
            .collect();

        let temperature = request
            .temperature_millis
            .map(|t| t as f32 / 1000.0)
            .or(self.config.temperature);

        let max_tokens = request.max_output_tokens.or(self.config.max_tokens);

        ChatRequest {
            model: self.config.model.clone(),
            messages,
            temperature,
            max_tokens,
            tools: None,
            tool_choice: None,
        }
    }

    fn convert_request_with_tools(
        &self,
        request: &ModelRequest,
        tools: &[ToolDefinition],
        tool_choice: &ToolChoice,
    ) -> ChatRequest {
        let messages: Vec<ChatMessage> = request
            .messages
            .iter()
            .map(|m| ChatMessage {
                role: Self::convert_role(&m.role).to_string(),
                content: Some(m.text.clone()),
                tool_calls: None,
                tool_call_id: None,
            })
            .collect();

        let temperature = request.temperature_millis.map(|t| t as f32 / 1000.0);

        let max_tokens = request.max_output_tokens;

        let chat_tools: Vec<ChatTool> = tools
            .iter()
            .map(|t| ChatTool {
                tool_type: "function".to_string(),
                function: ChatFunction {
                    name: t.name.clone(),
                    description: t.description.clone(),
                    parameters: t.input_schema.clone(),
                },
            })
            .collect();

        let chat_tool_choice = match tool_choice {
            ToolChoice::Auto => ChatToolChoice::Auto,
            ToolChoice::None => ChatToolChoice::None,
            ToolChoice::Required => ChatToolChoice::Required,
            ToolChoice::Named(name) => ChatToolChoice::Named {
                function: ChatToolChoiceName { name: name.clone() },
            },
        };

        ChatRequest {
            model: self.config.model.clone(),
            messages,
            temperature,
            max_tokens,
            tools: Some(chat_tools),
            tool_choice: Some(chat_tool_choice),
        }
    }

    #[allow(dead_code)]
    fn convert_response(
        &self,
        resp: ChatResponse,
        _fallback_model: &str,
    ) -> Result<ModelOutput, ModelAdapterError> {
        let choice = resp.choices.into_iter().next().ok_or_else(|| {
            ModelAdapterError::ExecutorFailed("no choices in response".to_string())
        })?;

        let usage = resp.usage.map(|u| ModelUsage {
            input_tokens: u.prompt_tokens.unwrap_or(0),
            output_tokens: u.completion_tokens.unwrap_or(0),
        });

        // Check if tool_calls are present
        if let Some(tc_responses) = choice.message.tool_calls
            && !tc_responses.is_empty()
        {
            let tool_calls: Vec<ToolCall> = tc_responses
                .into_iter()
                .map(parse_tool_call)
                .collect::<Result<Vec<_>, _>>()?;

            return Ok(ModelOutput::ToolCalls {
                content: choice.message.content,
                tool_calls,
                usage,
            });
        }

        Ok(ModelOutput::Text {
            text: choice.message.content.unwrap_or_default(),
            usage,
        })
    }
}

/// Shared logic for sending a request and parsing the response.
async fn send_and_parse(
    client: &reqwest::Client,
    config: &OpenAiProviderConfig,
    body: &ChatRequest,
) -> Result<ModelOutput, ModelAdapterError> {
    let base = config.endpoint.trim_end_matches('/');
    let url = format!("{base}/chat/completions");

    let response = client
        .post(&url)
        .header("Authorization", format!("Bearer {}", config.api_key))
        .header("Content-Type", "application/json")
        .json(body)
        .send()
        .await
        .map_err(|e| ModelAdapterError::ExecutorFailed(format!("HTTP request failed: {e}")))?;

    let status = response.status();

    if !status.is_success() {
        let body_text = response
            .text()
            .await
            .unwrap_or_else(|_| "could not read error body".to_string());

        let error_message = serde_json::from_str::<ErrorResponse>(&body_text)
            .ok()
            .and_then(|e| e.error)
            .and_then(|e| e.message)
            .unwrap_or(body_text);

        return Err(ModelAdapterError::ExecutorFailed(format!(
            "API returned {status}: {error_message}"
        )));
    }

    let chat_response: ChatResponse = response
        .json()
        .await
        .map_err(|e| ModelAdapterError::ExecutorFailed(format!("failed to parse response: {e}")))?;

    // Inline convert to avoid borrow checker issues with &self
    let choice =
        chat_response.choices.into_iter().next().ok_or_else(|| {
            ModelAdapterError::ExecutorFailed("no choices in response".to_string())
        })?;

    let usage = chat_response.usage.map(|u| ModelUsage {
        input_tokens: u.prompt_tokens.unwrap_or(0),
        output_tokens: u.completion_tokens.unwrap_or(0),
    });

    if let Some(tc_responses) = choice.message.tool_calls
        && !tc_responses.is_empty()
    {
        let tool_calls: Vec<ToolCall> = tc_responses
            .into_iter()
            .map(parse_tool_call)
            .collect::<Result<Vec<_>, _>>()?;

        return Ok(ModelOutput::ToolCalls {
            content: choice.message.content,
            tool_calls,
            usage,
        });
    }

    Ok(ModelOutput::Text {
        text: choice.message.content.unwrap_or_default(),
        usage,
    })
}

#[async_trait]
impl ModelAdapter for OpenAiCompatibleAdapter {
    async fn complete(&self, request: ModelRequest) -> Result<ModelOutput, ModelAdapterError> {
        if request.messages.is_empty() {
            return Err(ModelAdapterError::EmptyRequest);
        }

        let body = self.convert_request(&request);
        send_and_parse(&self.client, &self.config, &body).await
    }
}

#[async_trait]
impl ToolCallingModelAdapter for OpenAiCompatibleAdapter {
    async fn complete_with_tools(
        &self,
        request: ModelRequest,
        tools: Vec<ToolDefinition>,
        tool_choice: ToolChoice,
    ) -> Result<ModelOutput, ModelAdapterError> {
        if request.messages.is_empty() {
            return Err(ModelAdapterError::EmptyRequest);
        }

        let body = self.convert_request_with_tools(&request, &tools, &tool_choice);
        send_and_parse(&self.client, &self.config, &body).await
    }
}

// ---------------------------------------------------------------------------
// Environment-based config helper
// ---------------------------------------------------------------------------

impl OpenAiProviderConfig {
    /// Build a config from environment variables:
    /// - `OPENAI_API_KEY` (required)
    /// - `OPENAI_ENDPOINT` (default: `https://api.openai.com/v1`)
    /// - `OPENAI_MODEL` (default: `gpt-4o-mini`)
    /// - `OPENAI_TEMPERATURE` (optional, as f32 string)
    /// - `OPENAI_MAX_TOKENS` (optional, as u32 string)
    /// - `OPENAI_TIMEOUT_SECS` (optional, as u64 string)
    pub fn from_env() -> Result<Self, ModelAdapterError> {
        let api_key = std::env::var("OPENAI_API_KEY")
            .map_err(|_| ModelAdapterError::ExecutorFailed("OPENAI_API_KEY not set".to_string()))?;

        let endpoint = std::env::var("OPENAI_ENDPOINT")
            .unwrap_or_else(|_| "https://api.openai.com/v1".to_string());

        let model = std::env::var("OPENAI_MODEL").unwrap_or_else(|_| "gpt-4o-mini".to_string());

        let temperature = std::env::var("OPENAI_TEMPERATURE")
            .ok()
            .and_then(|v| v.parse::<f32>().ok());

        let max_tokens = std::env::var("OPENAI_MAX_TOKENS")
            .ok()
            .and_then(|v| v.parse::<u32>().ok());

        let timeout_secs = std::env::var("OPENAI_TIMEOUT_SECS")
            .ok()
            .and_then(|v| v.parse::<u64>().ok());

        Ok(Self {
            endpoint,
            api_key,
            model,
            temperature,
            max_tokens,
            timeout_secs,
        })
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ModelMessage, ModelRequest};

    #[test]
    fn openai_provider_config_serializes() {
        let config = OpenAiProviderConfig::new(
            "https://api.deepseek.com/v1",
            "sk-test-key",
            "deepseek-chat",
        );

        let json = serde_json::to_string(&config).unwrap();
        assert!(json.contains("deepseek-chat"));
        assert!(json.contains("sk-test-key"));

        let decoded: OpenAiProviderConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded.endpoint, "https://api.deepseek.com/v1");
        assert_eq!(decoded.model, "deepseek-chat");
    }

    #[test]
    fn convert_role_maps_correctly() {
        assert_eq!(
            OpenAiCompatibleAdapter::convert_role(&ModelRole::System),
            "system"
        );
        assert_eq!(
            OpenAiCompatibleAdapter::convert_role(&ModelRole::User),
            "user"
        );
        assert_eq!(
            OpenAiCompatibleAdapter::convert_role(&ModelRole::Assistant),
            "assistant"
        );
    }

    #[test]
    fn convert_request_preserves_messages() {
        let config = OpenAiProviderConfig::new("https://api.example.com/v1", "key", "model-x");
        let adapter = OpenAiCompatibleAdapter::new(config);

        let request = ModelRequest::new(
            "model-x",
            vec![
                ModelMessage::system("Be concise."),
                ModelMessage::user("Hello"),
                ModelMessage::assistant("Hi there"),
                ModelMessage::user("Summarize"),
            ],
        );

        let chat_req = adapter.convert_request(&request);
        assert_eq!(chat_req.messages.len(), 4);
        assert_eq!(chat_req.messages[0].role, "system");
        assert_eq!(chat_req.messages[0].content.as_deref(), Some("Be concise."));
        assert_eq!(chat_req.messages[1].role, "user");
        assert_eq!(chat_req.messages[1].content.as_deref(), Some("Hello"));
        assert_eq!(chat_req.messages[2].role, "assistant");
        assert_eq!(chat_req.messages[2].content.as_deref(), Some("Hi there"));
        assert_eq!(chat_req.messages[3].role, "user");
        assert_eq!(chat_req.messages[3].content.as_deref(), Some("Summarize"));
    }

    #[test]
    fn convert_request_uses_per_request_temperature_over_config() {
        let config = OpenAiProviderConfig {
            endpoint: "https://api.example.com/v1".to_string(),
            api_key: "key".to_string(),
            model: "m".to_string(),
            temperature: Some(0.8),
            max_tokens: None,
            timeout_secs: None,
        };
        let adapter = OpenAiCompatibleAdapter::new(config);

        // Per-request temperature_millis=500 → 0.5
        let mut request = ModelRequest::new("m", vec![ModelMessage::user("hi")]);
        request.temperature_millis = Some(500);

        let chat_req = adapter.convert_request(&request);
        assert_eq!(chat_req.temperature, Some(0.5));
    }

    #[test]
    fn convert_request_falls_back_to_config_temperature() {
        let config = OpenAiProviderConfig {
            endpoint: "https://api.example.com/v1".to_string(),
            api_key: "key".to_string(),
            model: "m".to_string(),
            temperature: Some(1.2),
            max_tokens: None,
            timeout_secs: None,
        };
        let adapter = OpenAiCompatibleAdapter::new(config);

        let request = ModelRequest::new("m", vec![ModelMessage::user("hi")]);
        let chat_req = adapter.convert_request(&request);
        assert_eq!(chat_req.temperature, Some(1.2));
    }

    #[test]
    fn convert_request_uses_per_request_max_tokens_over_config() {
        let config = OpenAiProviderConfig {
            endpoint: "https://api.example.com/v1".to_string(),
            api_key: "key".to_string(),
            model: "m".to_string(),
            temperature: None,
            max_tokens: Some(2048),
            timeout_secs: None,
        };
        let adapter = OpenAiCompatibleAdapter::new(config);

        let mut request = ModelRequest::new("m", vec![ModelMessage::user("hi")]);
        request.max_output_tokens = Some(512);

        let chat_req = adapter.convert_request(&request);
        assert_eq!(chat_req.max_tokens, Some(512));
    }

    #[test]
    fn convert_request_falls_back_to_config_max_tokens() {
        let config = OpenAiProviderConfig {
            endpoint: "https://api.example.com/v1".to_string(),
            api_key: "key".to_string(),
            model: "m".to_string(),
            temperature: None,
            max_tokens: Some(2048),
            timeout_secs: None,
        };
        let adapter = OpenAiCompatibleAdapter::new(config);

        let request = ModelRequest::new("m", vec![ModelMessage::user("hi")]);
        let chat_req = adapter.convert_request(&request);
        assert_eq!(chat_req.max_tokens, Some(2048));
    }

    #[test]
    fn convert_request_omits_none_fields() {
        let config = OpenAiProviderConfig::new("https://api.example.com/v1", "key", "m");
        let adapter = OpenAiCompatibleAdapter::new(config);

        let request = ModelRequest::new("m", vec![ModelMessage::user("hi")]);
        let chat_req = adapter.convert_request(&request);

        let json = serde_json::to_value(&chat_req).unwrap();
        // temperature and max_tokens should be absent
        assert!(!json.as_object().unwrap().contains_key("temperature"));
        assert!(!json.as_object().unwrap().contains_key("max_tokens"));
    }

    #[test]
    fn convert_response_extracts_text_and_usage() {
        let config = OpenAiProviderConfig::new("https://api.example.com/v1", "key", "model-x");
        let adapter = OpenAiCompatibleAdapter::new(config);

        let chat_resp = ChatResponse {
            id: Some("chatcmpl-123".to_string()),
            choices: vec![ChatChoice {
                message: ChatMessage {
                    role: "assistant".to_string(),
                    content: Some("Hello! How can I help?".to_string()),
                    tool_calls: None,
                    tool_call_id: None,
                },
                finish_reason: Some("stop".to_string()),
            }],
            usage: Some(ChatUsage {
                prompt_tokens: Some(10),
                completion_tokens: Some(8),
            }),
            model: Some("model-x".to_string()),
        };

        let output = adapter.convert_response(chat_resp, "model-x").unwrap();
        match output {
            crate::ModelOutput::Text { text, usage } => {
                assert_eq!(text, "Hello! How can I help?");
                let u = usage.unwrap();
                assert_eq!(u.input_tokens, 10);
                assert_eq!(u.output_tokens, 8);
            }
            _ => panic!("expected text output"),
        }
    }

    #[test]
    fn convert_response_falls_back_to_config_model() {
        let config = OpenAiProviderConfig::new("https://api.example.com/v1", "key", "model-x");
        let adapter = OpenAiCompatibleAdapter::new(config);

        let chat_resp = ChatResponse {
            id: None,
            choices: vec![ChatChoice {
                message: ChatMessage {
                    role: "assistant".to_string(),
                    content: Some("Hi".to_string()),
                    tool_calls: None,
                    tool_call_id: None,
                },
                finish_reason: None,
            }],
            usage: None,
            model: None,
        };

        let output = adapter.convert_response(chat_resp, "model-x").unwrap();
        match output {
            crate::ModelOutput::Text { text, usage } => {
                assert_eq!(text, "Hi");
                assert!(usage.is_none());
            }
            _ => panic!("expected text output"),
        }
    }

    #[test]
    fn convert_response_fails_on_empty_choices() {
        let config = OpenAiProviderConfig::new("https://api.example.com/v1", "key", "model-x");
        let adapter = OpenAiCompatibleAdapter::new(config);

        let chat_resp = ChatResponse {
            id: None,
            choices: vec![],
            usage: None,
            model: None,
        };

        let err = adapter.convert_response(chat_resp, "model-x").unwrap_err();
        match err {
            ModelAdapterError::ExecutorFailed(msg) => {
                assert!(msg.contains("no choices"), "unexpected error: {msg}");
            }
            other => panic!("expected ExecutorFailed, got: {other:?}"),
        }
    }

    #[test]
    fn convert_response_parses_tool_calls() {
        let config = OpenAiProviderConfig::new("https://api.example.com/v1", "key", "model-x");
        let adapter = OpenAiCompatibleAdapter::new(config);

        let chat_resp = ChatResponse {
            id: None,
            choices: vec![ChatChoice {
                message: ChatMessage {
                    role: "assistant".to_string(),
                    content: None,
                    tool_calls: Some(vec![ChatToolCallResponse {
                        id: Some("call_001".to_string()),
                        call_type: Some("function".to_string()),
                        function: ChatFunctionCall {
                            name: Some("knowledge.search".to_string()),
                            arguments: r#"{"query":"agent os"}"#.to_string(),
                        },
                    }]),
                    tool_call_id: None,
                },
                finish_reason: Some("tool_calls".to_string()),
            }],
            usage: Some(ChatUsage {
                prompt_tokens: Some(20),
                completion_tokens: Some(5),
            }),
            model: Some("model-x".to_string()),
        };

        let output = adapter.convert_response(chat_resp, "model-x").unwrap();
        match output {
            crate::ModelOutput::ToolCalls {
                content,
                tool_calls,
                usage,
            } => {
                assert!(content.is_none());
                assert_eq!(tool_calls.len(), 1);
                assert_eq!(tool_calls[0].id, "call_001");
                assert_eq!(tool_calls[0].name, "knowledge.search");
                assert_eq!(
                    tool_calls[0].arguments,
                    serde_json::json!({"query": "agent os"})
                );
                assert_eq!(tool_calls[0].raw_arguments, r#"{"query":"agent os"}"#);
                assert!(usage.is_some());
            }
            _ => panic!("expected tool_calls output"),
        }
    }

    #[test]
    fn convert_response_parses_multiple_tool_calls() {
        let config = OpenAiProviderConfig::new("https://api.example.com/v1", "key", "model-x");
        let adapter = OpenAiCompatibleAdapter::new(config);

        let chat_resp = ChatResponse {
            id: None,
            choices: vec![ChatChoice {
                message: ChatMessage {
                    role: "assistant".to_string(),
                    content: Some("Searching...".to_string()),
                    tool_calls: Some(vec![
                        ChatToolCallResponse {
                            id: Some("call_001".to_string()),
                            call_type: Some("function".to_string()),
                            function: ChatFunctionCall {
                                name: Some("knowledge.search".to_string()),
                                arguments: r#"{"query":"a"}"#.to_string(),
                            },
                        },
                        ChatToolCallResponse {
                            id: Some("call_002".to_string()),
                            call_type: Some("function".to_string()),
                            function: ChatFunctionCall {
                                name: Some("browser.extract_content".to_string()),
                                arguments: r#"{"url":"https://example.com"}"#.to_string(),
                            },
                        },
                    ]),
                    tool_call_id: None,
                },
                finish_reason: Some("tool_calls".to_string()),
            }],
            usage: None,
            model: Some("model-x".to_string()),
        };

        let output = adapter.convert_response(chat_resp, "model-x").unwrap();
        match output {
            crate::ModelOutput::ToolCalls {
                content,
                tool_calls,
                ..
            } => {
                assert_eq!(content.as_deref(), Some("Searching..."));
                assert_eq!(tool_calls.len(), 2);
                assert_eq!(tool_calls[0].name, "knowledge.search");
                assert_eq!(tool_calls[1].name, "browser.extract_content");
            }
            _ => panic!("expected tool_calls output"),
        }
    }

    #[test]
    fn convert_response_malformed_tool_arguments_returns_error() {
        let config = OpenAiProviderConfig::new("https://api.example.com/v1", "key", "model-x");
        let adapter = OpenAiCompatibleAdapter::new(config);

        let chat_resp = ChatResponse {
            id: None,
            choices: vec![ChatChoice {
                message: ChatMessage {
                    role: "assistant".to_string(),
                    content: None,
                    tool_calls: Some(vec![ChatToolCallResponse {
                        id: Some("call_bad".to_string()),
                        call_type: Some("function".to_string()),
                        function: ChatFunctionCall {
                            name: Some("knowledge.search".to_string()),
                            arguments: "not-json".to_string(),
                        },
                    }]),
                    tool_call_id: None,
                },
                finish_reason: None,
            }],
            usage: None,
            model: None,
        };

        let err = adapter.convert_response(chat_resp, "model-x").unwrap_err();
        match err {
            ModelAdapterError::MalformedToolCallArguments(msg) => {
                assert!(msg.contains("knowledge.search"), "unexpected: {msg}");
            }
            other => panic!("expected MalformedToolCallArguments, got: {other:?}"),
        }
    }

    #[test]
    fn convert_response_missing_tool_call_id_returns_error() {
        let config = OpenAiProviderConfig::new("https://api.example.com/v1", "key", "model-x");
        let adapter = OpenAiCompatibleAdapter::new(config);

        let chat_resp = ChatResponse {
            id: None,
            choices: vec![ChatChoice {
                message: ChatMessage {
                    role: "assistant".to_string(),
                    content: None,
                    tool_calls: Some(vec![ChatToolCallResponse {
                        id: None,
                        call_type: Some("function".to_string()),
                        function: ChatFunctionCall {
                            name: Some("knowledge.search".to_string()),
                            arguments: "{}".to_string(),
                        },
                    }]),
                    tool_call_id: None,
                },
                finish_reason: None,
            }],
            usage: None,
            model: None,
        };

        let err = adapter.convert_response(chat_resp, "model-x").unwrap_err();
        assert_eq!(err, ModelAdapterError::MissingToolCallId);
    }

    #[test]
    fn convert_response_missing_tool_call_name_returns_error() {
        let config = OpenAiProviderConfig::new("https://api.example.com/v1", "key", "model-x");
        let adapter = OpenAiCompatibleAdapter::new(config);

        let chat_resp = ChatResponse {
            id: None,
            choices: vec![ChatChoice {
                message: ChatMessage {
                    role: "assistant".to_string(),
                    content: None,
                    tool_calls: Some(vec![ChatToolCallResponse {
                        id: Some("call_no_name".to_string()),
                        call_type: Some("function".to_string()),
                        function: ChatFunctionCall {
                            name: None,
                            arguments: "{}".to_string(),
                        },
                    }]),
                    tool_call_id: None,
                },
                finish_reason: None,
            }],
            usage: None,
            model: None,
        };

        let err = adapter.convert_response(chat_resp, "model-x").unwrap_err();
        assert_eq!(err, ModelAdapterError::MissingToolCallName);
    }

    #[test]
    fn chat_completions_url_construction() {
        // Trailing slash is stripped
        let config = OpenAiProviderConfig::new("https://api.deepseek.com/v1/", "key", "m");
        let adapter = OpenAiCompatibleAdapter::new(config);
        assert_eq!(
            adapter.chat_completions_url(),
            "https://api.deepseek.com/v1/chat/completions"
        );
    }

    #[test]
    fn chat_completions_url_no_trailing_slash() {
        let config = OpenAiProviderConfig::new("https://api.openai.com/v1", "key", "m");
        let adapter = OpenAiCompatibleAdapter::new(config);
        assert_eq!(
            adapter.chat_completions_url(),
            "https://api.openai.com/v1/chat/completions"
        );
    }

    #[test]
    fn error_response_deserializes() {
        let json = r#"{"error": {"message": "Invalid API key", "type": "authentication_error"}}"#;
        let err: ErrorResponse = serde_json::from_str(json).unwrap();
        assert_eq!(err.error.unwrap().message.unwrap(), "Invalid API key");
    }

    #[test]
    fn chat_response_deserializes_typical_deepseek() {
        let json = r#"{
            "id": "chatcmpl-abc123",
            "object": "chat.completion",
            "created": 1717000000,
            "model": "deepseek-chat",
            "choices": [{
                "index": 0,
                "message": {
                    "role": "assistant",
                    "content": "Sure, here's the summary:"
                },
                "finish_reason": "stop"
            }],
            "usage": {
                "prompt_tokens": 42,
                "completion_tokens": 15,
                "total_tokens": 57
            }
        }"#;

        let resp: ChatResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.choices.len(), 1);
        assert_eq!(
            resp.choices[0].message.content.as_deref(),
            Some("Sure, here's the summary:")
        );
        assert_eq!(resp.usage.as_ref().unwrap().prompt_tokens, Some(42));
        assert_eq!(resp.model.as_deref(), Some("deepseek-chat"));
    }

    #[tokio::test]
    async fn empty_request_returns_error() {
        let config = OpenAiProviderConfig::new("https://api.example.com/v1", "key", "m");
        let adapter = OpenAiCompatibleAdapter::new(config);

        let request = ModelRequest::new("m", vec![]);
        let err = adapter.complete(request).await.unwrap_err();
        assert_eq!(err, ModelAdapterError::EmptyRequest);
    }
}
