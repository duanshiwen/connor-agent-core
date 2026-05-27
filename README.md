# Connor Agent Core

`connor-agent-core` is the Rust workspace for AgentOS core runtime boundaries: conversation event sourcing, agent runs, action execution, audit, storage, identity, connectors, browser/kernel entities, and host-facing integration APIs.

The workspace is intentionally layered. Domain crates define stable data and policy shapes; runtime crates orchestrate side effects through explicit boundaries; host-facing crates compose those pieces without hiding audit, permission, storage, or lifecycle decisions.

## Design Principles

- **Append-only conversation history**: conversation state changes are represented as events and can be replayed deterministically.
- **Explicit side-effect boundary**: external effects flow through `action-core` / `action-runtime`, policy checks, and audit logging.
- **Host-owned production integration**: credentials, telemetry export, release artifacts, and product UX are represented by boundaries and examples, not hard-coded product infrastructure.
- **Testable defaults**: in-memory stores and fake providers are available for deterministic tests and examples.
- **Stable host API first**: `agentos-kernel` exposes the current host-facing composition boundary for backend/macOS integration.

## Workspace Map

### Conversation Layer

- `conversation-core` — IDs, participants, messages, events, visibility, slices, action/run lifecycle types.
- `conversation-journal` — append-only journal abstraction plus memory and segmented JSONL implementations.
- `conversation-kernel` — commands, projector, state, policies, snapshots, and context slice building.

### Agent and Model Layer

- `agent-runtime` — current agent run processor, context building, tool/action proposal routing, retry/run/action stores, approval queues, checkpoints.
- `client-substrate` — commercial client facade with typed commands/events, UI projections, and conservative safety defaults.
- `model-adapter` — model provider abstraction plus OpenAI-compatible and Anthropic adapters, streaming/tool call support, token budgeting, and fake adapters.
- `assistant-core` — assistant profiles, capabilities, preferences, and conversation helpers.

### Action, Policy, and Audit Layer

- `action-core` — action IDs, schemas, requests, results, and registry primitives.
- `action-runtime` — policy → executor → audit → conversation lifecycle orchestration.
- `capability-policy` — allow/ask/deny policy evaluation and policy-file loading.
- `audit-log` — memory/JSONL/enterprise audit sinks, audit queries, export, and integrity reporting.
- `enterprise-permission-core` — enterprise users, roles, grants, lifecycle/offboarding, cached and server-backed permission stores.

### Host, Storage, and Observability Layer

- `agentos-kernel` — host-facing composition root, runtime builder, service registries, host API, diagnostics, and error taxonomy.
- `agentos-config` — typed config parsing, overlays, validation, and redaction.
- `agentos-storage` — durable storage layout, artifact store, migration, backup, locking, and repair primitives.
- `agentos-observability` — structured telemetry, metrics, redaction, JSONL export, and pilot operations drill types.

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

## Release Checklist

Run the release gate from the repository root before cutting or reviewing a release candidate:

```bash
./scripts/release-gate.sh
```

The release gate verifies:

1. README release checklist remains discoverable.
2. Host examples compile.
3. Formatting passes: `cargo fmt --all --check`.
4. Linting passes across normal, test, and example targets: `cargo clippy --workspace --all-targets -- -D warnings`.
5. Tests pass: `cargo test --workspace`.
6. Fast commercial-client substrate smoke passes: `./scripts/perf-smoke-gate.sh`.

For a stricter local preflight, run:

```bash
cargo fmt --all -- --check
cargo check -p client-substrate --all-targets
cargo check --workspace --all-targets --locked
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace --all-targets
./scripts/perf-smoke-gate.sh
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

- `agentos-kernel`
- `action-runtime`
- `audit-log`
- `enterprise-permission-core`

These APIs may grow additively. Breaking signature or semantic changes require an intentional migration path, compatibility coverage, and a release note unless the change is needed for a security/privacy fix.

### Unstable API

The following remain unstable unless specifically documented otherwise:

- Internal module layout inside crates.
- Test-only fakes, fixtures, and helper constructors.
- Experimental domain crates and connector implementations.
- Concrete heuristics for audit export filtering, browser evidence extraction, or diagnostics enrichment.

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

## License

Apache-2.0. See [LICENSE](LICENSE).
