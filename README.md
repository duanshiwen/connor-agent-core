# Connor Agent Core

`connor-agent-core` is the Rust workspace for AgentOS core runtime boundaries: conversation event sourcing, agent runs, action execution, audit, storage, identity, connectors, browser/kernel entities, and host-facing integration APIs.

This repository is a kernel/runtime SDK, not a branded product UI. It provides durable contracts, orchestration boundaries, and host integration surfaces that macOS, desktop, server, or other AgentOS clients can embed.

The workspace is intentionally layered. Domain crates define stable data and policy shapes; runtime crates orchestrate side effects through explicit boundaries; host-facing crates compose those pieces without hiding audit, permission, storage, or lifecycle decisions.

## Design Principles

- **Append-only conversation history**: conversation state changes are represented as events and can be replayed deterministically.
- **Explicit side-effect boundary**: external effects flow through `action-core` / `action-runtime`, policy checks, and audit logging.
- **Host-owned production integration**: credentials, telemetry export, release artifacts, native notification delivery, provider accounts, OAuth apps, signing, updates, and product UX stay with the host application.
- **No fake production success**: test adapters and host-required integrations must not silently return placeholder success values in production-facing paths.
- **Testable defaults**: in-memory stores and fake providers are available for deterministic tests and examples.
- **Stable host API first**: `client-substrate`, `agentos-client-bridge`, and `agentos-kernel` expose the primary host-facing composition boundaries.

## Workspace Map

### Conversation Layer

- `conversation-core` — IDs, participants, messages, events, visibility, slices, action/run lifecycle types.
- `conversation-journal` — append-only journal abstraction plus memory and segmented JSONL implementations.
- `conversation-kernel` — commands, projector, state, policies, snapshots, and context slice building.

### Agent and Model Layer

- `agent-runtime` — current agent run processor, context building, tool/action proposal routing, retry/run/action stores, approval queues, checkpoints.
- `client-substrate` — commercial client facade with typed commands/events, UI projections, and conservative safety defaults.
- `agentos-client-bridge` — JSON-safe bridge boundary for native bindings and host applications.
- `model-adapter` — model provider abstraction plus OpenAI-compatible and Anthropic adapters, streaming/tool call support, token budgeting, and fake adapters.
- `assistant-core` — assistant profiles, capabilities, preferences, and conversation helpers.

### Action, Policy, and Audit Layer

- `action-core` — action IDs, schemas, requests, results, and registry primitives.
- `action-runtime` — policy → executor → audit → conversation lifecycle orchestration.
- `capability-policy` — allow/ask/deny policy evaluation, approval receipts, payload hashing, and policy-file loading.
- `audit-log` — memory/JSONL/enterprise audit sinks, audit queries, export, and integrity reporting.
- `enterprise-permission-core` — enterprise users, roles, grants, lifecycle/offboarding, cached and server-backed permission stores.

### Host, Storage, and Observability Layer

- `agentos-kernel` — host-facing composition root, runtime builder, service registries, host API, diagnostics, and error taxonomy.
- `agentos-config` — typed config parsing, overlays, validation, and redaction.
- `agentos-storage` — durable storage layout, artifact store, migration, backup, locking, and repair primitives.
- `agentos-observability` — structured telemetry, metrics, redaction, JSONL export, diagnostics, and pilot operations drill types.

### Domain and Connector Layer

- `entity-core`, `artifact-core`, `surface-core`, `asset-core`, `asset-index` — shared domain objects and indexes.
- `browser-entity`, `browser-kernel-core` — browser action schemas, permission profiles, CDP-oriented kernel domain and executor skeletons.
- `knowledge-entity`, `mail-entity`, `calendar-entity`, `reminder-core`, `notification-core`, `scheduler` — product-domain entities and deterministic stores/executors.
- `identity-core`, `server-account-core`, `connector-runtime` — local identity, credentials, server bindings, OAuth/provider lifecycle, connector audit/offboarding boundaries.
- `device-pairing-core`, `sync-runtime`, `p2p-sync-runtime` — device trust, sync manifests/merge, and P2P sync orchestration.
- `person-entity`, `relationship-core`, `people-intelligence`, `server-search-core` — people/relationship/search policy domains.

## Quick Start

```bash
cargo build --workspace
cargo test --workspace
```

Run host examples:

```bash
cargo run -p agentos-kernel --example minimal-cli-host
cargo run -p agentos-kernel --example minimal-server-host
cargo run -p agentos-kernel --example minimal-desktop-host
```

Run the commercial client example:

```bash
cargo run -p client-substrate --example minimal-commercial-client-host
```

## Production Host Responsibilities

A production host must provide real implementations for product-owned infrastructure instead of relying on test-only defaults:

- Durable conversation journal and storage root.
- Non-fake model provider configuration.
- Durable or managed audit log.
- System keychain, secure enclave, or backend credential storage.
- Real OAuth app registration and connector authorization flows.
- Telemetry and crash-report consent, export, retention, and deletion policy.
- Native notification, browser, update, signing, and notarization infrastructure where applicable.
- Enterprise admin, offboarding, and remote revocation flows when enterprise mode is enabled.

## Safety and Privacy Baseline

- Production builders reject known test-only dependency declarations.
- Action execution is mediated by side-effect classification, capability policy, approval receipts, and audit logging.
- Diagnostics default to credential exclusion and redaction-aware export.
- Telemetry and crash reporting are host-owned consented flows.
- Connector paths that require host/provider integration should fail explicitly instead of returning placeholder success data.
- Storage layout, migration, backup, repair, and lock primitives are compatibility-sensitive.

