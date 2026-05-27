# M24 Beta Hardening Decision

This document records the controlled-beta decision for the v2 commercial kernel after M24 readiness review and PR189A tool-loop observability wiring.

## Decision

`connor-agent-core` may enter **controlled beta hardening** when `./scripts/release-gate.sh` passes on the release commit.

This is not a commercial-pilot approval. Controlled beta is limited to host/application integration work where the host team accepts the remaining documented risks and keeps high-risk capabilities behind explicit opt-in gates.

## Stable Boundary Freeze

The following crates enter a beta additive-only stability posture:

- `agentos-kernel`
- `action-runtime`
- `audit-log`
- `enterprise-permission-core`

Rules during beta:

- Additive fields, builder options, and trait extension methods are allowed when existing callers keep compiling.
- Breaking changes require a deprecation note, migration guidance, and compatibility tests.
- Security/privacy fixes may bypass the normal deprecation period only when the replacement path is documented in the same PR.
- Internal module layout, test fakes, and crates outside the stable boundary remain unstable.

## Storage / Journal Freeze Decision

Storage and journal formats are **accepted for controlled beta but not commercial-pilot frozen**.

Beta rules:

- Current compatibility fixtures are treated as beta baseline fixtures.
- Storage layout changes require migration, backup/rollback expectation, release note, and fixture coverage.
- Conversation journal event-shape changes require replay compatibility fixtures.
- Destructive migrations are blocked unless explicitly approved for a commercial pilot.

See [storage-journal-fixture-freeze-policy.md](storage-journal-fixture-freeze-policy.md).

## Remaining Gap Disposition

| Gap | Beta disposition | Commercial pilot disposition | Owner | Exit criteria |
| --- | --- | --- | --- | --- |
| External connector threat reviews and irreversible side-effect tests | Accepted risk for read-only/demo connectors; write connectors blocked | Must be closed for each enabled connector | Connector owner | Per-connector review completed and referenced from high-risk PR |
| Browser automation product-level permission UX | Accepted only behind explicit host opt-in; broad end-user exposure blocked | Must be closed for each enabled browser capability | Host product owner | Browser exposure review completed; permission UX documented |
| Credential storage integration and rotation guidance | Documented and code-level rehearsal recorded | Must be operationally rehearsed by pilot host | Host/security owner | Credential runbook and rehearsal evidence exist; host-level backend rehearsal remains |
| Production telemetry export/retention policy | Must be documented before beta release candidate | Must be enforced by pilot host | Host/security owner | Observability policy exists and names allowed sinks/redaction/retention |
| Storage and journal compatibility fixtures | Accepted with beta fixture policy | Must be frozen or explicitly deferred | Kernel owner | Fixture lifecycle policy exists and release gate remains green |
| Release packaging, changelog, rollback runbooks | Must be documented before beta release candidate | Must be exercised for pilot release | Release owner | Release operations runbook exists and is linked from README |

## Beta Entry Conditions

- `./scripts/release-gate.sh` passes: **required**.
- Stable boundary crates documented in README and feature matrix: **pass**.
- High-risk PRs reference [security-review-checklist.md](security-review-checklist.md): **pass, expanded with M25 docs**.
- No known critical gaps in permission denial, audit export redaction, storage migration, or host-facing error taxonomy: **pass for controlled beta**.
- Minimal host examples compile and run: **covered by release gate/workspace tests**.
- Remaining gaps are closed or explicitly accepted: **pass via this document**.

## Commercial Pilot Blockers

Commercial pilot remains blocked until:

- Connector/browser reviews are complete for every enabled commercial capability.
- Credential runbook is rehearsed by the pilot host using the selected production backend; code-level rehearsal evidence exists in [credential-operations-rehearsal.md](credential-operations-rehearsal.md).
- Production observability export and retention policy is enforced by the pilot host.
- Release rollback and incident escalation runbooks are exercised.
- Storage/journal fixture freeze is accepted as a commercial-pilot compatibility contract.
