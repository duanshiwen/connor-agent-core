# Roadmap Progress

## 2026-05-27 — M22 PR136: Embedding / semantic search boundary

Status: implemented.

### Scope

- Added the first stable semantic search boundary in `knowledge-entity`:
  - `KnowledgeEmbeddingVector`
  - `KnowledgeEmbeddingDocument`
  - `KnowledgeSemanticQuery`
  - `KnowledgeEmbeddingRebuildRequest`
  - `KnowledgeEmbeddingIndex` trait
- Added typed embedding/index validation:
  - empty embedding vectors are rejected
  - non-finite values (`NaN` / infinities) are rejected
  - mismatched vector dimensions return a typed `DimensionMismatch` error
- Added `KnowledgeEmbeddingBackendKind::DeterministicInProcess` as the initial backend selection boundary.
- Added deterministic in-memory semantic backend:
  - `DeterministicEmbeddingKnowledgeBackend`
  - `MemorySemanticKnowledgeIndex` alias
- Implemented cosine-similarity ranking with deterministic tie-breaking by entry id.
- Reused the existing `KnowledgeSearchResult` shape so callers can consume full-text and semantic results through the same lightweight result type.
- Kept real embedding model calls, vector persistence, ANN backends, and hybrid rank fusion out of this PR.

### Acceptance coverage

- Embedding vectors reject empty and non-finite values.
- Semantic query defaults are stable.
- Embedding backend kind defaults to deterministic in-process.
- Memory semantic index ranks by cosine similarity.
- Memory semantic index filters by tags and frontmatter.
- Memory semantic index rejects dimension mismatch.
- Memory semantic index supports upsert, delete, and rebuild flows.
- Semantic ranking ties break by stable entry id.

### Verification commands

```bash
cargo test -p knowledge-entity semantic
cargo test -p knowledge-entity embedding
cargo test -p knowledge-entity
cargo fmt --all --check
cargo clippy --workspace -- -D warnings
```

### Next planned step

M22 PR137: Hybrid search query boundary.

## 2026-05-26 — M22 PR135: Full-text search backend

Status: implemented.

### Scope

- Chose and implemented the first full-text backend as a dependency-free deterministic in-process backend:
  - `KnowledgeFullTextBackendKind::DeterministicInProcess`
  - `DeterministicFullTextKnowledgeBackend`
- Indexed title/body/tags/frontmatter through one backend implementation shared with the PR134 fake index boundary.
- Added weighted deterministic ranking:
  - title matches outrank body matches
  - tag matches contribute to score
  - frontmatter matches contribute to score
  - score ties break by stable entry id
- Kept Tantivy/SQLite FTS out of this PR to avoid introducing a storage dependency before the broader runtime/storage choice is made; the backend kind documents the current first implementation choice.

### Acceptance coverage

- Backend kind defaults to deterministic in-process full-text backend.
- Title matches rank above body-only matches for the same query term.
- Backend indexes title, body, tags, and frontmatter text.
- Ranking is deterministic when scores tie by sorting by entry id.
- Existing memory/full-text fake index tests continue to pass through the shared backend implementation.

### Verification commands

```bash
cargo test -p knowledge-entity deterministic_fulltext_backend
cargo test -p knowledge-entity memory_fulltext_index
cargo test -p knowledge-entity knowledge_fulltext_backend_kind
cargo test -p knowledge-entity
```

### Next planned step

M22 PR136: Embedding / semantic search boundary.

## 2026-05-26 — M22 PR134: Knowledge index abstraction

Status: implemented.

### Scope

- Added first stable knowledge index boundary in `knowledge-entity`:
  - `KnowledgeIndex` trait
  - `KnowledgeFullTextQuery`
  - `KnowledgeIndexDocument`
  - `KnowledgeIndexRebuildRequest`
  - `KnowledgeIndexRebuildReport`
  - `KnowledgeIndexError`
