use agentos_client_bridge::AgentOsClientBridge;
use serde_json::Value;

fn response_json(response: agentos_client_bridge::BridgeResponse) -> Value {
    serde_json::from_str(&response.json).unwrap()
}

#[test]
fn bridge_health_projection_schema_contains_required_fields() {
    let bridge = AgentOsClientBridge::for_tests().unwrap();
    let value = response_json(bridge.storage_health_report_json().unwrap());
    assert!(value.get("profile_id").is_some());
    assert!(value.get("workspace_id").is_some());
    assert!(value.get("healthy").is_some());
    assert!(value.get("requires_migration").is_some());
    assert!(value.get("issues").is_some());
}

#[test]
fn bridge_knowledge_and_asset_schema_contains_required_fields() {
    let bridge = AgentOsClientBridge::for_tests().unwrap();
    let knowledge = response_json(bridge.knowledge_projection_json().unwrap());
    let assets = response_json(bridge.asset_projection_json().unwrap());
    assert!(knowledge.get("last_query").is_some());
    assert!(knowledge.get("results").is_some());
    assert!(assets.get("assets").is_some());
}
