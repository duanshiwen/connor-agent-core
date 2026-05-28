# Commercial Client Posture

This document defines the commercial-client baseline for `connor-agent-core`.

## Boundary

The Rust workspace provides a kernel and client substrate, not a full branded desktop product. Native products should depend on:

1. `client-substrate` for typed commands, events, projections, safety profiles, and production guards.
2. `agentos-client-bridge` for JSON-safe native bridge contracts.
3. `agentos-kernel` only when building advanced host integrations.

## Production Requirements

A production host must provide:

- durable conversation journal
- non-fake model adapter
- durable or managed audit log
- initialized `AgentOsStorage`
- system or backend credential store
- non-fake identity crypto
- telemetry consent state
- crash report policy
- update/signing/notarization infrastructure at the app layer

## Client UI Contract

Client UIs should consume substrate projections rather than reconstructing UI state from low-level kernel events:

- conversation list projection
- timeline projection
- run projection
- approval projection
- event cursor stream

## Security and Privacy

- Production mode rejects fake/in-memory declarations.
- Diagnostics default to credential exclusion and secret scan requirement.
- Telemetry is opt-in through host-owned `ClientTelemetryConsent`.
- Crash reports default to disabled and require consent.
- Approval cards surface risk and reversibility hints.

## Release Operations

Release candidates must pass:

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace --quiet
./scripts/release-gate.sh
```

The release gate includes API compatibility, client substrate, bridge, and security smoke checks.

## Host Responsibilities

The host application remains responsible for:

- native app signing and notarization
- auto-update backend
- crash report backend
- telemetry export backend
- real OAuth app registration
- enterprise admin UI
- cloud sync/server deployment if enabled
