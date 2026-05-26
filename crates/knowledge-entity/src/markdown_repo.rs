//! Filesystem-backed knowledge repository using Markdown files with YAML frontmatter.
//!
//! Each knowledge entry is stored as a `.md` file under a configurable root directory.
//! The `KnowledgeEntryId` corresponds to the relative path (without `.md` extension)
//! from the root directory, e.g. `KnowledgeEntryId("frameworks/blue-ocean-strategy")`
//! maps to `{root_dir}/frameworks/blue-ocean-strategy.md`.
//!
//! File format:
//! ```markdown
//! ---
//! title: "Entry Title"
//! summary: "A brief summary..."
//! tags: [tag1, tag2]
//! source: "https://example.com"
//! author: "Author Name"
//! last_updated: 2026-05-24
//! ---
//!
//! # Entry Title
//!
//! Body content in Markdown.
//! ```

use crate::{
    KnowledgeEntryDraft, KnowledgeEntryId, KnowledgeEntryRef, KnowledgeRepository,
    KnowledgeRepositoryError, KnowledgeSearchQuery, KnowledgeSearchResult,
};
use async_trait::async_trait;
use chrono::{NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use tokio::fs;

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

/// Errors specific to the Markdown filesystem repository.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum MarkdownRepoError {
    #[error("io error: {0}")]
    Io(String),
    #[error("frontmatter parse error in {path}: {reason}")]
    FrontmatterParse { path: String, reason: String },
    #[error("entry already exists: {0}")]
    EntryExists(String),
    #[error("invalid entry id (path traversal detected): {0}")]
    InvalidId(String),
}

impl From<MarkdownRepoError> for KnowledgeRepositoryError {
    fn from(err: MarkdownRepoError) -> Self {
        match err {
            MarkdownRepoError::Io(msg) => KnowledgeRepositoryError::Io(msg),
            MarkdownRepoError::FrontmatterParse { path, reason } => {
                KnowledgeRepositoryError::FrontmatterParse { path, reason }
            }
            MarkdownRepoError::EntryExists(id) => KnowledgeRepositoryError::EntryExists(id),
            MarkdownRepoError::InvalidId(msg) => KnowledgeRepositoryError::InvalidId(msg),
        }
    }
}

// ---------------------------------------------------------------------------
// Frontmatter types
// ---------------------------------------------------------------------------

/// Parsed YAML frontmatter from a Markdown file.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct Frontmatter {
    title: String,
    #[serde(default)]
    summary: String,
    #[serde(default)]
    category: String,
    #[serde(default)]
    tags: Vec<String>,
    #[serde(default)]
    industry: String,
    #[serde(default)]
    source: String,
    #[serde(default)]
    author: String,
    #[serde(default)]
    related: Vec<String>,
    #[serde(default)]
    applicable_to: Vec<String>,
    #[serde(default)]
    last_updated: Option<String>,
    /// Catch-all for extra v2 metadata fields.
    #[serde(flatten)]
    extra: HashMap<String, serde_json::Value>,
}

// ---------------------------------------------------------------------------
// MarkdownKnowledgeRepository
// ---------------------------------------------------------------------------

/// Filesystem-backed knowledge repository.
///
/// Stores entries as `{root_dir}/{id}.md` with YAML frontmatter.
#[derive(Debug, Clone)]
pub struct MarkdownKnowledgeRepository {
    root_dir: PathBuf,
}

impl MarkdownKnowledgeRepository {
    /// Create a new repository rooted at the given directory.
    pub fn new(root_dir: impl Into<PathBuf>) -> Self {
        Self {
            root_dir: root_dir.into(),
        }
    }

    /// The root directory of this repository.
    pub fn root_dir(&self) -> &Path {
        &self.root_dir
    }

    /// Resolve an entry id to its file path.
    fn entry_path(&self, id: &KnowledgeEntryId) -> PathBuf {
        self.root_dir.join(format!("{}.md", id.0))
    }

