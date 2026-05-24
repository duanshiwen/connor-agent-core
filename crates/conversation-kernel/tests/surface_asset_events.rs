use asset_core::{
    AssetId, AssetKind, AssetMetadata, AssetProcessingStatus, AssetRelevance, AssetSource,
};
use chrono::{DateTime, Utc};
use conversation_core::*;
use conversation_journal::MemoryConversationJournal;
use conversation_kernel::*;
use std::sync::{Arc, Mutex};
use surface_core::{SurfaceDescriptor, SurfaceKind, SurfaceLifecycleStatus, SurfaceRendererHint};

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

fn surface() -> SurfaceDescriptor {
    SurfaceDescriptor::new(
        "surface-web-1",
        SurfaceKind::WebSurface,
        SurfaceRendererHint::Html,
        "2026-05-24T12:00:00Z".parse().unwrap(),
    )
    .with_title("Agent OS Roadmap")
}

fn updated_surface() -> SurfaceDescriptor {
    SurfaceDescriptor::new(
        "surface-web-1",
        SurfaceKind::WebSurface,
        SurfaceRendererHint::Markdown,
        "2026-05-24T12:01:00Z".parse().unwrap(),
    )
    .with_title("Agent OS Roadmap — extracted")
}

fn asset() -> AssetMetadata {
    AssetMetadata::new(
        "asset-image-1",
        AssetKind::Image,
        AssetSource::new("2026-05-24T12:00:00Z".parse().unwrap())
            .with_uri("https://example.com/photo.jpg"),
        AssetRelevance::High,
        "2026-05-24T12:00:00Z".parse().unwrap(),
    )
    .with_title("Example Photo")
    .with_mime_type("image/jpeg")
}

async fn create_conversation(kernel: &ConversationKernel) -> ConversationId {
    kernel
        .create_conversation(CreateConversationCommand {
            kind: ConversationKind::AgentTask,
            title: Some("Surface and asset events".to_string()),
            participants: vec![human(), agent()],
            actor_id: Some(ParticipantId::from("user-1")),
        })
        .await
        .unwrap()
}

#[tokio::test]
async fn conversation_can_attach_surface() {
    let kernel = test_kernel();
    let conversation_id = create_conversation(&kernel).await;
    let surface = surface();

    kernel
        .attach_surface(AttachSurfaceCommand {
            conversation_id: conversation_id.clone(),
            surface: surface.clone(),
            attached_by: Some(ParticipantId::from("user-1")),
        })
        .await
        .unwrap();

    let state = kernel.load_state(&conversation_id).await.unwrap();
    let attached = state.attached_surfaces.get(&surface.id).unwrap();
    assert_eq!(attached.descriptor, surface);
    assert_eq!(attached.status, SurfaceLifecycleStatus::Attached);
}

#[tokio::test]
async fn surface_does_not_become_message_or_participant() {
    let kernel = test_kernel();
    let conversation_id = create_conversation(&kernel).await;

    kernel
        .attach_surface(AttachSurfaceCommand {
            conversation_id: conversation_id.clone(),
            surface: surface(),
            attached_by: Some(ParticipantId::from("user-1")),
        })
        .await
        .unwrap();

    let state = kernel.load_state(&conversation_id).await.unwrap();
    assert_eq!(state.attached_surfaces.len(), 1);
    assert!(state.messages.is_empty());
    assert_eq!(state.participants.len(), 2);
}

#[tokio::test]
async fn conversation_can_update_surface() {
    let kernel = test_kernel();
    let conversation_id = create_conversation(&kernel).await;

    kernel
        .attach_surface(AttachSurfaceCommand {
            conversation_id: conversation_id.clone(),
            surface: surface(),
            attached_by: Some(ParticipantId::from("user-1")),
        })
        .await
        .unwrap();

    let updated = updated_surface();
    kernel
        .update_surface(UpdateSurfaceCommand {
            conversation_id: conversation_id.clone(),
            surface: updated.clone(),
            updated_by: Some(ParticipantId::from("user-1")),
        })
        .await
        .unwrap();

    let state = kernel.load_state(&conversation_id).await.unwrap();
    let surface_state = state.attached_surfaces.get(&updated.id).unwrap();
    assert_eq!(surface_state.descriptor, updated);
    assert_eq!(surface_state.status, SurfaceLifecycleStatus::Updated);
}

#[tokio::test]
async fn conversation_can_close_surface() {
    let kernel = test_kernel();
    let conversation_id = create_conversation(&kernel).await;
    let surface = surface();
    let surface_id = surface.id.clone();

    kernel
        .attach_surface(AttachSurfaceCommand {
            conversation_id: conversation_id.clone(),
            surface,
            attached_by: Some(ParticipantId::from("user-1")),
        })
        .await
        .unwrap();

    kernel
        .close_surface(CloseSurfaceCommand {
            conversation_id: conversation_id.clone(),
            surface_id: surface_id.clone(),
            reason: "user closed preview".to_string(),
            closed_by: Some(ParticipantId::from("user-1")),
        })
        .await
        .unwrap();

    let state = kernel.load_state(&conversation_id).await.unwrap();
    assert_eq!(
        state.attached_surfaces.get(&surface_id).unwrap().status,
        SurfaceLifecycleStatus::Closed
    );
}