- Added deterministic in-memory full-text fake index: `MemoryFullTextKnowledgeIndex`.
- Added query boundary support for:
  - text terms
  - tag filters
  - frontmatter key/value filters
  - limit handling
- Added index rebuild API that replaces the in-memory document set and returns indexed/deleted counts.
- Kept Tantivy/SQLite FTS backend selection outside this PR; PR134 defines the backend-agnostic abstraction and fake test backend.

### Acceptance coverage

- Memory full-text fake index rebuilds from documents and queries title/body/tags/frontmatter text.
- Full-text query returns deterministic ranking and snippets.
- Tag and frontmatter filters constrain search results.
- Upsert replaces existing documents by entry id.
- Delete removes documents from subsequent query results.
- Query defaults are stable.

### Verification commands

```bash
cargo test -p knowledge-entity memory_fulltext_index
cargo test -p knowledge-entity knowledge_fulltext_query_defaults
cargo test -p knowledge-entity
```

### Next planned step

M22 PR135: Full-text search backend.

## 2026-05-26 — M21 PR133: Browser security policy

Status: implemented.

### Scope

- Added browser security policy boundary types:
  - `BrowserSecurityPolicy`
  - `BrowserSecurityDecision`
  - `BrowserSecurityEvaluation`
  - `BrowserJsRisk`
  - `BrowserCredentialExposureWarning`
- Added domain allow/deny/high-risk matching with subdomain support.
- Added JavaScript execution risk classification for low/medium/high risk scripts.
- Added credential exposure warning detection for authorization, cookies, passwords, tokens, API keys, secrets, and bearer terms.
- Added `CdpBrowserConfig::security_policy` and `with_security_policy(...)` builder support.
- Kept executor-wide enforcement outside this PR; PR133 defines the policy and evaluation boundary needed before runtime Ask/Deny wiring.

### Acceptance coverage

- Allowed domains are allowed and denied domains are denied, including subdomains.
- Unknown domains default to Ask.
- JavaScript risk classification distinguishes mutation/credential scripts from read-only DOM access and simple expressions.
- Credential exposure warning captures sensitive terms with high severity.
- `execute_js` on high-risk domains evaluates to Ask by default and can be configured to Deny.
- Browser config carries a security policy via default config and builder.

### Verification commands

```bash
cargo test -p browser-kernel-core browser_security_policy
cargo test -p browser-kernel-core execute_js_on_high_risk_domain
cargo test -p browser-kernel-core cdp_browser_config_builder_sets_security_policy
cargo test -p browser-kernel-core
```

### Next planned step

M22 PR134: Knowledge index abstraction.

## 2026-05-26 — M21 PR132: Human takeover boundary

Status: implemented.

### Scope

- Added browser automation pause/resume boundary types:
  - `BrowserAutomationState`
  - `BrowserAutomationGate`
  - `BrowserHumanTakeoverRequest`
  - `BrowserHumanTakeoverLease`
  - `BrowserHumanTakeoverReason`
- Added metadata-only `BrowserHostSessionHandle` for exposing browser session/page/profile metadata to a host during human takeover.
- Added `BrowserMutationActionKind` and action-name mapping for browser actions that mutate or may mutate browser/page state.
- Added typed `BrowserKernelError::AutomationPaused(...)` for blocked mutation actions while takeover is active.
- Kept real UI/browser handoff and executor-wide shared-state wiring outside this PR; PR132 defines the stable, testable policy boundary.

### Acceptance coverage

- Automation gate starts in `Running` state.
- Human takeover requests pause automation and create a takeover lease.
- Matching sessions can resume automation and clear active takeover state.
- Wrong-session resume attempts are rejected.
- Paused automation rejects all classified mutation actions with `AutomationPaused`.
- Running automation allows classified mutation actions.
- Host session handle exposes session metadata during active takeover and is rejected without active takeover.
- Browser mutation action names map deterministically from action ids.

