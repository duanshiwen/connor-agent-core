#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

if [[ -z "${AGENTOS_CONNECTOR_SMOKE:-}" ]]; then
  echo "connector smoke gate skipped: AGENTOS_CONNECTOR_SMOKE not configured"
  exit 0
fi

echo "connector smoke gate has no real connector suite configured yet; deterministic connector tests run in workspace gate"
