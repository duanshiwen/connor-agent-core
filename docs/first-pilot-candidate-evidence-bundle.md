# First Pilot Candidate Evidence Bundle

This document records PR213 evidence for assembling the first lean commercial-pilot candidate bundle. It is an evidence bundle, not a published release tag, and it does not push artifacts.

Related documents:

- [commercial-pilot-readiness-plan.md](commercial-pilot-readiness-plan.md)
- [pilot-release-rollback-incident-exercise.md](pilot-release-rollback-incident-exercise.md)
- [release-artifact-rollback-rehearsal.md](release-artifact-rollback-rehearsal.md)
- [connector-browser-commercial-review-evidence.md](connector-browser-commercial-review-evidence.md)
- [production-observability-policy.md](production-observability-policy.md)
- [storage-journal-fixture-freeze-acceptance.md](storage-journal-fixture-freeze-acceptance.md)
- [credential-operations-rehearsal.md](credential-operations-rehearsal.md)

## Candidate Identity

Date: 2026-05-28
Candidate bundle shape: `v0.1.0-pilot.0-candidate-bundle`
Candidate commit: `9ec2677a9f08b7494685fa4cb2dc3983bbdeee49`
Candidate scope: lean first commercial pilot
Release tag created: no
Artifacts pushed: no
Release gate status: release gate passed on the PR212 exercise line; PR213 release gate must pass before commit.

This candidate bundle assumes a lean first pilot that enables stable kernel/host APIs, local storage/journal, credential backend integration, Gmail read-only connector evidence, production-like observability export, audit/offboarding evidence, and backend/macOS host examples. Browser broad exposure remains disabled by the pilot profile.

## Lean First Pilot Scope

Included in the first pilot candidate:

- Kernel host API and host-facing error/diagnostics boundaries.
- Durable run/action/approval flow.
- Audit query/export boundary and metadata-only connector audit events.
- Local storage and conversation journal with commercial fixture baseline acceptance.
- Credential backend selected and rehearsed by host profile.
- OAuth provider-shaped revocation/offboarding boundary.
- Gmail read-only connector with retry, timeout, rate-limit, audit, and offboarding evidence.
- Production-like JSONL observability export with retention, admin-only access, tenant partitioning, incident audit, and debug-bundle workflow evidence.
- Backend/macOS host integration examples and release-gated compile checks.
- Release/rollback/incident exercise evidence.

Operational stance:

- Gmail read-only connector is conditionally included when host wires real account lifecycle and audit backend to the documented connector boundaries.
- Browser broad exposure disabled by default for first pilot; PR210 remains optional and required only if browser broad automation is enabled.
- No real release tag or pushed distribution is created by this bundle.

## Evidence Matrix

| Area | Evidence | Status |
| --- | --- | --- |
| PR200 host API freeze | [host-api-freeze.md](host-api-freeze.md), stable host-facing boundary checks | Complete |
| PR201 backend/macOS host examples | `minimal-server-host`, `minimal-desktop-host`, `minimal-cli-host` release-gated cargo checks | Complete |
| PR202 credential host rehearsal | [credential-operations-rehearsal.md](credential-operations-rehearsal.md), host-level pilot backend decision | Complete |
| PR203 production-like observability sink | [production-observability-policy.md](production-observability-policy.md), `JsonlObservabilityFileSink` | Complete |
| PR204 release artifact rollback rehearsal | [release-artifact-rollback-rehearsal.md](release-artifact-rollback-rehearsal.md) | Complete |
| PR205 storage/journal fixture baseline | [storage-journal-fixture-freeze-acceptance.md](storage-journal-fixture-freeze-acceptance.md), commercial-pilot compatibility baseline | Complete |
| PR206 OAuth lifecycle | [connector-browser-commercial-review-evidence.md](connector-browser-commercial-review-evidence.md), provider endpoint/revocation/offboarding evidence | Complete |
| PR207 Gmail provider hardening | retry/timeout/rate-limit policy evidence | Complete |
| PR208 Gmail audit/offboarding | metadata-only audit shape and host account lifecycle denial gate | Complete |
| PR209 browser pilot profile | browser broad exposure disabled by first pilot default profile | Complete |
| PR211 observability operations drill | retention/access-control/debug-bundle readiness contract | Complete |
| PR212 release/rollback/incident exercise | [pilot-release-rollback-incident-exercise.md](pilot-release-rollback-incident-exercise.md) | Complete |

Release gate command:

```bash
./scripts/release-gate.sh
```

Expected result for this bundle: release gate passed.

## Excluded or Deferred Capabilities

Excluded or deferred from the lean first pilot:

- Browser broad automation beyond internal/host-opt-in beta workflows; browser broad exposure disabled by PR209.
- Browser real CDP irreversible side-effect enablement; PR210 required before broad exposure.
- Mail write/send or destructive connector operations.
- Multi-device P2P sync.
- People intelligence.
- Full scheduler daemon.
- Unreviewed connectors such as Slack, Notion, GitHub, Linear, Outlook, IMAP/SMTP.
- Remote telemetry vendor export unless the host supplies retention, redaction, and access-control approval.

## Open Host-Product Integration Items

The kernel/runtime evidence is sufficient to assemble this lean candidate bundle, but a real commercial pilot still requires host-product integration closure:

- Wire the real host account lifecycle source into `evaluate_connector_account_access`.
- Persist/export `ConnectorOperationAuditEvent` through the selected host audit backend.
- Select and permission the production credential backend for each host profile.
- Configure provider OAuth token and revocation endpoints for real Gmail accounts.
- Provision telemetry export root, cleanup job, admin-only access control, tenant partitioning, and incident access audit.
- Decide artifact storage location for release gate output archives, candidate bundle, backup manifests, and incident evidence.
- Record pilot owner approval before creating a real `v0.1.0-pilot.N` tag.

## Pilot Go/No-Go Prerequisites

Before Go:

- `./scripts/release-gate.sh` passes on the final candidate commit.
- Candidate tag shape and commit are recorded.
- Release gate output archive is stored in the approved artifact location.
- Storage/journal fixture baseline accepted status is included.
- Credential backend, revocation, and offboarding evidence is included.
- Gmail read-only connector audit/retry/offboarding evidence is included.
- Observability retention/access/debug-bundle workflow evidence is included.
- Browser broad exposure disabled decision is included, or PR210/product evidence is completed.
- Rollback owner, incident owners, and escalation path are recorded.
- Pilot owner signs the go/no-go decision.

No-Go conditions:

- Release gate failure.
- Missing credential revocation/offboarding evidence.
- Missing connector audit/offboarding evidence for enabled connectors.
- Telemetry/debug-bundle evidence includes secrets, connector content, browser DOM/screenshot/page text, model prompt/output payload, or denied resource metadata without approval.
- Browser broad exposure enabled without PR210/product UX/real CDP irreversible evidence.
- Storage/journal fixture baseline regression without migration + backup + rollback evidence.

## Acceptance

- The first pilot candidate evidence bundle is assembled under `v0.1.0-pilot.0-candidate-bundle`.
- The lean first pilot scope is explicit and includes Gmail read-only connector while keeping browser broad exposure disabled.
- PR200 through PR212 evidence is linked in one reviewable matrix.
- Excluded/deferred capabilities and open host-product integration items are explicit.
- Pilot go/no-go prerequisites and no-go conditions are documented.