    /// Validate that an entry id does not attempt path traversal.
    fn validate_id(id: &KnowledgeEntryId) -> Result<(), MarkdownRepoError> {
        let path = Path::new(&id.0);
        if path.is_absolute() {
            return Err(MarkdownRepoError::InvalidId(format!(
                "id must be relative: {}",
                id.0
            )));
        }
        // Reject any component that is ".." to prevent traversal.
        for component in path.components() {
            if let std::path::Component::ParentDir = component {
                return Err(MarkdownRepoError::InvalidId(format!(
                    "id must not contain '..': {}",
                    id.0
                )));
            }
        }
        Ok(())
    }

    /// Parse a Markdown file into (Frontmatter, body).
    async fn parse_file(path: &Path) -> Result<(Frontmatter, String), MarkdownRepoError> {
        let content = fs::read_to_string(path).await.map_err(|e| {
            MarkdownRepoError::Io(format!("failed to read {}: {}", path.display(), e))
        })?;
        Self::parse_content(&content, &path.display().to_string())
    }

    /// Parse raw file content into (Frontmatter, body).
    fn parse_content(
        content: &str,
        path_display: &str,
    ) -> Result<(Frontmatter, String), MarkdownRepoError> {
        let stripped = content.trim_start();
        if !stripped.starts_with("---") {
            return Err(MarkdownRepoError::FrontmatterParse {
                path: path_display.to_string(),
                reason: "file does not start with YAML frontmatter delimiter '---'".to_string(),
            });
        }

        // Find the closing ---
        let after_opening = &stripped[3..];
        let closing =
            after_opening
                .find("\n---")
                .ok_or_else(|| MarkdownRepoError::FrontmatterParse {
                    path: path_display.to_string(),
                    reason: "missing closing '---' delimiter for YAML frontmatter".to_string(),
                })?;

        let yaml_str = &after_opening[..closing];
        let body_start = closing + 4; // skip "\n---"
        let body = after_opening[body_start..]
            .trim_start_matches('\n')
            .to_string();

        let frontmatter: Frontmatter =
            serde_yml::from_str(yaml_str).map_err(|e| MarkdownRepoError::FrontmatterParse {
                path: path_display.to_string(),
                reason: format!("YAML parse error: {}", e),
            })?;

        Ok((frontmatter, body))
    }

    /// Build a `KnowledgeEntryRef` from parsed frontmatter and id.
    fn entry_ref_from_frontmatter(id: KnowledgeEntryId, fm: &Frontmatter) -> KnowledgeEntryRef {
        let created_at = fm
            .last_updated
            .as_deref()
            .and_then(|s| NaiveDate::parse_from_str(s, "%Y-%m-%d").ok())
            .map(|d| {
                d.and_hms_opt(0, 0, 0)
                    .unwrap()
                    .and_local_timezone(Utc)
                    .unwrap()
            })
            .unwrap_or_else(Utc::now);

        let source_uri = if fm.source.is_empty() {
            None
        } else {
            Some(fm.source.clone())
        };

        KnowledgeEntryRef {
            id,
            title: fm.title.clone(),
            source_uri,
            artifact_id: None,
            asset_id: None,
            created_at,
        }
    }

