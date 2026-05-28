use agentos_kernel::{
    CURRENT_KERNEL_EVENT_SCHEMA_VERSION, JsonlKernelEventStore, KernelAggregateRef,
    KernelEventCursor, KernelEventEnvelope, KernelEventId, KernelEventStore, KernelEventStoreError,
    KernelRedactionClass, MemoryKernelEventStore,
};
use serde_json::json;

fn event(id: u64, aggregate_id: &str) -> KernelEventEnvelope {
    KernelEventEnvelope::new(
        KernelEventId(id),
        "conversation.created",
        KernelAggregateRef::new("conversation", aggregate_id),
        json!({ "conversation_id": aggregate_id }),
    )
}

#[tokio::test]
async fn memory_event_store_appends_and_reads_after_cursor() {
    let store = MemoryKernelEventStore::new();
    let cursor = store.append(event(1, "c1")).await.unwrap();
    store.append(event(2, "c1")).await.unwrap();
    store.append(event(3, "c2")).await.unwrap();

    assert_eq!(cursor.last_seen, Some(KernelEventId(1)));
    let after = store
        .events_after(Some(KernelEventCursor::after(KernelEventId(1))))
        .await
        .unwrap();
    assert_eq!(after.len(), 2);
    assert_eq!(after[0].event_id, KernelEventId(2));
    assert_eq!(after[1].event_id, KernelEventId(3));
}

#[tokio::test]
async fn memory_event_store_rejects_duplicate_and_non_monotonic_events() {
    let store = MemoryKernelEventStore::new();
    store.append(event(2, "c1")).await.unwrap();

    let duplicate = store.append(event(2, "c1")).await.unwrap_err();
    assert!(matches!(
        duplicate,
        KernelEventStoreError::AlreadyExists { event_id: 2 }
    ));

    let non_monotonic = store.append(event(1, "c1")).await.unwrap_err();
    assert!(matches!(
        non_monotonic,
        KernelEventStoreError::NonMonotonicId { last: 2, next: 1 }
    ));
}

#[tokio::test]
async fn memory_event_store_supports_idempotent_append_and_aggregate_query() {
    let store = MemoryKernelEventStore::new();
    store.append_idempotent(event(1, "c1")).await.unwrap();
    store.append_idempotent(event(1, "c1")).await.unwrap();
    store.append_idempotent(event(2, "c2")).await.unwrap();

    let c1 = store
        .events_for_aggregate(&KernelAggregateRef::new("conversation", "c1"))
        .await
        .unwrap();
    assert_eq!(c1.len(), 1);
    assert_eq!(store.events_after(None).await.unwrap().len(), 2);
}

#[tokio::test]
async fn jsonl_event_store_persists_and_reloads_events() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("events.jsonl");
    let store = JsonlKernelEventStore::open(&path).await.unwrap();
    store
        .append(event(1, "c1").with_redaction_class(KernelRedactionClass::SensitiveContent))
        .await
        .unwrap();
    store.append(event(2, "c1")).await.unwrap();

    let reopened = JsonlKernelEventStore::open(&path).await.unwrap();
    let events = reopened.events_after(None).await.unwrap();
    assert_eq!(events.len(), 2);
    assert_eq!(
        events[0].schema_version,
        CURRENT_KERNEL_EVENT_SCHEMA_VERSION
    );
    assert_eq!(
        events[0].redaction_class,
        KernelRedactionClass::SensitiveContent
    );
    assert_eq!(
        reopened.latest_cursor().await.unwrap().unwrap().last_seen,
        Some(KernelEventId(2))
    );
}
