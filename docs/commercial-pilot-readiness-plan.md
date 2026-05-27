# Commercial Pilot Readiness Plan

This document defines the practical plan for taking `connor-agent-core` from its current controlled-beta-ready posture to a commercial pilot foundation that can support backend services and macOS clients.

It intentionally includes both completed foundations and remaining work so host, backend, desktop, security, connector, and release owners can share one execution view.

## Executive Summary

`connor-agent-core` is ready to serve as the integration foundation for backend and macOS client development now, under the controlled beta constraints already documented in [m24-beta-hardening-decision.md](m24-beta-hardening-decision.md).

Commercial pilot remains conditional. The remaining work is not primarily about inventing the core architecture; it is about freezing the host-facing contract, rehearsing host-level credential and release operations, enforcing production observability, and closing connector/browser risk evidence for the enabled pilot capabilities.

Recommended commercial pilot path:

1. Freeze and document the beta/commercial host API contract.
2. Add backend/macOS host integration examples that compile and remain release-gated.
3. Rehearse host credential storage, rotation, revocation, and offboarding.
4. Add production-like observability export and retention/access-control policy enforcement.
5. Exercise beta/pilot release packaging, changelog, rollback, and incident response.
6. Accept storage/journal fixtures as the commercial compatibility baseline.
7. Close Gmail read-only provider hardening if Gmail is enabled for the pilot.
8. Keep browser automation disabled or host-opt-in until product permission UX and real CDP irreversible side-effect evidence are complete.

Estimated remaining PRs for commercial pilot: **8-12 PRs**, depending on whether broad browser automation is included in the first pilot.

## Current Readiness State

### Already Ready for Controlled Beta

The current stable host-facing boundary is intentionally narrow and documented in [feature-matrix.md](feature-matrix.md):

| Crate | Stable boundary role | Current status |
| --- | --- | --- |
| `agentos-kernel` | Host API, kernel runtime lifecycle, diagnostics, error taxonomy | Controlled-beta ready |
| `action-runtime` | Policy-to-execution orchestration, approval/deny behavior, audit integration | Controlled-beta ready |
| `audit-log` | Audit recording, JSONL export, redaction, permission filtering | Controlled-beta ready |
| `enterprise-permission-core` | Grants, lifecycle/offboarding semantics, admin checks | Controlled-beta ready |

The following foundations are already in place:

- Kernel composition root with `KernelRuntime`, `KernelRuntimeBuilder`, lifecycle, recovery, health checks, diagnostics bundle, and host API boundary.
- Durable agent/action execution: run queue, action queue, approval queue, checkpoint, retry, timeout, cancellation, and recovery tests.
- Storage and journal baseline: migration framework, backup, restore, repair, integrity verification, compatibility fixtures, and controlled-beta acceptance evidence.
- Capability policy, approval, denial, and audit invariants.
- Credential/identity first version: Ed25519 crypto, macOS Keychain boundary, encrypted file credential store boundary, OAuth boundary, token refresh integration, scoping, rotation, device credential revocation, and server trust binding.
- Model adapter hardening: streaming, retry/backoff, circuit breaker, usage/cost accounting, structured output validation, capability registry, compatibility matrix, and observability hooks.
- Browser Kernel first commercial hardening path: CDP read/interaction path, session/page lifecycle, navigation wait, frame support, downloads/uploads, crash recovery, network trace, human takeover, security policy, and side-effect policy tests.
- Connector Runtime first version: external connector abstraction, registry, service kind, credential boundary, Gmail read-only connector boundary, and OAuth refresh integration.
- Observability first version: structured trace/metric schema, in-memory sink, redaction policy, tool-loop observability wiring, model trace, browser trace boundary, diagnostics bundle.
- Release gate: docs checks, feature matrix checks, security checklist checks, formatting, clippy, and full workspace tests.

### Most Recent Evidence

Recent completed evidence PRs:

- PR197: Connector/browser commercial review evidence.
- PR198: Storage/journal fixture freeze acceptance evidence.
- PR199: Browser irreversible side-effect approval/deny/audit evidence for click/type/fill/upload/download and upload/download schema registration.
- PR200: Host API freeze contract accepted for backend/macOS integration and release-gate-checkable via [host-api-freeze.md](host-api-freeze.md).

The full release gate passed after PR199.

## Commercial Pilot Definition

