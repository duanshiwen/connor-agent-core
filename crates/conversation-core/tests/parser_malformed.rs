use conversation_core::ConversationEventEnvelope;

#[test]
fn malformed_event_envelopes_return_errors_without_panicking() {
    let malformed_inputs = [
        "not-json",
        "{}",
        r#"{"schema_version":"one","event_id":"event-1","conversation_id":"conv-1","occurred_at":"not-a-date","event":{"type":"conversation_created"}}"#,
        r#"{"schema_version":1,"event_id":"event-1","conversation_id":"conv-1","occurred_at":"2026-05-27T00:00:00Z","event":{"type":"unknown_event"}}"#,
        r#"{"schema_version":1,"event_id":"event-1","conversation_id":"conv-1","occurred_at":"2026-05-27T00:00:00Z","event":{"type":"message_appended","message":null}}"#,
        "[1, 2, 3]",
    ];

    for input in malformed_inputs {
        let result = serde_json::from_str::<ConversationEventEnvelope>(input);
        assert!(
            result.is_err(),
            "malformed event envelope should return an error for input: {input:?}"
        );
    }
}
