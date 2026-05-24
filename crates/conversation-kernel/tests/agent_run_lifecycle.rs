use chrono::{DateTime, Utc};
use conversation_core::*;
use conversation_journal::MemoryConversationJournal;
use conversation_kernel::*;
use std::sync::Arc;

struct SequentialIdGenerator {
    counter: std::sync::Mutex<u64>,
}

impl SequentialIdGenerator {
    fn new() -> Self {
        Self {
            counter: std::sync::Mutex::new(0),
        }
    }
}

impl IdGenerator for SequentialIdGenerator {
    fn new_id(&self) -> String {
        let mut c = self.counter.lock().unwrap();
        *c += 1;
        format!("id-{}", c)
    }
}

struct FixedClock {
    time: DateTime<Utc>,
}

impl FixedClock {
    fn new(time: DateTime<Utc>) -> Self {
        Self { time }
    }
}

impl Clock for FixedClock {
    fn now(&self) -> DateTime<Utc> {
        self.time
    }
}

fn setup() -> ConversationKernel {
    let journal = Arc::new(MemoryConversationJournal::new());
    let id_gen = Arc::new(SequentialIdGenerator::new());
    let clock = Arc::new(FixedClock::new("2026-05-24T08:00:00Z".parse().unwrap()));
    ConversationKernel::with_generators(journal, id_gen, clock)
}

fn human(id: &str, name: &str) -> Participant {
    Participant {
        id: ParticipantId::from(id),
        kind: ParticipantKind::Human,
        display_name: name.to_string(),
    }
}

fn agent(id: &str, name: &str) -> Participant {
    Participant {
        id: ParticipantId::from(id),
        kind: ParticipantKind::Agent,
        display_name: name.to_string(),
    }
}

async fn create_conversation_with_run(kernel: &ConversationKernel) -> (ConversationId, String) {
    let conversation_id = kernel
        .create_conversation(CreateConversationCommand {
            kind: ConversationKind::AgentTask,
            title: Some("Agent run lifecycle".to_string()),
            participants: vec![human("u1", "Test User"), agent("a1", "Assistant")],
            actor_id: Some(ParticipantId::from("u1")),
        })
        .await
        .unwrap();

    let trigger_message_id = kernel
        .append_message(AppendMessageCommand {
            conversation_id: conversation_id.clone(),
            sender_id: ParticipantId::from("u1"),
            content: MessageContent::Text {
                text: "帮我测试 agent run lifecycle".to_string(),
            },
            reply_to: None,
            thread_id: None,
            visibility: Visibility::Conversation,
        })
        .await
        .unwrap();

    let run_id = kernel
        .request_agent_run(RequestAgentRunCommand {
            conversation_id: conversation_id.clone(),
            trigger_message_id,
            requested_by: ParticipantId::from("u1"),
        })
        .await
        .unwrap();

    (conversation_id, run_id)
}

#[tokio::test]
async fn request_projects_requested_status() {
    let kernel = setup();
    let (conversation_id, run_id) = create_conversation_with_run(&kernel).await;

    let state = kernel.load_state(&conversation_id).await.unwrap();
    let run = state.agent_runs.get(&run_id).unwrap();

    assert_eq!(run.status, AgentRunStatus::Requested);
    assert!(run.output_message_id.is_none());
}

#[tokio::test]
async fn request_then_start_projects_started_status() {
    let kernel = setup();
    let (conversation_id, run_id) = create_conversation_with_run(&kernel).await;

    kernel
        .start_agent_run(StartAgentRunCommand {
            conversation_id: conversation_id.clone(),
            run_id: run_id.clone(),
            started_by: ParticipantId::from("a1"),
        })
        .await
        .unwrap();

    let state = kernel.load_state(&conversation_id).await.unwrap();
    assert_eq!(state.agent_runs[&run_id].status, AgentRunStatus::Started);
}