## Release Checklist

Run the release gate from the repository root before cutting or reviewing a release candidate:

```bash
./scripts/release-gate.sh
```

The release gate verifies:

1. README release checklist remains discoverable.
2. Host examples compile.
3. `client-substrate` targets and commercial client example compile.
4. API compatibility, client-substrate, bridge, and security smoke gates pass.
5. Formatting passes: `cargo fmt --all --check`.
6. Linting passes across normal, test, and example targets: `cargo clippy --workspace --all-targets -- -D warnings`.
7. Tests pass: `cargo test --workspace`.
8. Fast commercial-client substrate smoke passes: `./scripts/perf-smoke-gate.sh`.

For a stricter local preflight, run:

```bash
cargo fmt --all -- --check
cargo check --workspace --all-targets --locked
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace --all-targets
./scripts/release-gate.sh
```

Real provider and connector smoke tests are optional because they require external accounts and secrets. Use these only in an environment configured for real integrations:

```bash
./scripts/provider-smoke-gate.sh
./scripts/connector-smoke-gate.sh
```

Real provider smoke tests in `model-adapter` are ignored by default and require provider-specific environment variables. If those variables are missing, the ignored smoke tests print a skip message instead of failing.

## Minimal Conversation Kernel Example

```rust
use conversation_core::*;
use conversation_journal::MemoryConversationJournal;
use conversation_kernel::*;
use std::sync::Arc;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let journal = Arc::new(MemoryConversationJournal::new());
    let kernel = ConversationKernel::new(journal);

    let user = Participant {
        id: ParticipantId::from("user-1"),
        kind: ParticipantKind::Human,
        display_name: "User".to_string(),
    };

    let agent = Participant {
        id: ParticipantId::from("agent-1"),
        kind: ParticipantKind::Agent,
        display_name: "Assistant".to_string(),
    };

    let conversation_id = kernel
        .create_conversation(CreateConversationCommand {
            kind: ConversationKind::AgentTask,
            title: Some("Design conversation kernel".to_string()),
            participants: vec![user, agent],
            actor_id: Some(ParticipantId::from("user-1")),
        })
        .await?;

    let message_id = kernel
        .append_message(AppendMessageCommand {
            conversation_id: conversation_id.clone(),
            sender_id: ParticipantId::from("user-1"),
            content: MessageContent::Text {
                text: "Help me design the event model.".to_string(),
            },
            reply_to: None,
            thread_id: None,
            visibility: Visibility::Conversation,
        })
        .await?;

    let state = kernel.load_state(&conversation_id).await?;
    let slice = ConversationSliceBuilder::new(10)
        .build_recent_window(&state, &message_id)?;

    println!("slice messages: {}", slice.messages.len());
    Ok(())
}
```

## Public API Stability Boundary

The current stable host/application integration boundary for the `0.1.x` line is maintained directly in this README and enforced by tests plus the release gate.

### Stable API

Stable host-facing crates include:

- `client-substrate`
- `agentos-client-bridge`
- `agentos-kernel`
- `agent-runtime`
- `action-core`
- `action-runtime`
- `capability-policy`
- `audit-log`
- `agentos-storage`
- `agentos-observability`
- `identity-core`
- `enterprise-permission-core`

These APIs may grow additively. Breaking signature or semantic changes require an intentional migration path, compatibility coverage, and a release note unless the change is needed for a security/privacy fix.

Serialized compatibility-sensitive surfaces include bridge JSON payloads, kernel events, client projections, storage manifests/migrations, diagnostics bundles, approval receipts, and policy decisions. Incompatible serialized changes require a schema/API version bump and compatibility or migration coverage.

### Semi-Internal API

Domain crates such as `conversation-core`, `conversation-kernel`, connector/entity crates, people/relationship/search domains, and browser kernel domains may be used by advanced hosts, but product clients should prefer `client-substrate` and `agentos-client-bridge` unless they intentionally embed lower-level kernel behavior.

### Unstable API

The following remain unstable unless specifically documented otherwise:

- Internal module layout inside crates.
- Test-only fakes, fixtures, and helper constructors.
- Experimental domain crates and connector implementations.
- Concrete heuristics for audit export filtering, browser evidence extraction, diagnostics enrichment, and provider-specific connector behavior.

### Deprecation Policy

Deprecated stable APIs must remain available for at least one subsequent roadmap PR before removal unless removal is required for a security/privacy issue. Deprecated compatibility code that is not part of the stable host-facing boundary may be removed once current `agent-runtime` / `agentos-kernel` coverage and release gates pass. The old event-consumer conversation runtime crate has been removed; new integrations should use `agent-runtime` and `agentos-kernel`.

## Testing Strategy

The workspace uses layered tests:

1. Domain serialization and invariant tests.
2. Journal/storage append, reload, fixture, and integrity tests.
3. Projector and kernel command lifecycle tests.
4. Runtime, policy, action, approval, and audit orchestration tests.
5. Host API, diagnostics, release-gate, and example compile tests.
6. Provider compatibility tests with deterministic mocks plus optional ignored real-provider smoke tests.

## Documentation Policy

Top-level project posture, API stability, security/privacy baseline, and release instructions live in this README. Avoid adding new top-level `docs/` files unless a topic is too large to keep maintainable here and has a clear owner, lifecycle, and release-gate reference.

Schema-specific notes belong beside their schema under `schemas/`. Crate-specific notes belong in that crate's README or rustdoc.

## License

Apache-2.0. See [LICENSE](LICENSE).