For this plan, **commercial pilot** means a small, controlled real-user or pilot-customer deployment where:

- A backend service and macOS client can integrate with the kernel through documented host-facing APIs.
- Enabled connectors and browser capabilities have explicit permission, audit, and operational evidence.
- User credentials are handled by host-approved storage and revocation/offboarding flows.
- Storage/journal compatibility is treated as a long-lived contract.
- Production-like observability and release/rollback operations are available.
- Known high-risk capabilities are either closed, disabled, or explicitly host-opt-in with documented risk acceptance.

Commercial pilot does **not** mean every roadmap capability is complete. People intelligence, full P2P sync, every connector adapter, and broad browser automation can remain outside the pilot scope if explicitly disabled.

## Commercial Pilot Entry Criteria

The pilot release candidate should satisfy all controlled beta criteria plus the following:

1. **Host API freeze accepted**
   - Stable crates and types are documented.
   - Breaking-change policy exists.
   - Host-facing examples compile.

2. **Storage/journal commercial compatibility accepted**
   - Current fixtures are accepted as long-lived baseline fixtures.
   - Migration, backup, rollback, and release-note rules are explicit.
   - Rollback rehearsal exists against non-production fixtures.

3. **Credential operations host-rehearsed**
   - Pilot credential backend is selected.
   - Rotation, revocation, and offboarding have host-level evidence.
   - OAuth provider revocation/token refresh behavior is covered for enabled connectors.

4. **Connector risk closed for enabled connectors**
   - Gmail read-only may be enabled only with retry, timeout, rate-limit, audit, redaction, and offboarding evidence.
   - Write connectors remain disabled unless separately reviewed.

5. **Browser risk disposition explicit**
   - Broad browser exposure remains disabled unless product-level permission UX and real CDP irreversible side-effect evidence are complete.
   - If browser is enabled, capabilities must be host-opt-in and auditable.

6. **Production observability available**
   - At least one production-like export sink exists.
   - Redaction, retention, and access-control policy are documented and tested.

7. **Release operations exercised**
   - Tagging, changelog, release artifact bundle, rollback, and incident escalation are rehearsed.

## Proposed PR Plan

The plan below assumes the target is commercial pilot, not only controlled beta.

### PR200: Beta/Commercial Host API Freeze Acceptance ✅ Completed

**Goal:** Turn the current stable boundary into an explicit host integration contract for backend and macOS client teams.

**Already available:**

- `agentos-kernel` host API boundary.
- `KernelRuntime` lifecycle and diagnostics.
- `KernelHostApi` integration tests.
- README and `feature-matrix.md` stable boundary references.

**Deliverables:**

- ✅ New [host-api-freeze.md](host-api-freeze.md) documenting:
  - stable crates;
  - stable host-facing types;
  - allowed additive changes;
  - breaking-change/deprecation policy;
  - unstable crates/features outside the pilot contract.
- ✅ README update linking the freeze contract.
- ✅ Feature matrix update linking the freeze contract and commercial pilot plan.
- ✅ Release gate check that the host API freeze doc and commercial pilot readiness plan exist.
- ✅ Doc tests requiring the freeze contract from README, feature matrix, and release gate.

**Acceptance:**

- ✅ `cargo test -p agentos-kernel --test public_api_docs host_api_freeze_document_records_beta_commercial_contract` passes.
- ✅ `cargo test -p agentos-kernel --test release_gate_docs release_gate_script_documents_and_runs_required_checks` passes.
- ✅ `./scripts/release-gate.sh` passes.
- ✅ Backend/macOS host owners can identify exactly which crate APIs are safe to depend on.

### PR201: Backend and macOS Host Integration Examples ✅ Completed

**Goal:** Provide minimal host-shaped examples that prove the kernel can be embedded by backend and desktop hosts.

**Already available:**

- Kernel host API.
- Action processing and approval flow.
- Diagnostics bundle.
- Audit query/export boundary.

**Deliverables:**

- ✅ Server-shaped example:
  - initialize kernel;
  - submit message;
  - start run;
  - process action;
  - approve/deny action;
  - query audit/diagnostics.
- ✅ Desktop-shaped/macOS-oriented example:
  - local storage root;
  - local credential provider boundary;
  - approval UX handoff shape;
  - diagnostics/debug bundle export shape.
- ✅ CI/release-gate coverage that examples compile.

**Acceptance:**

