#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

echo "==> bridge crate check"
cargo check -p agentos-client-bridge --all-targets

echo "==> bridge contract tests"
cargo test -p agentos-client-bridge --all-targets
cargo test -p agentos-client-bridge --test schema_contract
