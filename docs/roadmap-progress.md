# Roadmap Progress

## 2026-05-26 — Next action: M21 PR128 Browser crash recovery

Status: planned.

### Current baseline

- Latest completed M21 work: PR127 Dialog / permission prompt handling.
- Verification baseline: `cargo test --workspace` equivalent via manifest path passed with 1149 passed, 2 ignored.
- Existing browser primitives include `BrowserPageHealthStatus::{Healthy, Unresponsive, Crashed, Closed}`, `BrowserPageStatus::Crashed`, `BrowserSession`, and `BrowserPageLifecycleManager`.

### Recommended scope

- Add browser crash classification types.
- Add session/page recovery policy.
- Add deterministic recovery plan generation.
- Add retry-after-relaunch boundary without performing real process relaunch yet.
- Keep actual CDP process supervision as a later integration step if needed.

### Acceptance tests

- Simulated page crash marks page status/health as crashed.
- Recovery policy can choose fail-fast, reopen-active-page, or relaunch-session plan.
- Recovery plan preserves profile binding and page URL metadata.
- Non-idempotent mutation actions are not automatically retried after crash.
- `cargo test -p browser-kernel-core` passes.
- `cargo test --workspace` passes.

### Implementation sequence

1. RED: add failing tests for crash classification and recovery plan generation.
2. GREEN: add minimal types and pure functions in `browser-kernel-core`.
3. RED: add tests for lifecycle integration with `BrowserPageLifecycleManager`.
4. GREEN: wire crash marking and recovery plan derivation into lifecycle manager.
5. Verify with fmt, clippy, browser crate tests, workspace tests.

### Next planned step after PR128

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
