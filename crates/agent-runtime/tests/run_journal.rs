use agent_runtime::{
    DurableAgentRunStatus, JsonlRunJournal, MemoryRunJournal, RunCheckpoint, RunEvent, RunEventId,
    RunEventKind, RunJournal, RunJournalCursor, RunRecoveryAction,
};
use chrono::Utc;
use serde_json::json;

fn run_event(id: u64, run_id: &str, kind: RunEventKind) -> RunEvent {
    RunEvent::new(RunEventId(id), run_id, kind)
}

#[tokio::test]
async fn memory_run_journal_appends_and_reads_after_cursor() {
    let journal = MemoryRunJournal::new();
    journal
        .append_run_event(run_event(1, "run-1", RunEventKind::RunRequested))
        .await
        .unwrap();
    journal
        .append_run_event(run_event(2, "run-1", RunEventKind::RunStarted))
        .await
        .unwrap();
    journal
        .append_run_event(run_event(3, "run-2", RunEventKind::RunRequested))
        .await
        .unwrap();

    let after = journal
        .events_after(Some(RunJournalCursor::after(RunEventId(1))))
        .await
        .unwrap();
    assert_eq!(after.len(), 2);
    assert_eq!(after[0].event_id, RunEventId(2));
    assert_eq!(after[1].run_id, "run-2");
}

#[tokio::test]
async fn recovery_report_is_conservative_for_pending_side_effects() {
    let journal = MemoryRunJournal::new();
    journal
        .append_run_event(run_event(1, "completed", RunEventKind::RunRequested))
        .await
        .unwrap();
    journal
        .append_run_event(run_event(2, "completed", RunEventKind::RunCompleted))
        .await
        .unwrap();
    journal
        .append_run_event(run_event(3, "approval", RunEventKind::ApprovalRequested))
        .await
        .unwrap();
    journal
        .append_run_event(run_event(4, "tool", RunEventKind::ToolCallStarted))
        .await
        .unwrap();
    journal
        .append_run_event(run_event(5, "model", RunEventKind::ModelCallStarted))
        .await
        .unwrap();

    let report = journal.recover_pending_runs().await.unwrap();
    assert_eq!(report.recoverable_runs.len(), 3);
    assert!(report.recoverable_runs.iter().any(|run| {
        run.run_id == "approval"
            && run.recommended_action == RunRecoveryAction::RestorePendingApproval
    }));
    assert!(report.recoverable_runs.iter().any(|run| {
        run.run_id == "tool" && run.recommended_action == RunRecoveryAction::NeedsInspection
    }));
    assert!(report.recoverable_runs.iter().any(|run| {
        run.run_id == "model"
            && run.recommended_action == RunRecoveryAction::MarkCancelledAfterRestart
    }));
}

#[tokio::test]
async fn jsonl_run_journal_persists_events_and_checkpoints() {
    let dir = tempfile::tempdir().unwrap();
    let journal = JsonlRunJournal::open(dir.path()).await.unwrap();
    journal
        .append_run_event(run_event(1, "run-1", RunEventKind::RunRequested))
        .await
        .unwrap();
    journal
        .checkpoint_run(RunCheckpoint {
            run_id: "run-1".to_string(),
            event_cursor: RunJournalCursor::after(RunEventId(1)),
            status: DurableAgentRunStatus::Running,
            checkpointed_at: Utc::now(),
            payload: json!({ "step": "model_call" }),
        })
        .await
        .unwrap();

    let reopened = JsonlRunJournal::open(dir.path()).await.unwrap();
    let events = reopened.events_for_run("run-1").await.unwrap();
    let checkpoints = reopened.checkpoints("run-1").await.unwrap();
    assert_eq!(events.len(), 1);
    assert_eq!(checkpoints.len(), 1);
    assert_eq!(checkpoints[0].status, DurableAgentRunStatus::Running);
}
