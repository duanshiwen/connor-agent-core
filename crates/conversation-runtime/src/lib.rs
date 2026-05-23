//! # Conversation Runtime
//!
//! Runtime boundary for consuming `AgentRunRequested` events and producing
//! assistant output messages.
//!
//! This crate intentionally does not embed any concrete LLM. Model execution is
//! abstracted behind `AgentRunExecutor`, allowing deterministic tests via
//! `FakeAgentRunExecutor` and future platform-specific executors.

use anyhow::{Context, Result, bail};
use async_trait::async_trait;
use conversation_core::{
    ConversationEvent, ConversationId, Message, MessageContent, MessageId, ParticipantKind,
    Visibility,
};
use conversation_kernel::{AppendMessageCommand, CompleteAgentRunCommand, ConversationKernel};

/// Input passed from the runtime to an agent executor.
#[derive(Debug, Clone, PartialEq)]
pub struct AgentRunRequest {
    pub conversation_id: ConversationId,
    pub run_id: String,
    pub trigger_message_id: MessageId,
    pub context_slice_id: String,
    pub context_messages: Vec<Message>,
}

/// Output produced by an agent executor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentRunOutput {
    pub text: String,
}

/// Executes an agent run request.
#[async_trait]
pub trait AgentRunExecutor: Send + Sync {
    async fn execute(&self, request: AgentRunRequest) -> Result<AgentRunOutput>;
}

/// Deterministic executor for tests and early runtime integration.
#[derive(Debug, Clone, Default)]
pub struct FakeAgentRunExecutor;

#[async_trait]
impl AgentRunExecutor for FakeAgentRunExecutor {
    async fn execute(&self, request: AgentRunRequest) -> Result<AgentRunOutput> {
        Ok(AgentRunOutput {
            text: format!(
                "Fake response for run {} with {} context message(s)",
                request.run_id,
                request.context_messages.len()
            ),
        })
    }
}

/// Runtime that consumes requested agent runs and writes their output back into
/// the conversation via the kernel.
pub struct ConversationRuntime<E> {
    kernel: ConversationKernel,
    executor: E,
}

impl<E> ConversationRuntime<E>
where
    E: AgentRunExecutor,
{
    /// Create a new runtime using the given kernel and executor.
    pub fn new(kernel: ConversationKernel, executor: E) -> Self {
        Self { kernel, executor }
    }

    /// Process a pending agent run.
    ///
    /// If the run has already completed, this method is idempotent and returns
    /// the existing output message ID without appending another assistant
    /// message.
    pub async fn process_pending_run(
        &self,
        conversation_id: &ConversationId,
        run_id: &str,
    ) -> Result<MessageId> {
        let state = self.kernel.load_state(conversation_id).await?;

        if let Some(output_message_id) = state.completed_agent_runs.get(run_id) {
            return Ok(output_message_id.clone());
        }

        let requested = self
            .find_agent_run_requested(conversation_id, run_id)
            .await?;
        let context_messages = requested
            .message_ids
            .iter()
            .map(|message_id| {
                state
                    .messages_by_id
                    .get(message_id)
                    .cloned()
                    .with_context(|| format!("context message not found: {message_id}"))
            })
            .collect::<Result<Vec<_>>>()?;

        let request = AgentRunRequest {
            conversation_id: conversation_id.clone(),
            run_id: run_id.to_string(),
            trigger_message_id: requested.trigger_message_id.clone(),
            context_slice_id: requested.context_slice_id.clone(),
            context_messages,
        };

        let output = self.executor.execute(request).await?;
        let agent_sender_id = state
            .participants
            .values()
            .find(|participant| participant.kind == ParticipantKind::Agent)
            .map(|participant| participant.id.clone())
            .context("no agent participant found in conversation")?;

        let output_message_id = self
            .kernel
            .append_message(AppendMessageCommand {
                conversation_id: conversation_id.clone(),
                sender_id: agent_sender_id.clone(),
                content: MessageContent::Text { text: output.text },
                reply_to: Some(requested.trigger_message_id),
                thread_id: None,
                visibility: Visibility::Conversation,
            })
            .await?;

        self.kernel
            .complete_agent_run(CompleteAgentRunCommand {
                conversation_id: conversation_id.clone(),
                run_id: run_id.to_string(),
                output_message_id: output_message_id.clone(),
                completed_by: agent_sender_id,
            })
            .await?;

        Ok(output_message_id)
    }

    async fn find_agent_run_requested(
        &self,
        conversation_id: &ConversationId,
        run_id: &str,
    ) -> Result<RequestedRun> {
        let events = self.kernel.load_events(conversation_id).await?;

        let mut requested_run = None;
        for envelope in &events {
            if let ConversationEvent::AgentRunRequested {
                run_id: candidate_run_id,
                trigger_message_id,
                context_slice_id,
            } = &envelope.event
                && candidate_run_id == run_id
            {
                requested_run = Some((trigger_message_id.clone(), context_slice_id.clone()));
                break;
            }
        }

        let Some((trigger_message_id, context_slice_id)) = requested_run else {
            bail!("agent run not found: {run_id}");
        };

        let mut message_ids = None;
        for envelope in &events {
            if let ConversationEvent::ContextSliceBuilt {
                slice_id,
                message_ids: candidate_message_ids,
                ..
            } = &envelope.event
                && slice_id == &context_slice_id
            {
                message_ids = Some(candidate_message_ids.clone());
                break;
            }
        }

        let Some(message_ids) = message_ids else {
            bail!("context slice not found: {context_slice_id}");
        };

        Ok(RequestedRun {
            trigger_message_id,
            context_slice_id,
            message_ids,
        })
    }
}

