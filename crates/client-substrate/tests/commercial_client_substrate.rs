use std::sync::Arc;

use agentos_storage::AgentOsStorage;
use audit_log::MemoryAuditSink;
use client_substrate::*;
use conversation_core::{MessageId, ParticipantId};
use conversation_journal::MemoryConversationJournal;
use model_adapter::FakeModelAdapter;

fn user() -> ParticipantId {
    ParticipantId::from("user-1")
}

fn agent() -> ParticipantId {
    ParticipantId::from("agent-1")
}

#[test]
fn public_api_version_is_stable_v1() {
    assert_eq!(CLIENT_SUBSTRATE_API_VERSION, 1);
    let event = ClientEventEnvelope {
        id: ClientEventId(1),
        occurred_at: chrono::Utc::now(),
        api_version: CLIENT_SUBSTRATE_API_VERSION,
        event: ClientEvent::RuntimeStatusChanged {
            status: ClientRuntimeStatus::Ready,
        },
    };
    let json = serde_json::to_value(event).unwrap();
    assert_eq!(json["api_version"], 1);
    assert_eq!(json["event"]["type"], "runtime_status_changed");
}

#[test]
fn production_builder_requires_dependencies() {
    let result = ClientSubstrateBuilder::new()
        .runtime_mode(ClientRuntimeMode::Production)
        .build();
    assert!(matches!(
        result,
        Err(ClientSubstrateError::ProductionGuardFailed { .. })
    ));
}

#[test]
fn production_builder_rejects_test_only_components() {
    let temp = tempfile::tempdir().unwrap();
    let storage = Arc::new(AgentOsStorage::init(temp.path()).unwrap());
    let deps = ClientProductionDependencies {
        conversation_journal: Arc::new(MemoryConversationJournal::new()),
        model_adapter: Arc::new(FakeModelAdapter::default()),
        audit_log: Arc::new(MemoryAuditSink::new()),
        storage,
        component_kinds: ClientProductionComponentKinds {
            conversation_journal: ClientDependencyKind::InMemoryTest,
            model_adapter: ClientDependencyKind::FakeTest,
            audit_log: ClientDependencyKind::InMemoryTest,
            credential_backend: SystemCredentialBackendKind::InMemoryTest,
            identity_crypto: ClientDependencyKind::FakeTest,
        },
    };

    let result = ClientSubstrateBuilder::production(
        ClientProfileId::from("profile-1"),
        ClientWorkspaceId::from("workspace-1"),
        deps,
    )
    .build();

    match result {
        Err(err) => match err {
            ClientSubstrateError::ProductionGuardFailed { blockers } => {
                assert!(blockers.len() >= 5);
                assert!(blockers.iter().any(|item| item.contains("model adapter")));
            }
            other => panic!("unexpected error: {other:?}"),
        },
        Ok(_) => panic!("production builder unexpectedly accepted test-only components"),
    }
}

#[tokio::test]
async fn event_cursor_and_projections_track_client_commands() {
    let substrate = ClientSubstrate::builder().build().unwrap();
    let initial_cursor = substrate.latest_event_cursor();

    let conversation_id = match substrate
        .dispatch(ClientCommand::CreateConversation {
            title: Some("Commercial substrate".to_string()),
            user_id: user(),
            agent_id: agent(),
        })
        .await
        .unwrap()
    {
        ClientCommandResult::ConversationCreated { conversation_id } => conversation_id,
        other => panic!("unexpected result: {other:?}"),
    };

    let message_id = match substrate
        .dispatch(ClientCommand::SubmitUserMessage {
            conversation_id: conversation_id.clone(),
            user_id: user(),
            text: "hello".to_string(),
        })
        .await
        .unwrap()
    {
        ClientCommandResult::UserMessageSubmitted { message_id } => message_id,
        other => panic!("unexpected result: {other:?}"),
    };

    substrate
        .dispatch(ClientCommand::StartAgentRun {
            conversation_id: conversation_id.clone(),
            trigger_message_id: MessageId(message_id.0),
            requested_by: user(),
        })
        .await
        .unwrap();

    let events = substrate.events_after(initial_cursor);
    assert!(events.len() >= 3);
    assert!(events.windows(2).all(|pair| pair[0].id < pair[1].id));

    let conversations = substrate.conversation_list_projection();
    assert_eq!(conversations.conversations.len(), 1);
    assert_eq!(
        conversations.conversations[0].conversation_id,
        conversation_id
    );

    let timeline = substrate.timeline_projection(conversation_id);
    assert_eq!(timeline.items.len(), 1);
    assert_eq!(timeline.items[0].text, "hello");

    let runs = substrate.run_projection();
    assert_eq!(runs.runs.len(), 1);
}
