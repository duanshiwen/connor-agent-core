use agentos_client_bridge::{ClientReadySyncProjection, apply_sync_pull_response_json};
use serde_json::json;
use sync_runtime::{ServerSyncEvent, ServerSyncObjectType, ServerSyncOperation};

#[test]
fn sync_runtime_decodes_contact_object_type() {
    let event_json = json!({
        "id": "evt-contact-1",
        "user_id": "user-1",
        "device_id": "device-b",
        "event_type": "contact.created",
        "schema_version": 1,
        "object_type": "contact",
        "object_id": "contact-alice",
        "operation": "created",
        "source_device_id": "device-a",
        "client_event_id": "contact-create-1",
        "payload": {"contact_id": "contact-alice", "display_name": "Alice", "status": "active"},
        "timestamp": "2026-06-02T00:00:00Z",
        "sequence": 1
    });
    let event: ServerSyncEvent = serde_json::from_value(event_json).unwrap();
    assert_eq!(event.object_type, ServerSyncObjectType::Contact);
    assert_eq!(event.operation, ServerSyncOperation::Created);
}

#[test]
fn client_ready_projection_applies_contact_lifecycle_events() {
    let projection = serde_json::to_string(&ClientReadySyncProjection::new()).unwrap();
    let pull = json!({
        "code": 0,
        "message": "ok",
        "data": {
            "events": [
                {
                    "id": "evt-contact-1",
                    "user_id": "user-1",
                    "device_id": "device-b",
                    "event_type": "contact.created",
                    "schema_version": 1,
                    "object_type": "contact",
                    "object_id": "contact-alice",
                    "operation": "created",
                    "source_device_id": "device-a",
                    "client_event_id": "contact-create-1",
                    "payload": {"contact_id": "contact-alice", "display_name": "Alice", "status": "active"},
                    "timestamp": "2026-06-02T00:00:00Z",
                    "sequence": 1
                },
                {
                    "id": "evt-contact-2",
                    "user_id": "user-1",
                    "device_id": "device-b",
                    "event_type": "contact.updated",
                    "schema_version": 1,
                    "object_type": "contact",
                    "object_id": "contact-alice",
                    "operation": "updated",
                    "source_device_id": "device-a",
                    "client_event_id": "contact-update-1",
                    "payload": {"contact_id": "contact-alice", "display_name": "Alice Zhang", "status": "active"},
                    "timestamp": "2026-06-02T00:00:01Z",
                    "sequence": 2
                }
            ],
            "next_after_sequence": 2,
            "has_more": false,
            "server_time": 1780330000000_i64,
            "schema_version": 1
        }
    });

    let response = apply_sync_pull_response_json(&projection, &pull.to_string()).unwrap();
    let projection: ClientReadySyncProjection = serde_json::from_str(&response.json).unwrap();
    assert_eq!(projection.cursor.last_applied_sequence, 2);
    assert_eq!(
        projection.contacts["contact-alice"]["display_name"],
        "Alice Zhang"
    );

    let delete_pull = json!({
        "code": 0,
        "message": "ok",
        "data": {
            "events": [{
                "id": "evt-contact-3",
                "user_id": "user-1",
                "device_id": "device-b",
                "event_type": "contact.deleted",
                "schema_version": 1,
                "object_type": "contact",
                "object_id": "contact-alice",
                "operation": "deleted",
                "source_device_id": "device-a",
                "client_event_id": "contact-delete-1",
                "payload": {"contact_id": "contact-alice", "display_name": "Alice Zhang", "status": "deleted"},
                "timestamp": "2026-06-02T00:00:02Z",
                "sequence": 3
            }],
            "next_after_sequence": 3,
            "has_more": false,
            "server_time": 1780330000000_i64,
            "schema_version": 1
        }
    });

    let response = apply_sync_pull_response_json(&response.json, &delete_pull.to_string()).unwrap();
    let projection: ClientReadySyncProjection = serde_json::from_str(&response.json).unwrap();
    assert_eq!(projection.cursor.last_applied_sequence, 3);
    assert!(!projection.contacts.contains_key("contact-alice"));
}
