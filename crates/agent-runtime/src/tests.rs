use super::*;
use async_trait::async_trait;
use audit_log::AuditLog;
use conversation_journal::MemoryConversationJournal;
use conversation_kernel::{Clock, IdGenerator};
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
    time: chrono::DateTime<chrono::Utc>,
}

impl FixedClock {
    fn new(time: chrono::DateTime<chrono::Utc>) -> Self {
        Self { time }
    }
}

impl Clock for FixedClock {
    fn now(&self) -> chrono::DateTime<chrono::Utc> {
        self.time
    }
}

fn test_kernel() -> ConversationKernel {
    let journal = Arc::new(MemoryConversationJournal::new());
    let id_gen = Arc::new(SequentialIdGenerator::new());
    let clock = Arc::new(FixedClock::new("2026-01-01T00:00:00Z".parse().unwrap()));
    ConversationKernel::with_generators(journal, id_gen, clock)
}

fn human(id: &str, name: &str) -> Participant {
    Participant {
        id: ParticipantId::from(id),
        kind: ParticipantKind::Human,
        display_name: name.to_string(),
    }
}

fn agent_participant(id: &str, name: &str) -> Participant {
    Participant {
        id: ParticipantId::from(id),
        kind: ParticipantKind::Agent,
        display_name: name.to_string(),
    }
}

// ── Context Builder tests ──────────────────────────────────────────

#[tokio::test]
async fn context_builder_selects_messages_up_to_trigger() {
    let kernel = test_kernel();

    let conv_id = kernel
        .create_conversation(conversation_kernel::CreateConversationCommand {
            kind: ConversationKind::Direct,
            title: None,
            participants: vec![
                human("u1", "Test User"),
                agent_participant("a1", "Assistant"),
            ],
            actor_id: Some(ParticipantId::from("u1")),
        })
        .await
        .unwrap();

    // Append 5 messages.
    let mut msg_ids = Vec::new();
    for i in 0..5 {
        let sender = if i % 2 == 0 { "u1" } else { "a1" };
        let id = kernel
            .append_message(conversation_kernel::AppendMessageCommand {
                conversation_id: conv_id.clone(),
                sender_id: ParticipantId::from(sender),
                content: MessageContent::Text {
                    text: format!("message {i}"),
                },
                reply_to: None,
                thread_id: None,
                visibility: Visibility::Conversation,
            })
            .await
            .unwrap();
        msg_ids.push(id);
    }

    let state = kernel.load_state(&conv_id).await.unwrap();
    let builder = AgentContextBuilder::new(10);
    let config = AgentRuntimeConfig::default();

    let context = builder
        .build(&state, "run-1", &msg_ids[3], &config)
        .unwrap();

    // Should include messages 0..=3 (4 messages), respecting max_context_messages.
    assert_eq!(context.messages.len(), 4);
    assert_eq!(context.trigger_message.id, msg_ids[3]);
    assert_eq!(context.conversation_id, conv_id);
}

#[tokio::test]
async fn context_builder_truncates_to_max_messages() {
    let kernel = test_kernel();

    let conv_id = kernel
        .create_conversation(conversation_kernel::CreateConversationCommand {
            kind: ConversationKind::Direct,
            title: None,
            participants: vec![
                human("u1", "Test User"),
                agent_participant("a1", "Assistant"),
            ],
            actor_id: Some(ParticipantId::from("u1")),
        })
        .await
        .unwrap();

    // Append 10 messages.
    let mut msg_ids = Vec::new();
    for i in 0..10 {
        let sender = if i % 2 == 0 { "u1" } else { "a1" };
        let id = kernel
            .append_message(conversation_kernel::AppendMessageCommand {
                conversation_id: conv_id.clone(),
                sender_id: ParticipantId::from(sender),
                content: MessageContent::Text {
                    text: format!("message {i}"),
                },
                reply_to: None,
                thread_id: None,
                visibility: Visibility::Conversation,
            })
            .await
            .unwrap();
        msg_ids.push(id);
    }

    let state = kernel.load_state(&conv_id).await.unwrap();
    let builder = AgentContextBuilder::new(3); // Only 3 messages max.
    let config = AgentRuntimeConfig::default();

    let context = builder
        .build(&state, "run-1", &msg_ids[9], &config)
        .unwrap();

    // Should include only the last 3 messages (7, 8, 9).
    assert_eq!(context.messages.len(), 3);
}

#[tokio::test]
async fn context_builder_fails_for_missing_trigger() {
    let kernel = test_kernel();

    let conv_id = kernel
        .create_conversation(conversation_kernel::CreateConversationCommand {
            kind: ConversationKind::Direct,
            title: None,
            participants: vec![
                human("u1", "Test User"),
                agent_participant("a1", "Assistant"),
            ],
            actor_id: Some(ParticipantId::from("u1")),
        })
        .await
        .unwrap();

    let state = kernel.load_state(&conv_id).await.unwrap();
    let builder = AgentContextBuilder::new(10);
    let config = AgentRuntimeConfig::default();

    let result = builder.build(&state, "run-1", &MessageId::from("nonexistent"), &config);
    assert!(result.is_err());
}

// ── Prompt Renderer tests ──────────────────────────────────────────

#[tokio::test]
async fn prompt_renderer_converts_messages_to_model_roles() {
    let context = AgentContext {
        conversation_id: ConversationId::from("conv-1"),
        run_id: "run-1".to_string(),
        trigger_message: Message {
            id: MessageId::from("msg-1"),
            conversation_id: ConversationId::from("conv-1"),
            sender_id: ParticipantId::from("u1"),
            content: MessageContent::Text {
                text: "hello".to_string(),
            },
            reply_to: None,
            thread_id: None,
            visibility: Visibility::Conversation,
            created_at: chrono::Utc::now(),
            edited_at: None,
        },
        messages: vec![
            Message {
                id: MessageId::from("msg-1"),
                conversation_id: ConversationId::from("conv-1"),
                sender_id: ParticipantId::from("u1"),
                content: MessageContent::Text {
                    text: "帮我总结".to_string(),
                },
                reply_to: None,
                thread_id: None,
                visibility: Visibility::Conversation,
                created_at: chrono::Utc::now(),
                edited_at: None,
            },
            Message {
                id: MessageId::from("msg-2"),
                conversation_id: ConversationId::from("conv-1"),
                sender_id: ParticipantId::from("a1"),
                content: MessageContent::Text {
                    text: "好的".to_string(),
                },
                reply_to: None,
                thread_id: None,
                visibility: Visibility::Conversation,
                created_at: chrono::Utc::now(),
                edited_at: None,
            },
        ],
        participants: HashMap::from([
            (ParticipantId::from("u1"), human("u1", "Test User")),
            (
                ParticipantId::from("a1"),
                agent_participant("a1", "Assistant"),
            ),
        ]),
        linked_entities: vec![],
        model_id: ModelId::from("test-model"),
        system_prompt: Some("You are helpful.".to_string()),
    };

    let request = PromptRenderer::render(&context);

    assert_eq!(request.model_id, ModelId::from("test-model"));
    // System prompt + 2 messages = 3.
    assert_eq!(request.messages.len(), 3);
    assert_eq!(request.messages[0].role, ModelRole::System);
    assert_eq!(request.messages[0].text, "You are helpful.");
    assert_eq!(request.messages[1].role, ModelRole::User);
    assert_eq!(request.messages[1].text, "帮我总结");
    assert_eq!(request.messages[2].role, ModelRole::Assistant);
    assert_eq!(request.messages[2].text, "好的");
}

