# Release Artifact and Rollback Rehearsal

This document records PR204 evidence for turning the release runbook into a rehearsed beta/pilot release artifact process.

Related runbook: [release-operations-runbook.md](release-operations-runbook.md)

## Rehearsal Scope

Date: 2026-05-27  
Candidate type: beta rehearsal, not a published tag  
Candidate tag shape: `v0.1.0-beta.rehearsal`  
Candidate commit: `c92f067ef1d4af1b7b59a087f3a0d699d214d1d3`  
Storage root: non-production storage root only  
Result: release gate passed

This rehearsal does not create or push a real release tag. It verifies that a beta/pilot candidate can assemble the required evidence bundle and that the rollback and incident paths are operationally explicit before commercial pilot.

## Release Artifact Rehearsal

The rehearsed release artifact bundle contains:

| Artifact item | Rehearsal evidence | Status |
| --- | --- | --- |
| Source commit hash | `c92f067ef1d4af1b7b59a087f3a0d699d214d1d3` | Recorded |
| Candidate tag format | `v0.1.0-beta.rehearsal`; real candidates use `v0.1.0-beta.N` or `v0.1.0-pilot.N` | Recorded |
| Cargo lockfile | `Cargo.lock` from the candidate commit | Present |
| Changelog input | Commits since previous beta/pilot tag or, before first tag, controlled-beta hardening commits | Required for real candidate |
| Release gate output | `./scripts/release-gate.sh` output captured for candidate commit | Rehearsed |
| Compatibility fixture status | Storage/journal fixture acceptance remains linked through [storage-journal-fixture-freeze-acceptance.md](storage-journal-fixture-freeze-acceptance.md) | Recorded |
| Known accepted risks | [m24-beta-hardening-decision.md](m24-beta-hardening-decision.md) and [commercial-pilot-readiness-plan.md](commercial-pilot-readiness-plan.md) | Recorded |

Release gate command:

```bash
./scripts/release-gate.sh
```

Observed result: release gate passed.

## Rollback Rehearsal Evidence

Rollback rehearsal target: non-production storage root.

Rehearsed decision path:

1. Detect candidate failure condition: stable API regression, storage/journal corruption, credential leakage, permission/audit invariant failure, or unreproducible release gate.
2. Stop the host process using the affected storage root.
3. Preserve the current root as incident evidence if allowed by data policy.
4. Restore from the latest verified backup manifest.
5. Run storage/journal verification and replay checks.
6. Re-run `./scripts/release-gate.sh` on rollback or forward-fix commit.
7. Record migration version, backup manifest, restore evidence, owner, and timestamp.

Evidence mapping:

| Rollback evidence | Source |
| --- | --- |
| Backup / restore / verify boundary | `agentos-storage` backup and compatibility tests |
| Journal verification / replay boundary | `conversation-journal` compatibility and verify tests |
| Commercial fixture baseline | [storage-journal-fixture-freeze-acceptance.md](storage-journal-fixture-freeze-acceptance.md) |
| Release quality gate | `./scripts/release-gate.sh` |

Outcome: rollback path is documented and exercised as a tabletop against a non-production storage root. A destructive migration remains blocked unless commercial-pilot approval is recorded.

## Incident Escalation Tabletop

Scenario: S1 audit export/redaction failure found after beta candidate assembly.

Tabletop response:

1. Pause rollout and block tag publication.
2. Assign kernel owner and audit/security owner.
3. Preserve redacted release-gate and failing evidence logs.
4. Confirm whether secret material was exposed; if yes, reclassify to S0 and trigger credential revocation path.
5. Produce rollback or forward-fix plan.
6. Re-run release gate on the fix/rollback commit.
7. Record incident closure and artifact retention/deletion deadline.

Severity mapping:

| Severity | Example trigger | PR204 disposition |
| --- | --- | --- |
| S0 | Credential leak, permission bypass, data loss/corruption | Stop rollout, revoke affected credentials, notify pilot owner |
| S1 | Stable API outage, runtime unrecoverable failure, audit export/redaction failure | Pause rollout, assign owner, produce fix/rollback plan |
| S2 | Degraded connector/browser/model operation with workaround | Document workaround and schedule fix |
| S3 | Docs/test/release automation issue without runtime impact | Fix before next candidate |

## Acceptance

- Release artifact checklist can be produced for a `v0.1.0-beta.N` or `v0.1.0-pilot.N` candidate.
- Rollback rehearsal exists against a non-production storage root.
- Incident escalation tabletop exists for S0/S1 classification and owner handoff.
- The release gate passed on the rehearsal candidate line before PR204 evidence was recorded.
