#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

echo "==> docs check: release checklist"
grep -q "## Release Checklist" README.md
grep -q "./scripts/release-gate.sh" README.md
test -f docs/security-review-checklist.md
grep -q "High-risk PRs must reference this checklist" docs/security-review-checklist.md
test -f docs/v2-commercial-readiness-review.md
grep -q "## Commercial Pilot Entry Conditions" docs/v2-commercial-readiness-review.md
test -f docs/m24-beta-hardening-decision.md
grep -q "## Remaining Gap Disposition" docs/m24-beta-hardening-decision.md
test -f docs/credential-operations-runbook.md
grep -q "## Offboarding Sequence" docs/credential-operations-runbook.md
test -f docs/credential-operations-rehearsal.md
grep -q "## Rehearsal Scope" docs/credential-operations-rehearsal.md
test -f docs/production-observability-policy.md
grep -q "## Redaction Requirements" docs/production-observability-policy.md
test -f docs/release-operations-runbook.md
grep -q "## Rollback Decision Tree" docs/release-operations-runbook.md
test -f docs/connector-browser-risk-review-templates.md
grep -q "## Per-Connector Threat Review Template" docs/connector-browser-risk-review-templates.md
test -f docs/storage-journal-fixture-freeze-policy.md
grep -q "## Required PR Checklist for Persisted Shape Changes" docs/storage-journal-fixture-freeze-policy.md

echo "==> feature matrix check"
test -f docs/feature-matrix.md
grep -q "agentos-kernel" docs/feature-matrix.md
grep -q "action-runtime" docs/feature-matrix.md
grep -q "audit-log" docs/feature-matrix.md
grep -q "enterprise-permission-core" docs/feature-matrix.md
grep -q "m24-beta-hardening-decision" docs/feature-matrix.md
grep -q "storage-journal-fixture-freeze-policy" docs/feature-matrix.md
grep -q "credential-operations-rehearsal" docs/feature-matrix.md

echo "==> cargo fmt --all --check"
cargo fmt --all --check

echo "==> cargo clippy --workspace -- -D warnings"
cargo clippy --workspace -- -D warnings

echo "==> cargo test --workspace"
cargo test --workspace

echo "release gate passed"
