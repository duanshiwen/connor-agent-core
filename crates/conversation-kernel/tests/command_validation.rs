use chrono::{DateTime, Utc};
use conversation_core::*;
use conversation_journal::{ConversationJournal, MemoryConversationJournal};
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

fn setup() -> (ConversationKernel, Arc<MemoryConversationJournal>) {
    let journal = Arc::new(MemoryConversationJournal::new());
    let id_gen = Arc::new(SequentialIdGenerator::new());
    let clock = Arc::new(FixedClock::new("2026-05-24T10:00:00Z".parse().unwrap()));
    (
        ConversationKernel::with_generators(journal.clone(), id_gen, clock),
        journal,
    )
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

async fn create_conversation(kernel: &ConversationKernel) -> ConversationId {
    kernel
        .create_conversation(CreateConversationCommand {
            kind: ConversationKind::Direct,
            title: Some("Validation".to_string()),
            participants: vec![human("u1", "Test User"), agent("a1", "Assistant")],
            actor_id: Some(ParticipantId::from("u1")),
        })
        .await
        .unwrap()
}

fn append_text_command(conversation_id: ConversationId) -> AppendMessageCommand {
    AppendMessageCommand {
        conversation_id,
        sender_id: ParticipantId::from("u1"),
        content: MessageContent::Text {
            text: "hello".to_string(),
        },
        reply_to: None,
        thread_id: None,
        visibility: Visibility::Conversation,
    }
}

async fn append_text(kernel: &ConversationKernel, conversation_id: ConversationId) -> MessageId {
    kernel
        .append_message(append_text_command(conversation_id))
        .await
        .unwrap()
}

#[tokio::test]
async fn append_message_rejects_archived_conversation() {
    let (kernel, journal) = setup();
    let conversation_id = ConversationId::from("archived-conv");
    let now = "2026-05-24T10:00:00Z".parse().unwrap();

    journal
        .append(ConversationEventEnvelope {
            schema_version: CURRENT_SCHEMA_VERSION,
            event_id: EventId::from("event-archived-created"),
            conversation_id: conversation_id.clone(),
            occurred_at: now,
            actor_id: Some(ParticipantId::from("u1")),
            event: ConversationEvent::ConversationCreated {
                session: ConversationSession {
                    id: conversation_id.clone(),
                    kind: ConversationKind::Direct,
                    title: None,
                    participants: vec![ParticipantId::from("u1")],
                    created_at: now,
                    updated_at: now,
                    status: ConversationStatus::Archived,
                },
            },
        })
        .await
        .unwrap();
    journal
        .append(ConversationEventEnvelope {
            schema_version: CURRENT_SCHEMA_VERSION,
            event_id: EventId::from("event-archived-participant"),
            conversation_id: conversation_id.clone(),
            occurred_at: now,
            actor_id: Some(ParticipantId::from("u1")),
            event: ConversationEvent::ParticipantAdded {
                participant: human("u1", "Test User"),
            },
        })
        .await
        .unwrap();

    let result = kernel
        .append_message(append_text_command(conversation_id))
        .await;

    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("not active"));
}

#[tokio::test]
async fn append_message_rejects_missing_reply_to() {
    let (kernel, _) = setup();
    let conversation_id = create_conversation(&kernel).await;
    let mut command = append_text_command(conversation_id);
    command.reply_to = Some(MessageId::from("missing-message"));

    let result = kernel.append_message(command).await;

    assert!(result.is_err());
    assert!(
        result
            .unwrap_err()
            .to_string()
            .contains("reply_to message not found")
    );
}

#[tokio::test]
async fn append_message_rejects_missing_thread_id() {
    let (kernel, _) = setup();
    let conversation_id = create_conversation(&kernel).await;
    let mut command = append_text_command(conversation_id);
    command.thread_id = Some(ThreadId::from("missing-thread"));

    let result = kernel.append_message(command).await;

    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("thread not found"));
}

#[tokio::test]
async fn append_message_accepts_existing_thread_id() {
    let (kernel, journal) = setup();
    let conversation_id = create_conversation(&kernel).await;
    let root_message_id = append_text(&kernel, conversation_id.clone()).await;
    let now = "2026-05-24T10:01:00Z".parse().unwrap();

    journal
        .append(ConversationEventEnvelope {
            schema_version: CURRENT_SCHEMA_VERSION,
            event_id: EventId::from("event-thread-started"),
            conversation_id: conversation_id.clone(),
            occurred_at: now,
            actor_id: Some(ParticipantId::from("u1")),
            event: ConversationEvent::ThreadStarted {
                thread_id: ThreadId::from("thread-1"),
                root_message_id,
            },
        })
        .await
        .unwrap();

    let mut command = append_text_command(conversation_id);
    command.thread_id = Some(ThreadId::from("thread-1"));

    let result = kernel.append_message(command).await;

    assert!(result.is_ok());
}

#[tokio::test]
async fn append_message_rejects_private_to_unknown_user() {
    let (kernel, _) = setup();
    let conversation_id = create_conversation(&kernel).await;
    let mut command = append_text_command(conversation_id);
    command.visibility = Visibility::PrivateToUser {
        user_id: ParticipantId::from("ghost"),
    };

    let result = kernel.append_message(command).await;

    assert!(result.is_err());
    assert!(
        result
            .unwrap_err()
            .to_string()
            .contains("visibility user is not a participant")
    );
}

#[tokio::test]
async fn append_message_rejects_participants_visibility_with_unknown_participant() {
    let (kernel, _) = setup();
    let conversation_id = create_conversation(&kernel).await;
    let mut command = append_text_command(conversation_id);
    command.visibility = Visibility::Participants {
        participant_ids: vec![ParticipantId::from("u1"), ParticipantId::from("ghost")],
    };

    let result = kernel.append_message(command).await;

    assert!(result.is_err());
    assert!(
        result
            .unwrap_err()
            .to_string()
            .contains("visibility participant is not a participant")
    );
}

#[tokio::test]
async fn append_message_rejects_agent_only_without_agent_participant() {
    let (kernel, _) = setup();
    let conversation_id = kernel
        .create_conversation(CreateConversationCommand {
            kind: ConversationKind::Direct,
            title: None,
            participants: vec![human("u1", "Test User")],
            actor_id: Some(ParticipantId::from("u1")),
        })
        .await
        .unwrap();
    let mut command = append_text_command(conversation_id);
    command.visibility = Visibility::AgentOnly;

    let result = kernel.append_message(command).await;

    assert!(result.is_err());
    assert!(
        result
            .unwrap_err()
            .to_string()
            .contains("requires an agent participant")
    );
}

#[tokio::test]
async fn create_assistant_suggestion_rejects_conversation_without_agent() {
    let (kernel, _) = setup();
    let conversation_id = kernel
        .create_conversation(CreateConversationCommand {
            kind: ConversationKind::Direct,
            title: None,
            participants: vec![human("u1", "Test User")],
            actor_id: Some(ParticipantId::from("u1")),
        })
        .await
        .unwrap();

    let result = kernel
        .create_assistant_suggestion(CreateAssistantSuggestionCommand {
            conversation_id,
            target_user_id: ParticipantId::from("u1"),
            text: "suggestion".to_string(),
            actions: vec![],
            trigger: SuggestionTrigger::Proactive,
        })
        .await;

    assert!(result.is_err());
    assert!(
        result
            .unwrap_err()
            .to_string()
            .contains("no agent participant")
    );
}
