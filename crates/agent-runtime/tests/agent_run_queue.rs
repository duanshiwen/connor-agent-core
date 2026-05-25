use std::sync::Arc;
use std::time::Duration;

use agent_runtime::{
    AgentRunQueue, AgentRunRecord, AgentRunStore, DurableAgentRunStatus, MemoryAgentRunStore,
};
use chrono::Utc;
use conversation_core::{ConversationId, MessageId, ParticipantId};

fn record(run_id: &str) -> AgentRunRecord {
    AgentRunRecord {
        run_id: run_id.to_string(),
        conversation_id: ConversationId("conversation-1".to_string()),
        trigger_message_id: MessageId("message-1".to_string()),
        requested_by: ParticipantId("user-1".to_string()),
        status: DurableAgentRunStatus::Queued,
        created_at: Utc::now(),
        updated_at: Utc::now(),
        error_message: None,
    }
}

#[tokio::test]
async fn queue_enqueues_and_leases_runs_in_fifo_order() {
    let store = Arc::new(MemoryAgentRunStore::new());
    let queue = AgentRunQueue::new(store.clone(), Duration::from_secs(30));

    queue.enqueue(record("run-1")).await.unwrap();
    queue.enqueue(record("run-2")).await.unwrap();

    let lease_1 = queue.lease().await.unwrap().unwrap();
    let lease_2 = queue.lease().await.unwrap().unwrap();

    assert_eq!(lease_1.run_id, "run-1");
    assert_eq!(lease_2.run_id, "run-2");
    assert_eq!(
        store.get("run-1").await.unwrap().unwrap().status,
        DurableAgentRunStatus::Running
    );
}

#[tokio::test]
async fn queue_does_not_lease_same_run_twice_while_lease_active() {
    let store = Arc::new(MemoryAgentRunStore::new());
    let queue = AgentRunQueue::new(store, Duration::from_secs(30));

    queue.enqueue(record("run-1")).await.unwrap();

    let first = queue.lease().await.unwrap().unwrap();
    let second = queue.lease().await.unwrap();

    assert_eq!(first.run_id, "run-1");
    assert!(second.is_none());
}

#[tokio::test]
async fn queue_ack_completes_and_removes_active_lease() {
    let store = Arc::new(MemoryAgentRunStore::new());
    let queue = AgentRunQueue::new(store.clone(), Duration::from_secs(30));

    queue.enqueue(record("run-1")).await.unwrap();
    queue.lease().await.unwrap().unwrap();
    queue.ack("run-1").await.unwrap();

    assert_eq!(
        store.get("run-1").await.unwrap().unwrap().status,
        DurableAgentRunStatus::Completed
    );
    assert!(queue.lease().await.unwrap().is_none());
}

#[tokio::test]
async fn queue_nack_requeues_active_lease() {
    let store = Arc::new(MemoryAgentRunStore::new());
    let queue = AgentRunQueue::new(store.clone(), Duration::from_secs(30));

    queue.enqueue(record("run-1")).await.unwrap();
    queue.lease().await.unwrap().unwrap();
    queue.nack("run-1").await.unwrap();

    assert_eq!(
        store.get("run-1").await.unwrap().unwrap().status,
        DurableAgentRunStatus::Queued
    );
    assert_eq!(queue.lease().await.unwrap().unwrap().run_id, "run-1");
}

#[tokio::test]
async fn queue_recovers_expired_lease() {
    let store = Arc::new(MemoryAgentRunStore::new());
    let queue = AgentRunQueue::new(store.clone(), Duration::from_millis(0));

    queue.enqueue(record("run-1")).await.unwrap();
    queue.lease().await.unwrap().unwrap();

    let recovered = queue.recover_expired_leases().await.unwrap();

    assert_eq!(recovered, vec!["run-1".to_string()]);
    assert_eq!(
        store.get("run-1").await.unwrap().unwrap().status,
        DurableAgentRunStatus::Queued
    );
    assert_eq!(queue.lease().await.unwrap().unwrap().run_id, "run-1");
}
