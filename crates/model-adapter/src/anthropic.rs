//! Anthropic Messages API Adapter
//!
//! Implements `ModelAdapter` for the Anthropic Messages API (Claude).
//!
//! Reference: https://docs.anthropic.com/claude/reference/messages-post
//!
//! This adapter:
//! - Uses the Messages API (not the legacy Completions API)
//! - Supports text-only completions (no tool use in this PR)
//! - Configures from environment variables
//! - Maps Anthropic-specific errors to `ModelAdapterError`

use crate::{ModelAdapter, ModelAdapterError, ModelOutput, ModelRequest, ModelRole, ModelUsage};
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
}

/// Anthropic message format.
#[derive(Debug, Serialize, Deserialize)]
struct AnthropicMessage {
    role: String,
    content: String,
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
                content: m.text.clone(),
            })
            .collect();

        let max_tokens = request.max_output_tokens.unwrap_or(4096);

        let temperature = request.temperature_millis.map(|t| t as f64 / 1000.0);

        AnthropicRequest {
            model: model_id,
            max_tokens,
            messages,
            system,
            temperature,
        }
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

        if text.is_empty() {
            return Err(ModelAdapterError::EmptyResponse);
        }

        let usage = ModelUsage {
            input_tokens: response.usage.input_tokens,
            output_tokens: response.usage.output_tokens,
        };

        Ok(ModelOutput::Text {
            text,
            usage: Some(usage),
        })
    }
}

#[async_trait]
impl ModelAdapter for AnthropicAdapter {
    async fn complete(&self, request: ModelRequest) -> Result<ModelOutput, ModelAdapterError> {
        let anthropic_request = self.convert_request(&request);

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
                },
                AnthropicContent {
                    content_type: "text".to_string(),
                    text: Some("world!".to_string()),
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
                content: "Hello".to_string(),
            }],
            system: Some("You are helpful".to_string()),
            temperature: Some(0.7),
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
                content: "Hello".to_string(),
            }],
            system: None,
            temperature: None,
        };

        let json = serde_json::to_string(&request).unwrap();
        assert!(!json.contains("\"system\""));
        assert!(!json.contains("\"temperature\""));
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