#[tokio::test]
async fn prompt_renderer_no_system_prompt() {
    let context = AgentContext {
        conversation_id: ConversationId::from("conv-1"),
        run_id: "run-1".to_string(),
        trigger_message: Message {
            id: MessageId::from("msg-1"),
            conversation_id: ConversationId::from("conv-1"),
            sender_id: ParticipantId::from("u1"),
            content: MessageContent::Text {
                text: "hello".to_string(),
            },
            reply_to: None,
            thread_id: None,
            visibility: Visibility::Conversation,
            created_at: chrono::Utc::now(),
            edited_at: None,
        },
        messages: vec![Message {
            id: MessageId::from("msg-1"),
            conversation_id: ConversationId::from("conv-1"),
            sender_id: ParticipantId::from("u1"),
            content: MessageContent::Text {
                text: "hello".to_string(),
            },
            reply_to: None,
            thread_id: None,
            visibility: Visibility::Conversation,
            created_at: chrono::Utc::now(),
            edited_at: None,
        }],
        participants: HashMap::from([(ParticipantId::from("u1"), human("u1", "Test User"))]),
        linked_entities: vec![],
        model_id: ModelId::from("test-model"),
        system_prompt: None,
    };

    let request = PromptRenderer::render(&context);
    // No system prompt, just 1 message.
    assert_eq!(request.messages.len(), 1);
    assert_eq!(request.messages[0].role, ModelRole::User);
}

// ── AgentRunProcessor E2E tests ────────────────────────────────────

#[tokio::test]
async fn e2e_user_message_to_assistant_response() {
    let kernel = test_kernel();
    let adapter = StaticModelAdapter::new("Assistant reply");

    // Create conversation.
    let conv_id = kernel
        .create_conversation(conversation_kernel::CreateConversationCommand {
            kind: ConversationKind::AgentTask,
            title: Some("Test Task".to_string()),
            participants: vec![
                human("u1", "Test User"),
                agent_participant("a1", "Assistant"),
            ],
            actor_id: Some(ParticipantId::from("u1")),
        })
        .await
        .unwrap();

    // User sends a message.
    let user_msg = kernel
        .append_message(conversation_kernel::AppendMessageCommand {
            conversation_id: conv_id.clone(),
            sender_id: ParticipantId::from("u1"),
            content: MessageContent::Text {
                text: "帮我总结这段话".to_string(),
            },
            reply_to: None,
            thread_id: None,
            visibility: Visibility::Conversation,
        })
        .await
        .unwrap();

    // Request agent run.
    let run_id = kernel
        .request_agent_run(conversation_kernel::RequestAgentRunCommand {
            conversation_id: conv_id.clone(),
            trigger_message_id: user_msg.clone(),
            requested_by: ParticipantId::from("u1"),
        })
        .await
        .unwrap();

    // Process the run.
    let context_builder = AgentContextBuilder::new(50);
    let config = AgentRuntimeConfig::default();

    let outcome = AgentRunProcessor::process(ProcessRunRequest {
        kernel: &kernel,
        adapter: &adapter,
        context_builder: &context_builder,
        config: &config,
        conversation_id: &conv_id,
        run_id: &run_id,
        trigger_message_id: &user_msg,
        agent_participant_id: &ParticipantId::from("a1"),
    })
    .await
    .unwrap();

    // Verify outcome.
    match &outcome {
        AgentRunOutcome::Completed {
            run_id: rid,
            response_text,
            ..
        } => {
            assert_eq!(rid, &run_id);
            assert!(response_text.contains("Assistant reply"));
            assert!(response_text.contains("帮我总结这段话"));
        }
        AgentRunOutcome::Failed { .. } => panic!("expected completion"),
    }

    // Verify state.
    let state = kernel.load_state(&conv_id).await.unwrap();
    // 2 participants + user message + assistant message = 4 messages? No.
    // Messages: user_msg + assistant_msg = 2
    assert_eq!(state.messages.len(), 2);
    assert_eq!(state.messages[1].sender_id, ParticipantId::from("a1"));

    // Verify agent run is completed.
    let run_state = state.agent_runs.get(&run_id).unwrap();
    assert!(matches!(run_state.status, AgentRunStatus::Completed));
}

#[tokio::test]
async fn e2e_model_failure_records_failed_run() {
    let kernel = test_kernel();
    // Use an adapter that rejects empty requests.
    let _adapter = StaticModelAdapter::default();

    let conv_id = kernel
        .create_conversation(conversation_kernel::CreateConversationCommand {
            kind: ConversationKind::AgentTask,
            title: None,
            participants: vec![
                human("u1", "Test User"),
                agent_participant("a1", "Assistant"),
            ],
            actor_id: Some(ParticipantId::from("u1")),
        })
        .await
        .unwrap();

    // Request agent run without any messages (empty context → model fails).
    // First append a message so request_agent_run has a trigger.
    let user_msg = kernel
        .append_message(conversation_kernel::AppendMessageCommand {
            conversation_id: conv_id.clone(),
            sender_id: ParticipantId::from("u1"),
            content: MessageContent::Text {
                text: "hello".to_string(),
            },
            reply_to: None,
            thread_id: None,
            visibility: Visibility::Conversation,
        })
        .await
        .unwrap();

    let run_id = kernel
        .request_agent_run(conversation_kernel::RequestAgentRunCommand {
            conversation_id: conv_id.clone(),
            trigger_message_id: user_msg.clone(),
            requested_by: ParticipantId::from("u1"),
        })
        .await
        .unwrap();

    // Use a custom adapter that always fails.
    struct FailingAdapter;
    #[async_trait]
    impl ModelAdapter for FailingAdapter {
        async fn complete(&self, _request: ModelRequest) -> Result<ModelOutput, ModelAdapterError> {
            Err(ModelAdapterError::ExecutorFailed(
                "simulated failure".to_string(),
            ))
        }
    }

    let context_builder = AgentContextBuilder::new(50);
    let config = AgentRuntimeConfig::default();

    let outcome = AgentRunProcessor::process(ProcessRunRequest {
        kernel: &kernel,
        adapter: &FailingAdapter,
        context_builder: &context_builder,
        config: &config,
        conversation_id: &conv_id,
        run_id: &run_id,
        trigger_message_id: &user_msg,
        agent_participant_id: &ParticipantId::from("a1"),
    })
    .await
    .unwrap();

    // Should be a failed outcome.
    match &outcome {
        AgentRunOutcome::Failed {
            error_code,
            error_message,
            ..
        } => {
            assert_eq!(error_code, "model_call_failed");
            assert!(error_message.contains("simulated failure"));
        }
        AgentRunOutcome::Completed { .. } => panic!("expected failure"),
    }

    // Verify agent run is failed in state.
    let state = kernel.load_state(&conv_id).await.unwrap();
    let run_state = state.agent_runs.get(&run_id).unwrap();
    assert!(matches!(run_state.status, AgentRunStatus::Failed));
}

