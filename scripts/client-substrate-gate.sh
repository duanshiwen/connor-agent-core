#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

echo "==> client substrate tests"
cargo test -p client-substrate --all-targets

echo "==> client substrate production guard smoke"
cargo test -p client-substrate production_builder --test commercial_client_substrate
