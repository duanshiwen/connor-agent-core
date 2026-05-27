use action_core::ActionRequest;

#[test]
fn malformed_action_requests_return_errors_without_panicking() {
    let malformed_inputs = [
        "not-json",
        "{}",
        r#"{"action_id":5,"action_kind":"knowledge.search","input":{},"requested_by":"agent","requested_at":"2026-05-27T00:00:00Z"}"#,
        r#"{"action_id":"action-1","action_kind":"knowledge.search","input":{},"requested_by":"agent","requested_at":"not-a-date"}"#,
        r#"{"action_id":"action-1","action_kind":"knowledge.search","requested_by":"agent","requested_at":"2026-05-27T00:00:00Z"}"#,
        "[]",
    ];

    for input in malformed_inputs {
        let result = serde_json::from_str::<ActionRequest>(input);
        assert!(
            result.is_err(),
            "malformed action request should return an error for input: {input:?}"
        );
    }
}