### Verification commands

```bash
cargo test -p browser-kernel-core browser_automation_gate
cargo test -p browser-kernel-core browser_mutation_action_kind
cargo test -p browser-kernel-core host_session_handle
cargo test -p browser-kernel-core paused_automation
cargo test -p browser-kernel-core
```

### Next planned step

M21 PR133: Browser security policy.

## 2026-05-26 — M21 PR131: HAR / network trace boundary

Status: implemented.

### Scope

- Added HAR-like network trace boundary types:
  - `BrowserNetworkTrace`
  - `BrowserNetworkTraceEntry`
  - `BrowserNetworkHeader`
- Added optional capture policy and redaction policy types:
  - `BrowserNetworkTracePolicy`
  - `BrowserNetworkRedactionPolicy`
- Added default network capture boundary with `capture_enabled = true` and `max_entries = 200`.
- Added default auth/credential header redaction for `Authorization`, `Cookie`, `Set-Cookie`, API key/token headers, and proxy auth headers.
- Added `BrowserNetworkTrace::to_artifact_descriptor(...)` for future HAR persistence as `ArtifactKind::ToolResult` with `application/har+json` MIME type.

### Acceptance coverage

- Network trace policy defaults to auth header redaction.
- Sensitive request/response headers are replaced with `[REDACTED]`.
- Non-sensitive headers are preserved.
- Network trace payloads roundtrip through serde with redacted entries.
- Network trace artifact descriptor records source action, entry count, and redaction metadata.

### Verification commands

```bash
cargo test -p browser-kernel-core browser_network
cargo test -p browser-kernel-core
```

### Next planned step

M21 PR132: Human takeover boundary.

## 2026-05-26 — M21 PR130: DOM snapshot artifact

Status: implemented.

### Scope

- Added `BrowserDomSnapshotArtifact` for HTML/DOM snapshot evidence metadata.
- DOM snapshot artifacts record source URL, optional page title, `text/html` MIME type, byte size, SHA-256 hash, and capture timestamp.
- Added `BrowserDomSnapshotArtifact::to_artifact_descriptor(...)` to link snapshots to `ArtifactDescriptor` with `ArtifactKind::WebPage`.
- Added `BrowserExtractContentInput::save_dom_snapshot` with a serde default of `false` for backwards-compatible action input.
- Updated `browser.extract_content` to optionally capture page HTML and persist a DOM snapshot descriptor through the configured `ArtifactStore`.
- `browser.extract_content` payload now includes `title` and `dom_snapshot_artifact_id` so callers can link extracted content to evidence artifacts.

### Acceptance coverage

- DOM snapshot artifact metadata records content type, byte size, source URL, title, and SHA-256 hash.
- DOM snapshot artifact descriptor links to the source URL and `browser.extract_content` action.
- Empty DOM snapshot HTML is rejected with a typed config error.
- `browser.extract_content` keeps `save_dom_snapshot` disabled by default.

### Verification commands

```bash
cargo test -p browser-kernel-core browser_dom_snapshot_artifact
cargo test -p browser-kernel-core browser_extract_content_input
cargo test -p browser-kernel-core
```

### Next planned step

M21 PR131: HAR / network trace boundary.

## 2026-05-26 — M21 PR129: Frame / iframe support

Status: implemented.

### Scope

- Added stable frame identity and frame-aware selector types:
  - `BrowserFrameId`
  - `BrowserFrameSelector`
  - `ElementSelector::Frame(...)`
- Added frame metadata to interactive snapshots via `BrowserFrameMetadata` and `InteractiveSnapshot::frames`.
- Added `InteractiveElement::frame_id` so elements discovered inside same-origin iframes can be attributed to their source frame.
- Upgraded interactive snapshot extraction to return both elements and frames.
- Added raw mapping helpers for frame metadata and iframe elements.
- Updated snapshot JavaScript to collect iframe/frame metadata and same-origin iframe interactive elements while safely ignoring cross-origin frame contents.