#[derive(Debug, Clone)]
struct RequestedRun {
    trigger_message_id: MessageId,
    context_slice_id: String,
    message_ids: Vec<MessageId>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{DateTime, Utc};
    use conversation_core::*;
    use conversation_journal::MemoryConversationJournal;
    use conversation_kernel::{
        Clock, CreateConversationCommand, IdGenerator, RequestAgentRunCommand,
    };
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
            format!("id-{c}")
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

    fn test_kernel() -> ConversationKernel {
        let journal = Arc::new(MemoryConversationJournal::new());
        let id_gen = Arc::new(SequentialIdGenerator::new());
        let clock = Arc::new(FixedClock::new("2026-05-23T10:00:00Z".parse().unwrap()));
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

    #[tokio::test]
    async fn fake_executor_completes_requested_run() {
        let kernel = test_kernel();
        let conversation_id = kernel
            .create_conversation(CreateConversationCommand {
                kind: ConversationKind::AgentTask,
                title: Some("Runtime Test".into()),
                participants: vec![human("u1", "诗闻"), agent("a1", "Assistant")],
                actor_id: Some(ParticipantId::from("u1")),
            })
            .await
            .unwrap();

        let trigger_message_id = kernel
            .append_message(AppendMessageCommand {
                conversation_id: conversation_id.clone(),
                sender_id: ParticipantId::from("u1"),
                content: MessageContent::Text {
                    text: "帮我总结一下".into(),
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

        let runtime = ConversationRuntime::new(kernel, FakeAgentRunExecutor);
        let output_message_id = runtime
            .process_pending_run(&conversation_id, &run_id)
            .await
            .unwrap();

        let state = runtime.kernel.load_state(&conversation_id).await.unwrap();
        assert_eq!(state.messages.len(), 2);
        assert_eq!(
            state.completed_agent_runs.get(&run_id),
            Some(&output_message_id)
        );

        let output_message = state.messages_by_id.get(&output_message_id).unwrap();
        match &output_message.content {
            MessageContent::Text { text } => {
                assert!(text.contains(&format!("Fake response for run {run_id}")));
            }
            _ => panic!("expected text output"),
        }
    }

    #[tokio::test]
    async fn process_pending_run_is_idempotent() {
        let kernel = test_kernel();
        let conversation_id = kernel
            .create_conversation(CreateConversationCommand {
                kind: ConversationKind::AgentTask,
                title: None,
                participants: vec![human("u1", "诗闻"), agent("a1", "Assistant")],
                actor_id: Some(ParticipantId::from("u1")),
            })
            .await
            .unwrap();

        let trigger_message_id = kernel
            .append_message(AppendMessageCommand {
                conversation_id: conversation_id.clone(),
                sender_id: ParticipantId::from("u1"),
                content: MessageContent::Text {
                    text: "帮我分析".into(),
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

        let runtime = ConversationRuntime::new(kernel, FakeAgentRunExecutor);
        let first = runtime
            .process_pending_run(&conversation_id, &run_id)
            .await
            .unwrap();
        let second = runtime
            .process_pending_run(&conversation_id, &run_id)
            .await
            .unwrap();

        assert_eq!(first, second);

        let state = runtime.kernel.load_state(&conversation_id).await.unwrap();
        assert_eq!(state.messages.len(), 2);
        assert_eq!(state.completed_agent_runs.len(), 1);
    }

    #[tokio::test]
    async fn missing_run_returns_error() {
        let kernel = test_kernel();
        let conversation_id = kernel
            .create_conversation(CreateConversationCommand {
                kind: ConversationKind::AgentTask,
                title: None,
                participants: vec![human("u1", "诗闻"), agent("a1", "Assistant")],
                actor_id: Some(ParticipantId::from("u1")),
            })
            .await
            .unwrap();

        let runtime = ConversationRuntime::new(kernel, FakeAgentRunExecutor);
        let err = runtime
            .process_pending_run(&conversation_id, "missing-run")
            .await
            .unwrap_err();

        assert!(err.to_string().contains("agent run not found"));
    }
}
