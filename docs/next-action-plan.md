# Next Action Plan — M22 PR136: Embedding / Semantic Search Boundary

Date: 2026-05-27
Repository: `/Users/yakii/code/agent-os/Infrastructure/connor-agent-core`
Reference roadmap: `/Users/yakii/notes/经过初审的创意/AgentOS/AgentOS-技术路线图-connor-agent-core.md`
Latest progress source: `docs/roadmap-progress.md`

## Current factual baseline

The external roadmap is useful for architectural direction, but the repository has advanced beyond the roadmap snapshot. The latest committed progress in `docs/roadmap-progress.md` shows:

- M21 PR128–PR133 completed browser-kernel hardening boundaries:
  - crash recovery
  - frame / iframe support
  - DOM snapshot artifacts
  - HAR-like network trace boundary
  - human takeover boundary
  - browser security policy
- M22 PR134 completed the knowledge index abstraction in `knowledge-entity`.
- M22 PR135 completed the deterministic in-process full-text backend.
- The explicit next planned step is **M22 PR136: Embedding / semantic search boundary**.

Therefore, the next action should not return to earlier roadmap PR75–PR77 kernel composition work. It should continue the current M22 knowledge retrieval line.

## Recommended next PR

### M22 PR136: Embedding / Semantic Search Boundary

Goal: introduce a stable, dependency-light semantic search boundary in `knowledge-entity` without selecting or integrating a real embedding provider yet.

This keeps the project aligned with the roadmap principles:

- Assistant-first retrieval foundation before app-level UX.
- Fake/deterministic implementation before real integrations.
- Small PR with isolated tests.
- No storage/provider lock-in before runtime/storage choices are finalized.

## Scope

Implement only boundary and deterministic fake behavior in `crates/knowledge-entity/src/lib.rs`.

Suggested public types:

- `KnowledgeEmbeddingVector`
  - owns `Vec<f32>`
  - validates non-empty vectors
  - validates finite values only, rejecting `NaN` / infinities
- `KnowledgeEmbeddingDocument`
  - `entry: KnowledgeEntryRef`
  - `embedding: KnowledgeEmbeddingVector`
  - optional `tags: Vec<String>`
  - optional `frontmatter: serde_json::Value`
- `KnowledgeSemanticQuery`
  - `embedding: KnowledgeEmbeddingVector`
  - `tags: Vec<String>`
  - `frontmatter_filters: BTreeMap<String, String>`
  - `limit: usize`
- `KnowledgeEmbeddingRebuildRequest`
- `KnowledgeEmbeddingRebuildReport`
- `KnowledgeEmbeddingIndex` async trait
  - `upsert_embedding(...)`
  - `delete_embedding(...)`
  - `semantic_query(...)`
  - `rebuild_embeddings(...)`
- `KnowledgeEmbeddingBackendKind`
  - initially `DeterministicInProcess`
- `DeterministicEmbeddingKnowledgeBackend`
  - in-memory deterministic cosine-similarity backend
- alias `MemorySemanticKnowledgeIndex = DeterministicEmbeddingKnowledgeBackend`

Reuse existing `KnowledgeIndexError` unless a truly new error variant is needed. Prefer adding variants such as:

- `InvalidEmbedding(String)`
- `DimensionMismatch { expected: usize, actual: usize }`

## Non-goals

Do not implement in PR136:

- real embedding model adapter calls
- OpenAI / Anthropic / local embedding API integration
- Tantivy / SQLite vector index / HNSW / ANN backend
- hybrid full-text + vector rank fusion
- persistence format for vectors
- runtime ingestion pipeline
- changes to `agentos-kernel` composition

These should remain future PRs after the semantic boundary is stable.

## Test-first checklist

Add failing tests first in `crates/knowledge-entity/src/lib.rs` test module.

Recommended tests:

1. `knowledge_embedding_vector_rejects_empty_vectors`
2. `knowledge_embedding_vector_rejects_non_finite_values`
3. `knowledge_semantic_query_defaults_are_stable`
4. `knowledge_embedding_backend_kind_defaults_to_deterministic_in_process`
5. `memory_semantic_index_ranks_by_cosine_similarity`
6. `memory_semantic_index_filters_by_tags_and_frontmatter`
7. `memory_semantic_index_rejects_dimension_mismatch`
8. `memory_semantic_index_upserts_and_deletes_embeddings`
9. `memory_semantic_index_rebuilds_embeddings`
10. `semantic_ranking_ties_break_by_entry_id`

## Implementation steps

1. Add embedding vector/query/document/rebuild types near the existing full-text index types.
2. Add validation helpers for vector non-empty, finite values, and matching dimensions.
3. Add `KnowledgeEmbeddingIndex` trait.
4. Add `KnowledgeEmbeddingBackendKind` with deterministic default.
5. Implement `DeterministicEmbeddingKnowledgeBackend` with:
   - in-memory `HashMap<KnowledgeEntryId, KnowledgeEmbeddingDocument>`
   - cosine similarity
   - tag and frontmatter filters matching the existing full-text behavior
   - deterministic sorting by score descending, then entry id ascending
6. Add `MemorySemanticKnowledgeIndex` alias.
7. Update tests until `cargo test -p knowledge-entity semantic` passes.
8. Run broader verification.
9. Update `docs/roadmap-progress.md` with PR136 completion summary after implementation.
10. Commit implementation.

## Verification commands for PR136

```bash
cargo fmt --all --check
cargo test -p knowledge-entity semantic
cargo test -p knowledge-entity embedding
cargo test -p knowledge-entity
cargo clippy --workspace -- -D warnings
```

Before claiming completion, run the full fresh verification and include command output in the final PR/commit note.

## Suggested follow-up PRs

After PR136 lands:

1. **M22 PR137: Hybrid search query boundary**
   - Combine full-text and semantic query requests at the type level.
   - Add deterministic weighted fusion fake, but still no provider/storage dependency.
2. **M22 PR138: Knowledge retrieval service boundary**
   - Provide a higher-level service that can choose full-text, semantic, or hybrid.
   - Keep it independent from `agentos-kernel` until behavior is stable.
3. **M22 PR139: Kernel registry integration for knowledge retrieval**
   - Register retrieval service in the kernel service registry.
   - Add diagnostics/health reporting.

## Recommended immediate next command

Start PR136 with the RED tests:

```bash
cargo test -p knowledge-entity semantic
```

Then add the first failing test and proceed through the TDD cycle.