#[tokio::test]
async fn request_then_complete_projects_completed_status() {
    let kernel = setup();
    let (conversation_id, run_id) = create_conversation_with_run(&kernel).await;

    let output_message_id = kernel
        .append_message(AppendMessageCommand {
            conversation_id: conversation_id.clone(),
            sender_id: ParticipantId::from("a1"),
            content: MessageContent::Text {
                text: "Done".to_string(),
            },
            reply_to: None,
            thread_id: None,
            visibility: Visibility::Conversation,
        })
        .await
        .unwrap();

    kernel
        .complete_agent_run(CompleteAgentRunCommand {
            conversation_id: conversation_id.clone(),
            run_id: run_id.clone(),
            output_message_id: output_message_id.clone(),
            completed_by: ParticipantId::from("a1"),
        })
        .await
        .unwrap();

    let state = kernel.load_state(&conversation_id).await.unwrap();
    let run = &state.agent_runs[&run_id];
    assert_eq!(run.status, AgentRunStatus::Completed);
    assert_eq!(run.output_message_id, Some(output_message_id.clone()));
    assert_eq!(state.completed_agent_runs[&run_id], output_message_id);
}

#[tokio::test]
async fn request_then_fail_projects_failed_status() {
    let kernel = setup();
    let (conversation_id, run_id) = create_conversation_with_run(&kernel).await;

    kernel
        .fail_agent_run(FailAgentRunCommand {
            conversation_id: conversation_id.clone(),
            run_id: run_id.clone(),
            error_code: "executor_error".to_string(),
            error_message: "boom".to_string(),
            failed_by: ParticipantId::from("a1"),
        })
        .await
        .unwrap();

    let state = kernel.load_state(&conversation_id).await.unwrap();
    let run = &state.agent_runs[&run_id];
    assert_eq!(run.status, AgentRunStatus::Failed);
    assert_eq!(run.error_code.as_deref(), Some("executor_error"));
    assert_eq!(run.error_message.as_deref(), Some("boom"));
}

#[tokio::test]
async fn request_then_cancel_projects_cancelled_status() {
    let kernel = setup();
    let (conversation_id, run_id) = create_conversation_with_run(&kernel).await;

    kernel
        .cancel_agent_run(CancelAgentRunCommand {
            conversation_id: conversation_id.clone(),
            run_id: run_id.clone(),
            reason: "user cancelled".to_string(),
            cancelled_by: ParticipantId::from("u1"),
        })
        .await
        .unwrap();

    let state = kernel.load_state(&conversation_id).await.unwrap();
    let run = &state.agent_runs[&run_id];
    assert_eq!(run.status, AgentRunStatus::Cancelled);
    assert_eq!(run.cancel_reason.as_deref(), Some("user cancelled"));
}

#[tokio::test]
async fn request_then_timeout_projects_timed_out_status() {
    let kernel = setup();
    let (conversation_id, run_id) = create_conversation_with_run(&kernel).await;

    kernel
        .timeout_agent_run(TimeoutAgentRunCommand {
            conversation_id: conversation_id.clone(),
            run_id: run_id.clone(),
            timed_out_by: Some(ParticipantId::from("a1")),
        })
        .await
        .unwrap();

    let state = kernel.load_state(&conversation_id).await.unwrap();
    assert_eq!(state.agent_runs[&run_id].status, AgentRunStatus::TimedOut);
}

#[tokio::test]
async fn cannot_start_missing_run() {
    let kernel = setup();
    let (conversation_id, _) = create_conversation_with_run(&kernel).await;

    let result = kernel
        .start_agent_run(StartAgentRunCommand {
            conversation_id,
            run_id: "missing-run".to_string(),
            started_by: ParticipantId::from("a1"),
        })
        .await;

    assert!(result.is_err());
    assert!(
        result
            .unwrap_err()
            .to_string()
            .contains("agent run not found")
    );
}

#[tokio::test]
async fn terminal_run_rejects_later_transition() {
    let kernel = setup();
    let (conversation_id, run_id) = create_conversation_with_run(&kernel).await;

    kernel
        .fail_agent_run(FailAgentRunCommand {
            conversation_id: conversation_id.clone(),
            run_id: run_id.clone(),
            error_code: "executor_error".to_string(),
            error_message: "boom".to_string(),
            failed_by: ParticipantId::from("a1"),
        })
        .await
        .unwrap();

    let result = kernel
        .start_agent_run(StartAgentRunCommand {
            conversation_id,
            run_id,
            started_by: ParticipantId::from("a1"),
        })
        .await;

    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("already terminal"));
}