#[tokio::test]
async fn e2e_agent_runtime_high_level_entry() {
    let kernel = test_kernel();
    let adapter = StaticModelAdapter::new("High-level reply");
    let config = AgentRuntimeConfig {
        system_prompt: Some("You are a concise assistant.".to_string()),
        ..Default::default()
    };

    let runtime = AgentRuntime::new(
        kernel.clone(),
        Box::new(adapter),
        config,
        ParticipantId::from("a1"),
    );

    // Create conversation and message.
    let conv_id = runtime
        .kernel
        .create_conversation(conversation_kernel::CreateConversationCommand {
            kind: ConversationKind::AgentTask,
            title: None,
            participants: vec![
                human("u1", "Test User"),
                agent_participant("a1", "Assistant"),
            ],
            actor_id: Some(ParticipantId::from("u1")),
        })
        .await
        .unwrap();

    let user_msg = runtime
        .kernel
        .append_message(conversation_kernel::AppendMessageCommand {
            conversation_id: conv_id.clone(),
            sender_id: ParticipantId::from("u1"),
            content: MessageContent::Text {
                text: "你好".to_string(),
            },
            reply_to: None,
            thread_id: None,
            visibility: Visibility::Conversation,
        })
        .await
        .unwrap();

    let run_id = runtime
        .kernel
        .request_agent_run(conversation_kernel::RequestAgentRunCommand {
            conversation_id: conv_id.clone(),
            trigger_message_id: user_msg.clone(),
            requested_by: ParticipantId::from("u1"),
        })
        .await
        .unwrap();

    let outcome = runtime
        .process_run(&conv_id, &run_id, &user_msg)
        .await
        .unwrap();

    assert!(matches!(outcome, AgentRunOutcome::Completed { .. }));
}

// ── AgentRunProcessor action integration tests ───────────────────────

struct StaticAdapter {
    text: String,
}

#[async_trait]
impl ModelAdapter for StaticAdapter {
    async fn complete(&self, _request: ModelRequest) -> Result<ModelOutput, ModelAdapterError> {
        Ok(ModelOutput::Text {
            text: self.text.clone(),
            usage: None,
        })
    }
}

fn action_registry() -> action_core::ActionRegistry {
    let mut registry = action_core::ActionRegistry::new();
    for (kind, side_effect) in [
        ("knowledge.search", action_core::SideEffectKind::ReadOnly),
        (
            "knowledge.save_entry",
            action_core::SideEffectKind::RuntimeStateMutation,
        ),
        (
            "mail.send",
            action_core::SideEffectKind::ExternalSystemMutation,
        ),
    ] {
        registry
            .register(action_core::ActionSchema {
                kind: action_core::ActionKind::from(kind),
                display_name: kind.to_string(),
                description: kind.to_string(),
                side_effect,
                input_schema: None,
                output_schema: None,
            })
            .unwrap();
    }
    registry
}

async fn setup_run(kernel: &ConversationKernel) -> (ConversationId, MessageId, String) {
    let conv_id = kernel
        .create_conversation(conversation_kernel::CreateConversationCommand {
            kind: ConversationKind::AgentTask,
            title: Some("Action integration".to_string()),
            participants: vec![
                human("u1", "Test User"),
                agent_participant("a1", "Assistant"),
            ],
            actor_id: Some(ParticipantId::from("u1")),
        })
        .await
        .unwrap();
    let user_msg = kernel
        .append_message(conversation_kernel::AppendMessageCommand {
            conversation_id: conv_id.clone(),
            sender_id: ParticipantId::from("u1"),
            content: MessageContent::Text {
                text: "please act".to_string(),
            },
            reply_to: None,
            thread_id: None,
            visibility: Visibility::Conversation,
        })
        .await
        .unwrap();
    let run_id = kernel
        .request_agent_run(conversation_kernel::RequestAgentRunCommand {
            conversation_id: conv_id.clone(),
            trigger_message_id: user_msg.clone(),
            requested_by: ParticipantId::from("u1"),
        })
        .await
        .unwrap();
    (conv_id, user_msg, run_id)
}

#[derive(Debug)]
struct FailingActionExecutor;

#[async_trait]
impl action_core::ActionExecutor for FailingActionExecutor {
    async fn execute(
        &self,
        _request: &action_core::ActionRequest,
    ) -> Result<action_core::ActionResult, action_core::ActionExecutorError> {
        Err(action_core::ActionExecutorError::ExecutionFailed(
            "intentional failure".to_string(),
        ))
    }
}

async fn process_static_action_response_with_executor(
    response_text: &str,
    executor: &dyn action_core::ActionExecutor,
    detector: &dyn ActionProposalDetector,
) -> (
    AgentRunWithActionsOutcome,
    ConversationKernel,
    ConversationId,
    String,
    audit_log::MemoryAuditSink,
) {
    let kernel = test_kernel();
    let (conv_id, user_msg, run_id) = setup_run(&kernel).await;
    let adapter = StaticAdapter {
        text: response_text.to_string(),
    };
    let context_builder = AgentContextBuilder::new(50);
    let config = AgentRuntimeConfig::default();
    let registry = action_registry();
    let policy = capability_policy::CapabilityPolicy::default_safe();
    let audit = audit_log::MemoryAuditSink::new();
    let action_runtime = action_runtime::ActionRuntime {
        kernel: &kernel,
        registry: &registry,
        policy: &policy,
        executor,
        audit_log: &audit,
        artifact_resolver: None,
    };

    let outcome = AgentRunProcessor::process_with_actions(ProcessRunWithActionsRequest {
        kernel: &kernel,
        adapter: &adapter,
        context_builder: &context_builder,
        config: &config,
        conversation_id: &conv_id,
        run_id: &run_id,
        trigger_message_id: &user_msg,
        agent_participant_id: &ParticipantId::from("a1"),
        detector,
        action_runtime: &action_runtime,
    })
    .await
    .unwrap();

    (outcome, kernel, conv_id, run_id, audit)
}

async fn process_static_action_response(
    response_text: &str,
) -> (
    AgentRunWithActionsOutcome,
    ConversationKernel,
    ConversationId,
    String,
    audit_log::MemoryAuditSink,
) {
    let executor = action_core::StaticActionExecutor::new("from agent runtime");
    let detector = KeywordActionProposalDetector;
    process_static_action_response_with_executor(response_text, &executor, &detector).await
}

fn assert_agent_run_completed(
    state: &ConversationState,
    run_id: &str,
    output_message_id: &MessageId,
) {
    let run = state.agent_runs.get(run_id).unwrap();
    assert_eq!(run.status, AgentRunStatus::Completed);
    assert_eq!(run.output_message_id.as_ref(), Some(output_message_id));
}

fn knowledge_draft(title: &str, content_markdown: &str) -> knowledge_entity::KnowledgeEntryDraft {
    knowledge_entity::KnowledgeEntryDraft::new(title, content_markdown, chrono::Utc::now())
        .with_tags(vec!["agent-os".to_string()])
}

async fn process_static_knowledge_action_response(
    response_text: &str,
    repository: std::sync::Arc<knowledge_entity::MemoryKnowledgeRepository>,
) -> (
    AgentRunWithActionsOutcome,
    ConversationKernel,
    ConversationId,
    String,
    audit_log::MemoryAuditSink,
) {
    let kernel = test_kernel();
    let (conv_id, user_msg, run_id) = setup_run(&kernel).await;
    let adapter = StaticAdapter {
        text: response_text.to_string(),
    };
    let context_builder = AgentContextBuilder::new(50);
    let config = AgentRuntimeConfig::default();
    let mut registry = action_core::ActionRegistry::new();
    knowledge_entity::register_knowledge_action_schemas(&mut registry).unwrap();
    let policy = capability_policy::CapabilityPolicy::default_safe();
    let executor = knowledge_entity::KnowledgeActionExecutor::new(repository);
    let audit = audit_log::MemoryAuditSink::new();
    let action_runtime = action_runtime::ActionRuntime {
        kernel: &kernel,
        registry: &registry,
        policy: &policy,
        executor: &executor,
        audit_log: &audit,
        artifact_resolver: None,
    };
    let detector = KeywordActionProposalDetector;

    let outcome = AgentRunProcessor::process_with_actions(ProcessRunWithActionsRequest {
        kernel: &kernel,
        adapter: &adapter,
        context_builder: &context_builder,
        config: &config,
        conversation_id: &conv_id,
        run_id: &run_id,
        trigger_message_id: &user_msg,
        agent_participant_id: &ParticipantId::from("a1"),
        detector: &detector,
        action_runtime: &action_runtime,
    })
    .await
    .unwrap();

    (outcome, kernel, conv_id, run_id, audit)
}

