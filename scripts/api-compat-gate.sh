#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

echo "==> public API compatibility smoke"
cargo test -p client-substrate public_api_version_is_stable_v1 --test commercial_client_substrate
cargo test -p agentos-kernel public_api --test public_api_docs
cargo test -p agentos-kernel --test kernel_event_store
cargo test -p agent-runtime --test run_journal