    /// Serialize a draft to Markdown with YAML frontmatter.
    fn serialize_draft(
        id: &KnowledgeEntryId,
        draft: &KnowledgeEntryDraft,
        category: &str,
    ) -> String {
        let mut fm = Frontmatter {
            title: draft.title.clone(),
            summary: draft
                .metadata
                .get("summary")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
            category: category.to_string(),
            tags: draft.tags.clone(),
            industry: draft
                .metadata
                .get("industry")
                .and_then(|v| v.as_str())
                .unwrap_or("general")
                .to_string(),
            source: draft.source_uri.clone().unwrap_or_default(),
            author: draft
                .metadata
                .get("author")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
            related: draft
                .metadata
                .get("related")
                .and_then(|v| v.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|v| v.as_str().map(String::from))
                        .collect()
                })
                .unwrap_or_default(),
            applicable_to: draft
                .metadata
                .get("applicable_to")
                .and_then(|v| v.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|v| v.as_str().map(String::from))
                        .collect()
                })
                .unwrap_or_default(),
            last_updated: Some(draft.created_at.format("%Y-%m-%d").to_string()),
            extra: HashMap::new(),
        };

        // Extract category from id if not in metadata.
        if fm.category.is_empty()
            && let Some(parent) = Path::new(&id.0).parent()
        {
            let cat = parent.to_string_lossy().to_string();
            if !cat.is_empty() && cat != "." {
                fm.category = cat;
            }
        }

        let yaml =
            serde_yml::to_string(&fm).unwrap_or_else(|_| "---\ntitle: unknown\n---\n".to_string());

        format!(
            "---\n{}\n---\n\n{}",
            yaml.trim_end_matches('\n'),
            draft.content_markdown
        )
    }

    /// Walk the root directory recursively and collect all `.md` file paths.
    async fn walk_md_files(dir: &Path) -> Result<Vec<PathBuf>, MarkdownRepoError> {
        let mut results = Vec::new();
        Self::walk_md_files_inner(dir, &mut results).await?;
        results.sort();
        Ok(results)
    }

    fn walk_md_files_inner<'a>(
        dir: &'a Path,
        results: &'a mut Vec<PathBuf>,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<(), MarkdownRepoError>> + Send + 'a>,
    > {
        Box::pin(async move {
            let mut entries = fs::read_dir(dir).await.map_err(|e| {
                MarkdownRepoError::Io(format!("failed to read dir {}: {}", dir.display(), e))
            })?;

            while let Some(entry) = entries.next_entry().await.map_err(|e| {
                MarkdownRepoError::Io(format!("failed to read entry in {}: {}", dir.display(), e))
            })? {
                let path = entry.path();
                let file_type = entry.file_type().await.map_err(|e| {
                    MarkdownRepoError::Io(format!(
                        "failed to read file type {}: {}",
                        path.display(),
                        e
                    ))
                })?;

                if file_type.is_dir() {
                    // Skip hidden directories (e.g., .git, _system).
                    let name = path
                        .file_name()
                        .map(|n| n.to_string_lossy().to_string())
                        .unwrap_or_default();
                    if name.starts_with('.') || name.starts_with('_') {
                        continue;
                    }
                    Self::walk_md_files_inner(&path, results).await?;
                } else if file_type.is_file()
                    && path.extension().map(|ext| ext == "md").unwrap_or(false)
                {
                    results.push(path);
                }
            }

            Ok(())
        })
    }

    /// Convert a file path to a `KnowledgeEntryId` relative to root_dir.
    fn path_to_id(&self, path: &Path) -> Option<KnowledgeEntryId> {
        let relative = path.strip_prefix(&self.root_dir).ok()?;
        let without_ext = relative.with_extension("");
        let id_str = without_ext.to_string_lossy().to_string();
        if id_str.is_empty() {
            None
        } else {
            // Normalize path separators to forward slash for cross-platform consistency.
            Some(KnowledgeEntryId::from(id_str.replace('\\', "/")))
        }
    }

    /// Simple text match: case-insensitive substring in title or body.
    fn text_matches(text: &str, title: &str, body: &str) -> bool {
        let text_lower = text.to_lowercase();
        title.to_lowercase().contains(&text_lower) || body.to_lowercase().contains(&text_lower)
    }

    /// Check if all required tags are present in the entry's tags.
    fn tags_match(required: &[String], entry_tags: &[String]) -> bool {
        required
            .iter()
            .all(|tag| entry_tags.iter().any(|candidate| candidate == tag))
    }
}

