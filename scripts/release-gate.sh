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

echo "==> feature matrix check"
test -f docs/feature-matrix.md
grep -q "agentos-kernel" docs/feature-matrix.md
grep -q "action-runtime" docs/feature-matrix.md
grep -q "audit-log" docs/feature-matrix.md
grep -q "enterprise-permission-core" docs/feature-matrix.md

echo "==> cargo fmt --all --check"
cargo fmt --all --check

echo "==> cargo clippy --workspace -- -D warnings"
cargo clippy --workspace -- -D warnings

echo "==> cargo test --workspace"
cargo test --workspace

echo "release gate passed"
