use artifact_core::{ArtifactDescriptor, ArtifactId, ArtifactKind};
use chrono::{DateTime, Utc};
use conversation_core::*;
use conversation_journal::MemoryConversationJournal;
use conversation_kernel::*;
use std::sync::{Arc, Mutex};

struct SequentialIdGenerator {
    counter: Mutex<u64>,
}

impl SequentialIdGenerator {
    fn new() -> Self {
        Self {
            counter: Mutex::new(0),
        }
    }
}

impl IdGenerator for SequentialIdGenerator {
    fn new_id(&self) -> String {
        let mut counter = self.counter.lock().unwrap();
        *counter += 1;
        format!("id-{counter}")
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
    ConversationKernel::with_generators(
        journal,
        Arc::new(SequentialIdGenerator::new()),
        Arc::new(FixedClock::new("2026-05-24T12:00:00Z".parse().unwrap())),
    )
}

fn human() -> Participant {
    Participant {
        id: ParticipantId::from("user-1"),
        kind: ParticipantKind::Human,
        display_name: "Test User".to_string(),
    }
}

fn agent() -> Participant {
    Participant {
        id: ParticipantId::from("agent-1"),
        kind: ParticipantKind::Agent,
        display_name: "Assistant".to_string(),
    }
}

fn artifact() -> ArtifactDescriptor {
    ArtifactDescriptor {
        id: ArtifactId::from("artifact-web-1"),
        kind: ArtifactKind::WebPage,
        title: Some("Agent OS Roadmap".to_string()),
        source_uri: Some("https://example.com/agent-os".to_string()),
        mime_type: Some("text/html".to_string()),
        metadata: serde_json::json!({"captured_by":"browser-entity"}),
        created_at: "2026-05-24T12:00:00Z".parse().unwrap(),
    }
}

async fn create_conversation(kernel: &ConversationKernel) -> ConversationId {
    kernel
        .create_conversation(CreateConversationCommand {
            kind: ConversationKind::AgentTask,
            title: Some("Artifact linking".to_string()),
            participants: vec![human(), agent()],
            actor_id: Some(ParticipantId::from("user-1")),
        })
        .await
        .unwrap()
}

#[tokio::test]
async fn conversation_can_link_artifact() {
    let kernel = test_kernel();
    let conversation_id = create_conversation(&kernel).await;
    let artifact = artifact();

    kernel
        .link_artifact(LinkArtifactCommand {
            conversation_id: conversation_id.clone(),
            artifact: artifact.clone(),
            linked_by: Some(ParticipantId::from("user-1")),
        })
        .await
        .unwrap();

    let state = kernel.load_state(&conversation_id).await.unwrap();
    assert_eq!(state.linked_artifacts.len(), 1);
    assert_eq!(state.linked_artifacts.get(&artifact.id), Some(&artifact));
}

#[tokio::test]
async fn linked_artifact_does_not_become_message_or_participant() {
    let kernel = test_kernel();
    let conversation_id = create_conversation(&kernel).await;

    kernel
        .link_artifact(LinkArtifactCommand {
            conversation_id: conversation_id.clone(),
            artifact: artifact(),
            linked_by: Some(ParticipantId::from("user-1")),
        })
        .await
        .unwrap();

    let state = kernel.load_state(&conversation_id).await.unwrap();
    assert_eq!(state.linked_artifacts.len(), 1);
    assert!(state.messages.is_empty());
    assert_eq!(state.participants.len(), 2);
}

#[tokio::test]
async fn projection_includes_linked_artifact() {
    let kernel = test_kernel();
    let conversation_id = create_conversation(&kernel).await;
    let artifact = artifact();

    kernel
        .link_artifact(LinkArtifactCommand {
            conversation_id: conversation_id.clone(),
            artifact: artifact.clone(),
            linked_by: Some(ParticipantId::from("agent-1")),
        })
        .await
        .unwrap();

    let events = kernel.load_events(&conversation_id).await.unwrap();
    let state = ConversationProjector::project(&events).unwrap();
    assert_eq!(state.linked_artifacts.get(&artifact.id), Some(&artifact));
}

#[tokio::test]
async fn conversation_can_unlink_artifact() {
    let kernel = test_kernel();
    let conversation_id = create_conversation(&kernel).await;
    let artifact = artifact();
    let artifact_id = artifact.id.clone();

    kernel
        .link_artifact(LinkArtifactCommand {
            conversation_id: conversation_id.clone(),
            artifact,
            linked_by: Some(ParticipantId::from("user-1")),
        })
        .await
        .unwrap();

    kernel
        .unlink_artifact(UnlinkArtifactCommand {
            conversation_id: conversation_id.clone(),
            artifact_id: artifact_id.clone(),
            reason: "user removed artifact".to_string(),
            unlinked_by: Some(ParticipantId::from("user-1")),
        })
        .await
        .unwrap();

    let state = kernel.load_state(&conversation_id).await.unwrap();
    assert!(state.linked_artifacts.is_empty());
}

#[tokio::test]
async fn cannot_unlink_missing_artifact() {
    let kernel = test_kernel();
    let conversation_id = create_conversation(&kernel).await;

    let result = kernel
        .unlink_artifact(UnlinkArtifactCommand {
            conversation_id,
            artifact_id: ArtifactId::from("missing-artifact"),
            reason: "not linked".to_string(),
            unlinked_by: Some(ParticipantId::from("user-1")),
        })
        .await;

    assert!(
        result
            .unwrap_err()
            .to_string()
            .contains("linked artifact not found")
    );
}

#[tokio::test]
async fn artifact_link_projection_is_deterministic() {
    let kernel = test_kernel();
    let conversation_id = create_conversation(&kernel).await;

    kernel
        .link_artifact(LinkArtifactCommand {
            conversation_id: conversation_id.clone(),
            artifact: artifact(),
            linked_by: Some(ParticipantId::from("user-1")),
        })
        .await
        .unwrap();

    let events = kernel.load_events(&conversation_id).await.unwrap();
    let state1 = ConversationProjector::project(&events).unwrap();
    let state2 = ConversationProjector::project(&events).unwrap();

    assert_eq!(state1.linked_artifacts, state2.linked_artifacts);
    assert_eq!(state1.messages, state2.messages);
    assert_eq!(state1.participants, state2.participants);
}
