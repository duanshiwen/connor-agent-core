use agentos_client_bridge::AgentOsClientBridge;
use client_substrate::ClientEventCursor;

#[test]
fn bridge_contract_is_json_safe() {
    let bridge = AgentOsClientBridge::for_tests().unwrap();
    let cursor = serde_json::to_string(&ClientEventCursor::beginning()).unwrap();
    let events = bridge.events_after_json(&cursor).unwrap();
    assert!(events.ok);
    assert!(events.json.starts_with('['));

    let conversations = bridge.conversation_list_projection_json().unwrap();
    assert!(conversations.ok);
    assert!(conversations.json.contains("conversations"));
}
