use asset_core::{
    AssetBlobRef, AssetHash, AssetKind, AssetMetadata, AssetPolicy, AssetProcessingStatus,
    AssetRecord, AssetRelevance, AssetSource, AssetWorkObjectLink, AssetWorkObjectLinkReason,
    WorkObjectType,
};
use chrono::Utc;

#[test]
fn asset_record_tracks_blob_processing_and_work_object_links() {
    let now = Utc::now();
    let metadata = AssetMetadata::new(
        "asset-1",
        AssetKind::Image,
        AssetSource::new(now).with_uri("file:///tmp/screenshot.png"),
        AssetRelevance::High,
        now,
    )
    .with_title("Screenshot");

    let record = AssetRecord::new(metadata, AssetPolicy::default(), now)
        .with_blob(AssetBlobRef {
            uri: "file:///tmp/screenshot.png".to_string(),
            content_hash: Some(AssetHash("sha256:abc".to_string())),
            size_bytes: Some(42),
        })
        .link_work_object(AssetWorkObjectLink::new(
            WorkObjectType::Project,
            "connor-agent-core",
            AssetWorkObjectLinkReason::Evidence,
            now,
        ));

    assert_eq!(record.processing_status, AssetProcessingStatus::Observed);
    assert_eq!(record.blob.unwrap().content_hash.unwrap().0, "sha256:abc");
    assert_eq!(record.linked_work_objects.len(), 1);
    assert_eq!(
        record.linked_work_objects[0].work_object_id.0,
        "connor-agent-core"
    );
}
