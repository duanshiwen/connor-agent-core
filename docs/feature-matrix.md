# Feature Matrix

This matrix records the minimum release-gate coverage for the stable public API boundary. It is intentionally lightweight: PR167 only automates the presence check and keeps detailed feature certification in crate-level tests.

| Crate | Stable boundary role | Release-gate coverage |
| --- | --- | --- |
| `agentos-kernel` | Host API, kernel error taxonomy, public API stability docs | Workspace tests, release gate docs test, public API docs test |
| `action-runtime` | Policy → execution → audit → conversation lifecycle orchestration | Workspace tests and performance baseline coverage |
| `audit-log` | Audit recording and JSONL export boundary | Workspace tests and export permission/redaction coverage |
| `enterprise-permission-core` | Enterprise grants, user lifecycle, offboarding denial invariants | Workspace tests and concurrency/offboarding coverage |

## Release expectation

A release candidate should run `./scripts/release-gate.sh` from the repository root. The script verifies this matrix exists before running formatting, linting, and the full workspace test suite.
