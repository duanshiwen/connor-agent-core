# Pilot Go/No-Go Decision

PR214 records the go/no-go decision for the first lean commercial pilot candidate.

Decision date: 2026-05-28
Decision scope: lean first pilot candidate only
Candidate bundle: `v0.1.0-pilot.0-candidate-bundle`
Decision: Conditional Go for lean first pilot
Browser broad exposure decision: No-Go for browser broad exposure

## Decision Summary

The first lean commercial pilot may proceed to host-product integration and controlled distribution preparation if the host product completes the open integration items listed in [host-product-integration-closure.md](host-product-integration-closure.md).

This is a Conditional Go for lean first pilot because PR200 through PR213 evidence is assembled and release-gated, while real host-product wiring remains outside the kernel/runtime repository.

This is a No-Go for browser broad exposure because PR209 keeps browser broad automation disabled by default and PR210 remains deferred until product permission UX and real CDP irreversible side-effect evidence are complete.

## Go Evidence

- Release gate passed on the candidate evidence line.
- Host API freeze evidence complete.
- Backend/macOS host examples compile under release gate.
- Credential host backend rehearsal complete.
- OAuth revocation/offboarding boundary complete.
- Storage/journal fixture baseline accepted for commercial pilot compatibility.
- Gmail read-only retry/timeout/rate-limit/audit/offboarding evidence complete.
- Observability file export, retention, access-control, and debug-bundle workflow evidence complete.
- Release, rollback, and S0/S1 incident exercise complete.
- First pilot candidate evidence bundle assembled.

## No-Go / Deferred Evidence

- Browser broad exposure is not approved for the first lean pilot.
- Mail write/send and destructive connector operations are not approved.
- Unreviewed connectors are not approved.
- Remote telemetry vendor export is not approved without host retention/redaction/access-control policy.

## Conditional Go Requirements

Before a real pilot tag or distribution:

1. Host product wires real account lifecycle into `evaluate_connector_account_access`.
2. Host product persists/exports `ConnectorOperationAuditEvent` through the selected audit backend.
3. Host product configures production credential backend and provider OAuth endpoints.
4. Host product provisions telemetry export root, cleanup job, admin-only access, tenant partitioning, and incident access audit.
5. Host product selects artifact storage for release gate output, candidate bundle, backup manifests, and incident evidence.
6. Pilot owner signs the final go/no-go decision before creating `v0.1.0-pilot.N`.

## Outcome

- Conditional Go for lean first pilot after host-product integration closure.
- No-Go for browser broad exposure.
- PR210 remains deferred.
- PR214 complete.