#[tokio::test]
async fn agent_runtime_executes_read_only_action_from_static_proposal() {
    let (outcome, kernel, conv_id, run_id, audit) = process_static_action_response(
        "I will search. ACTION knowledge.search {\"query\":\"agent os\"}",
    )
    .await;

    let output_message_id = match outcome {
        AgentRunWithActionsOutcome::CompletedWithAction {
            action_outcome,
            response_text,
            output_message_id,
            ..
        } => {
            assert!(matches!(
                action_outcome,
                action_runtime::ActionRuntimeOutcome::Completed { .. }
            ));
            assert!(response_text.contains("[Action outcome]"));
            output_message_id
        }
        _ => panic!("expected completed with action"),
    };

    let state = kernel.load_state(&conv_id).await.unwrap();
    assert_agent_run_completed(&state, &run_id, &output_message_id);
    assert_eq!(state.actions.len(), 1);
    let action = state.actions.values().next().unwrap();
    assert_eq!(action.status, ConversationActionStatus::Completed);
    assert_eq!(
        action.action_kind,
        action_core::ActionKind::from("knowledge.search")
    );
    assert_eq!(audit.list().await.unwrap()[0].result_status, "completed");
}

#[tokio::test]
async fn agent_runtime_reports_approval_required_for_write_action() {
    let (outcome, kernel, conv_id, run_id, audit) = process_static_action_response(
        "I will save. ACTION knowledge.save_entry {\"title\":\"AgentOS\"}",
    )
    .await;

    let output_message_id = match outcome {
        AgentRunWithActionsOutcome::CompletedWithAction {
            action_outcome: action_runtime::ActionRuntimeOutcome::ApprovalRequired { .. },
            output_message_id,
            ..
        } => output_message_id,
        _ => panic!("expected approval required action outcome"),
    };

    let state = kernel.load_state(&conv_id).await.unwrap();
    assert_agent_run_completed(&state, &run_id, &output_message_id);
    let action = state.actions.values().next().unwrap();
    assert_eq!(action.status, ConversationActionStatus::ApprovalRequired);
    assert_eq!(
        audit.list().await.unwrap()[0].result_status,
        "approval_required"
    );
}

#[tokio::test]
async fn agent_runtime_reports_denied_action_without_execution() {
    let (outcome, kernel, conv_id, run_id, audit) =
        process_static_action_response("I will send. ACTION mail.send {\"to\":\"x@y.z\"}").await;

    let output_message_id = match outcome {
        AgentRunWithActionsOutcome::CompletedWithAction {
            action_outcome: action_runtime::ActionRuntimeOutcome::Denied { .. },
            output_message_id,
            ..
        } => output_message_id,
        _ => panic!("expected denied action outcome"),
    };

    let state = kernel.load_state(&conv_id).await.unwrap();
    assert_agent_run_completed(&state, &run_id, &output_message_id);
    let action = state.actions.values().next().unwrap();
    assert_eq!(action.status, ConversationActionStatus::Denied);
    assert_eq!(audit.list().await.unwrap()[0].result_status, "denied");
}

#[tokio::test]
async fn agent_runtime_executes_real_knowledge_search_proposal() {
    let repository = std::sync::Arc::new(knowledge_entity::MemoryKnowledgeRepository::new());
    knowledge_entity::KnowledgeRepository::save_draft(
        repository.as_ref(),
        knowledge_draft("AgentOS Notes", "foundation content"),
    )
    .await
    .unwrap();
    let (outcome, kernel, conv_id, run_id, audit) = process_static_knowledge_action_response(
            "I will search. ACTION knowledge.search {\"query\":{\"text\":\"agentos\",\"tags\":[],\"limit\":10}}",
            repository,
        )
        .await;

    let output_message_id = match outcome {
        AgentRunWithActionsOutcome::CompletedWithAction {
            action_outcome: action_runtime::ActionRuntimeOutcome::Completed { result, .. },
            response_text,
            output_message_id,
            ..
        } => {
            assert!(response_text.contains("[Action outcome]"));
            let action_core::ActionResultPayload::Json(value) = result.payload else {
                panic!("expected json payload");
            };
            let results: Vec<knowledge_entity::KnowledgeSearchResult> =
                serde_json::from_value(value).unwrap();
            assert_eq!(results.len(), 1);
            assert_eq!(results[0].entry.title, "AgentOS Notes");
            output_message_id
        }
        _ => panic!("expected completed knowledge action outcome"),
    };

    let state = kernel.load_state(&conv_id).await.unwrap();
    assert_agent_run_completed(&state, &run_id, &output_message_id);
    let action = state.actions.values().next().unwrap();
    assert_eq!(action.status, ConversationActionStatus::Completed);
    assert_eq!(
        action.action_kind,
        action_core::ActionKind::from("knowledge.search")
    );
    let events = audit.list().await.unwrap();
    assert_eq!(events[0].policy_decision, "allow");
    assert_eq!(events[0].result_status, "completed");
}

#[tokio::test]
async fn agent_runtime_reports_real_knowledge_create_draft_approval_required() {
    let repository = std::sync::Arc::new(knowledge_entity::MemoryKnowledgeRepository::new());
    let repository_for_assert = repository.clone();
    let (outcome, kernel, conv_id, run_id, audit) = process_static_knowledge_action_response(
            "I will draft. ACTION knowledge.create_draft {\"title\":\"AgentOS Notes\",\"content_markdown\":\"draft content\",\"source_uri\":null,\"source_artifact_id\":null,\"source_asset_id\":null,\"tags\":[\"agent-os\"],\"metadata\":{},\"created_at\":\"2026-05-24T12:00:00Z\"}",
            repository,
        )
        .await;

    let output_message_id = match outcome {
        AgentRunWithActionsOutcome::CompletedWithAction {
            action_outcome: action_runtime::ActionRuntimeOutcome::ApprovalRequired { .. },
            response_text,
            output_message_id,
            ..
        } => {
            assert!(response_text.contains("[Action outcome]"));
            output_message_id
        }
        _ => panic!("expected approval required knowledge action outcome"),
    };

    assert!(
        knowledge_entity::KnowledgeRepository::list_entries(repository_for_assert.as_ref())
            .await
            .unwrap()
            .is_empty()
    );
    let state = kernel.load_state(&conv_id).await.unwrap();
    assert_agent_run_completed(&state, &run_id, &output_message_id);
    let action = state.actions.values().next().unwrap();
    assert_eq!(action.status, ConversationActionStatus::ApprovalRequired);
    let events = audit.list().await.unwrap();
    assert_eq!(events[0].policy_decision, "ask");
    assert_eq!(events[0].result_status, "approval_required");
}

