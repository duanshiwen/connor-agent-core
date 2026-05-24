//! End-to-end integration tests for [`OpenAiCompatibleAdapter`].
//!
//! These tests spin up a local mock HTTP server and exercise the full
//! `ModelAdapter::complete()` → HTTP round-trip → response parsing pipeline.

use model_adapter::{
    ModelAdapter, ModelMessage, ModelRequest, OpenAiCompatibleAdapter, OpenAiProviderConfig,
};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpListener;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Read a full HTTP request from a stream, returning (headers, body).
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

    // Read Content-Length bytes
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

    let body_str = String::from_utf8_lossy(&body).to_string();
    (headers, body_str)
}

/// Start a mock server that captures the request body and responds with `response_body`.
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

/// Start a simple mock server that responds with `response_body` (no request capture).
async fn start_mock_server(response_body: String) -> (tokio::task::JoinHandle<()>, String) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let base_url = format!("http://{addr}");

    let handle = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let _ = read_http_request(&mut stream).await;

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
    });

    (handle, base_url)
}

/// Start a mock server that responds with an HTTP error status.
async fn start_error_server(status: &str, body: String) -> (tokio::task::JoinHandle<()>, String) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let base_url = format!("http://{addr}");

    let status_line = status.to_string();
    let handle = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let _ = read_http_request(&mut stream).await;

        let response = format!(
            "HTTP/1.1 {status_line}\r\n\
             Content-Type: application/json\r\n\
             Content-Length: {}\r\n\
             Connection: close\r\n\
             \r\n\
             {}",
            body.len(),
            body
        );
        stream.write_all(response.as_bytes()).await.unwrap();
        stream.flush().await.unwrap();
    });

    (handle, base_url)
}

fn config_for(url: &str) -> OpenAiProviderConfig {
    OpenAiProviderConfig::new(url, "test-api-key", "test-model")
}