- ✅ `cargo check -p agentos-kernel --example minimal-cli-host` passes.
- ✅ `cargo check -p agentos-kernel --example minimal-server-host` passes.
- ✅ `cargo check -p agentos-kernel --example minimal-desktop-host` passes.
- ✅ `cargo test -p agentos-kernel --test kernel_host_examples` passes.
- ✅ Host teams can bootstrap integration without reverse-engineering tests.

### PR202: Credential Host Rehearsal ✅ Completed

**Goal:** Move credentials from crate-level boundaries to host-level operational confidence.

**Already available:**

- macOS Keychain provider boundary.
- Encrypted file credential store v1.
- OAuth boundary and token refresh provider integration.
- Credential scoping, rotation, and device revocation v1.
- Credential operations runbook and code-level rehearsal evidence.

**Deliverables:**

- ✅ Host-selected credential backend decision for pilot:
  - macOS Keychain for desktop;
  - encrypted file or service-managed secret storage for backend, if applicable.
- ✅ Rehearsal evidence for:
  - create/store credential boundary ownership;
  - refresh token boundary ownership, with real provider revocation deferred to PR206;
  - rotate credential;
  - revoke credential;
  - device/account offboarding;
  - audit event expectations.
- ✅ Updated [credential-operations-runbook.md](credential-operations-runbook.md) and [credential-operations-rehearsal.md](credential-operations-rehearsal.md).
- ✅ Release-gate/docs tests require host-level pilot backend decision and rehearsal evidence sections.

**Acceptance:**

- ✅ Credential flows fail closed after revocation/offboarding according to existing rotation, revocation, and enterprise lifecycle tests.
- ✅ No plaintext secret evidence may be retained in diagnostics, traces, audit output, release artifacts, or host evidence records.
- ✅ `cargo test -p agentos-kernel --test public_api_docs credential_rehearsal_records_host_level_pilot_decision` passes.
- ✅ `cargo test -p agentos-kernel --test release_gate_docs release_gate_script_documents_and_runs_required_checks` passes.
- ✅ `./scripts/release-gate.sh` passes.

### PR203: Production-Like Observability Export Sink ✅ Completed

**Goal:** Move beyond in-memory observability so backend/macOS dogfood and pilot incidents can be diagnosed.

**Already available:**

- `agentos-observability` crate.
- `TraceEvent`, `MetricSample`, and metric kinds.
- In-memory sink.
- Tool-loop and model observability wiring.
- Redaction policy and diagnostics bundle.

**Deliverables:**

- ✅ File/JSONL observability sink: `JsonlObservabilityFileSink` writes redacted `traces.jsonl` and `metrics.jsonl` under a host-owned export root.
- ✅ Redaction tests for trace payloads and export tests proving plaintext token values are not written.
- ✅ Diagnostics/export metadata linkage shape via `ObservabilityExportMetadata` with export mode, trace path, metric path, retention days, and redaction status.
- ✅ Updated [production-observability-policy.md](production-observability-policy.md) with `## Production-Like File Export Sink` and evidence command.
- ✅ Release gate checks that production observability policy documents the file export sink.

**Acceptance:**

- ✅ Production-like trace export can be enabled without leaking secrets.
- ✅ Retention and access assumptions are documented; default file sink retention metadata is 14 days.
- ✅ `cargo test -p agentos-observability jsonl_file_sink_exports_redacted_traces_and_metrics` passes.
- ✅ `./scripts/release-gate.sh` passes.

### PR204: Beta/Pilot Release Artifact and Rollback Rehearsal ✅ Completed

**Goal:** Turn the release gate into a releaseable artifact process.

**Already available:**

- `./scripts/release-gate.sh`.
- [release-operations-runbook.md](release-operations-runbook.md).
- Storage backup/restore and fixture acceptance evidence.

**Deliverables:**

- ✅ Release artifact checklist generated or manually recorded for a rehearsal candidate in [release-artifact-rollback-rehearsal.md](release-artifact-rollback-rehearsal.md):
  - commit hash;
  - tag format;
  - `Cargo.lock`;
  - changelog input;
  - release gate output;
  - compatibility fixture status;
  - known accepted risks.
- ✅ Rollback rehearsal against a non-production storage root recorded as tabletop evidence.
- ✅ Incident escalation rehearsal notes for S0/S1 classification and owner handoff.
- ✅ Release gate checks that rehearsal evidence exists and contains release artifact, rollback, and incident sections.

