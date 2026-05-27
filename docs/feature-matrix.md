# Feature Matrix

This matrix records the minimum release-gate coverage for the stable public API boundary. It also links the M25 controlled-beta posture docs that turn the M24 readiness review into operational gates. Detailed feature certification remains in crate-level tests.

| Crate | Stable boundary role | Release-gate coverage |
| --- | --- | --- |
| `agentos-kernel` | Host API, kernel error taxonomy, public API stability docs | Workspace tests, release gate docs test, public API docs test |
| `action-runtime` | Policy → execution → audit → conversation lifecycle orchestration | Workspace tests and performance baseline coverage |
| `audit-log` | Audit recording and JSONL export boundary | Workspace tests and export permission/redaction coverage |
| `enterprise-permission-core` | Enterprise grants, user lifecycle, offboarding denial invariants | Workspace tests and concurrency/offboarding coverage |

## Beta hardening posture

Controlled beta is governed by:

- [m24-beta-hardening-decision.md](m24-beta-hardening-decision.md)
- [credential-operations-runbook.md](credential-operations-runbook.md)
- [credential-operations-rehearsal.md](credential-operations-rehearsal.md)
- [production-observability-policy.md](production-observability-policy.md)
- [release-operations-runbook.md](release-operations-runbook.md)
- [connector-browser-risk-review-templates.md](connector-browser-risk-review-templates.md)
- [connector-browser-commercial-review-evidence.md](connector-browser-commercial-review-evidence.md)
- [storage-journal-fixture-freeze-policy.md](storage-journal-fixture-freeze-policy.md)
- [storage-journal-fixture-freeze-acceptance.md](storage-journal-fixture-freeze-acceptance.md)

## Release expectation

A release candidate should run `./scripts/release-gate.sh` from the repository root. The script verifies this matrix and M25 beta hardening docs exist before running formatting, linting, and the full workspace test suite.
