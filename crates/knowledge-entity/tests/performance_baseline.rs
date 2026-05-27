use std::time::Instant;

use knowledge_entity::{
    DeterministicFullTextKnowledgeBackend, KnowledgeEntryId, KnowledgeEntryRef,
    KnowledgeFullTextQuery, KnowledgeIndex, KnowledgeIndexDocument, KnowledgeIndexRebuildRequest,
};

const KNOWLEDGE_DOCUMENT_COUNT: usize = 400;

fn entry(index: usize) -> KnowledgeEntryRef {
    KnowledgeEntryRef {
        id: KnowledgeEntryId::from(format!("knowledge-entry-{index:04}")),
        title: format!("AgentOS Performance Baseline {index}"),
        source_uri: None,
        artifact_id: None,
        asset_id: None,
        created_at: chrono::Utc::now(),
    }
}

#[tokio::test]
async fn knowledge_fulltext_search_400_document_baseline() {
    let documents = (0..KNOWLEDGE_DOCUMENT_COUNT)
        .map(|index| {
            KnowledgeIndexDocument::new(
                entry(index),
                format!(
                    "AgentOS knowledge search baseline document {index}. Memory kernel action runtime replay search."
                ),
            )
            .with_tags(vec!["performance".to_string()])
        })
        .collect::<Vec<_>>();

    let mut backend = DeterministicFullTextKnowledgeBackend::new();
    backend
        .rebuild(KnowledgeIndexRebuildRequest {
            documents,
            requested_at: chrono::Utc::now(),
        })
        .await
        .unwrap();

    let query = KnowledgeFullTextQuery::new("AgentOS baseline search")
        .with_tags(vec!["performance".to_string()])
        .with_limit(25);
    let started = Instant::now();
    let results = backend.query(&query).await.unwrap();
    let elapsed = started.elapsed();

    assert_eq!(results.len(), 25);
    assert!(
        elapsed.as_millis() < 1_000,
        "knowledge search baseline regressed: searched {KNOWLEDGE_DOCUMENT_COUNT} docs in {elapsed:?}"
    );
    eprintln!(
        "performance baseline: knowledge search scanned {KNOWLEDGE_DOCUMENT_COUNT} docs in {elapsed:?}"
    );
}