fn ok_response(content: &str) -> String {
    serde_json::json!({
        "id": "chatcmpl-test",
        "object": "chat.completion",
        "created": 1717000000,
        "model": "test-model",
        "choices": [{
            "index": 0,
            "message": { "role": "assistant", "content": content },
            "finish_reason": "stop"
        }],
        "usage": { "prompt_tokens": 15, "completion_tokens": 10, "total_tokens": 25 }
    })
    .to_string()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn complete_returns_text_and_usage() {
    let (handle, base_url) =
        start_mock_server(ok_response("Hello! I am a helpful assistant.")).await;

    let adapter = OpenAiCompatibleAdapter::new(config_for(&base_url));
    let request = ModelRequest::new(
        "test-model",
        vec![
            ModelMessage::system("You are helpful."),
            ModelMessage::user("Hello"),
        ],
    );

    let response = adapter.complete(request).await.unwrap();
    assert_eq!(response.text, "Hello! I am a helpful assistant.");
    assert_eq!(
        response.model_id,
        model_adapter::ModelId("test-model".to_string())
    );

    let usage = response.usage.unwrap();
    assert_eq!(usage.input_tokens, 15);
    assert_eq!(usage.output_tokens, 10);

    handle.await.unwrap();
}

#[tokio::test]
async fn complete_passes_auth_header_and_model_in_body() {
    let (handle, base_url) = start_mock_server_with_capture(ok_response("Short.")).await;

    let adapter = OpenAiCompatibleAdapter::new(config_for(&base_url));
    let request = ModelRequest::new("test-model", vec![ModelMessage::user("test")]);

    let response = adapter.complete(request).await.unwrap();
    assert_eq!(response.text, "Short.");

    let (headers, body) = handle.await.unwrap();

    // Auth header (reqwest uses lowercase)
    assert!(
        headers.to_lowercase().contains("bearer test-api-key"),
        "missing auth header in: {headers}"
    );
    // Model in body
    assert!(
        body.contains(r#""model":"test-model""#),
        "model missing: {body}"
    );
}

#[tokio::test]
async fn complete_passes_temperature_and_max_tokens() {
    let (handle, base_url) = start_mock_server_with_capture(ok_response("Short.")).await;

    let config = OpenAiProviderConfig {
        endpoint: base_url,
        api_key: "test-api-key".to_string(),
        model: "test-model".to_string(),
        temperature: None,
        max_tokens: None,
        timeout_secs: None,
    };
    let adapter = OpenAiCompatibleAdapter::new(config);

    let mut request = ModelRequest::new("test-model", vec![ModelMessage::user("test")]);
    request.temperature_millis = Some(700); // → 0.7
    request.max_output_tokens = Some(256);

    let response = adapter.complete(request).await.unwrap();
    assert_eq!(response.text, "Short.");

    let (_headers, body) = handle.await.unwrap();
    assert!(
        body.contains(r#""temperature":0.7"#),
        "temperature missing: {body}"
    );
    assert!(
        body.contains(r#""max_tokens":256"#),
        "max_tokens missing: {body}"
    );
}

#[tokio::test]
async fn complete_preserves_multi_turn_order() {
    let (handle, base_url) =
        start_mock_server_with_capture(ok_response("Summary of the conversation.")).await;

    let adapter = OpenAiCompatibleAdapter::new(config_for(&base_url));
    let request = ModelRequest::new(
        "test-model",
        vec![
            ModelMessage::system("You are concise."),
            ModelMessage::user("Hello"),
            ModelMessage::assistant("Hi there"),
            ModelMessage::user("Summarize"),
        ],
    );

    let response = adapter.complete(request).await.unwrap();
    assert_eq!(response.text, "Summary of the conversation.");

    let (_headers, body) = handle.await.unwrap();
    assert!(body.contains("You are concise."));
    assert!(body.contains("Hello"));
    assert!(body.contains("Hi there"));
    assert!(body.contains("Summarize"));
}

#[tokio::test]
async fn complete_handles_api_error() {
    let error_body = serde_json::json!({
        "error": {
            "message": "Invalid API key provided.",
            "type": "authentication_error",
            "code": "invalid_api_key"
        }
    })
    .to_string();

    let (handle, base_url) = start_error_server("401 Unauthorized", error_body).await;

    let adapter = OpenAiCompatibleAdapter::new(config_for(&base_url));
    let request = ModelRequest::new("test-model", vec![ModelMessage::user("test")]);

    let err = adapter.complete(request).await.unwrap_err();
    let err_str = err.to_string();
    assert!(
        err_str.contains("401") || err_str.contains("Invalid API key"),
        "unexpected error: {err_str}"
    );

    handle.await.unwrap();
}

#[tokio::test]
async fn complete_rejects_empty_request() {
    let adapter = OpenAiCompatibleAdapter::new(config_for("http://127.0.0.1:1"));
    let request = ModelRequest::new("test-model", vec![]);

    let err = adapter.complete(request).await.unwrap_err();
    assert_eq!(err, model_adapter::ModelAdapterError::EmptyRequest);
}

#[tokio::test]
async fn complete_handles_malformed_json() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let base_url = format!("http://{addr}");

    let handle = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let _ = read_http_request(&mut stream).await;

        let body = "this is not json";
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body
        );
        stream.write_all(response.as_bytes()).await.unwrap();
        stream.flush().await.unwrap();
    });

    let adapter = OpenAiCompatibleAdapter::new(config_for(&base_url));
    let request = ModelRequest::new("test-model", vec![ModelMessage::user("test")]);

    let err = adapter.complete(request).await.unwrap_err();
    assert!(
        err.to_string().contains("failed to parse response"),
        "unexpected error: {err}"
    );

    handle.await.unwrap();
}

#[tokio::test]
async fn complete_handles_empty_choices() {
    let empty_choices = serde_json::json!({
        "id": "chatcmpl-empty",
        "choices": [],
        "usage": null,
        "model": "test-model"
    })
    .to_string();

    let (handle, base_url) = start_mock_server(empty_choices).await;

    let adapter = OpenAiCompatibleAdapter::new(config_for(&base_url));
    let request = ModelRequest::new("test-model", vec![ModelMessage::user("test")]);

    let err = adapter.complete(request).await.unwrap_err();
    assert!(
        err.to_string().contains("no choices"),
        "unexpected error: {err}"
    );

    handle.await.unwrap();
}

#[tokio::test]
async fn complete_without_usage_field() {
    let no_usage = serde_json::json!({
        "id": "chatcmpl-nousage",
        "choices": [{
            "index": 0,
            "message": { "role": "assistant", "content": "OK" },
            "finish_reason": "stop"
        }],
        "model": "test-model"
    })
    .to_string();

    let (handle, base_url) = start_mock_server(no_usage).await;

    let adapter = OpenAiCompatibleAdapter::new(config_for(&base_url));
    let request = ModelRequest::new("test-model", vec![ModelMessage::user("test")]);

    let response = adapter.complete(request).await.unwrap();
    assert_eq!(response.text, "OK");
    assert!(response.usage.is_none());

    handle.await.unwrap();
}

#[tokio::test]
async fn config_from_env_fails_without_api_key() {
    // Safety: this test modifies environment. Run serially.
    unsafe {
        std::env::remove_var("OPENAI_API_KEY");
    }
    let result = OpenAiProviderConfig::from_env();
    assert!(result.is_err());
    assert!(
        result
            .unwrap_err()
            .to_string()
            .contains("OPENAI_API_KEY not set")
    );
}