**Acceptance:**

- ✅ A `v0.1.0-beta.N` candidate can be produced reproducibly from the recorded artifact checklist.
- ✅ Rollback path is documented and exercised against a non-production storage root.
- ✅ `cargo test -p agentos-kernel --test release_gate_docs release_artifact_rollback_rehearsal_records_pr204_evidence` passes.
- ✅ `./scripts/release-gate.sh` passes.

### PR205: Storage/Journal Commercial Fixture Freeze Acceptance ✅ Completed

**Goal:** Promote PR198 controlled-beta fixture acceptance into a commercial-pilot compatibility contract.

**Already available:**

- Storage layout version.
- Migration registry.
- Backup/restore/repair.
- Journal checksum/hash chain/verification.
- Compatibility fixtures.
- [storage-journal-fixture-freeze-policy.md](storage-journal-fixture-freeze-policy.md).
- [storage-journal-fixture-freeze-acceptance.md](storage-journal-fixture-freeze-acceptance.md).

**Deliverables:**

- ✅ Commercial fixture freeze acceptance recorded in [storage-journal-fixture-freeze-acceptance.md](storage-journal-fixture-freeze-acceptance.md) under `## Commercial-Pilot Fixture Freeze Acceptance`.
- ✅ Explicit long-lived fixture support policy recorded under `## Long-Lived Fixture Support Policy`.
- ✅ Required release note format for storage/journal changes recorded under `## Migration Release Note Template`.
- ✅ Rollback/backup expectation for every migration recorded under `## Rollback and Backup Expectations`.
- ✅ Release gate checks commercial freeze acceptance, long-lived support policy, and migration release note template.

**Acceptance:**

- ✅ Pilot owner accepts current fixtures as the commercial-pilot compatibility baseline for the first supported pilot line.
- ✅ Future storage/journal changes require migration + fixture + rollback evidence.
- ✅ `cargo test -p agentos-kernel --test release_gate_docs storage_journal_fixture_freeze_records_pr205_commercial_acceptance` passes.
- ✅ `./scripts/release-gate.sh` passes.

### PR206: OAuth Provider Endpoint, Revocation, and Offboarding Evidence ✅ Completed

**Goal:** Close the gap between OAuth boundary code and real provider lifecycle behavior for enabled connectors.

**Already available:**

- OAuth authorization boundary.
- Token set conversion and store-backed refresh/write-back- Gmail connector refresh integration.
- Credential scoping, rotation, and revocation helpers.

**Deliverables:**

- ✅ Provider-shaped OAuth token/revocation endpoint adapter boundary: `OAuthProviderEndpointConfig`, `OAuthTokenRevoker`, and `FakeOAuthTokenRevoker`.
- ✅ Revocation behavior documentation for enabled providers in [connector-browser-commercial-review-evidence.md](connector-browser-commercial-review-evidence.md) under `## PR206 OAuth Provider Lifecycle Evidence`.
- ✅ Offboarding test proving connector-account OAuth credentials are revoked and future token refresh fails closed after credential deletion.
- ✅ Audit expectations for refresh, refresh failure, revocation, and offboarding denial via metadata-only `OAuthCredentialLifecycleAuditEvent`.

**Acceptance:**

- ✅ Token refresh and revocation behavior are covered by deterministic provider-shaped fakes.
- ✅ Offboarded connector accounts cannot continue token refresh once credentials are revoked from the credential store.
- ✅ Secrets are redacted from debug/audit-shaped output; OAuth lifecycle audit evidence omits access/refresh token material.
- ✅ `cargo test -p identity-core revoke_oauth_credential_calls_provider_and_deletes_store_record` passes.
- ✅ `cargo test -p identity-core offboard_connector_account_revokes_credentials_and_refresh_fails_closed` passes.
- ✅ `cargo test -p identity-core oauth_lifecycle_audit_event_is_metadata_only` passes.
- ✅ `./scripts/release-gate.sh` passes.

### PR207: Gmail Read-Only Retry, Timeout, and Rate-Limit Hardening ✅ Completed

**Goal:** Make Gmail read-only suitable as the first conditional commercial connector.

**Already available:**

- Gmail read-only connector boundary.
- Gmail service kind and resource model.
- Authentication and unsupported-kind failure tests.
- Explicit OAuth scope evidence.
- Mail action policy contract: list/get read-only, create draft approval, send denied.

