# AgentOS Kernel SDK API Stability

`connor-agent-core` is a Rust kernel/runtime SDK for commercial AgentOS clients. It is not a product UI repository. This document defines the public API surfaces that macOS, desktop, or other host clients may rely on.

## Public SDK crates

The following crates are intended to expose stable client/kernel contracts:

- `client-substrate` — commercial client-facing runtime facade, production wiring contracts, UI-safe projections.
- `agentos-client-bridge` — JSON-safe/native bridge boundary for future UniFFI, C ABI, N-API, or Swift package wrapping.
- `agentos-kernel` — kernel composition root, host API, canonical kernel event contracts.
- `agent-runtime` — agent run lifecycle, durable run journal, recovery contracts.
- `action-core` / `action-runtime` — action schema, execution, and side-effect contracts.
- `capability-policy` — policy decisions, approval receipt, action payload hash validation.
- `agentos-storage` — storage layout, manifest, migration, backup, repair contracts.
- `agentos-observability` — telemetry, diagnostics, redaction-safe observability contracts.
- `knowledge-entity`, `asset-core`, `asset-index` — knowledge/asset/work-object fact model contracts.
- `identity-core` — identity and credential lifecycle contracts.

## Semi-internal crates

Domain crates such as `conversation-core`, `conversation-kernel`, `audit-log`, and connector/entity crates may be used by advanced hosts, but client products should prefer `client-substrate` and `agentos-client-bridge` unless they intentionally embed lower-level kernel behavior.

## Internal implementation details

The following should not be relied upon by host clients without an explicit stability note:

- fake/test adapters
- in-memory stores except for tests/dev
- low-level helper modules
- private module paths
- internal test fixtures

## Versioned compatibility surfaces

Commercial clients should treat the following as compatibility-sensitive:

1. Rust public API of public SDK crates.
2. Bridge JSON response schemas.
3. Kernel event schemas.
4. Client event/projection schemas.
5. Storage manifest and migration schemas.
6. Diagnostics/support-bundle schemas.
7. Approval receipt and policy decision schemas.

## SemVer policy

Before `1.0`, this repository still allows API evolution, but public SDK changes must be accompanied by:

- tests or fixtures documenting the new contract;
- changelog/release notes when a downstream client would be affected;
- schema version bump when serialized contracts change incompatibly;
- migration path for persistent storage/event/projection changes.

After `1.0`, breaking changes require a major version bump unless hidden behind an explicit experimental API.

## Schema versioning policy

- `client-substrate` currently exposes `CLIENT_SUBSTRATE_API_VERSION = 1`.
- Kernel events expose `CURRENT_KERNEL_EVENT_SCHEMA_VERSION = 1`.
- Observability exposes `CURRENT_OBSERVABILITY_SCHEMA_VERSION = 1`.
- Storage uses `STORAGE_LAYOUT_VERSION = 1`.

When serialized payload fields are removed, renamed, or semantically changed, bump the appropriate schema/API version and add compatibility tests or migration logic.

## Deprecation policy

Deprecated public APIs should remain available for at least one minor release line after a replacement exists. Deprecations should document:

- replacement API;
- reason for deprecation;
- removal target if known.

## Host guidance

Commercial clients should depend on:

```text
client-substrate -> agentos-client-bridge -> generated native binding
```

They should avoid direct reliance on private module internals. If a client needs lower-level behavior, promote that capability into a documented public SDK contract first.
