//! # Asset Index
//!
//! Deterministic in-memory index for AgentOS asset metadata.

use asset_core::{
    AssetId, AssetKind, AssetMetadata, AssetProcessingStatus, AssetRelevance, AssetWorkObjectLink,
    WorkObjectId, WorkObjectType,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// A deterministic index entry for one asset.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AssetIndexEntry {
    pub metadata: AssetMetadata,
    pub status: AssetProcessingStatus,
    pub indexed_at: DateTime<Utc>,
    pub linked_work_objects: Vec<AssetWorkObjectLink>,
}

impl AssetIndexEntry {
    pub fn new(
        metadata: AssetMetadata,
        status: AssetProcessingStatus,
        indexed_at: DateTime<Utc>,
    ) -> Self {
        Self {
            metadata,
            status,
            indexed_at,
            linked_work_objects: vec![],
        }
    }

    pub fn with_linked_work_objects(mut self, links: Vec<AssetWorkObjectLink>) -> Self {
        self.linked_work_objects = links;
        self
    }
}

/// Query filters for the in-memory asset index.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AssetIndexQuery {
    pub kind: Option<AssetKind>,
    pub status: Option<AssetProcessingStatus>,
    pub min_relevance: Option<AssetRelevance>,
    pub tag: Option<String>,
    pub source_uri_prefix: Option<String>,
    pub work_object_type: Option<WorkObjectType>,
    pub work_object_id: Option<WorkObjectId>,
}

/// Errors from asset index operations.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum AssetIndexError {
    #[error("duplicate asset id: {0}")]
    DuplicateAssetId(AssetId),
    #[error("asset not found: {0}")]
    AssetNotFound(AssetId),
    #[error(
        "duplicate work object link for asset {asset_id}: {work_object_type:?}/{work_object_id}"
    )]
    DuplicateWorkObjectLink {
        asset_id: AssetId,
        work_object_type: WorkObjectType,
        work_object_id: WorkObjectId,
    },
}

/// Deterministic in-memory asset index.
#[derive(Debug, Clone, Default)]
pub struct AssetIndex {
    entries: HashMap<AssetId, AssetIndexEntry>,
}

impl AssetIndex {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert(&mut self, entry: AssetIndexEntry) -> Result<(), AssetIndexError> {
        if self.entries.contains_key(&entry.metadata.id) {
            return Err(AssetIndexError::DuplicateAssetId(entry.metadata.id));
        }
        self.entries.insert(entry.metadata.id.clone(), entry);
        Ok(())
    }

    pub fn upsert(&mut self, entry: AssetIndexEntry) {
        self.entries.insert(entry.metadata.id.clone(), entry);
    }

    pub fn get(&self, id: &AssetId) -> Option<&AssetIndexEntry> {
        self.entries.get(id)
    }

    pub fn update_status(
        &mut self,
        id: &AssetId,
        status: AssetProcessingStatus,
    ) -> Result<(), AssetIndexError> {
        let entry = self
            .entries
            .get_mut(id)
            .ok_or_else(|| AssetIndexError::AssetNotFound(id.clone()))?;
        entry.status = status;
        Ok(())
    }

    pub fn link_work_object(
        &mut self,
        asset_id: &AssetId,
        link: AssetWorkObjectLink,
    ) -> Result<(), AssetIndexError> {
        let entry = self
            .entries
            .get_mut(asset_id)
            .ok_or_else(|| AssetIndexError::AssetNotFound(asset_id.clone()))?;

        if entry.linked_work_objects.iter().any(|candidate| {
            candidate.work_object_type == link.work_object_type
                && candidate.work_object_id == link.work_object_id
        }) {
            return Err(AssetIndexError::DuplicateWorkObjectLink {
                asset_id: asset_id.clone(),
                work_object_type: link.work_object_type,
                work_object_id: link.work_object_id,
            });
        }

        entry.linked_work_objects.push(link);
        Ok(())
    }

