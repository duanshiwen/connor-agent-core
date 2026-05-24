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
    let clock = Arc::new(FixedClock::new("2026-05-24T09:00:00Z".parse().unwrap()));
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

async fn create_conversation_with_message(
    kernel: &ConversationKernel,
) -> (ConversationId, MessageId) {
    let conversation_id = kernel
        .create_conversation(CreateConversationCommand {
            kind: ConversationKind::Direct,
            title: Some("Edit message".to_string()),
            participants: vec![human("u1", "Test User"), agent("a1", "Assistant")],
            actor_id: Some(ParticipantId::from("u1")),
        })
        .await
        .unwrap();

    let message_id = kernel
        .append_message(AppendMessageCommand {
            conversation_id: conversation_id.clone(),
            sender_id: ParticipantId::from("u1"),
            content: MessageContent::Text {
                text: "original".to_string(),
            },
            reply_to: None,
            thread_id: None,
            visibility: Visibility::Conversation,
        })
        .await
        .unwrap();

    (conversation_id, message_id)
}

#[tokio::test]
async fn edit_message_updates_content_and_uses_event_timestamp() {
    let kernel = setup();
    let (conversation_id, message_id) = create_conversation_with_message(&kernel).await;

    kernel
        .edit_message(EditMessageCommand {
            conversation_id: conversation_id.clone(),
            message_id: message_id.clone(),
            new_content: MessageContent::Text {
                text: "edited".to_string(),
            },
            edited_by: ParticipantId::from("u1"),
        })
        .await
        .unwrap();

    let state = kernel.load_state(&conversation_id).await.unwrap();
    let message = state.messages_by_id.get(&message_id).unwrap();

    assert_eq!(
        message.edited_at,
        Some("2026-05-24T09:00:00Z".parse().unwrap())
    );
    match &message.content {
        MessageContent::Text { text } => assert_eq!(text, "edited"),
        _ => panic!("expected text content"),
    }

    let ordered_message = state
        .messages
        .iter()
        .find(|message| message.id == message_id)
        .unwrap();
    assert_eq!(ordered_message.edited_at, message.edited_at);
    assert_eq!(ordered_message.content, message.content);
}

#[tokio::test]
async fn projecting_same_edit_events_twice_is_deterministic() {
    let kernel = setup();
    let (conversation_id, message_id) = create_conversation_with_message(&kernel).await;

    kernel
        .edit_message(EditMessageCommand {
            conversation_id: conversation_id.clone(),
            message_id,
            new_content: MessageContent::Text {
                text: "edited".to_string(),
            },
            edited_by: ParticipantId::from("u1"),
        })
        .await
        .unwrap();

    let events = kernel.load_events(&conversation_id).await.unwrap();
    let state1 = ConversationProjector::project(&events).unwrap();
    let state2 = ConversationProjector::project(&events).unwrap();

    assert_eq!(state1.messages, state2.messages);
    assert_eq!(state1.messages_by_id, state2.messages_by_id);
}

#[tokio::test]
async fn edit_message_rejects_missing_message() {
    let kernel = setup();
    let (conversation_id, _) = create_conversation_with_message(&kernel).await;

    let result = kernel
        .edit_message(EditMessageCommand {
            conversation_id,
            message_id: MessageId::from("missing-message"),
            new_content: MessageContent::Text {
                text: "edited".to_string(),
            },
            edited_by: ParticipantId::from("u1"),
        })
        .await;

    assert!(result.is_err());
    assert!(
        result
            .unwrap_err()
            .to_string()
            .contains("message not found")
    );
}

#[tokio::test]
async fn edit_message_rejects_unknown_editor() {
    let kernel = setup();
    let (conversation_id, message_id) = create_conversation_with_message(&kernel).await;

    let result = kernel
        .edit_message(EditMessageCommand {
            conversation_id,
            message_id,
            new_content: MessageContent::Text {
                text: "edited".to_string(),
            },
            edited_by: ParticipantId::from("ghost"),
        })
        .await;

    assert!(result.is_err());
    assert!(
        result
            .unwrap_err()
            .to_string()
            .contains("not a participant")
    );
}
