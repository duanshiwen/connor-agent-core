use client_substrate::*;

#[test]
fn host_can_project_knowledge_results_with_citations() {
    let substrate = ClientSubstrate::builder().build().unwrap();
    substrate.replace_knowledge_results_for_host(
        "agent os",
        vec![ClientKnowledgeResultCard {
            entry_id: "entry-1".to_string(),
            title: "Agent OS".to_string(),
            snippet: Some("local-first agent operating system".to_string()),
            score: 0.91,
            confidentiality: Some("internal".to_string()),
            permission_required: true,
            citations: vec![ClientCitationRef {
                source_uri: Some("agentos://knowledge/entry-1".to_string()),
                artifact_id: None,
                asset_id: Some("asset-1".to_string()),
                evidence_label: Some("source note".to_string()),
            }],
        }],
    );

    let projection = substrate.knowledge_projection();
    assert_eq!(projection.last_query.as_deref(), Some("agent os"));
    assert_eq!(projection.results.len(), 1);
    assert_eq!(
        projection.results[0].confidentiality.as_deref(),
        Some("internal")
    );
    assert!(projection.results[0].permission_required);
    assert_eq!(
        projection.results[0].citations[0].asset_id.as_deref(),
        Some("asset-1")
    );
}

#[test]
fn host_can_project_assets_linked_to_work_objects() {
    let substrate = ClientSubstrate::builder().build().unwrap();
    substrate.upsert_asset_card_for_host(ClientAssetCard {
        asset_id: "asset-1".to_string(),
        title: Some("browser screenshot".to_string()),
        kind: "image".to_string(),
        source_uri: Some("file:///artifacts/screenshot.png".to_string()),
        processing_status: "processed".to_string(),
        linked_work_objects: vec![ClientWorkObjectLinkSummary {
            work_object_type: "project".to_string(),
            work_object_id: "connor-agent-core".to_string(),
            reason: Some("evidence".to_string()),
        }],
    });

    let projection = substrate.asset_projection();
    assert_eq!(projection.assets.len(), 1);
    assert_eq!(projection.assets[0].asset_id, "asset-1");
    assert_eq!(
        projection.assets[0].linked_work_objects[0].work_object_id,
        "connor-agent-core"
    );
}
