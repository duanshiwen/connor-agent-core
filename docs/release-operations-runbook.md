# Release Operations Runbook

This runbook defines the beta/commercial-pilot release process for `connor-agent-core`.

Rehearsal evidence: [release-artifact-rollback-rehearsal.md](release-artifact-rollback-rehearsal.md)

## Release Gate

`./scripts/release-gate.sh` is the mandatory pre-release quality gate. It is not complete packaging automation.

Run from the repository root:

```bash
./scripts/release-gate.sh
```

The release commit is eligible only if the gate passes without warnings treated as failures by clippy.

## Version Tagging

Recommended beta tag format:

```text
v0.1.0-beta.N
```

Recommended commercial-pilot tag format:

```text
v0.1.0-pilot.N
```

Tag only the commit that passed the release gate.

## Changelog Generation

For each candidate:

1. Collect commits since the previous beta/pilot tag.
2. Group changes by category:
   - Stable API boundary
   - Storage/journal compatibility
   - Credential/security
   - Connector/browser
   - Observability/diagnostics
   - Release/docs/tests
3. Call out breaking changes, deprecations, migrations, and accepted risks.
4. Link security checklist references for high-risk PRs.
5. Record release-gate evidence.

## Release Artifact Checklist

A beta release artifact set should include:

- Source commit hash and tag.
- `Cargo.lock` from the release commit.
- Changelog.
- Release-gate output.
- Compatibility fixture status.
- Known accepted risks from [m24-beta-hardening-decision.md](m24-beta-hardening-decision.md).

## Rollback Decision Tree

Rollback immediately when:

- Stable host API behavior regresses.
- Storage/journal migration corrupts or loses data.
- Credential leakage is suspected.
- Permission denial or audit redaction invariant fails.
- Release gate cannot be reproduced on the tagged commit.

Prefer forward fix when:

- The issue is docs-only and not misleading for security/operations.
- The issue affects only unstable/internal APIs.
- A compatibility fixture can be added without changing persisted data.

## Storage / Journal Rollback

1. Stop the host process using the affected storage root.
2. Preserve the current root as incident evidence if allowed by data policy.
3. Restore from the latest verified backup manifest.
4. Run storage/journal verification and replay checks.
5. Re-run release gate on the rollback/fix commit.
6. Record migration version, backup manifest, and restore evidence.

Destructive migrations require explicit commercial-pilot approval and are otherwise blocked.

## Incident Escalation

Severity classes:

- `S0`: credential leak, permission bypass, data loss/corruption.
- `S1`: stable API outage, unrecoverable runtime failure, audit export/redaction failure.
- `S2`: degraded connector/browser/model operation with workaround.
- `S3`: docs/test/release automation issue without runtime impact.

Escalation expectations:

- S0: stop release/pilot rollout, revoke affected credentials, notify pilot owner, preserve evidence.
- S1: pause rollout, assign kernel owner, produce fix/rollback plan.
- S2: document workaround and schedule fix.
- S3: fix before next release candidate.

## Beta vs Commercial Pilot Approval

Controlled beta requires:

- Release gate passed.
- Stable boundary documented.
- Accepted risks recorded.
- High-risk PRs cite the security checklist.

Commercial pilot additionally requires:

- Connector/browser reviews for enabled capabilities.
- Credential operations runbook rehearsed.
- Production observability policy enforced by the host.
- Storage/journal fixture freeze accepted.
- Rollback path exercised at least once against a non-production fixture.