#[tokio::test]
async fn agent_runtime_denies_real_knowledge_save_entry_by_default_safe_policy() {
    let repository = std::sync::Arc::new(knowledge_entity::MemoryKnowledgeRepository::new());
    let repository_for_assert = repository.clone();
    let (outcome, kernel, conv_id, run_id, audit) = process_static_knowledge_action_response(
            "I will save. ACTION knowledge.save_entry {\"draft\":{\"title\":\"AgentOS Notes\",\"content_markdown\":\"saved content\",\"source_uri\":null,\"source_artifact_id\":null,\"source_asset_id\":null,\"tags\":[\"agent-os\"],\"metadata\":{},\"created_at\":\"2026-05-24T12:00:00Z\"}}",
            repository,
        )
        .await;

    let output_message_id = match outcome {
        AgentRunWithActionsOutcome::CompletedWithAction {
            action_outcome: action_runtime::ActionRuntimeOutcome::Denied { .. },
            response_text,
            output_message_id,
            ..
        } => {
            assert!(response_text.contains("[Action outcome]"));
            output_message_id
        }
        _ => panic!("expected denied knowledge action outcome"),
    };

    assert!(
        knowledge_entity::KnowledgeRepository::list_entries(repository_for_assert.as_ref())
            .await
            .unwrap()
            .is_empty()
    );
    let state = kernel.load_state(&conv_id).await.unwrap();
    assert_agent_run_completed(&state, &run_id, &output_message_id);
    let action = state.actions.values().next().unwrap();
    assert_eq!(action.status, ConversationActionStatus::Denied);
    let events = audit.list().await.unwrap();
    assert_eq!(events[0].policy_decision, "deny");
    assert_eq!(events[0].result_status, "denied");
}

#[tokio::test]
async fn agent_runtime_noop_detector_preserves_text_only_outcome() {
    let kernel = test_kernel();
    let (conv_id, user_msg, run_id) = setup_run(&kernel).await;
    let adapter = StaticAdapter {
        text: "Plain text response".to_string(),
    };
    let context_builder = AgentContextBuilder::new(50);
    let config = AgentRuntimeConfig::default();
    let registry = action_registry();
    let policy = capability_policy::CapabilityPolicy::default_safe();
    let executor = action_core::StaticActionExecutor::default();
    let audit = audit_log::MemoryAuditSink::new();
    let action_runtime = action_runtime::ActionRuntime {
        kernel: &kernel,
        registry: &registry,
        policy: &policy,
        executor: &executor,
        audit_log: &audit,
        artifact_resolver: None,
    };
    let detector = NoopActionProposalDetector;

    let outcome = AgentRunProcessor::process_with_actions(ProcessRunWithActionsRequest {
        kernel: &kernel,
        adapter: &adapter,
        context_builder: &context_builder,
        config: &config,
        conversation_id: &conv_id,
        run_id: &run_id,
        trigger_message_id: &user_msg,
        agent_participant_id: &ParticipantId::from("a1"),
        detector: &detector,
        action_runtime: &action_runtime,
    })
    .await
    .unwrap();

    let output_message_id = match outcome {
        AgentRunWithActionsOutcome::CompletedText {
            output_message_id, ..
        } => output_message_id,
        _ => panic!("expected text-only outcome"),
    };
    let state = kernel.load_state(&conv_id).await.unwrap();
    assert_agent_run_completed(&state, &run_id, &output_message_id);
    assert!(state.actions.is_empty());
    assert!(audit.list().await.unwrap().is_empty());
}

fn detector_context(run_id: &str) -> AgentContext {
    let trigger_message = Message {
        id: MessageId::from("msg-detector"),
        conversation_id: ConversationId::from("conv-detector"),
        sender_id: ParticipantId::from("u1"),
        content: MessageContent::Text {
            text: "detect".to_string(),
        },
        reply_to: None,
        thread_id: None,
        visibility: Visibility::Conversation,
        created_at: chrono::Utc::now(),
        edited_at: None,
    };
    AgentContext {
        conversation_id: ConversationId::from("conv-detector"),
        run_id: run_id.to_string(),
        trigger_message: trigger_message.clone(),
        messages: vec![trigger_message],
        participants: HashMap::from([(ParticipantId::from("u1"), human("u1", "Test User"))]),
        linked_entities: Vec::new(),
        model_id: ModelId::from("test-model"),
        system_prompt: None,
    }
}

fn detector_response(text: &str) -> ModelOutput {
    ModelOutput::Text {
        text: text.to_string(),
        usage: None,
    }
}

#[test]
fn registry_detector_accepts_registered_action_with_json_input() {
    let registry = action_registry();
    let detector = RegistryActionProposalDetector::new(&registry);
    let proposal = detector
        .detect(
            &detector_context("run-typed"),
            &detector_response("Ready. ACTION knowledge.search {\"query\":\"agent os\"}"),
        )
        .unwrap();

    assert_eq!(
        proposal.action_id,
        action_core::ActionId::from("action-run-typed-knowledge-search")
    );
    assert_eq!(
        proposal.action_kind,
        action_core::ActionKind::from("knowledge.search")
    );
    assert_eq!(proposal.input, serde_json::json!({"query":"agent os"}));
}

#[test]
fn registry_detector_rejects_unknown_action_kind() {
    let registry = action_registry();
    let detector = RegistryActionProposalDetector::new(&registry);
    assert!(
        detector
            .detect(
                &detector_context("run-typed"),
                &detector_response("ACTION unknown.action {\"x\":1}"),
            )
            .is_none()
    );
}

#[test]
fn registry_detector_rejects_malformed_json() {
    let registry = action_registry();
    let detector = RegistryActionProposalDetector::new(&registry);
    assert!(
        detector
            .detect(
                &detector_context("run-typed"),
                &detector_response("ACTION knowledge.search not-json"),
            )
            .is_none()
    );
}

#[test]
fn registry_detector_defaults_empty_input_to_empty_object() {
    let registry = action_registry();
    let detector = RegistryActionProposalDetector::new(&registry);
    let proposal = detector
        .detect(
            &detector_context("run-typed"),
            &detector_response("ACTION knowledge.search"),
        )
        .unwrap();
    assert_eq!(proposal.input, serde_json::json!({}));
}

#[tokio::test]
async fn registry_detector_preserves_text_only_outcome_for_unknown_action() {
    let kernel = test_kernel();
    let (conv_id, user_msg, run_id) = setup_run(&kernel).await;
    let adapter = StaticAdapter {
        text: "I will act. ACTION unknown.action {\"x\":1}".to_string(),
    };
    let context_builder = AgentContextBuilder::new(50);
    let config = AgentRuntimeConfig::default();
    let registry = action_registry();
    let policy = capability_policy::CapabilityPolicy::default_safe();
    let executor = action_core::StaticActionExecutor::default();
    let audit = audit_log::MemoryAuditSink::new();
    let action_runtime = action_runtime::ActionRuntime {
        kernel: &kernel,
        registry: &registry,
        policy: &policy,
        executor: &executor,
        audit_log: &audit,
        artifact_resolver: None,
    };
    let detector = RegistryActionProposalDetector::new(&registry);

    let outcome = AgentRunProcessor::process_with_actions(ProcessRunWithActionsRequest {
        kernel: &kernel,
        adapter: &adapter,
        context_builder: &context_builder,
        config: &config,
        conversation_id: &conv_id,
        run_id: &run_id,
        trigger_message_id: &user_msg,
        agent_participant_id: &ParticipantId::from("a1"),
        detector: &detector,
        action_runtime: &action_runtime,
    })
    .await
    .unwrap();

    let output_message_id = match outcome {
        AgentRunWithActionsOutcome::CompletedText {
            output_message_id, ..
        } => output_message_id,
        _ => panic!("expected text-only outcome"),
    };
    let state = kernel.load_state(&conv_id).await.unwrap();
    assert_agent_run_completed(&state, &run_id, &output_message_id);
    assert!(state.actions.is_empty());
    assert!(audit.list().await.unwrap().is_empty());
}

