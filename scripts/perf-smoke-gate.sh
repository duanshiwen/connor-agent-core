#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

echo "==> perf smoke: client substrate conversation/run path"
cargo test -p client-substrate client_substrate_can_create_conversation_and_start_run -- --nocapture

echo "==> perf smoke: client substrate data lifecycle contract"
cargo test -p client-substrate data_lifecycle_contract_is_ui_safe -- --nocapture
