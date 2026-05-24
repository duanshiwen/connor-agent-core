use chrono::{DateTime, Utc};
use conversation_core::*;
use conversation_journal::MemoryConversationJournal;
use conversation_kernel::*;
use entity_core::{EntityCapability, EntityDescriptor, EntityId, EntityKind, LinkReason};
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
    let clock = Arc::new(FixedClock::new("2026-05-24T12:00:00Z".parse().unwrap()));
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

fn browser_entity() -> EntityDescriptor {
    EntityDescriptor {
        id: EntityId::from("browser-main"),
        kind: EntityKind::Browser,
        display_name: "Browser".to_string(),
        capabilities: vec![EntityCapability::new("read_page")],
        default_policy_ref: Some("policy/browser/default".to_string()),
    }
}

async fn create_conversation(kernel: &ConversationKernel) -> ConversationId {
    kernel
        .create_conversation(CreateConversationCommand {
            kind: ConversationKind::AgentTask,
            title: Some("Linked entity".to_string()),
            participants: vec![human("u1", "诗闻"), agent("a1", "Assistant")],
            actor_id: Some(ParticipantId::from("u1")),
        })
        .await
        .unwrap()
}

async fn create_conversation_with_browser(kernel: &ConversationKernel) -> ConversationId {
    let conversation_id = create_conversation(kernel).await;
    kernel
        .link_entity(LinkEntityCommand {
            conversation_id: conversation_id.clone(),
            entity: browser_entity(),
            reason: LinkReason::UserRequested,
            linked_by: Some(ParticipantId::from("u1")),
        })
        .await
        .unwrap();
    conversation_id
}

#[tokio::test]
async fn conversation_can_link_browser_entity_without_adding_participant() {
    let kernel = setup();
    let conversation_id = create_conversation_with_browser(&kernel).await;

    let state = kernel.load_state(&conversation_id).await.unwrap();

    assert!(
        state
            .linked_entities
            .contains_key(&EntityId::from("browser-main"))
    );
    assert!(
        !state
            .participants
            .contains_key(&ParticipantId::from("browser-main"))
    );
    assert_eq!(state.participants.len(), 2);
}

#[tokio::test]
async fn linked_entity_projection_replays_deterministically() {
    let kernel = setup();
    let conversation_id = create_conversation_with_browser(&kernel).await;

    let events = kernel.load_events(&conversation_id).await.unwrap();
    let state1 = ConversationProjector::project(&events).unwrap();
    let state2 = ConversationProjector::project(&events).unwrap();

    assert_eq!(state1.linked_entities, state2.linked_entities);
    assert_eq!(state1.participants, state2.participants);
}

#[tokio::test]
async fn unlink_entity_removes_it_from_projection() {
    let kernel = setup();
    let conversation_id = create_conversation_with_browser(&kernel).await;

    kernel
        .unlink_entity(UnlinkEntityCommand {
            conversation_id: conversation_id.clone(),
            entity_id: EntityId::from("browser-main"),
            reason: "no longer needed".to_string(),
            unlinked_by: Some(ParticipantId::from("u1")),
        })
        .await
        .unwrap();

    let state = kernel.load_state(&conversation_id).await.unwrap();
    assert!(
        !state
            .linked_entities
            .contains_key(&EntityId::from("browser-main"))
    );
}

#[tokio::test]
async fn entity_query_event_is_append_only_and_replayable() {
    let kernel = setup();
    let conversation_id = create_conversation_with_browser(&kernel).await;

    kernel
        .query_entity(QueryEntityCommand {
            conversation_id: conversation_id.clone(),
            entity_id: EntityId::from("browser-main"),
            query: "current page title".to_string(),
            result_ref: Some("query/browser-main/001".to_string()),
            queried_by: Some(ParticipantId::from("a1")),
        })
        .await
        .unwrap();

    let events = kernel.load_events(&conversation_id).await.unwrap();
    assert!(
        events
            .iter()
            .any(|event| matches!(event.event, ConversationEvent::EntityQueried { .. }))
    );

    let state = ConversationProjector::project(&events).unwrap();
    assert!(
        state
            .linked_entities
            .contains_key(&EntityId::from("browser-main"))
    );
}

#[tokio::test]
async fn observe_entity_state_requires_linked_entity() {
    let kernel = setup();
    let conversation_id = create_conversation(&kernel).await;

    let result = kernel
        .observe_entity_state(ObserveEntityStateCommand {
            conversation_id,
            entity_id: EntityId::from("browser-main"),
            state_ref: "state/browser-main/001".to_string(),
            observed_by: Some(ParticipantId::from("a1")),
        })
        .await;

    assert!(result.is_err());
    assert!(
        result
            .unwrap_err()
            .to_string()
            .contains("linked entity not found")
    );
}

#[tokio::test]
async fn link_entity_rejects_unknown_actor() {
    let kernel = setup();
    let conversation_id = create_conversation(&kernel).await;

    let result = kernel
        .link_entity(LinkEntityCommand {
            conversation_id,
            entity: browser_entity(),
            reason: LinkReason::UserRequested,
            linked_by: Some(ParticipantId::from("ghost")),
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
