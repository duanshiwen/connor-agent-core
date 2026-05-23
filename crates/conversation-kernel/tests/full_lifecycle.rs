//! Full lifecycle integration test.
//!
//! Demonstrates the complete flow:
//! 1. Create conversation
//! 2. Participants exchange messages
//! 3. Policy evaluates messages for agent runs
//! 4. Slice builder constructs context windows
//! 5. Kernel produces boundary events for agent runs
//! 6. Projector replays everything deterministically

use chrono::{DateTime, Utc};
use conversation_core::*;
use conversation_journal::MemoryConversationJournal;
use conversation_kernel::*;
use std::sync::Arc;

/// A deterministic ID generator for testing.
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

/// A fixed clock for testing.
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
async fn full_agent_task_lifecycle() {
    let kernel = setup();
    let policy = RuleBasedPolicy;
    let slice_builder = ConversationSliceBuilder::new(10);

    // ── Step 1: Create an AgentTask conversation ─────────────────────────
    let conv_id = kernel
        .create_conversation(CreateConversationCommand {
            kind: ConversationKind::AgentTask,
            title: Some("Design Conversation Kernel".into()),
            participants: vec![human("u1", "诗闻"), agent("a1", "小助理")],
            actor_id: Some(ParticipantId::from("u1")),
        })
        .await
        .unwrap();

    // ── Step 2: User sends a message ─────────────────────────────────────
    let user_msg_1 = kernel
        .append_message(AppendMessageCommand {
            conversation_id: conv_id.clone(),
            sender_id: ParticipantId::from("u1"),
            content: MessageContent::Text {
                text: "帮我设计 Conversation Kernel 的事件模型".into(),
            },
            reply_to: None,
            thread_id: None,
            visibility: Visibility::Conversation,
        })
        .await
        .unwrap();

    // ── Step 3: Policy evaluates the message ─────────────────────────────
    let state_after_msg1 = kernel.load_state(&conv_id).await.unwrap();
    let msg1_ref = state_after_msg1.messages_by_id.get(&user_msg_1).unwrap();
    let policy_result = policy.should_request_agent_run(&state_after_msg1, msg1_ref);
    assert!(policy_result.is_some(), "\"帮我\" should trigger agent run");
    assert_eq!(policy_result.unwrap(), AgentRunReason::HelpRequest);

    // ── Step 4: Request agent run ─────────────────────────────────────────
    let run_id = kernel
        .request_agent_run(RequestAgentRunCommand {
            conversation_id: conv_id.clone(),
            trigger_message_id: user_msg_1.clone(),
            requested_by: ParticipantId::from("u1"),
        })
        .await
        .unwrap();
    assert!(!run_id.is_empty());

    // ── Step 5: Agent replies ─────────────────────────────────────────────
    let agent_msg = kernel
        .append_message(AppendMessageCommand {
            conversation_id: conv_id.clone(),
            sender_id: ParticipantId::from("a1"),
            content: MessageContent::Text {
                text: "好的，我来帮你设计事件模型。首先我们需要定义核心事件类型。".into(),
            },
            reply_to: Some(user_msg_1.clone()),
            thread_id: None,
            visibility: Visibility::Conversation,
        })
        .await
        .unwrap();

    // ── Step 6: User sends another message (normal, no trigger) ──────────
    let user_msg_2 = kernel
        .append_message(AppendMessageCommand {
            conversation_id: conv_id.clone(),
            sender_id: ParticipantId::from("u1"),
            content: MessageContent::Text {
                text: "好的，继续".into(),
            },
            reply_to: None,
            thread_id: None,
            visibility: Visibility::Conversation,
        })
        .await
        .unwrap();

    // Policy should NOT trigger for "好的，继续"
    let state_after_msg2 = kernel.load_state(&conv_id).await.unwrap();
    let msg2_ref = state_after_msg2.messages_by_id.get(&user_msg_2).unwrap();
    assert!(
        policy
            .should_request_agent_run(&state_after_msg2, msg2_ref)
            .is_none(),
        "\"好的，继续\" should NOT trigger agent run"
    );

    // ── Step 7: Build conversation slice ──────────────────────────────────
    let slice = slice_builder
        .build_recent_window(&state_after_msg2, &user_msg_2)
        .unwrap();

    // Should contain all 3 messages (under limit of 10)
    assert_eq!(slice.messages.len(), 3);
    assert_eq!(slice.messages[0].id, user_msg_1);
    assert_eq!(slice.messages[1].id, agent_msg);
    assert_eq!(slice.messages[2].id, user_msg_2);
    assert_eq!(slice.reason, SliceBuildReason::RecentWindow);
    assert_eq!(slice.conversation_id, conv_id);

    // ── Step 8: Verify all events are correct ────────────────────────────
    let events = kernel.load_events(&conv_id).await.unwrap();
    // 1 Created + 2 ParticipantAdded + 3 MessageAppended + 1 SliceBuilt + 1 RunRequested = 8
    assert_eq!(events.len(), 8);

    // Verify event sequence (order-dependent)
    assert!(matches!(
        &events[0].event,
        ConversationEvent::ConversationCreated { .. }
    ));
    assert!(matches!(
        &events[1].event,
        ConversationEvent::ParticipantAdded { .. }
    ));
    assert!(matches!(
        &events[2].event,
        ConversationEvent::ParticipantAdded { .. }
    ));
    assert!(matches!(
        &events[3].event,
        ConversationEvent::MessageAppended { .. }
    ));
    // Agent run request produces: ContextSliceBuilt + AgentRunRequested (positions 4-5)
    assert!(matches!(
        &events[4].event,
        ConversationEvent::ContextSliceBuilt { .. }
    ));
    assert!(matches!(
        &events[5].event,
        ConversationEvent::AgentRunRequested { .. }
    ));
    assert!(matches!(
        &events[6].event,
        ConversationEvent::MessageAppended { .. }
    ));
    assert!(matches!(
        &events[7].event,
        ConversationEvent::MessageAppended { .. }
    ));

    // ── Step 9: Projector replay is deterministic ─────────────────────────
    let state_replayed = ConversationProjector::project(&events).unwrap();
    assert_eq!(state_replayed.messages.len(), 3);
    assert_eq!(state_replayed.participants.len(), 2);
    assert!(state_replayed.session.is_some());
    assert_eq!(
        state_replayed.session.as_ref().unwrap().title,
        Some("Design Conversation Kernel".into())
    );
}