#[tokio::test]
async fn cannot_update_missing_surface() {
    let kernel = test_kernel();
    let conversation_id = create_conversation(&kernel).await;

    let result = kernel
        .update_surface(UpdateSurfaceCommand {
            conversation_id,
            surface: surface(),
            updated_by: Some(ParticipantId::from("user-1")),
        })
        .await;

    assert!(
        result
            .unwrap_err()
            .to_string()
            .contains("surface not attached")
    );
}

#[tokio::test]
async fn conversation_can_observe_asset() {
    let kernel = test_kernel();
    let conversation_id = create_conversation(&kernel).await;
    let asset = asset();

    kernel
        .observe_asset(ObserveAssetCommand {
            conversation_id: conversation_id.clone(),
            asset: asset.clone(),
            observed_by: Some(ParticipantId::from("user-1")),
        })
        .await
        .unwrap();

    let state = kernel.load_state(&conversation_id).await.unwrap();
    assert_eq!(state.observed_assets.get(&asset.id), Some(&asset));
    assert_eq!(
        state.asset_statuses.get(&asset.id),
        Some(&AssetProcessingStatus::Observed)
    );
}

#[tokio::test]
async fn conversation_can_capture_asset() {
    let kernel = test_kernel();
    let conversation_id = create_conversation(&kernel).await;
    let asset = asset();

    kernel
        .observe_asset(ObserveAssetCommand {
            conversation_id: conversation_id.clone(),
            asset: asset.clone(),
            observed_by: Some(ParticipantId::from("user-1")),
        })
        .await
        .unwrap();

    kernel
        .capture_asset(CaptureAssetCommand {
            conversation_id: conversation_id.clone(),
            asset: asset.clone(),
            captured_by: Some(ParticipantId::from("user-1")),
        })
        .await
        .unwrap();

    let state = kernel.load_state(&conversation_id).await.unwrap();
    assert_eq!(state.observed_assets.get(&asset.id), Some(&asset));
    assert_eq!(
        state.asset_statuses.get(&asset.id),
        Some(&AssetProcessingStatus::Captured)
    );
}

#[tokio::test]
async fn conversation_can_process_asset() {
    let kernel = test_kernel();
    let conversation_id = create_conversation(&kernel).await;
    let asset = asset();
    let asset_id = asset.id.clone();

    kernel
        .observe_asset(ObserveAssetCommand {
            conversation_id: conversation_id.clone(),
            asset,
            observed_by: Some(ParticipantId::from("user-1")),
        })
        .await
        .unwrap();

    kernel
        .process_asset(ProcessAssetCommand {
            conversation_id: conversation_id.clone(),
            asset_id: asset_id.clone(),
            status: AssetProcessingStatus::Processed,
            processed_by: Some(ParticipantId::from("user-1")),
        })
        .await
        .unwrap();

    let state = kernel.load_state(&conversation_id).await.unwrap();
    assert_eq!(
        state.asset_statuses.get(&asset_id),
        Some(&AssetProcessingStatus::Processed)
    );
}

#[tokio::test]
async fn cannot_process_missing_asset() {
    let kernel = test_kernel();
    let conversation_id = create_conversation(&kernel).await;

    let result = kernel
        .process_asset(ProcessAssetCommand {
            conversation_id,
            asset_id: AssetId::from("missing-asset"),
            status: AssetProcessingStatus::Processed,
            processed_by: Some(ParticipantId::from("user-1")),
        })
        .await;

    assert!(
        result
            .unwrap_err()
            .to_string()
            .contains("asset not observed")
    );
}

#[tokio::test]
async fn surface_and_asset_projection_is_deterministic() {
    let kernel = test_kernel();
    let conversation_id = create_conversation(&kernel).await;
    let surface = surface();
    let asset = asset();

    kernel
        .attach_surface(AttachSurfaceCommand {
            conversation_id: conversation_id.clone(),
            surface,
            attached_by: Some(ParticipantId::from("user-1")),
        })
        .await
        .unwrap();

    kernel
        .observe_asset(ObserveAssetCommand {
            conversation_id: conversation_id.clone(),
            asset,
            observed_by: Some(ParticipantId::from("user-1")),
        })
        .await
        .unwrap();

    let events = kernel.load_events(&conversation_id).await.unwrap();
    let state1 = ConversationProjector::project(&events).unwrap();
    let state2 = ConversationProjector::project(&events).unwrap();

    assert_eq!(state1.attached_surfaces, state2.attached_surfaces);
    assert_eq!(state1.observed_assets, state2.observed_assets);
    assert_eq!(state1.asset_statuses, state2.asset_statuses);
    assert_eq!(state1.messages, state2.messages);
    assert_eq!(state1.participants, state2.participants);
}