#[test]
fn keyword_detector_keeps_raw_fallback_for_malformed_json() {
    let detector = KeywordActionProposalDetector;
    let proposal = detector
        .detect(
            &detector_context("run-keyword"),
            &detector_response("ACTION unknown.action not-json"),
        )
        .unwrap();

    assert_eq!(
        proposal.action_kind,
        action_core::ActionKind::from("unknown.action")
    );
    assert_eq!(proposal.input, serde_json::json!({"raw":"not-json"}));
}

#[tokio::test]
async fn agent_runtime_reports_failed_action_and_completes_run() {
    let executor = FailingActionExecutor;
    let detector = KeywordActionProposalDetector;
    let (outcome, kernel, conv_id, run_id, audit) = process_static_action_response_with_executor(
        "I will search. ACTION knowledge.search {\"query\":\"agent os\"}",
        &executor,
        &detector,
    )
    .await;

    let output_message_id = match outcome {
        AgentRunWithActionsOutcome::CompletedWithAction {
            action_outcome: action_runtime::ActionRuntimeOutcome::Failed { .. },
            output_message_id,
            ..
        } => output_message_id,
        _ => panic!("expected failed action outcome"),
    };

    let state = kernel.load_state(&conv_id).await.unwrap();
    assert_agent_run_completed(&state, &run_id, &output_message_id);
    let action = state.actions.values().next().unwrap();
    assert_eq!(action.status, ConversationActionStatus::Failed);
    assert_eq!(audit.list().await.unwrap()[0].result_status, "failed");
}

// ---- PR 37: Browser AgentRuntime proposal integration ----

async fn process_static_browser_action_response(
    response_text: &str,
) -> (
    AgentRunWithActionsOutcome,
    ConversationKernel,
    ConversationId,
    String,
    audit_log::MemoryAuditSink,
) {
    let kernel = test_kernel();
    let (conv_id, user_msg, run_id) = setup_run(&kernel).await;
    let adapter = StaticAdapter {
        text: response_text.to_string(),
    };
    let context_builder = AgentContextBuilder::new(50);
    let config = AgentRuntimeConfig::default();
    let mut registry = action_core::ActionRegistry::new();
    browser_entity::register_browser_action_schemas(&mut registry).unwrap();
    let policy = capability_policy::CapabilityPolicy::default_safe();
    let executor = browser_entity::StaticBrowserExecutor::new(chrono::Utc::now());
    let audit = audit_log::MemoryAuditSink::new();
    let action_runtime = action_runtime::ActionRuntime {
        kernel: &kernel,
        registry: &registry,
        policy: &policy,
        executor: &executor,
        audit_log: &audit,
        artifact_resolver: None,
    };
    let detector = KeywordActionProposalDetector;

    let outcome = AgentRunProcessor::process_with_actions(ProcessRunWithActionsRequest {
        kernel: &kernel,
        adapter: &adapter,
        context_builder: &context_builder,
        config: &config,
        conversation_id: &conv_id,
        run_id: &run_id,
        trigger_message_id: &user_msg,
        agent_participant_id: &ParticipantId::from("a1"),
        detector: &detector,
        action_runtime: &action_runtime,
    })
    .await
    .unwrap();

    (outcome, kernel, conv_id, run_id, audit)
}

#[tokio::test]
async fn agent_runtime_executes_browser_extract_content_proposal() {
    let (outcome, kernel, conv_id, run_id, audit) = process_static_browser_action_response(
        "I will extract. ACTION browser.extract_content {\"url\":\"https://example.com\"}",
    )
    .await;

    let output_message_id = match outcome {
        AgentRunWithActionsOutcome::CompletedWithAction {
            action_outcome,
            output_message_id,
            ..
        } => {
            assert!(matches!(
                action_outcome,
                action_runtime::ActionRuntimeOutcome::Completed { .. }
            ));
            output_message_id
        }
        _ => panic!("expected completed with action"),
    };

    let state = kernel.load_state(&conv_id).await.unwrap();
    assert_agent_run_completed(&state, &run_id, &output_message_id);
    let action = state.actions.values().next().unwrap();
    assert_eq!(action.status, ConversationActionStatus::Completed);
    assert_eq!(audit.list().await.unwrap()[0].policy_decision, "allow");
    assert_eq!(audit.list().await.unwrap()[0].result_status, "completed");
}

#[tokio::test]
async fn agent_runtime_reports_browser_open_url_approval_required() {
    let (outcome, kernel, conv_id, run_id, audit) =
            process_static_browser_action_response(
                "I will open. ACTION browser.open_url {\"url\":\"https://example.com\",\"take_snapshot\":true}",
            )
            .await;

    let output_message_id = match outcome {
        AgentRunWithActionsOutcome::CompletedWithAction {
            action_outcome,
            output_message_id,
            ..
        } => {
            assert!(matches!(
                action_outcome,
                action_runtime::ActionRuntimeOutcome::ApprovalRequired { .. }
            ));
            output_message_id
        }
        _ => panic!("expected completed with action"),
    };

    let state = kernel.load_state(&conv_id).await.unwrap();
    assert_agent_run_completed(&state, &run_id, &output_message_id);
    let action = state.actions.values().next().unwrap();
    assert_eq!(action.status, ConversationActionStatus::ApprovalRequired);
    assert_eq!(audit.list().await.unwrap()[0].policy_decision, "ask");
    assert_eq!(
        audit.list().await.unwrap()[0].result_status,
        "approval_required"
    );
}

#[tokio::test]
async fn agent_runtime_reports_browser_capture_snapshot_approval_required() {
    let (outcome, kernel, conv_id, run_id, audit) =
            process_static_browser_action_response(
                "I will capture. ACTION browser.capture_snapshot {\"url\":\"https://example.com\",\"include_html\":false}",
            )
            .await;

    let output_message_id = match outcome {
        AgentRunWithActionsOutcome::CompletedWithAction {
            action_outcome,
            output_message_id,
            ..
        } => {
            assert!(matches!(
                action_outcome,
                action_runtime::ActionRuntimeOutcome::ApprovalRequired { .. }
            ));
            output_message_id
        }
        _ => panic!("expected completed with action"),
    };

    let state = kernel.load_state(&conv_id).await.unwrap();
    assert_agent_run_completed(&state, &run_id, &output_message_id);
    let action = state.actions.values().next().unwrap();
    assert_eq!(action.status, ConversationActionStatus::ApprovalRequired);
    assert_eq!(audit.list().await.unwrap()[0].policy_decision, "ask");
    assert_eq!(
        audit.list().await.unwrap()[0].result_status,
        "approval_required"
    );
}

// ─────────────────────────────────────────────────────────────────────
// PR 59: AgentToolLoop tests
// ─────────────────────────────────────────────────────────────────────

