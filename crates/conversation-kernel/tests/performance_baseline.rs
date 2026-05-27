use std::time::Instant;

use conversation_core::{
    CURRENT_SCHEMA_VERSION, ConversationEvent, ConversationEventEnvelope, ConversationId,
    ConversationKind, ConversationSession, ConversationStatus, EventId, Message, MessageContent,
    MessageId, ParticipantId, Visibility,
};
use conversation_kernel::ConversationProjector;

const REPLAY_EVENT_COUNT: usize = 501;

fn now() -> chrono::DateTime<chrono::Utc> {
    chrono::Utc::now()
}

fn envelope(event_id: usize, event: ConversationEvent) -> ConversationEventEnvelope {
    ConversationEventEnvelope {
        schema_version: CURRENT_SCHEMA_VERSION,
        event_id: EventId::from(format!("evt-{event_id:04}")),
        conversation_id: ConversationId::from("conv-baseline"),
        occurred_at: now(),
        actor_id: Some(ParticipantId::from("user-1")),
        event,
    }
}

fn conversation_created_event() -> ConversationEvent {
    ConversationEvent::ConversationCreated {
        session: ConversationSession {
            id: ConversationId::from("conv-baseline"),
            kind: ConversationKind::AgentTask,
            title: Some("Performance baseline".to_string()),
            participants: vec![],
            created_at: now(),
            updated_at: now(),
            status: ConversationStatus::Active,
        },
    }
}

fn message_appended_event(index: usize) -> ConversationEvent {
    ConversationEvent::MessageAppended {
        message: Message {
            id: MessageId::from(format!("msg-{index:04}")),
            conversation_id: ConversationId::from("conv-baseline"),
            sender_id: ParticipantId::from("user-1"),
            content: MessageContent::Text {
                text: format!("baseline replay message {index}"),
            },
            reply_to: None,
            thread_id: None,
            visibility: Visibility::Conversation,
            created_at: now(),
            edited_at: None,
        },
    }
}

#[test]
fn conversation_replay_500_message_baseline() {
    let mut events = Vec::with_capacity(REPLAY_EVENT_COUNT);
    events.push(envelope(0, conversation_created_event()));
    for index in 1..REPLAY_EVENT_COUNT {
        events.push(envelope(index, message_appended_event(index)));
    }

    let started = Instant::now();
    let state = ConversationProjector::project(&events).unwrap();
    let elapsed = started.elapsed();

    assert_eq!(state.messages.len(), 500);
    assert!(
        elapsed.as_millis() < 1_000,
        "conversation replay baseline regressed: projected {REPLAY_EVENT_COUNT} events in {elapsed:?}"
    );
    eprintln!(
        "performance baseline: conversation replay projected {REPLAY_EVENT_COUNT} events in {elapsed:?}"
    );
}
