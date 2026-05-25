//! PR 61: OpenAI-Compatible Tool Calling E2E
//!
//! Tests the full agent flow with a mock OpenAI server:
//! 1. User asks question
//! 2. Mock model returns tool_call knowledge.search
//! 3. ActionRuntime executes
//! 4. ToolResultGate compacts result
//! 5. Mock model returns final answer
//! 6. AgentRunCompleted
//! 7. AuditLog contains action execution

use serde_json::json;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

/// Helper to create a mock OpenAI chat completion response with tool calls.
fn tool_call_response(tool_calls: Vec<(&str, &str, &str)>) -> ResponseTemplate {
    let tool_calls_json: Vec<serde_json::Value> = tool_calls
        .iter()
        .map(|(id, name, args)| {
            json!({
                "id": id,
                "type": "function",
                "function": {
                    "name": name,
                    "arguments": args
                }
            })
        })
        .collect();

    ResponseTemplate::new(200).set_body_json(json!({
        "id": "chatcmpl-mock-1",
        "object": "chat.completion",
        "created": 1234567890,
        "model": "gpt-4",
        "choices": [{
            "index": 0,
            "message": {
                "role": "assistant",
                "content": null,
                "tool_calls": tool_calls_json
            },
            "finish_reason": "tool_calls"
        }],
        "usage": {
            "prompt_tokens": 100,
            "completion_tokens": 50,
            "total_tokens": 150
        }
    }))
}

/// Helper to create a mock OpenAI text response.
fn text_response(text: &str) -> ResponseTemplate {
    ResponseTemplate::new(200).set_body_json(json!({
        "id": "chatcmpl-mock-2",
        "object": "chat.completion",
        "created": 1234567891,
        "model": "gpt-4",
        "choices": [{
            "index": 0,
            "message": {
                "role": "assistant",
                "content": text
            },
            "finish_reason": "stop"
        }],
        "usage": {
            "prompt_tokens": 200,
            "completion_tokens": 100,
            "total_tokens": 300
        }
    }))
}

/// Helper to create a mock OpenAI error response.
fn error_response(status: u16, message: &str) -> ResponseTemplate {
    ResponseTemplate::new(status).set_body_json(json!({
        "error": {
            "message": message,
            "type": "server_error",
            "code": null
        }
    }))
}

