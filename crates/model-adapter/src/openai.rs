//! OpenAI-compatible model adapter.
//!
//! Implements [`ModelAdapter`] for any provider that exposes the OpenAI
//! Chat Completions API format (DeepSeek, Qwen, OpenAI, vLLM, etc.).

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::{ModelAdapter, ModelAdapterError, ModelRequest, ModelResponse, ModelRole, ModelUsage};

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
struct ChatRequest<'a> {
    model: &'a str,
    messages: Vec<ChatMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_tokens: Option<u32>,
}

#[derive(Debug, Serialize, Deserialize)]
struct ChatMessage {
    role: String,
    content: String,
}

#[derive(Debug, Deserialize)]
struct ChatResponse {
    #[allow(dead_code)]
    id: Option<String>,
    choices: Vec<ChatChoice>,
    usage: Option<ChatUsage>,
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

    fn chat_completions_url(&self) -> String {
        let base = self.config.endpoint.trim_end_matches('/');
        format!("{base}/chat/completions")
    }

    fn convert_role(role: &ModelRole) -> &'static str {
        match role {
            ModelRole::System => "system",
            ModelRole::User => "user",
            ModelRole::Assistant => "assistant",
        }
    }

    fn convert_request(&self, request: &ModelRequest) -> ChatRequest<'_> {
        let messages: Vec<ChatMessage> = request
            .messages
            .iter()
            .map(|m| ChatMessage {
                role: Self::convert_role(&m.role).to_string(),
                content: m.text.clone(),
            })
            .collect();

        // Priority: per-request temperature_millis → config temperature
        let temperature = request
            .temperature_millis
            .map(|t| t as f32 / 1000.0)
            .or(self.config.temperature);

        // Priority: per-request max_output_tokens → config max_tokens
        let max_tokens = request.max_output_tokens.or(self.config.max_tokens);

        ChatRequest {
            model: &self.config.model,
            messages,
            temperature,
            max_tokens,
        }
    }

    fn convert_response(
        &self,
        resp: ChatResponse,
        fallback_model: &str,
    ) -> Result<ModelResponse, ModelAdapterError> {
        let choice = resp.choices.into_iter().next().ok_or_else(|| {
            ModelAdapterError::ExecutorFailed("no choices in response".to_string())
        })?;

        let usage = resp.usage.map(|u| ModelUsage {
            input_tokens: u.prompt_tokens.unwrap_or(0),
            output_tokens: u.completion_tokens.unwrap_or(0),
        });

        let model_id_str = resp.model.unwrap_or_else(|| fallback_model.to_string());

        Ok(ModelResponse {
            text: choice.message.content,
            usage,
            model_id: crate::ModelId(model_id_str),
        })
    }
}

#[async_trait]
impl ModelAdapter for OpenAiCompatibleAdapter {
    async fn complete(&self, request: ModelRequest) -> Result<ModelResponse, ModelAdapterError> {
        if request.messages.is_empty() {
            return Err(ModelAdapterError::EmptyRequest);
        }

        let url = self.chat_completions_url();
        let body = self.convert_request(&request);

        let response = self
            .client
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.config.api_key))
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await
            .map_err(|e| ModelAdapterError::ExecutorFailed(format!("HTTP request failed: {e}")))?;

        let status = response.status();

        if !status.is_success() {
            // Try to parse error body
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

        let chat_response: ChatResponse = response.json().await.map_err(|e| {
            ModelAdapterError::ExecutorFailed(format!("failed to parse response: {e}"))
        })?;

        self.convert_response(chat_response, &self.config.model)
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
        assert_eq!(chat_req.messages[0].content, "Be concise.");
        assert_eq!(chat_req.messages[1].role, "user");
        assert_eq!(chat_req.messages[1].content, "Hello");
        assert_eq!(chat_req.messages[2].role, "assistant");
        assert_eq!(chat_req.messages[2].content, "Hi there");
        assert_eq!(chat_req.messages[3].role, "user");
        assert_eq!(chat_req.messages[3].content, "Summarize");
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
                    content: "Hello! How can I help?".to_string(),
                },
                finish_reason: Some("stop".to_string()),
            }],
            usage: Some(ChatUsage {
                prompt_tokens: Some(10),
                completion_tokens: Some(8),
            }),
            model: Some("model-x".to_string()),
        };

        let response = adapter.convert_response(chat_resp, "model-x").unwrap();
        assert_eq!(response.text, "Hello! How can I help?");
        assert_eq!(response.model_id, crate::ModelId("model-x".to_string()));

        let usage = response.usage.unwrap();
        assert_eq!(usage.input_tokens, 10);
        assert_eq!(usage.output_tokens, 8);
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
                    content: "Hi".to_string(),
                },
                finish_reason: None,
            }],
            usage: None,
            model: None, // no model in response
        };

        let response = adapter.convert_response(chat_resp, "model-x").unwrap();
        assert_eq!(response.model_id, crate::ModelId("model-x".to_string()));
        assert_eq!(response.usage, None);
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
        assert_eq!(resp.choices[0].message.content, "Sure, here's the summary:");
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
