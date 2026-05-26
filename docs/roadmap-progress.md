# Roadmap Progress

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
