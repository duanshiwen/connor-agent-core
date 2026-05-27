# Host API Freeze Contract

This document records the beta/commercial host-facing API freeze contract for `connor-agent-core`. It is the PR200 acceptance artifact for backend and macOS host integration.

The freeze is intentionally narrow: it protects the host-facing boundary needed by backend services, macOS clients, and release operations while leaving experimental crates and internal module layout free to evolve.

## Freeze Scope

This freeze applies to the `0.1.x` beta/commercial pilot line.

It covers:

- public host-facing types and behavior exported by the stable boundary crates listed below;
- documented lifecycle, action processing, approval, audit, and permission semantics;
- host-observable error taxonomy and response shape;
- compatibility expectations for additive changes, deprecations, and breaking changes.

It does not cover:

- private module layout;
- test-only fakes and fixtures;
- experimental crates not listed in the stable boundary;
- product-specific host UX, packaging, or transport decisions;
- internal implementation details that do not change documented host behavior.

## Stable Host-Facing Boundary

The following crates are accepted as the stable host-facing boundary for backend and macOS integration.

### `agentos-kernel`

Stable role:

- kernel composition root;
- runtime lifecycle;
- host API entry points;
- diagnostics bundle;
- host-facing error taxonomy;
- service registry wiring boundaries.

Stable host-facing items include:

- `KernelRuntimeBuilder`;
- `KernelRuntime`, `KernelRuntimeState`, `KernelHealthReport`;
- `KernelServices`;
- `KernelHostApi`;
- request/response/error types re-exported from `agentos-kernel::host_api`;
- diagnostics bundle types re-exported from `agentos-kernel::diagnostics`;
- service registry traits re-exported from `agentos-kernel::registries`.

### `action-runtime`

Stable role:

- policy-to-execution action orchestration;
- approval-required and denial behavior;
- audit recording expectations;
- conversation action lifecycle integration.

Stable host-facing items include:

- `ActionRuntime`;
- `ProcessActionRequest`;
- `ExecuteApprovedActionRequest`;
- `ActionRuntimeOutcome`;
- durable action/approval queue behavior visible through host APIs.

### `audit-log`

Stable role:

- audit event recording;
- audit query/export boundary;
- JSONL export schema;
- secret-like redaction expectations;
- permission-filtered export behavior.

Stable host-facing items include:

- `AuditLog`;
- audit event/query/export types;
- JSONL export behavior and schema version;
- redaction and permission-filtered export semantics.

### `enterprise-permission-core`

Stable role:

- enterprise grants;
- role/action/resource checks;
- organizational permission inheritance;
- lifecycle/offboarding denial behavior;
- administrative permission model.

Stable host-facing items include:

- permission grant, role, action, and resource types;
- `PermissionStore`, `OrganizationalPermissionStore`, and server-backed provider boundaries;
- `OrganizationId`, `TeamId`, `GroupId`, membership and offboarding types;
- offboarding denial invariants.

## Compatibility Rules

Stable APIs follow additive-first evolution.

Allowed during `0.1.x` beta/commercial pilot:

- new optional fields with safe defaults;
- new builder options that do not change existing defaults;
- new enum variants only when callers can ignore or safely handle unknown values;
- new trait extension methods when existing implementors remain source-compatible;
- internal refactors that preserve documented behavior;
- new diagnostics fields when existing fields keep their meaning.

Not allowed without the breaking change process:

- removing stable public items;
- renaming stable public items;
- changing stable function signatures;
- changing documented default policy decisions;
- changing host-facing error categories or response shape incompatibly;
- changing audit export semantics incompatibly;
- changing offboarding or permission-denial invariants incompatibly.

## Breaking Change Process

Breaking changes to stable host-facing APIs require all of the following unless the change fixes an urgent security/privacy issue:

1. A deprecation note in README, this document, or crate-level rustdoc.
2. Migration guidance for backend and macOS hosts.
3. Compatibility tests or compile tests showing the replacement path.
4. A release note entry in the candidate changelog.
5. Explicit pilot owner approval if the change affects a commercial pilot deployment.

Deprecated stable APIs should remain available for at least one subsequent roadmap PR unless they are unsound or create a security/privacy issue.

Security/privacy exceptions must document the replacement API and migration path in the same PR.

## Host Integration Expectations

Backend and macOS hosts should depend on the stable boundary through application-local adapters.

Recommended host adapter boundaries:

- kernel lifecycle adapter around `KernelRuntime` / `KernelHostApi`;
- action approval adapter mapping `ActionRuntimeOutcome` into product UX;
- audit adapter mapping `audit-log` query/export into host observability and compliance UI;
- permission adapter mapping `enterprise-permission-core` decisions into account, workspace, and organization state;
- diagnostics adapter mapping kernel health and bundle output into backend/macOS support workflows.

Hosts should avoid direct dependency on unstable internal modules or crates outside the stable boundary unless they accept churn.

## Pilot Acceptance Status

PR200 accepts this host API freeze contract for commercial pilot preparation.

Status:

- Controlled beta: accepted.
- Backend/macOS integration development: accepted.
- Commercial pilot: accepted as the host-facing API contract, subject to the remaining commercial pilot blockers in [commercial-pilot-readiness-plan.md](commercial-pilot-readiness-plan.md).

Remaining pilot work is tracked in [commercial-pilot-readiness-plan.md](commercial-pilot-readiness-plan.md), especially credential rehearsal, production observability, release rollback, storage/journal commercial fixture freeze, Gmail provider hardening, and browser exposure gating.

## Evidence Commands

Run the targeted PR200 evidence commands:

```bash
cargo test -p agentos-kernel --test public_api_docs host_api_freeze_document_records_beta_commercial_contract
cargo test -p agentos-kernel --test release_gate_docs release_gate_script_documents_and_runs_required_checks
```

Run the full release gate before release or commit acceptance:

```bash
./scripts/release-gate.sh
```