**Deliverables:**

- ✅ Provider retry/backoff policy for Gmail reads: `GmailProviderRetryPolicy` and `GmailProviderRetryConfig`.
- ✅ Timeout classification and error mapping: `GmailProviderErrorClass::Timeout` for timeout-like provider errors.
- ✅ Rate-limit treatment and retry-after handling: `ExternalConnectorError::RateLimited(seconds)` produces retry delay from provider `Retry-After` seconds.
- ✅ Tests for transient provider errors, timeout, rate limit, exhausted retries, and fail-closed auth/invalid-request behavior.
- ✅ Updated [connector-browser-commercial-review-evidence.md](connector-browser-commercial-review-evidence.md) under `## PR207 Gmail Retry Timeout Rate-Limit Evidence`.

**Acceptance:**

- ✅ Gmail read-only remains read-only under default safe policy; PR207 only adds provider retry decision boundaries.
- ✅ Provider failures do not bypass permission or credential boundaries; authentication/credential and invalid request failures are not retried.
- ✅ Rate-limit and timeout behavior are explicit and auditable through deterministic retry decisions.
- ✅ `cargo test -p connector-runtime gmail_provider_retry_policy` passes.
- ✅ `./scripts/release-gate.sh` passes.

### PR208: Gmail Host Audit and End-to-End Offboarding Evidence ✅ Completed

**Goal:** Ensure Gmail reads are observable and revoked/offboarded accounts cannot continue accessing mailbox resources.

**Already available:**

- Audit log boundary.
- Connector credential boundary.
- Credential revocation/offboarding helpers.
- Gmail read-only connector evidence.

**Deliverables:**

- ✅ Host-facing audit event shape for connector operation start/result/failure/denial: `ConnectorOperationAuditEvent`.
- ✅ Gmail read audit evidence with redacted metadata: `payload_redaction = "gmail message content omitted"`, `credential_redaction = "oauth token material omitted"`.
- ✅ Host lifecycle access gate: `evaluate_connector_account_access` with `ConnectorHostAccountLifecycle::{Active, Disabled, Offboarded}`.
- ✅ Offboarding e2e-shaped test:
  - ✅ account/read lifecycle is represented through connector id + account id metadata;
  - ✅ read allowed path produces start/result audit-shaped evidence;
  - ✅ offboarded host account is denied before connector read execution;
  - ✅ audit records denial with metadata-only payload and credential redaction.
- ✅ Updated [connector-browser-commercial-review-evidence.md](connector-browser-commercial-review-evidence.md) under `## PR208 Gmail Host Audit and Offboarding Evidence`.

**Acceptance:**

- ✅ Gmail connector access is auditable without leaking message content, snippets, or OAuth tokens.
- ✅ Offboarding is enforced across host account state and connector runtime via deterministic access gate.
- ✅ `cargo test -p connector-runtime gmail_connector_operation_audit_shape_is_metadata_only` passes.
- ✅ `cargo test -p connector-runtime gmail_read_records_start_and_result_audit_events` passes.
- ✅ `cargo test -p connector-runtime gmail_offboarded_account_access_is_denied_and_audited` passes.
- ✅ `./scripts/release-gate.sh` passes.

### PR209: Browser Pilot Permission Contract or Pilot Disable Profile ✅ Completed

**Goal:** Decide whether browser automation is enabled for the commercial pilot and enforce that decision.

**Already available:**

- Browser Kernel code-level review evidence.
- PR199 side-effect approval/deny/audit tests.
- Browser security policy, human takeover boundary, network trace, screenshot/artifact handling.

**Selected default:** Browser broad exposure remains disabled for the first commercial pilot unless product-level permission UX, irreversible side-effect mapping, real CDP irreversible evidence, and host risk acceptance are all recorded.

**Deliverables option A - disable broad browser exposure:**

- ✅ Pilot profile/config that disables broad browser automation by default: `BrowserPilotPermissionProfile::first_commercial_pilot_default`.
- ✅ Explicit host-opt-in gate for internal workflows: `BrowserPilotPermissionProfile::host_opt_in`.
- ✅ Release gate/doc evidence that browser broad exposure remains blocked: `## PR209 Browser Pilot Permission Profile Evidence`.

**Deliverables option B - enable limited browser capabilities:**

