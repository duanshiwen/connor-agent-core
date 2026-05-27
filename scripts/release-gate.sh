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
test -f docs/host-api-freeze.md
grep -q "## Stable Host-Facing Boundary" docs/host-api-freeze.md
grep -q "## Compatibility Rules" docs/host-api-freeze.md
grep -q "## Pilot Acceptance Status" docs/host-api-freeze.md
test -f docs/commercial-pilot-readiness-plan.md
grep -q "## Proposed PR Plan" docs/commercial-pilot-readiness-plan.md
grep -q "PR200: Beta/Commercial Host API Freeze Acceptance" docs/commercial-pilot-readiness-plan.md
test -f docs/credential-operations-runbook.md
grep -q "## Offboarding Sequence" docs/credential-operations-runbook.md
grep -q "## Host-Level Pilot Backend Decision" docs/credential-operations-runbook.md
test -f docs/credential-operations-rehearsal.md
grep -q "## Rehearsal Scope" docs/credential-operations-rehearsal.md
grep -q "## Host-Level Pilot Rehearsal Evidence" docs/credential-operations-rehearsal.md
test -f docs/production-observability-policy.md
grep -q "## Redaction Requirements" docs/production-observability-policy.md
grep -q "## Production-Like File Export Sink" docs/production-observability-policy.md
grep -q "## PR211 Pilot Observability Operations Drill" docs/production-observability-policy.md
grep -q "PilotObservabilityOperationsDrill" docs/production-observability-policy.md
test -f docs/release-operations-runbook.md
grep -q "## Rollback Decision Tree" docs/release-operations-runbook.md
test -f docs/release-artifact-rollback-rehearsal.md
grep -q "## Release Artifact Rehearsal" docs/release-artifact-rollback-rehearsal.md
grep -q "## Rollback Rehearsal Evidence" docs/release-artifact-rollback-rehearsal.md
grep -q "## Incident Escalation Tabletop" docs/release-artifact-rollback-rehearsal.md
test -f docs/pilot-release-rollback-incident-exercise.md
grep -q "## Pilot Release Candidate Exercise" docs/pilot-release-rollback-incident-exercise.md
grep -q "## Pilot Rollback Exercise" docs/pilot-release-rollback-incident-exercise.md
grep -q "## Pilot Incident Exercise" docs/pilot-release-rollback-incident-exercise.md
test -f docs/connector-browser-risk-review-templates.md
grep -q "## Per-Connector Threat Review Template" docs/connector-browser-risk-review-templates.md
test -f docs/connector-browser-commercial-review-evidence.md
grep -q "## Gmail Read-Only Connector Review Evidence" docs/connector-browser-commercial-review-evidence.md
grep -q "## PR206 OAuth Provider Lifecycle Evidence" docs/connector-browser-commercial-review-evidence.md
grep -q "## PR207 Gmail Retry Timeout Rate-Limit Evidence" docs/connector-browser-commercial-review-evidence.md
grep -q "## PR208 Gmail Host Audit and Offboarding Evidence" docs/connector-browser-commercial-review-evidence.md
grep -q "## Browser Kernel Current Capability Review Evidence" docs/connector-browser-commercial-review-evidence.md
grep -q "## PR209 Browser Pilot Permission Profile Evidence" docs/connector-browser-commercial-review-evidence.md
test -f docs/storage-journal-fixture-freeze-policy.md
grep -q "## Required PR Checklist for Persisted Shape Changes" docs/storage-journal-fixture-freeze-policy.md
test -f docs/storage-journal-fixture-freeze-acceptance.md
grep -q "## Acceptance Scope" docs/storage-journal-fixture-freeze-acceptance.md
grep -q "## Evidence Commands" docs/storage-journal-fixture-freeze-acceptance.md
grep -q "## Commercial-Pilot Fixture Freeze Acceptance" docs/storage-journal-fixture-freeze-acceptance.md
grep -q "## Long-Lived Fixture Support Policy" docs/storage-journal-fixture-freeze-acceptance.md
grep -q "## Migration Release Note Template" docs/storage-journal-fixture-freeze-acceptance.md

echo "==> feature matrix check"
test -f docs/feature-matrix.md
grep -q "agentos-kernel" docs/feature-matrix.md
grep -q "action-runtime" docs/feature-matrix.md
grep -q "audit-log" docs/feature-matrix.md
grep -q "enterprise-permission-core" docs/feature-matrix.md
grep -q "m24-beta-hardening-decision" docs/feature-matrix.md
grep -q "host-api-freeze.md" docs/feature-matrix.md
grep -q "commercial-pilot-readiness-plan.md" docs/feature-matrix.md
grep -q "storage-journal-fixture-freeze-policy" docs/feature-matrix.md
grep -q "storage-journal-fixture-freeze-acceptance" docs/feature-matrix.md
grep -q "credential-operations-rehearsal" docs/feature-matrix.md
grep -q "connector-browser-commercial-review-evidence" docs/feature-matrix.md

echo "==> cargo check host examples"
cargo check -p agentos-kernel --example minimal-cli-host
cargo check -p agentos-kernel --example minimal-server-host
cargo check -p agentos-kernel --example minimal-desktop-host

echo "==> cargo fmt --all --check"
cargo fmt --all --check

echo "==> cargo clippy --workspace -- -D warnings"
cargo clippy --workspace -- -D warnings

echo "==> cargo test --workspace"
cargo test --workspace

echo "release gate passed"