    pub fn query(&self, query: &AssetIndexQuery) -> Vec<&AssetIndexEntry> {
        let mut entries = self
            .entries
            .values()
            .filter(|entry| Self::matches(entry, query))
            .collect::<Vec<_>>();
        entries.sort_by(|a, b| a.metadata.id.0.cmp(&b.metadata.id.0));
        entries
    }

    fn matches(entry: &AssetIndexEntry, query: &AssetIndexQuery) -> bool {
        if let Some(kind) = &query.kind
            && entry.metadata.kind != *kind
        {
            return false;
        }

        if let Some(status) = &query.status
            && entry.status != *status
        {
            return false;
        }

        if let Some(min_relevance) = &query.min_relevance
            && !entry.metadata.relevance.meets_threshold(min_relevance)
        {
            return false;
        }

        if let Some(tag) = &query.tag
            && !entry.metadata.tags.iter().any(|candidate| candidate == tag)
        {
            return false;
        }

        if let Some(prefix) = &query.source_uri_prefix {
            let Some(uri) = &entry.metadata.source.uri else {
                return false;
            };
            if !uri.starts_with(prefix) {
                return false;
            }
        }

        if query.work_object_type.is_some() || query.work_object_id.is_some() {
            let matches_work_object = entry.linked_work_objects.iter().any(|link| {
                query
                    .work_object_type
                    .as_ref()
                    .is_none_or(|expected_type| link.work_object_type == *expected_type)
                    && query
                        .work_object_id
                        .as_ref()
                        .is_none_or(|expected_id| link.work_object_id == *expected_id)
            });
            if !matches_work_object {
                return false;
            }
        }

        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use asset_core::{
        AssetId, AssetKind, AssetMetadata, AssetProcessingStatus, AssetRelevance, AssetSource,
        AssetWorkObjectLink, AssetWorkObjectLinkReason, WorkObjectId, WorkObjectType,
    };
    use chrono::{TimeZone, Utc};

    fn ts(seconds: i64) -> chrono::DateTime<Utc> {
        Utc.timestamp_opt(seconds, 0).unwrap()
    }

    fn asset(
        id: &str,
        kind: AssetKind,
        relevance: AssetRelevance,
        uri: Option<&str>,
        tags: Vec<&str>,
    ) -> AssetMetadata {
        let mut source = AssetSource::new(ts(1));
        if let Some(uri) = uri {
            source = source.with_uri(uri);
        }
        AssetMetadata::new(id, kind, source, relevance, ts(2))
            .with_title(format!("asset {id}"))
            .with_tags(tags.into_iter().map(str::to_string).collect())
    }

    fn entry(
        id: &str,
        kind: AssetKind,
        status: AssetProcessingStatus,
        relevance: AssetRelevance,
        uri: Option<&str>,
        tags: Vec<&str>,
    ) -> AssetIndexEntry {
        AssetIndexEntry::new(asset(id, kind, relevance, uri, tags), status, ts(3))
    }

    #[test]
    fn asset_index_entry_roundtrips() {
        let entry = entry(
            "asset-image-1",
            AssetKind::Image,
            AssetProcessingStatus::Observed,
            AssetRelevance::High,
            Some("https://example.com/images/1.png"),
            vec!["screenshot"],
        );

        let json = serde_json::to_string(&entry).unwrap();
        let decoded: AssetIndexEntry = serde_json::from_str(&json).unwrap();

        assert_eq!(decoded, entry);
    }

    #[test]
    fn query_defaults_match_all() {
        let query = AssetIndexQuery::default();
        let mut index = AssetIndex::new();
        index
            .insert(entry(
                "asset-image-1",
                AssetKind::Image,
                AssetProcessingStatus::Observed,
                AssetRelevance::High,
                None,
                vec![],
            ))
            .unwrap();
        index
            .insert(entry(
                "asset-pdf-1",
                AssetKind::Pdf,
                AssetProcessingStatus::Processed,
                AssetRelevance::Low,
                None,
                vec![],
            ))
            .unwrap();

        let results = index.query(&query);

        assert_eq!(results.len(), 2);
    }

    #[test]
    fn insert_and_get_asset_entry() {
        let mut index = AssetIndex::new();
        let entry = entry(
            "asset-image-1",
            AssetKind::Image,
            AssetProcessingStatus::Observed,
            AssetRelevance::High,
            None,
            vec![],
        );

        index.insert(entry.clone()).unwrap();

        assert_eq!(index.get(&AssetId::from("asset-image-1")), Some(&entry));
    }

    #[test]
    fn duplicate_insert_fails() {
        let mut index = AssetIndex::new();
        let entry = entry(
            "asset-image-1",
            AssetKind::Image,
            AssetProcessingStatus::Observed,
            AssetRelevance::High,
            None,
            vec![],
        );

        index.insert(entry.clone()).unwrap();
        let err = index.insert(entry).unwrap_err();

        assert_eq!(
            err,
            AssetIndexError::DuplicateAssetId(AssetId::from("asset-image-1"))
        );
    }

    #[test]
    fn upsert_replaces_existing_entry() {
        let mut index = AssetIndex::new();
        index
            .insert(entry(
                "asset-1",
                AssetKind::Image,
                AssetProcessingStatus::Observed,
                AssetRelevance::Low,
                None,
                vec![],
            ))
            .unwrap();
        let replacement = entry(
            "asset-1",
            AssetKind::Pdf,
            AssetProcessingStatus::Processed,
            AssetRelevance::Critical,
            Some("https://example.com/report.pdf"),
            vec!["report"],
        );

        index.upsert(replacement.clone());

        assert_eq!(index.get(&AssetId::from("asset-1")), Some(&replacement));
    }

    #[test]
    fn update_status_changes_existing_entry() {
        let mut index = AssetIndex::new();
        index
            .insert(entry(
                "asset-1",
                AssetKind::Image,
                AssetProcessingStatus::Observed,
                AssetRelevance::High,
                None,
                vec![],
            ))
            .unwrap();

        index
            .update_status(&AssetId::from("asset-1"), AssetProcessingStatus::Processed)
            .unwrap();

        assert_eq!(
            index.get(&AssetId::from("asset-1")).unwrap().status,
            AssetProcessingStatus::Processed
        );
    }

    #[test]
    fn update_status_missing_asset_fails() {
        let mut index = AssetIndex::new();

        let err = index
            .update_status(&AssetId::from("missing"), AssetProcessingStatus::Failed)
            .unwrap_err();

        assert_eq!(
            err,
            AssetIndexError::AssetNotFound(AssetId::from("missing"))
        );
    }

    #[test]
    fn query_filters_by_kind() {
        let index = sample_index();
        let results = index.query(&AssetIndexQuery {
            kind: Some(AssetKind::Pdf),
            ..Default::default()
        });

        assert_eq!(ids(results), vec!["asset-pdf-1"]);
    }

    #[test]
    fn query_filters_by_status() {
        let index = sample_index();
        let results = index.query(&AssetIndexQuery {
            status: Some(AssetProcessingStatus::Captured),
            ..Default::default()
        });

        assert_eq!(ids(results), vec!["asset-image-1"]);
    }

    #[test]
    fn query_filters_by_min_relevance() {
        let index = sample_index();
        let results = index.query(&AssetIndexQuery {
            min_relevance: Some(AssetRelevance::High),
            ..Default::default()
        });

        assert_eq!(ids(results), vec!["asset-image-1", "asset-pdf-1"]);
    }

    #[test]
    fn asset_index_entry_defaults_to_no_work_object_links() {
        let entry = entry(
            "asset-image-1",
            AssetKind::Image,
            AssetProcessingStatus::Observed,
            AssetRelevance::High,
            None,
            vec![],
        );

        assert!(entry.linked_work_objects.is_empty());
    }

    #[test]
    fn asset_index_entry_can_include_work_object_links() {
        let link = work_object_link(
            WorkObjectType::KnowledgeEntry,
            "knowledge-entry-1",
            AssetWorkObjectLinkReason::Evidence,
        );
        let entry = entry(
            "asset-image-1",
            AssetKind::Image,
            AssetProcessingStatus::Observed,
            AssetRelevance::High,
            None,
            vec![],
        )
        .with_linked_work_objects(vec![link.clone()]);

        assert_eq!(entry.linked_work_objects, vec![link]);
    }

    #[test]
    fn asset_index_can_link_asset_to_work_object() {
        let mut index = AssetIndex::new();
        index
            .insert(entry(
                "asset-image-1",
                AssetKind::Image,
                AssetProcessingStatus::Observed,
                AssetRelevance::High,
                None,
                vec![],
            ))
            .unwrap();
        let link = work_object_link(
            WorkObjectType::KnowledgeEntry,
            "knowledge-entry-1",
            AssetWorkObjectLinkReason::Evidence,
        );

        index
            .link_work_object(&AssetId::from("asset-image-1"), link.clone())
            .unwrap();

        assert_eq!(
            index
                .get(&AssetId::from("asset-image-1"))
                .unwrap()
                .linked_work_objects,
            vec![link]
        );
    }

    #[test]
    fn asset_index_rejects_duplicate_work_object_link() {
        let mut index = AssetIndex::new();
        index
            .insert(entry(
                "asset-image-1",
                AssetKind::Image,
                AssetProcessingStatus::Observed,
                AssetRelevance::High,
                None,
                vec![],
            ))
            .unwrap();
        let link = work_object_link(
            WorkObjectType::KnowledgeEntry,
            "knowledge-entry-1",
            AssetWorkObjectLinkReason::Evidence,
        );

        index
            .link_work_object(&AssetId::from("asset-image-1"), link.clone())
            .unwrap();
        let err = index
            .link_work_object(&AssetId::from("asset-image-1"), link)
            .unwrap_err();

        assert_eq!(
            err,
            AssetIndexError::DuplicateWorkObjectLink {
                asset_id: AssetId::from("asset-image-1"),
                work_object_type: WorkObjectType::KnowledgeEntry,
                work_object_id: WorkObjectId::from("knowledge-entry-1"),
            }
        );
    }

    #[test]
    fn link_work_object_missing_asset_fails() {
        let mut index = AssetIndex::new();
        let err = index
            .link_work_object(
                &AssetId::from("missing"),
                work_object_link(
                    WorkObjectType::KnowledgeEntry,
                    "knowledge-entry-1",
                    AssetWorkObjectLinkReason::Evidence,
                ),
            )
            .unwrap_err();

        assert_eq!(
            err,
            AssetIndexError::AssetNotFound(AssetId::from("missing"))
        );
    }

    #[test]
    fn query_filters_by_tag() {
        let index = sample_index();
        let results = index.query(&AssetIndexQuery {
            tag: Some("report".to_string()),
            ..Default::default()
        });

        assert_eq!(ids(results), vec!["asset-pdf-1"]);
    }

    #[test]
    fn query_filters_by_source_uri_prefix() {
        let index = sample_index();
        let results = index.query(&AssetIndexQuery {
            source_uri_prefix: Some("https://example.com/images".to_string()),
            ..Default::default()
        });

        assert_eq!(ids(results), vec!["asset-image-1"]);
    }

    #[test]
    fn query_filters_by_work_object_type() {
        let index = sample_index_with_work_object_links();
        let results = index.query(&AssetIndexQuery {
            work_object_type: Some(WorkObjectType::KnowledgeEntry),
            ..Default::default()
        });

        assert_eq!(ids(results), vec!["asset-image-1", "asset-pdf-1"]);
    }

    #[test]
    fn query_filters_by_work_object_id() {
        let index = sample_index_with_work_object_links();
        let results = index.query(&AssetIndexQuery {
            work_object_id: Some(WorkObjectId::from("project-agent-os")),
            ..Default::default()
        });

        assert_eq!(ids(results), vec!["asset-audio-1", "asset-image-1"]);
    }

    #[test]
    fn query_filters_by_work_object_type_and_id() {
        let index = sample_index_with_work_object_links();
        let results = index.query(&AssetIndexQuery {
            work_object_type: Some(WorkObjectType::KnowledgeEntry),
            work_object_id: Some(WorkObjectId::from("knowledge-entry-1")),
            ..Default::default()
        });

        assert_eq!(ids(results), vec!["asset-image-1"]);
    }

    #[test]
    fn query_results_remain_sorted_by_asset_id_with_work_object_filter() {
        let index = sample_index_with_work_object_links();
        let results = index.query(&AssetIndexQuery {
            work_object_type: Some(WorkObjectType::Project),
            ..Default::default()
        });

        assert_eq!(ids(results), vec!["asset-audio-1", "asset-image-1"]);
    }

    #[test]
    fn query_results_are_sorted_by_asset_id() {
        let mut index = AssetIndex::new();
        index
            .insert(entry(
                "asset-z",
                AssetKind::Image,
                AssetProcessingStatus::Observed,
                AssetRelevance::Low,
                None,
                vec![],
            ))
            .unwrap();
        index
            .insert(entry(
                "asset-a",
                AssetKind::Pdf,
                AssetProcessingStatus::Observed,
                AssetRelevance::Low,
                None,
                vec![],
            ))
            .unwrap();

        assert_eq!(
            ids(index.query(&AssetIndexQuery::default())),
            vec!["asset-a", "asset-z"]
        );
    }

    fn sample_index_with_work_object_links() -> AssetIndex {
        let mut index = sample_index();
        index
            .link_work_object(
                &AssetId::from("asset-image-1"),
                work_object_link(
                    WorkObjectType::KnowledgeEntry,
                    "knowledge-entry-1",
                    AssetWorkObjectLinkReason::Evidence,
                ),
            )
            .unwrap();
        index
            .link_work_object(
                &AssetId::from("asset-image-1"),
                work_object_link(
                    WorkObjectType::Project,
                    "project-agent-os",
                    AssetWorkObjectLinkReason::Related,
                ),
            )
            .unwrap();
        index
            .link_work_object(
                &AssetId::from("asset-pdf-1"),
                work_object_link(
                    WorkObjectType::KnowledgeEntry,
                    "knowledge-entry-2",
                    AssetWorkObjectLinkReason::Source,
                ),
            )
            .unwrap();
        index
            .link_work_object(
                &AssetId::from("asset-audio-1"),
                work_object_link(
                    WorkObjectType::Project,
                    "project-agent-os",
                    AssetWorkObjectLinkReason::Attachment,
                ),
            )
            .unwrap();
        index
    }

    fn work_object_link(
        work_object_type: WorkObjectType,
        work_object_id: &str,
        reason: AssetWorkObjectLinkReason,
    ) -> AssetWorkObjectLink {
        AssetWorkObjectLink::new(work_object_type, work_object_id, reason, ts(4))
    }

    fn sample_index() -> AssetIndex {
        let mut index = AssetIndex::new();
        index
            .insert(entry(
                "asset-image-1",
                AssetKind::Image,
                AssetProcessingStatus::Captured,
                AssetRelevance::High,
                Some("https://example.com/images/1.png"),
                vec!["screenshot", "ui"],
            ))
            .unwrap();
        index
            .insert(entry(
                "asset-pdf-1",
                AssetKind::Pdf,
                AssetProcessingStatus::Processed,
                AssetRelevance::Critical,
                Some("https://example.com/reports/1.pdf"),
                vec!["report"],
            ))
            .unwrap();
        index
            .insert(entry(
                "asset-audio-1",
                AssetKind::Audio,
                AssetProcessingStatus::Observed,
                AssetRelevance::Background,
                Some("https://cdn.example.com/audio/1.mp3"),
                vec!["transcript-source"],
            ))
            .unwrap();
        index
    }

    fn ids(entries: Vec<&AssetIndexEntry>) -> Vec<String> {
        entries
            .into_iter()
            .map(|entry| entry.metadata.id.to_string())
            .collect()
    }
}
