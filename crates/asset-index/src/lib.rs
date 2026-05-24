//! # Asset Index
//!
//! Deterministic in-memory index for AgentOS asset metadata.

use asset_core::{AssetId, AssetKind, AssetMetadata, AssetProcessingStatus, AssetRelevance};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// A deterministic index entry for one asset.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AssetIndexEntry {
    pub metadata: AssetMetadata,
    pub status: AssetProcessingStatus,
    pub indexed_at: DateTime<Utc>,
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
        }
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
}

/// Errors from asset index operations.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum AssetIndexError {
    #[error("duplicate asset id: {0}")]
    DuplicateAssetId(AssetId),
    #[error("asset not found: {0}")]
    AssetNotFound(AssetId),
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

        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use asset_core::{
        AssetId, AssetKind, AssetMetadata, AssetProcessingStatus, AssetRelevance, AssetSource,
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
