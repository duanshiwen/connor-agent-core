# Roadmap Progress

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