- Deferred to PR210/product work. Host opt-in remains blocked unless all four PR209 evidence gates are complete:
  - product-level permission prompt contract;
  - irreversible side-effect UX mapping;
  - real CDP irreversible side-effect evidence;
  - host risk acceptance.

**Acceptance:**

- ✅ The pilot cannot accidentally expose broad browser automation; default profile blocks all registered browser action kinds.
- ✅ If browser is enabled, every enabled capability must have permission, audit, real CDP irreversible evidence, and host risk acceptance evidence.
- ✅ `cargo test -p browser-entity browser_pilot_profile` passes.
- ✅ `./scripts/release-gate.sh` passes.

### PR210: Real CDP Download/Upload, Origin, Retention, and Human Takeover Evidence

**Goal:** Close the remaining browser commercial gap if any browser automation beyond read-only/internal opt-in is enabled.

**Already available:**

- CDP browser tests for read/open/click/fill/execute-js approval behavior.
- Browser upload/download handling boundaries.
- Browser network trace boundary and redaction.
- Human takeover gate.
- PR199 action-runtime policy/audit coverage.

**Deliverables:**

- Real CDP upload/download tests using controlled fixtures.
- Origin isolation tests.
- DOM/screenshot/network trace retention policy tests.
- Human takeover interruption/resume/deny tests for irreversible actions.
- Debug bundle handling evidence.

**Acceptance:**

- Browser irreversible actions cannot execute silently.
- Human takeover prevents automation races.
- Captured DOM/screenshot/network artifacts follow retention/redaction policy.
- Release gate passes.

### PR211: Production Telemetry Retention and Access-Control Enforcement ✅ Completed

**Goal:** Ensure observability data can be used in pilot operations without becoming a privacy/security liability.

**Already available:**

- Production observability policy.
- Redaction policy.
- Observability schema and in-memory sink.
- Diagnostics bundle.
- PR203 `JsonlObservabilityFileSink` production-like file export sink.

**Deliverables:**

- ✅ Retention configuration for file/export sink: `TelemetryRetentionPolicy` checked against sink `retention_days` metadata.
- ✅ Access-control expectations for diagnostics/trace export: `TelemetryAccessPolicy` requires admin-only export access, tenant partitioning, and incident access audit.
- ✅ Tests for retention pruning or retention metadata: `pilot_observability_operations_drill_requires_retention_access_and_incident_workflow` verifies retention metadata and cleanup-job documentation requirements.
- ✅ Tests proving sensitive values are redacted in exported telemetry: PR203 `jsonl_file_sink_exports_redacted_traces_and_metrics` remains release-gated; PR211 requires redaction metadata before pilot readiness.
- ✅ Update observability policy: `## PR211 Pilot Observability Operations Drill`.

**Acceptance:**

- ✅ Pilot telemetry has documented retention and access semantics.
- ✅ Exported traces/metrics remain redacted and debug bundles require named incident, operator approval, secret scan, expiration, and access audit.
- ✅ `cargo test -p agentos-observability pilot_observability_operations_drill_requires_retention_access_and_incident_workflow` passes.
- ✅ `./scripts/release-gate.sh` passes.

### PR212: Pilot Release, Rollback, and Incident Exercise ✅ Completed

**Goal:** Produce a complete pilot release candidate and exercise operational response.

**Already available:**

- Release operations runbook.
- Release gate.
- Storage rollback decision tree.
- Incident severity definitions.
- PR204 beta/pilot release artifact and rollback rehearsal.
- PR211 pilot observability operations drill.

**Deliverables:**

- ✅ `v0.1.0-pilot.N` release candidate evidence bundle shape: `v0.1.0-pilot.0-exercise` in [pilot-release-rollback-incident-exercise.md](pilot-release-rollback-incident-exercise.md).
- ✅ Changelog since previous beta/pilot tag: PR200 through PR212 commercial-pilot readiness window recorded.
- ✅ Release gate output archive: `./scripts/release-gate.sh` result recorded as release gate passed.
- ✅ Storage/journal fixture status: PR205 storage/journal fixture baseline accepted status recorded.
- ✅ Rollback rehearsal evidence: pilot rollback exercise covers stop, preserve, restore, verify, release-gate rerun, owner, and deletion deadline.
- ✅ Incident escalation tabletop evidence for S0/S1 scenarios: credential leak/data-loss and telemetry redaction/audit export failure.