#[tokio::test]
async fn group_chat_with_private_suggestion() {
    let kernel = setup();
    let slice_builder = ConversationSliceBuilder::new(10);

    // Define participant IDs explicitly so we can reference them later.
    let u1_id = ParticipantId::from("u1");
    let u2_id = ParticipantId::from("u2");
    let a1_id = ParticipantId::from("a1");

    // Create a group chat
    let conv_id = kernel
        .create_conversation(CreateConversationCommand {
            kind: ConversationKind::Group,
            title: Some("Team Chat".into()),
            participants: vec![
                human("u1", "诗闻"),
                human("u2", "Alice"),
                agent("a1", "小助理"),
            ],
            actor_id: Some(u1_id.clone()),
        })
        .await
        .unwrap();

    // u1 sends a public message
    let u1_msg = kernel
        .append_message(AppendMessageCommand {
            conversation_id: conv_id.clone(),
            sender_id: u1_id.clone(),
            content: MessageContent::Text {
                text: "I think we should reconsider the architecture".into(),
            },
            reply_to: None,
            thread_id: None,
            visibility: Visibility::Conversation,
        })
        .await
        .unwrap();

    // u2 sends a message with sarcasm
    let _u2_msg = kernel
        .append_message(AppendMessageCommand {
            conversation_id: conv_id.clone(),
            sender_id: u2_id.clone(),
            content: MessageContent::Text {
                text: "Oh sure, let's just redo everything from scratch".into(),
            },
            reply_to: None,
            thread_id: None,
            visibility: Visibility::Conversation,
        })
        .await
        .unwrap();

    // Assistant creates a private suggestion for u1 only
    let _suggestion = kernel
        .create_assistant_suggestion(CreateAssistantSuggestionCommand {
            conversation_id: conv_id.clone(),
            target_user_id: u1_id.clone(),
            text: "Alice 的回复可能带有讽刺语气。建议耐心沟通，了解她的真实顾虑。".into(),
            actions: vec![SuggestedAction {
                id: "ack".into(),
                label: "知道了".into(),
                action_type: "dismiss".into(),
            }],
            trigger: SuggestionTrigger::ComplexConcept,
        })
        .await
        .unwrap();

    // Reload state AFTER suggestion is created
    let state = kernel.load_state(&conv_id).await.unwrap();
    assert_eq!(state.messages.len(), 3, "state should have 3 messages");

    // Verify the suggestion has the right visibility
    let suggestion_msg = state.messages.last().unwrap();
    assert!(
        matches!(&suggestion_msg.visibility, Visibility::PrivateToUser { user_id } if *user_id == u1_id),
        "suggestion should be PrivateToUser for u1"
    );

    // Slice for u1 should include ALL 3 messages (including private suggestion)
    let slice_u1 = slice_builder
        .build_for_user(&state, &u1_msg, &u1_id)
        .unwrap();
    assert_eq!(
        slice_u1.messages.len(),
        3,
        "u1 should see all 3 messages including private suggestion"
    );

    // Slice for u2 should NOT include the private suggestion
    let slice_u2 = slice_builder
        .build_for_user(&state, &u1_msg, &u2_id)
        .unwrap();
    assert_eq!(
        slice_u2.messages.len(),
        2,
        "u2 should only see 2 public messages"
    );

    // Verify the third message in u1's slice is the suggestion
    let last_msg = slice_u1.messages.last().unwrap();
    assert!(
        matches!(&last_msg.content, MessageContent::AgentSuggestion { .. }),
        "last message in u1's slice should be the agent suggestion"
    );
}