#[async_trait]
impl KnowledgeRepository for MarkdownKnowledgeRepository {
    async fn save_draft(
        &self,
        draft: KnowledgeEntryDraft,
    ) -> Result<KnowledgeEntryRef, KnowledgeRepositoryError> {
        // Derive id from title: use a slugified version.
        let id = KnowledgeEntryId::from(slugify(&draft.title));
        Self::validate_id(&id).map_err(KnowledgeRepositoryError::from)?;

        let path = self.entry_path(&id);

        // Check if entry already exists.
        if path.exists() {
            return Err(MarkdownRepoError::EntryExists(id.0.to_string()).into());
        }

        // Derive category from metadata or id path.
        let category = draft
            .metadata
            .get("category")
            .and_then(|v| v.as_str())
            .unwrap_or("");

        let content = Self::serialize_draft(&id, &draft, category);

        // Ensure parent directories exist.
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).await.map_err(|e| {
                MarkdownRepoError::Io(format!("failed to create dir {}: {}", parent.display(), e))
            })?;
        }

        // Atomic write: temp file → rename.
        let dir = path.parent().unwrap_or(&self.root_dir);
        let mut tmp = tempfile::NamedTempFile::new_in(dir)
            .map_err(|e| MarkdownRepoError::Io(format!("failed to create temp file: {}", e)))?;
        std::io::Write::write_all(&mut tmp, content.as_bytes())
            .map_err(|e| MarkdownRepoError::Io(format!("failed to write temp file: {}", e)))?;
        tmp.persist(&path)
            .map_err(|e| MarkdownRepoError::Io(format!("failed to persist temp file: {}", e)))?;

        Ok(KnowledgeEntryRef {
            id,
            title: draft.title,
            source_uri: draft.source_uri,
            artifact_id: draft.source_artifact_id,
            asset_id: draft.source_asset_id,
            created_at: draft.created_at,
        })
    }

    async fn get_entry(
        &self,
        id: &KnowledgeEntryId,
    ) -> Result<Option<KnowledgeEntryRef>, KnowledgeRepositoryError> {
        let path = self.entry_path(id);
        if !path.exists() {
            return Ok(None);
        }
        let (fm, _) = Self::parse_file(&path).await?;
        Ok(Some(Self::entry_ref_from_frontmatter(id.clone(), &fm)))
    }

    async fn search(
        &self,
        query: &KnowledgeSearchQuery,
    ) -> Result<Vec<KnowledgeSearchResult>, KnowledgeRepositoryError> {
        let files = Self::walk_md_files(&self.root_dir).await?;
        let mut results = Vec::new();

        for path in files {
            let id = match self.path_to_id(&path) {
                Some(id) => id,
                None => continue,
            };

            let (fm, body) = match Self::parse_file(&path).await {
                Ok(pair) => pair,
                Err(_) => continue, // Skip unparseable files.
            };

            let matches_text =
                query.text.is_empty() || Self::text_matches(&query.text, &fm.title, &body);
            let matches_tags = Self::tags_match(&query.tags, &fm.tags);

            if matches_text && matches_tags {
                let score = if query.text.is_empty() {
                    0.0
                } else {
                    // Simple scoring: title match worth more than body match.
                    let title_match = fm.title.to_lowercase().contains(&query.text.to_lowercase());
                    if title_match { 2.0 } else { 1.0 }
                };

                let snippet = body.chars().take(200).collect::<String>();
                let snippet = if snippet.is_empty() {
                    None
                } else {
                    Some(snippet)
                };

                results.push(KnowledgeSearchResult {
                    entry: Self::entry_ref_from_frontmatter(id, &fm),
                    score,
                    snippet,
                    permission_required: false,
                    confidentiality: None,
                });
            }
        }

        // Sort by score descending, then by id for determinism.
        results.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| a.entry.id.0.cmp(&b.entry.id.0))
        });
        results.truncate(query.limit);
        Ok(results)
    }

    async fn list_entries(&self) -> Result<Vec<KnowledgeEntryRef>, KnowledgeRepositoryError> {
        let files = Self::walk_md_files(&self.root_dir).await?;
        let mut entries = Vec::new();

        for path in files {
            let id = match self.path_to_id(&path) {
                Some(id) => id,
                None => continue,
            };

            let (fm, _) = match Self::parse_file(&path).await {
                Ok(pair) => pair,
                Err(_) => continue,
            };

            entries.push(Self::entry_ref_from_frontmatter(id, &fm));
        }

        // Sort by id for determinism.
        entries.sort_by(|a, b| a.id.0.cmp(&b.id.0));
        Ok(entries)
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Slugify a title into a filesystem-safe id segment.
///
/// Rules:
/// - Convert to lowercase.
/// - Replace whitespace and non-alphanumeric (except `-` and `_`) with `-`.
/// - Collapse consecutive `-`.
/// - Trim leading/trailing `-`.
/// - If the result is empty, use "untitled".
fn slugify(title: &str) -> String {
    let slug: String = title
        .to_lowercase()
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '-'
            }
        })
        .collect();

    // Collapse consecutive dashes.
    let mut collapsed = String::new();
    let mut prev_dash = false;
    for c in slug.chars() {
        if c == '-' {
            if !prev_dash {
                collapsed.push(c);
            }
            prev_dash = true;
        } else {
            collapsed.push(c);
            prev_dash = false;
        }
    }

    let result = collapsed.trim_matches('-').to_string();
    if result.is_empty() {
        "untitled".to_string()
    } else {
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slugify_basic() {
        assert_eq!(slugify("Blue Ocean Strategy"), "blue-ocean-strategy");
    }

    #[test]
    fn slugify_chinese() {
        // Chinese chars are alphanumeric in Unicode, so they pass through.
        let slug = slugify("蓝海战略");
        assert!(!slug.is_empty());
    }

    #[test]
    fn slugify_special_chars() {
        assert_eq!(slugify("Hello, World! @#$"), "hello-world");
    }

    #[test]
    fn slugify_empty() {
        assert_eq!(slugify(""), "untitled");
        assert_eq!(slugify("@#$%"), "untitled");
    }

    #[test]
    fn slugify_preserves_dashes() {
        assert_eq!(slugify("context-dedupe-gate"), "context-dedupe-gate");
    }

    #[test]
    fn parse_content_valid() {
        let content = r#"---
title: "Test Entry"
summary: "A test summary"
tags: [test, example]
source: "https://example.com"
last_updated: "2026-05-24"
---

# Test Entry

Body content here."#;

        let (fm, body) = MarkdownKnowledgeRepository::parse_content(content, "test.md").unwrap();
        assert_eq!(fm.title, "Test Entry");
        assert_eq!(fm.summary, "A test summary");
        assert_eq!(fm.tags, vec!["test", "example"]);
        assert_eq!(fm.source, "https://example.com");
        assert_eq!(fm.last_updated, Some("2026-05-24".to_string()));
        assert!(body.contains("Body content here."));
    }

    #[test]
    fn parse_content_missing_closing_delimiter() {
        let content = r#"---
title: "Test"
no closing delimiter"#;

        let result = MarkdownKnowledgeRepository::parse_content(content, "test.md");
        assert!(result.is_err());
    }

    #[test]
    fn parse_content_no_frontmatter() {
        let content = "# Just a heading\n\nNo frontmatter here.";
        let result = MarkdownKnowledgeRepository::parse_content(content, "test.md");
        assert!(result.is_err());
    }

    #[test]
    fn serialize_draft_roundtrip() {
        let ts = "2026-05-24T12:00:00Z".parse().unwrap();
        let draft = KnowledgeEntryDraft::new("Test Title", "# Content\n\nBody.", ts)
            .with_tags(vec!["test".to_string()])
            .with_metadata(serde_json::json!({
                "category": "test-category",
                "summary": "A test summary",
                "industry": "general"
            }));
        let id = KnowledgeEntryId::from("test-title");
        let serialized = MarkdownKnowledgeRepository::serialize_draft(&id, &draft, "test-category");

        let (fm, body) =
            MarkdownKnowledgeRepository::parse_content(&serialized, "test-title.md").unwrap();
        assert_eq!(fm.title, "Test Title");
        assert_eq!(fm.category, "test-category");
        assert_eq!(fm.tags, vec!["test"]);
        assert!(body.contains("# Content"));
    }

    #[test]
    fn validate_id_accepts_relative() {
        assert!(
            MarkdownKnowledgeRepository::validate_id(&KnowledgeEntryId::from("foo/bar")).is_ok()
        );
    }

    #[test]
    fn validate_id_rejects_traversal() {
        assert!(
            MarkdownKnowledgeRepository::validate_id(&KnowledgeEntryId::from("../etc/passwd"))
                .is_err()
        );
        assert!(
            MarkdownKnowledgeRepository::validate_id(&KnowledgeEntryId::from("foo/../../../etc"))
                .is_err()
        );
    }

    #[test]
    fn text_matches_case_insensitive() {
        assert!(MarkdownKnowledgeRepository::text_matches(
            "agent",
            "AgentOS Architecture",
            "some body"
        ));
        assert!(MarkdownKnowledgeRepository::text_matches(
            "BODY",
            "Title",
            "some body content"
        ));
        assert!(!MarkdownKnowledgeRepository::text_matches(
            "missing", "Title", "Body"
        ));
    }

    #[test]
    fn tags_match_all_required() {
        assert!(MarkdownKnowledgeRepository::tags_match(
            &["a".to_string(), "b".to_string()],
            &["a".to_string(), "b".to_string(), "c".to_string()]
        ));
        assert!(!MarkdownKnowledgeRepository::tags_match(
            &["a".to_string(), "missing".to_string()],
            &["a".to_string(), "b".to_string()]
        ));
    }

    // ----- Async integration tests -----

    fn ts() -> chrono::DateTime<Utc> {
        "2026-05-24T12:00:00Z".parse().unwrap()
    }

    fn make_draft(title: &str, content: &str, tags: Vec<&str>) -> KnowledgeEntryDraft {
        KnowledgeEntryDraft::new(title, content, ts())
            .with_source_uri("https://example.com/source")
            .with_tags(tags.into_iter().map(str::to_string).collect())
            .with_metadata(serde_json::json!({
                "category": "test-category",
                "summary": "A test summary",
                "industry": "general"
            }))
    }

    #[tokio::test]
    async fn markdown_repo_saves_and_gets_entry() {
        let dir = tempfile::tempdir().unwrap();
        let repo = MarkdownKnowledgeRepository::new(dir.path());

        let draft = make_draft("Test Entry", "# Test\n\nBody content.", vec!["test"]);
        let entry_ref = repo.save_draft(draft).await.unwrap();

        assert!(!entry_ref.id.0.is_empty());
        assert_eq!(entry_ref.title, "Test Entry");

        let retrieved = repo.get_entry(&entry_ref.id).await.unwrap();
        assert!(retrieved.is_some());
        let retrieved = retrieved.unwrap();
        assert_eq!(retrieved.title, "Test Entry");
        assert_eq!(
            retrieved.source_uri,
            Some("https://example.com/source".to_string())
        );
    }

    #[tokio::test]
    async fn markdown_repo_generates_valid_frontmatter() {
        let dir = tempfile::tempdir().unwrap();
        let repo = MarkdownKnowledgeRepository::new(dir.path());

        let draft = make_draft("Frontmatter Test", "# Content", vec!["fm-test"]);
        let entry_ref = repo.save_draft(draft).await.unwrap();

        let file_path = repo.entry_path(&entry_ref.id);
        let raw = std::fs::read_to_string(&file_path).unwrap();
        assert!(raw.starts_with("---\n"));
        assert!(raw.contains("title:"));
        assert!(raw.contains("Frontmatter Test"));
        assert!(raw.contains("tags:"));
        assert!(raw.contains("fm-test"));

        // Verify it's parseable.
        let (fm, body) = MarkdownKnowledgeRepository::parse_file(&file_path)
            .await
            .unwrap();
        assert_eq!(fm.title, "Frontmatter Test");
        assert!(body.contains("# Content"));
    }

    #[tokio::test]
    async fn markdown_repo_lists_entries_sorted() {
        let dir = tempfile::tempdir().unwrap();
        let repo = MarkdownKnowledgeRepository::new(dir.path());

        repo.save_draft(make_draft("Beta Entry", "Body", vec![]))
            .await
            .unwrap();
        repo.save_draft(make_draft("Alpha Entry", "Body", vec![]))
            .await
            .unwrap();
        repo.save_draft(make_draft("Gamma Entry", "Body", vec![]))
            .await
            .unwrap();

        let entries = repo.list_entries().await.unwrap();
        assert_eq!(entries.len(), 3);
        // Sorted by id.
        assert!(entries[0].id.0 <= entries[1].id.0);
        assert!(entries[1].id.0 <= entries[2].id.0);
    }

    #[tokio::test]
    async fn markdown_repo_searches_by_title() {
        let dir = tempfile::tempdir().unwrap();
        let repo = MarkdownKnowledgeRepository::new(dir.path());

        repo.save_draft(make_draft(
            "Blue Ocean Strategy",
            "Market innovation",
            vec![],
        ))
        .await
        .unwrap();
        repo.save_draft(make_draft("Red Ocean Strategy", "Competition", vec![]))
            .await
            .unwrap();

        let query = KnowledgeSearchQuery::new("Blue Ocean");
        let results = repo.search(&query).await.unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].entry.title, "Blue Ocean Strategy");
        assert!(results[0].score > 0.0);
    }

    #[tokio::test]
    async fn markdown_repo_searches_by_content() {
        let dir = tempfile::tempdir().unwrap();
        let repo = MarkdownKnowledgeRepository::new(dir.path());

        repo.save_draft(make_draft(
            "Strategy A",
            "Innovation in market space",
            vec![],
        ))
        .await
        .unwrap();
        repo.save_draft(make_draft(
            "Strategy B",
            "Competition and efficiency",
            vec![],
        ))
        .await
        .unwrap();

        let query = KnowledgeSearchQuery::new("innovation");
        let results = repo.search(&query).await.unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].entry.title, "Strategy A");
    }

    #[tokio::test]
    async fn markdown_repo_searches_by_tags() {
        let dir = tempfile::tempdir().unwrap();
        let repo = MarkdownKnowledgeRepository::new(dir.path());

        repo.save_draft(make_draft("Tagged A", "Body", vec!["strategy", "business"]))
            .await
            .unwrap();
        repo.save_draft(make_draft("Tagged B", "Body", vec!["tech"]))
            .await
            .unwrap();

        let query = KnowledgeSearchQuery::new("").with_tags(vec!["strategy".to_string()]);
        let results = repo.search(&query).await.unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].entry.title, "Tagged A");
    }

    #[tokio::test]
    async fn markdown_repo_get_entry_returns_none_for_missing() {
        let dir = tempfile::tempdir().unwrap();
        let repo = MarkdownKnowledgeRepository::new(dir.path());

        let result = repo
            .get_entry(&KnowledgeEntryId::from("nonexistent"))
            .await
            .unwrap();
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn markdown_repo_save_rejects_existing_entry() {
        let dir = tempfile::tempdir().unwrap();
        let repo = MarkdownKnowledgeRepository::new(dir.path());

        repo.save_draft(make_draft("Duplicate Test", "Body", vec![]))
            .await
            .unwrap();
        let result = repo
            .save_draft(make_draft("Duplicate Test", "Another body", vec![]))
            .await;
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(matches!(err, KnowledgeRepositoryError::EntryExists(_)));
    }

    #[tokio::test]
    async fn markdown_repo_roundtrip_preserves_all_fields() {
        let dir = tempfile::tempdir().unwrap();
        let repo = MarkdownKnowledgeRepository::new(dir.path());

        let ts_val = ts();
        let draft =
            KnowledgeEntryDraft::new("Full Fields", "# Full\n\nAll fields present.", ts_val)
                .with_source_uri("https://example.com/full")
                .with_tags(vec!["full".to_string(), "roundtrip".to_string()])
                .with_metadata(serde_json::json!({
                    "category": "roundtrip-category",
                    "summary": "Testing all fields roundtrip",
                    "industry": "technology",
                    "author": "Test Author",
                    "related": ["other-entry.md"],
                    "applicable_to": ["testing"]
                }));

        let entry_ref = repo.save_draft(draft).await.unwrap();
        assert_eq!(entry_ref.title, "Full Fields");
        assert_eq!(
            entry_ref.source_uri,
            Some("https://example.com/full".to_string())
        );
        assert_eq!(entry_ref.created_at, ts_val);

        // Read back and verify frontmatter.
        let file_path = repo.entry_path(&entry_ref.id);
        let (fm, body) = MarkdownKnowledgeRepository::parse_file(&file_path)
            .await
            .unwrap();
        assert_eq!(fm.title, "Full Fields");
        assert_eq!(fm.category, "roundtrip-category");
        assert_eq!(fm.industry, "technology");
        assert_eq!(fm.author, "Test Author");
        assert_eq!(fm.source, "https://example.com/full");
        assert_eq!(fm.tags, vec!["full", "roundtrip"]);
        assert!(body.contains("# Full"));
    }
}
