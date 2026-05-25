use agent_runtime::{
    MemoryToolLoopCheckpointStore, ToolLoopCheckpoint, ToolLoopCheckpointKind,
    ToolLoopCheckpointStore, ToolLoopResumePlan, ToolResultCheckpoint,
};

#[tokio::test]
async fn checkpoint_store_records_before_and_after_model_call() {
    let store = MemoryToolLoopCheckpointStore::new();

    store
        .append(ToolLoopCheckpoint::before_model_call("run-1", 1))
        .await
        .unwrap();
    store
        .append(ToolLoopCheckpoint::after_model_call("run-1", 1))
        .await
        .unwrap();

    let checkpoints = store.list("run-1").await.unwrap();

    assert_eq!(checkpoints.len(), 2);
    assert_eq!(checkpoints[0].kind, ToolLoopCheckpointKind::BeforeModelCall);
    assert_eq!(checkpoints[1].kind, ToolLoopCheckpointKind::AfterModelCall);
}

#[tokio::test]
async fn checkpoint_store_records_tool_results_once_by_tool_call_id() {
    let store = MemoryToolLoopCheckpointStore::new();

    store
        .append(ToolLoopCheckpoint::tool_result(
            "run-1",
            1,
            ToolResultCheckpoint {
                tool_call_id: "call-1".to_string(),
                action_id: "action-1".to_string(),
                result_text: "first result".to_string(),
                read_only: true,
            },
        ))
        .await
        .unwrap();
    store
        .append(ToolLoopCheckpoint::tool_result(
            "run-1",
            1,
            ToolResultCheckpoint {
                tool_call_id: "call-1".to_string(),
                action_id: "action-1".to_string(),
                result_text: "duplicate result".to_string(),
                read_only: true,
            },
        ))
        .await
        .unwrap();

    let plan = ToolLoopResumePlan::from_checkpoints(store.list("run-1").await.unwrap());

    assert_eq!(plan.completed_tool_result("call-1"), Some("first result"));
    assert_eq!(plan.completed_tool_results().len(), 1);
}

#[tokio::test]
async fn resume_plan_skips_completed_read_only_tool_result() {
    let checkpoints = vec![ToolLoopCheckpoint::tool_result(
        "run-1",
        2,
        ToolResultCheckpoint {
            tool_call_id: "search-1".to_string(),
            action_id: "action-search-1".to_string(),
            result_text: "cached search result".to_string(),
            read_only: true,
        },
    )];

    let plan = ToolLoopResumePlan::from_checkpoints(checkpoints);

    assert!(plan.should_skip_tool_call("search-1"));
    assert_eq!(
        plan.completed_tool_result("search-1"),
        Some("cached search result")
    );
    assert!(!plan.should_skip_tool_call("write-1"));
}

#[tokio::test]
async fn resume_plan_tracks_last_completed_turn() {
    let checkpoints = vec![
        ToolLoopCheckpoint::before_model_call("run-1", 1),
        ToolLoopCheckpoint::after_model_call("run-1", 1),
        ToolLoopCheckpoint::before_model_call("run-1", 2),
    ];

    let plan = ToolLoopResumePlan::from_checkpoints(checkpoints);

    assert_eq!(plan.last_started_turn(), Some(2));
    assert_eq!(plan.last_completed_model_turn(), Some(1));
}
