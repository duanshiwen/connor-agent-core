//! End-to-end integration tests for Anthropic tool use.
//!
//! These tests spin up a local mock HTTP server and exercise
//! `ToolCallingModelAdapter::complete_with_tools()` through the real HTTP
//! request/response path without contacting Anthropic.

use model_adapter::{
    ModelMessage, ModelOutput, ModelRequest, ToolCallingModelAdapter, ToolChoice, ToolDefinition,
    anthropic::{AnthropicAdapter, AnthropicConfig},
};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpListener;

async fn read_http_request(stream: &mut tokio::net::TcpStream) -> (String, String) {
    let mut reader = BufReader::new(stream);
    let mut headers = String::new();
    loop {
        let mut line = String::new();
        reader.read_line(&mut line).await.unwrap();
        if line == "\r\n" || line.is_empty() {
            break;
        }
        headers.push_str(&line);
    }

    let content_length = headers
        .lines()
        .find_map(|line| {
            let lower = line.to_lowercase();
            if lower.starts_with("content-length:") {
                line.split(':').nth(1)?.trim().parse::<usize>().ok()
            } else {
                None
            }
        })
        .unwrap_or(0);

    let mut body = vec![0u8; content_length];
    if content_length > 0 {
        reader.read_exact(&mut body).await.unwrap();
    }

    (headers, String::from_utf8_lossy(&body).to_string())
}

async fn start_mock_server_with_capture(
    response_body: String,
) -> (tokio::task::JoinHandle<(String, String)>, String) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let base_url = format!("http://{addr}");

    let handle = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let (headers, body) = read_http_request(&mut stream).await;

        let response = format!(
            "HTTP/1.1 200 OK\r\n\
             Content-Type: application/json\r\n\
             Content-Length: {}\r\n\
             Connection: close\r\n\
             \r\n\
             {}",
            response_body.len(),
            response_body
        );
        stream.write_all(response.as_bytes()).await.unwrap();
        stream.flush().await.unwrap();

        (headers, body)
    });

    (handle, base_url)
}

fn config_for(url: &str) -> AnthropicConfig {
    AnthropicConfig {
        api_key: "test-api-key".to_string(),
        base_url: url.to_string(),
        default_model: "claude-test".to_string(),
        api_version: "2023-06-01".to_string(),
    }
}

fn tool_definition() -> ToolDefinition {
    ToolDefinition {
        name: "knowledge.search".to_string(),
        description: "Search knowledge entries.".to_string(),
        input_schema: serde_json::json!({
            "type": "object",
            "properties": {
                "query": { "type": "string" }
            },
            "required": ["query"]
        }),
    }
}

fn tool_use_response() -> String {
    serde_json::json!({
        "id": "msg_123",
        "model": "claude-test",
        "content": [
            { "type": "text", "text": "I'll search the knowledge base." },
            {
                "type": "tool_use",
                "id": "toolu_123",
                "name": "knowledge_search",
                "input": { "query": "AgentOS" }
            }
        ],
        "usage": { "input_tokens": 30, "output_tokens": 15 },
        "stop_reason": "tool_use"
    })
    .to_string()
}

#[tokio::test]
async fn complete_with_tools_sends_anthropic_tool_schema_and_choice() {
    let (handle, base_url) = start_mock_server_with_capture(tool_use_response()).await;
    let adapter = AnthropicAdapter::new(config_for(&base_url));
    let request = ModelRequest::new("claude-test", vec![ModelMessage::user("Search AgentOS")]);

    let output = adapter
        .complete_with_tools(
            request,
            vec![tool_definition()],
            ToolChoice::Named("knowledge.search".to_string()),
        )
        .await
        .unwrap();

    assert!(matches!(output, ModelOutput::ToolCalls { .. }));
    let (headers, body) = handle.await.unwrap();
    assert!(headers.to_lowercase().contains("x-api-key: test-api-key"));

    let body_json: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(body_json["tools"][0]["name"], "knowledge_search");
    assert_eq!(
        body_json["tools"][0]["description"],
        "Search knowledge entries."
    );
    assert_eq!(body_json["tool_choice"]["type"], "tool");
    assert_eq!(body_json["tool_choice"]["name"], "knowledge_search");
}

#[tokio::test]
async fn complete_with_tools_parses_tool_use_response() {
    let (handle, base_url) = start_mock_server_with_capture(tool_use_response()).await;
    let adapter = AnthropicAdapter::new(config_for(&base_url));
    let request = ModelRequest::new("claude-test", vec![ModelMessage::user("Search AgentOS")]);

    let output = adapter
        .complete_with_tools(request, vec![tool_definition()], ToolChoice::Auto)
        .await
        .unwrap();

    match output {
        ModelOutput::ToolCalls {
            content,
            tool_calls,
            usage,
        } => {
            assert_eq!(content, Some("I'll search the knowledge base.".to_string()));
            assert_eq!(tool_calls.len(), 1);
            assert_eq!(tool_calls[0].id, "toolu_123");
            assert_eq!(tool_calls[0].name, "knowledge_search");
            assert_eq!(tool_calls[0].arguments["query"], "AgentOS");
            assert_eq!(usage.unwrap().total_tokens(), 45);
        }
        other => panic!("expected tool calls, got {other:?}"),
    }

    handle.await.unwrap();
}
