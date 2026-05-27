# Host Product Integration Closure

PR215 records the handoff boundary between the release-gated kernel/runtime readiness evidence and host-owned product integration work.

Status: closure evidence recorded; real host product wiring remains host-owned before a live pilot.

## Host-Owned Integration Items

The kernel/runtime repository provides deterministic boundaries and release-gated evidence. The host product must complete these items before a real `v0.1.0-pilot.N` tag or distribution:

| Integration item | Required host action | Kernel/runtime boundary |
| --- | --- | --- |
| Account lifecycle | Wire real account disabled/offboarded state into connector access checks | `evaluate_connector_account_access` |
| Connector audit backend | Persist/export connector operation audit events in host audit storage | `ConnectorOperationAuditEvent` |
| Credential backend | Select macOS Keychain or service backend, configure permissions, and document operator access | Credential operations runbook and rehearsal |
| OAuth endpoints | Configure real token and revocation endpoints for provider accounts | OAuth provider endpoint/revocation boundary |
| Telemetry export | Provision host-owned export root and cleanup job | `JsonlObservabilityFileSink`, `PilotObservabilityOperationsDrill` |
| Telemetry access | Enforce admin-only access, tenant partitioning, and incident access audit | `TelemetryAccessPolicy` |
| Debug bundles | Require named incident, operator approval, secret scan, expiration, access audit | `DebugBundleAccessWorkflow` |
| Release artifacts | Store release gate output, candidate bundle, backup manifests, incident evidence | Release/pilot candidate docs |
| Pilot approval | Record pilot owner approval before real tag creation | Pilot go/no-go record |

## Closure Criteria

Host product integration is closed only when:

- all enabled connector accounts fail closed when disabled/offboarded;
- connector audit events are persisted without OAuth tokens, Gmail bodies, snippets, or secrets;
- credential revocation/offboarding is rehearsed against host-selected backend;
- telemetry export retention and cleanup are operational;
- telemetry access audit is enabled;
- release artifact storage location is known;
- pilot owner approval is recorded.

## Deferred Items

- Browser broad exposure remains disabled.
- PR210 remains deferred until product permission UX and real CDP irreversible side-effect evidence are complete.
- Unreviewed connectors remain excluded from the lean first pilot.

## Outcome

PR215 closes the documentation handoff for host product integration. It does not claim live host integration has been performed inside this repository.

PR215 complete.