use std::sync::atomic::{AtomicU32, Ordering};

struct ScriptedToolAdapter {
    responses: tokio::sync::Mutex<Vec<ModelOutput>>,
    call_count: AtomicU32,
}

impl ScriptedToolAdapter {
    fn new(responses: Vec<ModelOutput>) -> Self {
        Self {
            responses: tokio::sync::Mutex::new(responses),
            call_count: AtomicU32::new(0),
        }
    }
}

#[async_trait]
impl ModelAdapter for ScriptedToolAdapter {
    async fn complete(
        &self,
        _request: ModelRequest,
    ) -> std::result::Result<ModelOutput, ModelAdapterError> {
        self.call_count.fetch_add(1, Ordering::SeqCst);
        let mut r = self.responses.lock().await;
        if r.is_empty() {
            return Err(ModelAdapterError::ExecutorFailed("empty".into()));
        }
        Ok(r.remove(0))
    }
}

#[async_trait]
impl ToolCallingModelAdapter for ScriptedToolAdapter {
    async fn complete_with_tools(
        &self,
        _request: ModelRequest,
        _tools: Vec<ToolDefinition>,
        _choice: ToolChoice,
    ) -> std::result::Result<ModelOutput, ModelAdapterError> {
        self.call_count.fetch_add(1, Ordering::SeqCst);
        let mut r = self.responses.lock().await;
        if r.is_empty() {
            return Err(ModelAdapterError::ExecutorFailed("empty".into()));
        }
        Ok(r.remove(0))
    }
}

struct TestMapper;
impl ToolCallMapper for TestMapper {
    fn map_to_action_request(
        &self,
        tc: &ToolCall,
        run_id: &str,
        conv_id: &str,
    ) -> Result<ActionRequest> {
        Ok(ActionRequest {
            action_id: ActionId(format!("{run_id}-{}", tc.id)),
            action_kind: ActionKind(tc.name.clone()),
            input: tc.arguments.clone(),
            requested_by: "agent".to_string(),
            conversation_id: Some(conv_id.to_string()),
            message_id: None,
            requested_at: chrono::Utc::now(),
        })
    }
}

macro_rules! with_tool_runtime {
    ($k:ident, $rt:ident, $body:block) => {{
        let $k = test_kernel();
        let registry = action_registry();
        let executor = action_core::StaticActionExecutor::new("ok");
        let audit = audit_log::MemoryAuditSink::new();
        let policy = capability_policy::CapabilityPolicy::default_safe();
        let $rt = ActionRuntime {
            kernel: &$k,
            registry: &registry,
            policy: &policy,
            executor: &executor,
            audit_log: &audit,
            artifact_resolver: None,
        };
        $body
    }};
}

#[tokio::test]
async fn tool_loop_text_only_completes() {
    let adapter = ScriptedToolAdapter::new(vec![ModelOutput::Text {
        text: "Hello!".to_string(),
        usage: Some(ModelUsage {
            input_tokens: 10,
            output_tokens: 5,
        }),
    }]);
    with_tool_runtime!(_k, rt, {
        let outcome = AgentToolLoop::run(ToolLoopRequest {
            adapter: &adapter,
            action_runtime: &rt,
            mapper: &TestMapper,
            config: &ToolLoopConfig::default(),
            initial_request: ModelRequest::new("test", vec![ModelMessage::user("hi")]),
            tools: vec![],
            tool_choice: ToolChoice::Auto,
            run_id: "run-1",
            conversation_id: "conv-1",
            observability: None,
        })
        .await;
        assert!(matches!(outcome, ToolLoopOutcome::Completed {
                response_text, turns_used: 0, tool_calls_made: 0, usage: Some(usage)
            } if response_text == "Hello!" && usage.total_tokens() == 15));
    });
}

#[tokio::test]
async fn tool_loop_executes_and_returns_text() {
    let adapter = ScriptedToolAdapter::new(vec![
        ModelOutput::ToolCalls {
            content: None,
            tool_calls: vec![ToolCall {
                id: "call_001".into(),
                name: "knowledge.search".into(),
                arguments: serde_json::json!({"q": "test"}),
                raw_arguments: r#"{"q":"test"}"#.into(),
            }],
            usage: Some(ModelUsage {
                input_tokens: 20,
                output_tokens: 10,
            }),
        },
        ModelOutput::Text {
            text: "Found it!".to_string(),
            usage: Some(ModelUsage {
                input_tokens: 30,
                output_tokens: 15,
            }),
        },
    ]);
    with_tool_runtime!(_k, rt, {
        let outcome = AgentToolLoop::run(ToolLoopRequest {
            adapter: &adapter,
            action_runtime: &rt,
            mapper: &TestMapper,
            config: &ToolLoopConfig::default(),
            initial_request: ModelRequest::new("test", vec![ModelMessage::user("search")]),
            tools: vec![ToolDefinition {
                name: "knowledge.search".into(),
                description: "Search".into(),
                input_schema: serde_json::json!({}),
            }],
            tool_choice: ToolChoice::Auto,
            run_id: "run-2",
            conversation_id: "conv-2",
            observability: None,
        })
        .await;
        assert!(matches!(outcome, ToolLoopOutcome::Completed {
                response_text, turns_used: 1, tool_calls_made: 1, usage: Some(usage)
            } if response_text == "Found it!" && usage.input_tokens == 50 && usage.output_tokens == 25));
    });
}

#[tokio::test]
async fn tool_loop_records_observability_for_model_and_action_path() {
    let adapter = ScriptedToolAdapter::new(vec![
        ModelOutput::ToolCalls {
            content: None,
            tool_calls: vec![ToolCall {
                id: "call_obs".into(),
                name: "knowledge.search".into(),
                arguments: serde_json::json!({"q": "test"}),
                raw_arguments: r#"{"q":"test"}"#.into(),
            }],
            usage: Some(ModelUsage {
                input_tokens: 20,
                output_tokens: 10,
            }),
        },
        ModelOutput::Text {
            text: "Observed".to_string(),
            usage: Some(ModelUsage {
                input_tokens: 30,
                output_tokens: 15,
            }),
        },
    ]);
    let mut sink = InMemoryObservabilitySink::default();
    with_tool_runtime!(_k, rt, {
        let outcome = AgentToolLoop::run(ToolLoopRequest {
            adapter: &adapter,
            action_runtime: &rt,
            mapper: &TestMapper,
            config: &ToolLoopConfig::default(),
            initial_request: ModelRequest::new("test", vec![ModelMessage::user("search")]),
            tools: vec![],
            tool_choice: ToolChoice::Auto,
            run_id: "run-observed",
            conversation_id: "conv-observed",
            observability: Some(ToolLoopObservability::new(&mut sink)),
        })
        .await;
        assert!(
            matches!(outcome, ToolLoopOutcome::Completed { response_text, .. } if response_text == "Observed")
        );
    });

    let operations = sink
        .traces()
        .iter()
        .filter_map(|event| event.operation.as_deref())
        .collect::<Vec<_>>();
    assert!(operations.contains(&"tool_loop.turn_started"));
    assert!(operations.contains(&"tool_loop.model_call_completed"));
    assert!(operations.contains(&"tool_loop.action_started"));
    assert!(operations.contains(&"tool_loop.action_failed"));
    assert!(operations.contains(&"tool_loop.completed"));
    assert!(sink.traces().iter().all(|event| {
        event.kind == ObservabilityEventKind::ToolLoop
            && event.scope == "agent-runtime.tool-loop"
            && event.correlation_id == "run-observed"
    }));
    assert!(
        sink.metrics().iter().any(|metric| {
            metric.name == "tool_loop.action.failed" && metric.values == vec![1.0]
        })
    );
    assert!(sink.metrics().iter().any(|metric| {
        metric.name == "tool_loop.tool_calls_made" && metric.values == vec![1.0]
    }));
}