### Acceptance coverage

- Frame-aware selectors roundtrip through serde and validate nested selectors.
- Invalid frame ids and invalid inner selectors are rejected.
- Frame metadata roundtrips in snapshot payloads.
- Raw iframe elements map to `ElementSelector::Frame(...)` with the correct `frame_id`.
- Raw interactive snapshots map iframe elements and frame metadata together, allowing iframe-origin elements to be located by frame-aware selectors.

### Verification commands

```bash
cargo test -p browser-kernel-core element_selector_frame
cargo test -p browser-kernel-core frame_metadata
cargo test -p browser-kernel-core raw_frame
cargo test -p browser-kernel-core
```

### Next planned step

M21 PR130: DOM snapshot artifact.

## 2026-05-26 — M21 PR128: Browser crash recovery

Status: implemented.

### Scope

- Added browser crash classification types:
  - `BrowserCrashScope`
  - `BrowserCrashReason`
  - `BrowserCrashEvent`
- Added deterministic session recovery policy and plan types:
  - `BrowserRecoveryStrategy`
  - `BrowserRecoveryPolicy`
  - `BrowserRecoveryAction`
  - `BrowserRecoveryPlan`
- Added `BrowserPageLifecycleManager::record_crash(...)` to mark crashed pages or all open pages after browser process crash.
- Added `BrowserPageLifecycleManager::recover_from_crash(...)` to produce host/runtime recovery plans after recording crash state.
- Added `CdpBrowserConfig::recovery_policy` and `with_recovery_policy(...)` builder support.
- Added typed `BrowserKernelError::BrowserCrashed(...)` for future CDP supervision integration.
- Kept actual Chromium process supervision/relaunch plumbing outside this PR; PR128 defines the stable, testable recovery boundary.

### Acceptance coverage

- Simulated page crash marks page status and health as crashed.
- Browser process crash marks all non-closed pages as crashed while preserving closed pages.
- Recovery policy supports fail-fast, reopen-active-page, and relaunch-session plans.
- Relaunch recovery preserves session id, profile binding, page metadata, and retry-after-relaunch boundary.
- Invalid page crash events and invalid relaunch policies are rejected with typed config errors.
- Recovery plan does not replay mutation actions implicitly; it only exposes a host/runtime retry boundary.

### Verification commands

```bash
cargo test -p browser-kernel-core browser_crash_event
cargo test -p browser-kernel-core browser_recovery_policy
cargo test -p browser-kernel-core page_lifecycle_manager_records
cargo test -p browser-kernel-core page_lifecycle_manager_recovery
cargo test -p browser-kernel-core
```

### Next planned step

M21 PR129: Frame / iframe support.

## 2026-05-26 — M20 PR113: OpenAI-compatible streaming

Status: implemented.

### Scope

- Added OpenAI-compatible SSE parsing for Chat Completions streaming responses.
- Mapped OpenAI stream chunks into provider-neutral `ModelStreamEvent` values:
  - `Started`
  - `TextDelta`
  - `ToolCallDelta`
  - `Usage`
  - `Finished`
- Added `StreamingModelAdapter` implementation for `OpenAiCompatibleAdapter`.
- Streaming requests now post `"stream": true` to `/chat/completions`.
- Malformed SSE JSON returns a typed `ModelAdapterError::ExecutorFailed(...)` instead of panicking.
- Added mock HTTP E2E coverage using `wiremock`.

### Verification commands

```bash
cargo test -p model-adapter
cargo test -p agent-runtime -p model-adapter
cargo fmt --all --check
cargo test --workspace
```

### Next planned step

M20 PR114: Anthropic streaming.

Recommended scope:

- Add Anthropic Messages streaming parser.
- Map Anthropic text deltas and tool use deltas into `ModelStreamEvent`.
- Add malformed stream tests and mock HTTP streaming E2E.