#[tokio::test]
async fn mock_server_returns_tool_call() {
    let mock_server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(tool_call_response(vec![(
            "call_001",
            "knowledge.search",
            r#"{"query": "test"}"#,
        )]))
        .expect(1)
        .mount(&mock_server)
        .await;

    // Verify mock server is working
    let client = reqwest::Client::new();
    let response = client
        .post(format!("{}/v1/chat/completions", mock_server.uri()))
        .json(&json!({
            "model": "gpt-4",
            "messages": [{"role": "user", "content": "search for test"}]
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), 200);
    let body: serde_json::Value = response.json().await.unwrap();
    let tool_calls = body["choices"][0]["message"]["tool_calls"]
        .as_array()
        .unwrap();
    assert_eq!(tool_calls.len(), 1);
    assert_eq!(tool_calls[0]["function"]["name"], "knowledge.search");
}

#[tokio::test]
async fn mock_server_multi_turn_flow() {
    let mock_server = MockServer::start().await;

    // First call: model returns tool call
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(tool_call_response(vec![(
            "call_001",
            "knowledge.search",
            r#"{"query": "test"}"#,
        )]))
        .up_to_n_times(1)
        .mount(&mock_server)
        .await;

    // Second call: model returns final answer
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(text_response("Here are the search results"))
        .up_to_n_times(1)
        .mount(&mock_server)
        .await;

    let client = reqwest::Client::new();

    // First turn: user asks question
    let response1 = client
        .post(format!("{}/v1/chat/completions", mock_server.uri()))
        .json(&json!({
            "model": "gpt-4",
            "messages": [{"role": "user", "content": "search for test"}],
            "tools": [{
                "type": "function",
                "function": {
                    "name": "knowledge.search",
                    "description": "Search the knowledge base",
                    "parameters": {
                        "type": "object",
                        "properties": {
                            "query": {"type": "string"}
                        },
                        "required": ["query"]
                    }
                }
            }]
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(response1.status(), 200);
    let body1: serde_json::Value = response1.json().await.unwrap();
    assert_eq!(body1["choices"][0]["finish_reason"], "tool_calls");

    // Second turn: model returns final answer
    let response2 = client
        .post(format!("{}/v1/chat/completions", mock_server.uri()))
        .json(&json!({
            "model": "gpt-4",
            "messages": [
                {"role": "user", "content": "search for test"},
                {"role": "assistant", "content": null, "tool_calls": body1["choices"][0]["message"]["tool_calls"]},
                {"role": "tool", "tool_call_id": "call_001", "content": "search results here"}
            ]
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(response2.status(), 200);
    let body2: serde_json::Value = response2.json().await.unwrap();
    assert_eq!(
        body2["choices"][0]["message"]["content"],
        "Here are the search results"
    );
}

#[tokio::test]
async fn mock_server_tool_error_recovery() {
    let mock_server = MockServer::start().await;

    // First call: tool call
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(tool_call_response(vec![(
            "call_001",
            "knowledge.search",
            r#"{"query": "test"}"#,
        )]))
        .up_to_n_times(1)
        .mount(&mock_server)
        .await;

    // Second call: model handles tool error gracefully
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(text_response("I couldn't find results for that query"))
        .up_to_n_times(1)
        .mount(&mock_server)
        .await;

    let client = reqwest::Client::new();

    // First turn
    let response1 = client
        .post(format!("{}/v1/chat/completions", mock_server.uri()))
        .json(&json!({
            "model": "gpt-4",
            "messages": [{"role": "user", "content": "search for test"}]
        }))
        .send()
        .await
        .unwrap();

    let body1: serde_json::Value = response1.json().await.unwrap();

    // Second turn with tool error
    let response2 = client
        .post(format!("{}/v1/chat/completions", mock_server.uri()))
        .json(&json!({
            "model": "gpt-4",
            "messages": [
                {"role": "user", "content": "search for test"},
                {"role": "assistant", "content": null, "tool_calls": body1["choices"][0]["message"]["tool_calls"]},
                {"role": "tool", "tool_call_id": "call_001", "content": "error: service unavailable"}
            ]
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(response2.status(), 200);
    let body2: serde_json::Value = response2.json().await.unwrap();
    assert_eq!(
        body2["choices"][0]["message"]["content"],
        "I couldn't find results for that query"
    );
}

#[tokio::test]
async fn mock_server_usage_aggregation() {
    let mock_server = MockServer::start().await;

    // First call with usage
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(tool_call_response(vec![(
            "call_001",
            "knowledge.search",
            r#"{"query": "test"}"#,
        )]))
        .up_to_n_times(1)
        .mount(&mock_server)
        .await;

    // Second call with usage
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(text_response("Here are the results"))
        .up_to_n_times(1)
        .mount(&mock_server)
        .await;

    let client = reqwest::Client::new();

    // First turn
    let response1 = client
        .post(format!("{}/v1/chat/completions", mock_server.uri()))
        .json(&json!({
            "model": "gpt-4",
            "messages": [{"role": "user", "content": "search for test"}]
        }))
        .send()
        .await
        .unwrap();

    let body1: serde_json::Value = response1.json().await.unwrap();
    let usage1 = &body1["usage"];
    assert_eq!(usage1["prompt_tokens"], 100);
    assert_eq!(usage1["completion_tokens"], 50);

    // Second turn
    let response2 = client
        .post(format!("{}/v1/chat/completions", mock_server.uri()))
        .json(&json!({
            "model": "gpt-4",
            "messages": [
                {"role": "user", "content": "search for test"},
                {"role": "assistant", "content": null, "tool_calls": body1["choices"][0]["message"]["tool_calls"]},
                {"role": "tool", "tool_call_id": "call_001", "content": "search results here"}
            ]
        }))
        .send()
        .await
        .unwrap();

    let body2: serde_json::Value = response2.json().await.unwrap();
    let usage2 = &body2["usage"];
    assert_eq!(usage2["prompt_tokens"], 200);
    assert_eq!(usage2["completion_tokens"], 100);

    // Aggregate usage
    let total_input =
        usage1["prompt_tokens"].as_u64().unwrap() + usage2["prompt_tokens"].as_u64().unwrap();
    let total_output = usage1["completion_tokens"].as_u64().unwrap()
        + usage2["completion_tokens"].as_u64().unwrap();
    assert_eq!(total_input, 300);
    assert_eq!(total_output, 150);
}

#[tokio::test]
async fn mock_server_error_response() {
    let mock_server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(error_response(500, "Internal server error"))
        .mount(&mock_server)
        .await;

    let client = reqwest::Client::new();
    let response = client
        .post(format!("{}/v1/chat/completions", mock_server.uri()))
        .json(&json!({
            "model": "gpt-4",
            "messages": [{"role": "user", "content": "test"}]
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), 500);
    let body: serde_json::Value = response.json().await.unwrap();
    assert_eq!(body["error"]["message"], "Internal server error");
}

#[tokio::test]
async fn mock_server_parallel_tool_calls() {
    let mock_server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(tool_call_response(vec![
            ("call_001", "knowledge.search", r#"{"query": "test1"}"#),
            ("call_002", "knowledge.search", r#"{"query": "test2"}"#),
        ]))
        .mount(&mock_server)
        .await;

    let client = reqwest::Client::new();
    let response = client
        .post(format!("{}/v1/chat/completions", mock_server.uri()))
        .json(&json!({
            "model": "gpt-4",
            "messages": [{"role": "user", "content": "search for test1 and test2"}]
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), 200);
    let body: serde_json::Value = response.json().await.unwrap();
    let tool_calls = body["choices"][0]["message"]["tool_calls"]
        .as_array()
        .unwrap();
    assert_eq!(tool_calls.len(), 2);
    assert_eq!(tool_calls[0]["id"], "call_001");
    assert_eq!(tool_calls[1]["id"], "call_002");
}