#[tokio::test]
async fn tool_loop_checkpoint_resume_skips_completed_read_only_tool_result() {
    let checkpoint_store = MemoryToolLoopCheckpointStore::new();
    checkpoint_store
        .append(ToolLoopCheckpoint::tool_result(
            "run-resume",
            1,
            ToolResultCheckpoint {
                tool_call_id: "call_001".to_string(),
                action_id: "run-resume-call_001".to_string(),
                result_text: "cached result".to_string(),
                read_only: true,
            },
        ))
        .await
        .unwrap();
    let adapter = ScriptedToolAdapter::new(vec![
        ModelOutput::ToolCalls {
            content: None,
            tool_calls: vec![ToolCall {
                id: "call_001".into(),
                name: "knowledge.search".into(),
                arguments: serde_json::json!({"q": "test"}),
                raw_arguments: r#"{"q":"test"}"#.into(),
            }],
            usage: None,
        },
        ModelOutput::Text {
            text: "Used cached result".to_string(),
            usage: None,
        },
    ]);
    with_tool_runtime!(_k, rt, {
        let outcome = AgentToolLoop::run_with_checkpoints(
            ToolLoopRequest {
                adapter: &adapter,
                action_runtime: &rt,
                mapper: &TestMapper,
                config: &ToolLoopConfig::default(),
                initial_request: ModelRequest::new("test", vec![ModelMessage::user("search")]),
                tools: vec![],
                tool_choice: ToolChoice::Auto,
                run_id: "run-resume",
                conversation_id: "conv-resume",
                observability: None,
            },
            &checkpoint_store,
        )
        .await;
        assert!(matches!(outcome, ToolLoopOutcome::Completed {
                response_text, turns_used: 1, tool_calls_made: 0, usage: Some(_)
            } if response_text == "Used cached result"));
    });
}

#[tokio::test]
async fn tool_loop_cancelled_run_does_not_execute_followup_tool() {
    let controls =
        ToolLoopExecutionControls::new().with_cancellation_token(RunCancellationToken::new());
    controls.cancel();
    let adapter = ScriptedToolAdapter::new(vec![ModelOutput::ToolCalls {
        content: None,
        tool_calls: vec![ToolCall {
            id: "call_cancelled".into(),
            name: "knowledge.search".into(),
            arguments: serde_json::json!({"q": "skip"}),
            raw_arguments: r#"{"q":"skip"}"#.into(),
        }],
        usage: None,
    }]);
    with_tool_runtime!(_k, rt, {
        let outcome = AgentToolLoop::run_with_controls(
            ToolLoopRequest {
                adapter: &adapter,
                action_runtime: &rt,
                mapper: &TestMapper,
                config: &ToolLoopConfig::default(),
                initial_request: ModelRequest::new("test", vec![ModelMessage::user("search")]),
                tools: vec![],
                tool_choice: ToolChoice::Auto,
                run_id: "run-cancelled",
                conversation_id: "conv-cancelled",
                observability: None,
            },
            controls,
        )
        .await;
        assert!(matches!(
            outcome,
            ToolLoopOutcome::Cancelled { turns_used: 0, .. }
        ));
    });
}

#[tokio::test]
async fn tool_loop_model_timeout_returns_typed_outcome() {
    struct SlowToolAdapter;
    #[async_trait]
    impl ModelAdapter for SlowToolAdapter {
        async fn complete(
            &self,
            _request: ModelRequest,
        ) -> std::result::Result<ModelOutput, ModelAdapterError> {
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            Ok(ModelOutput::Text {
                text: "late".into(),
                usage: None,
            })
        }
    }
    #[async_trait]
    impl ToolCallingModelAdapter for SlowToolAdapter {
        async fn complete_with_tools(
            &self,
            _request: ModelRequest,
            _tools: Vec<ToolDefinition>,
            _choice: ToolChoice,
        ) -> std::result::Result<ModelOutput, ModelAdapterError> {
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            Ok(ModelOutput::Text {
                text: "late".into(),
                usage: None,
            })
        }
    }
    let adapter = SlowToolAdapter;
    let controls = ToolLoopExecutionControls::new()
        .with_model_call_timeout(std::time::Duration::from_millis(1));
    with_tool_runtime!(_k, rt, {
        let outcome = AgentToolLoop::run_with_controls(
            ToolLoopRequest {
                adapter: &adapter,
                action_runtime: &rt,
                mapper: &TestMapper,
                config: &ToolLoopConfig::default(),
                initial_request: ModelRequest::new("test", vec![ModelMessage::user("timeout")]),
                tools: vec![],
                tool_choice: ToolChoice::Auto,
                run_id: "run-timeout",
                conversation_id: "conv-timeout",
                observability: None,
            },
            controls,
        )
        .await;
        assert!(
            matches!(outcome, ToolLoopOutcome::TimedOut { operation, turns_used: 0, .. } if operation == TimeoutOperation::ModelCall)
        );
    });
}

#[tokio::test]
async fn tool_loop_respects_max_turns() {
    let adapter = ScriptedToolAdapter::new(vec![
        ModelOutput::ToolCalls {
            content: None,
            tool_calls: vec![ToolCall {
                id: "c1".into(),
                name: "knowledge.search".into(),
                arguments: serde_json::json!({}),
                raw_arguments: "{}".into(),
            }],
            usage: None,
        },
        ModelOutput::ToolCalls {
            content: None,
            tool_calls: vec![ToolCall {
                id: "c2".into(),
                name: "knowledge.search".into(),
                arguments: serde_json::json!({}),
                raw_arguments: "{}".into(),
            }],
            usage: None,
        },
    ]);
    with_tool_runtime!(_k, rt, {
        let outcome = AgentToolLoop::run(ToolLoopRequest {
            adapter: &adapter,
            action_runtime: &rt,
            mapper: &TestMapper,
            config: &ToolLoopConfig { max_turns: 2 },
            initial_request: ModelRequest::new("test", vec![ModelMessage::user("loop")]),
            tools: vec![],
            tool_choice: ToolChoice::Auto,
            run_id: "run-3",
            conversation_id: "conv-3",
            observability: None,
        })
        .await;
        assert!(matches!(
            outcome,
            ToolLoopOutcome::MaxTurnsReached { turns_used: 2, .. }
        ));
    });
}

#[tokio::test]
async fn tool_loop_handles_model_error() {
    let adapter = ScriptedToolAdapter::new(vec![]);
    with_tool_runtime!(_k, rt, {
        let outcome = AgentToolLoop::run(ToolLoopRequest {
            adapter: &adapter,
            action_runtime: &rt,
            mapper: &TestMapper,
            config: &ToolLoopConfig::default(),
            initial_request: ModelRequest::new("test", vec![ModelMessage::user("fail")]),
            tools: vec![],
            tool_choice: ToolChoice::Auto,
            run_id: "run-4",
            conversation_id: "conv-4",
            observability: None,
        })
        .await;
        assert!(
            matches!(outcome, ToolLoopOutcome::Failed { error, turns_used: 0 }
                if error.contains("empty"))
        );
    });
}
