# M2.3 Knowledge Sync Bridge Host Example

This example shows the intended host-side flow for consuming AgentOS backend
`/api/v1/sync/events` responses through the Core SDK bridge. The sync payload uses
a storage-neutral `content` field; it is not a Markdown-backed knowledge
repository, and hosts should not treat synced entries as authoritative `.md`
files.

The goal is deliberately narrow:

1. pull ordered sync events from the backend;
2. apply them through the SDK/Core reducer;
3. durably persist the updated projection and cursor;
4. only then ack the backend sequence.

WebSocket `sync.event` notifications should be treated as hints that trigger a
pull. The pull response remains the canonical ordered stream.

## Rust bridge flow

Use `agentos-client-bridge` when the host can link Rust crates directly.

```rust
use agentos_client_bridge::apply_knowledge_sync_pull_response_json;

fn apply_backend_pull_response(
    stored_projection_json: &str,
    backend_pull_response_json: &str,
) -> anyhow::Result<String> {
    let response = apply_knowledge_sync_pull_response_json(
        stored_projection_json,
        backend_pull_response_json,
    )?;

    // response.json is a serialized BridgeResponse whose `json` field contains
    // the updated KnowledgeSyncProjection JSON.
    Ok(response.json)
}
```

A host should parse and persist the returned projection before acking the
backend. The projection contains `cursor.last_applied_sequence`.

## C ABI / dylib flow

Use `agentos-ffi` when the host talks to `libagentos_ffi.dylib`.

Available exported functions:

```c
char *agentos_apply_knowledge_sync_events_json(
    const char *projection_json,
    const char *events_json,
    char **error_out
);

char *agentos_apply_knowledge_sync_pull_response_json(
    const char *projection_json,
    const char *pull_response_json,
    char **error_out
);

void agentos_ffi_free_string(char *ptr);
```

Example host pseudocode:

```c
char *error = NULL;
char *out = agentos_apply_knowledge_sync_pull_response_json(
    stored_projection_json,
    backend_pull_response_json,
    &error
);

if (out == NULL) {
    // error is allocated by the dylib when non-null.
    log_error(error);
    agentos_ffi_free_string(error);
    return;
}

// `out` is a BridgeResponse JSON string. Persist the updated projection/cursor
// before acking the backend.
persist_bridge_response(out);
agentos_ffi_free_string(out);
```

## Backend response shape

The full response helper accepts the standard Go backend API envelope:

```json
{
  "code": 0,
  "message": "ok",
  "data": {
    "events": [
      {
        "id": "evt-1",
        "user_id": "user-1",
        "device_id": "device-b",
        "event_type": "knowledge.created",
        "schema_version": 1,
        "object_type": "knowledge",
        "object_id": "notes/alpha",
        "operation": "created",
        "source_device_id": "device-a",
        "client_event_id": "client-1",
        "payload": {
          "entry_id": "notes/alpha",
          "object_id": "notes/alpha",
          "title": "Alpha",
          "content": "Alpha knowledge note",
          "summary": "summary",
          "tags": ["agentos"],
          "metadata": {},
          "source_uri": "",
          "status": "active",
          "version": 1,
          "content_hash": "hash-1",
          "updated_by_device_id": "device-a",
          "updated_at": "2026-05-30T02:00:00Z"
        },
        "timestamp": "2026-05-30T02:00:01Z",
        "sequence": 1
      }
    ],
    "next_after_sequence": 1,
    "has_more": false,
    "server_time": 1780106401000,
    "schema_version": 1
  }
}
```

## Ack discipline

Hosts must not ack before durable local apply:

```text
GET /api/v1/sync/events?after_sequence=<stored cursor>
  -> apply through bridge / FFI
  -> persist updated KnowledgeSyncProjection JSON
  -> POST /api/v1/sync/ack {"last_sequence": projection.cursor.last_applied_sequence}
```

If apply fails, do not ack. Keep the old projection and retry after resolving the
error.

## Reducer behavior

The bridge currently applies only personal knowledge projection events. These are
legacy bridge projection events, not direct writes to `.ke-store`; authoritative
object/relation writes must go through the Engine API and storage layer.

The bridge currently applies only personal knowledge events:

- `knowledge.created`
- `knowledge.updated`
- `knowledge.deleted`

Rules:

- unsupported schema versions fail fast;
- events at or below the local cursor are ignored;
- newer knowledge versions replace local entries;
- stale versions are ignored while the cursor advances;
- same-version `status` / `content_hash` mismatches fail;
- `knowledge.deleted` must carry tombstone status `deleted`.

KB Hub, marketplace, billing, federation, semantic indexing, and generic CRDT
merge are intentionally out of scope for this M2.3 bridge.
