# Pilot Release, Rollback, and Incident Exercise

This document records PR212 evidence for a reproducible pilot release candidate exercise, rollback exercise, and S0/S1 incident tabletop. It does not create or push a real release tag.

Related documents:

- [release-operations-runbook.md](release-operations-runbook.md)
- [release-artifact-rollback-rehearsal.md](release-artifact-rollback-rehearsal.md)
- [storage-journal-fixture-freeze-acceptance.md](storage-journal-fixture-freeze-acceptance.md)
- [production-observability-policy.md](production-observability-policy.md)
- [commercial-pilot-readiness-plan.md](commercial-pilot-readiness-plan.md)

## Exercise Scope

Date: 2026-05-28
Candidate type: pilot release exercise, not a published tag
Candidate tag shape: `v0.1.0-pilot.0-exercise`
Candidate commit: `41c507ace1167393af9e2656c5de246df1906fb6`
Storage root: non-production storage root only
Result: release gate passed

The exercise verifies that the first pilot candidate can be reproduced from a tag-shaped identifier and commit, that rollback and incident response paths are operationally explicit, and that evidence can be assembled without leaking credentials, connector content, browser artifacts, or telemetry secrets.

## Pilot Release Candidate Exercise

Candidate bundle checklist:

| Artifact item | Pilot exercise evidence | Status |
| --- | --- | --- |
| Candidate tag shape | `v0.1.0-pilot.0-exercise`; real candidates use `v0.1.0-pilot.N` | Recorded |
| Source commit hash | `41c507ace1167393af9e2656c5de246df1906fb6` | Recorded |
| Changelog window | PR200 through PR212 commercial-pilot readiness commits, plus any final host-product integration commits before a real tag | Recorded |
| Release gate output archive | `./scripts/release-gate.sh` output captured in session evidence; summary result: release gate passed | Rehearsed |
| Storage/journal fixture status | storage/journal fixture baseline accepted in PR205 | Recorded |
| Credential backend status | host credential backend decision and rehearsal recorded in PR202 | Recorded |
| Observability status | JSONL file sink and pilot operations drill recorded in PR203 and PR211 | Recorded |
| Connector/Gmail status | OAuth lifecycle, retry policy, host audit, and offboarding evidence recorded in PR206-PR208 | Recorded |
| Browser status | broad exposure disabled by PR209 pilot profile; PR210 required only if enabling broad browser exposure | Recorded |
| Known accepted risks | Browser broad exposure disabled; real host must wire credential backend, account lifecycle, audit backend, and telemetry access controls before a real pilot tag | Recorded |

Release gate command:

```bash
./scripts/release-gate.sh
```

Observed result: release gate passed.

## Pilot Rollback Exercise

Rollback target: non-production storage root and tag-shaped pilot candidate.

Exercised decision path:

1. Detect pilot candidate failure condition: release gate regression, storage/journal corruption, credential leak, connector offboarding failure, telemetry redaction failure, permission/audit invariant failure, or unreproducible artifact.
2. Stop host process for the affected non-production storage root.
3. Preserve candidate commit, release gate output, storage manifest, and redacted observability metadata as incident evidence when allowed by data policy.
4. Restore from latest verified backup manifest.
5. Run storage/journal verification and replay checks against the restored root.
6. Re-run `./scripts/release-gate.sh` on rollback or forward-fix commit.
7. Record owner, timestamp, migration version, backup manifest, restore evidence, release gate result, and incident retention/deletion deadline.

Evidence mapping:

| Rollback evidence | Source |
| --- | --- |
| Rollback decision tree | [release-operations-runbook.md](release-operations-runbook.md) |
| PR204 rehearsal | [release-artifact-rollback-rehearsal.md](release-artifact-rollback-rehearsal.md) |
| Storage/journal fixture baseline | [storage-journal-fixture-freeze-acceptance.md](storage-journal-fixture-freeze-acceptance.md) |
| Telemetry incident/debug-bundle controls | [production-observability-policy.md](production-observability-policy.md) |
| Release quality gate | `./scripts/release-gate.sh` |

Outcome: rollback decision path is exercised for the pilot candidate shape. Destructive migrations remain blocked unless pilot owner approval, migration evidence, fixture update, backup manifest, rollback plan, and release gate result are recorded.

## Pilot Incident Exercise

Two tabletop scenarios were exercised for first-pilot readiness.

### S0 credential leak or data-loss scenario

Trigger: plaintext OAuth token, refresh token, credential blob, or unrecoverable storage/journal corruption is found in release artifact, diagnostics, telemetry, audit export, or incident evidence.

Response path:

1. Stop rollout and block tag publication.
2. Assign release owner, credential/security owner, and storage/kernel owner as applicable.
3. Preserve redacted release evidence and affected artifact manifests.
4. Revoke affected credentials through the provider-shaped revocation/offboarding path.
5. Restore or quarantine affected storage root from verified backup manifest.
6. Produce rollback or forward-fix commit.
7. Re-run `./scripts/release-gate.sh`.
8. Notify pilot owner before resuming any candidate process.
9. Record closure, credential rotation evidence, storage restore evidence, and deletion deadline for incident artifacts.

### S1 telemetry redaction or audit export failure scenario

Trigger: telemetry/debug bundle/audit export includes unauthorized connector content, browser DOM/screenshot/page text, model prompt/output payload, denied resource metadata, or a redaction policy regression without confirmed secret exposure.

Response path:

1. Pause rollout and block pilot tag publication.
2. Assign observability owner and audit/security owner.
3. Preserve redacted release-gate output and failing metadata-only evidence.
4. Confirm whether secret material was exposed; if yes, reclassify to S0.
5. Produce fix or rollback plan.
6. Re-run `./scripts/release-gate.sh` and targeted redaction/audit tests.
7. Record incident owner, fix/rollback commit, access audit, expiration, and deletion deadline.

## Go/No-Go Inputs

Pilot go/no-go review must include:

- release candidate tag shape and commit hash;
- release gate output archive with result `release gate passed`;
- storage/journal fixture baseline accepted status;
- credential backend decision and offboarding/revocation evidence;
- Gmail connector retry/audit/offboarding evidence;
- observability file export, retention, admin-only access, tenant partitioning, incident audit, and debug-bundle workflow evidence;
- browser broad exposure decision: disabled by default for the first pilot unless PR210/product evidence is completed;
- rollback decision record and incident escalation owner list;
- known accepted risks and explicit pilot owner approval before creating a real `v0.1.0-pilot.N` tag.

## Acceptance

- Pilot release candidate exercise is reproducible from `v0.1.0-pilot.0-exercise` and commit `41c507ace1167393af9e2656c5de246df1906fb6`.
- Changelog window, release gate output, fixture status, credential status, connector status, observability status, browser status, and accepted risks are recorded.
- Pilot rollback exercise covers stop, preserve, restore, verify, release-gate rerun, owner, and deletion deadline.
- S0 and S1 incident exercise paths are recorded with owner handoff and escalation criteria.
- Release gate passed on the exercised candidate line.