**Acceptance:**

- ✅ Pilot release can be reproduced from tag-shaped identifier and commit.
- ✅ Rollback decision tree has been exercised for pilot candidate shape.
- ✅ Incident response owner handoff and escalation path are known for S0/S1.
- ✅ `cargo test -p agentos-kernel --test release_gate_docs pilot_release_rollback_incident_exercise_records_pr212_evidence` passes.
- ✅ `./scripts/release-gate.sh` passes on the release commit.

## Recommended Sequencing

Recommended execution order:

1. PR200 - Host API freeze acceptance.
2. PR201 - Backend/macOS host integration examples.
3. PR202 - Credential host rehearsal.
4. PR203 - Production-like observability export sink.
5. PR204 - Beta release artifact and rollback rehearsal.
6. PR205 - Storage/journal commercial fixture freeze acceptance.
7. PR206 - OAuth provider endpoint/revocation/offboarding evidence.
8. PR207 - Gmail retry/timeout/rate-limit hardening.
9. PR208 - Gmail host audit/offboarding e2e.
10. PR209 - Browser pilot permission contract or disable profile.
11. PR210 - Real CDP irreversible side-effect evidence, only if browser automation is enabled.
12. PR211 - Production telemetry retention/access-control enforcement.
13. PR212 - Pilot release/rollback/incident exercise.

A lean first pilot that enables Gmail read-only but keeps browser broad exposure disabled can target PR200-PR209 plus PR211-PR212, deferring PR210.

## Suggested Pilot Scope

### Include in First Pilot

- Kernel host API.
- Durable run/action/approval flow.
- Audit query/export.
- Local storage and conversation journal.
- Credential backend selected by host.
- Gmail read-only connector, if PR206-PR208 are complete.
- Production-like observability export.
- Backend/macOS integration examples.

### Exclude or Keep Internal-Only

- Browser broad automation.
- Mail write/send.
- Multi-device P2P sync.
- People intelligence.
- Full scheduler daemon.
- Unreviewed connectors such as Slack, Notion, GitHub, Linear, Outlook, or IMAP/SMTP.

## Pilot Readiness Checklist

Before declaring commercial pilot readiness, verify:

- [ ] Release gate passes on the candidate commit.
- [ ] Stable host API contract is accepted.
- [ ] Backend/macOS examples compile.
- [ ] Credential host backend is selected and rehearsed.
- [ ] Credential revocation/offboarding fails closed.
- [ ] Storage/journal fixtures are accepted as commercial compatibility baseline.
- [ ] Observability export is available with redaction and retention policy.
- [ ] Enabled connectors have threat review, retry, timeout, rate-limit, audit, and offboarding evidence.
- [ ] Browser broad exposure is either disabled or fully reviewed with product UX and real CDP evidence.
- [ ] Release artifact bundle exists.
- [ ] Rollback is exercised.
- [ ] Incident escalation is rehearsed.

## Risk Register

| Risk | Current disposition | Pilot mitigation |
| --- | --- | --- |
| Host API churn | Stable boundary exists but freeze acceptance still needed | PR200 |
| Backend/macOS integration mismatch | Kernel tests exist, but host examples are minimal | PR201 |
| Credential leakage or stale access | Boundaries exist; host rehearsal needed | PR202, PR206 |
| Insufficient production diagnostics | In-memory sink exists; export/retention incomplete | PR203, PR211 |
| Storage/journal long-term compatibility | Controlled-beta accepted; commercial freeze pending | PR205 |
| Gmail provider operational edge cases | Code-level connector evidence exists; provider policy gaps remain | PR207, PR208 |
| Browser irreversible side effects | PR199 narrows policy/audit gap; product UX and real CDP gaps remain | PR209, PR210 or disable browser broad exposure |
| Release/rollback uncertainty | Runbook exists; exercise pending | PR204, PR212 |

## Decision Recommendation

The recommended decision is:

1. Start backend and macOS client integration immediately against the controlled-beta stable boundary.
2. Execute PR200-PR204 as the minimum beta-to-pilot foundation.
3. If Gmail read-only is part of the first pilot, execute PR206-PR208 before enabling it for real users.
4. Keep browser broad exposure disabled for the first pilot unless PR209-PR210 are both complete.
5. Declare commercial pilot readiness only after PR205, PR211, and PR212 close storage compatibility, telemetry operations, and release/incident operations.