#[tokio::test]
async fn jsonl_journal_persistence_roundtrip() {
    use conversation_journal::JsonlConversationJournal;

    let dir = tempfile::tempdir().unwrap();
    let dir_path = dir.path().to_path_buf();

    // Phase A: Create conversation and messages
    {
        let journal = Arc::new(JsonlConversationJournal::new(&dir_path));
        let id_gen = Arc::new(SequentialIdGenerator::new());
        let clock = Arc::new(FixedClock::new("2026-05-23T10:00:00Z".parse().unwrap()));
        let kernel = ConversationKernel::with_generators(journal, id_gen, clock);

        let conv_id = kernel
            .create_conversation(CreateConversationCommand {
                kind: ConversationKind::AgentTask,
                title: Some("Persistent Task".into()),
                participants: vec![human("u1", "诗闻"), agent("a1", "小助理")],
                actor_id: Some(ParticipantId::from("u1")),
            })
            .await
            .unwrap();

        kernel
            .append_message(AppendMessageCommand {
                conversation_id: conv_id.clone(),
                sender_id: ParticipantId::from("u1"),
                content: MessageContent::Text {
                    text: "记住这个任务".into(),
                },
                reply_to: None,
                thread_id: None,
                visibility: Visibility::Conversation,
            })
            .await
            .unwrap();
    }

    // Phase B: Reopen and verify persistence
    {
        let journal = Arc::new(JsonlConversationJournal::new(&dir_path));
        let id_gen = Arc::new(SequentialIdGenerator::new());
        let clock = Arc::new(FixedClock::new("2026-05-23T10:00:00Z".parse().unwrap()));
        let kernel = ConversationKernel::with_generators(journal, id_gen, clock);

        // Load events using the same conversation ID.
        // create_conversation generates the conversation ID via id_gen,
        // and in Phase A, id_gen started at 1, so conversation_id = "id-1".
        let conv_id = ConversationId::from("id-1");
        let events = kernel.load_events(&conv_id).await.unwrap();

        // Should have: 1 Created + 2 ParticipantAdded + 1 MessageAppended = 4
        assert_eq!(events.len(), 4);

        let state = ConversationProjector::project(&events).unwrap();
        assert!(state.session.is_some());
        assert_eq!(
            state.session.as_ref().unwrap().title,
            Some("Persistent Task".into())
        );
        assert_eq!(state.messages.len(), 1);
        match &state.messages[0].content {
            MessageContent::Text { text } => assert_eq!(text, "记住这个任务"),
            _ => panic!("expected Text"),
        }
    }
}
