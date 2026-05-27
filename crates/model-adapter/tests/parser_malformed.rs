use model_adapter::{ModelOutput, ModelStreamEvent, ToolCall};

#[test]
fn malformed_tool_call_payloads_return_errors_without_panicking() {
    let malformed_tool_calls = [
        "not-json",
        "{}",
        r#"{"id":"call-1","arguments":{},"raw_arguments":"{}"}"#,
        "[]",
    ];

    for input in malformed_tool_calls {
        let result = serde_json::from_str::<ToolCall>(input);
        assert!(
            result.is_err(),
            "malformed tool call should return an error for input: {input:?}"
        );
    }
}

#[test]
fn malformed_model_outputs_and_stream_events_return_errors_without_panicking() {
    let malformed_outputs = [
        "not-json",
        "{}",
        r#"{"type":"tool_calls","tool_calls":"not-a-list","usage":null}"#,
        r#"{"type":"unknown","text":"hello","usage":null}"#,
    ];

    for input in malformed_outputs {
        let result = serde_json::from_str::<ModelOutput>(input);
        assert!(
            result.is_err(),
            "malformed model output should return an error for input: {input:?}"
        );
    }

    let malformed_stream_events = [
        "not-json",
        "{}",
        r#"{"type":"tool_call_delta","index":"zero"}"#,
        r#"{"type":"finished","reason":"unknown"}"#,
    ];

    for input in malformed_stream_events {
        let result = serde_json::from_str::<ModelStreamEvent>(input);
        assert!(
            result.is_err(),
            "malformed model stream event should return an error for input: {input:?}"
        );
    }
}
