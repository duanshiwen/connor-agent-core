#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

if [[ -z "${AGENTOS_OPENAI_COMPAT_SMOKE_URL:-}" && -z "${AGENTOS_ANTHROPIC_SMOKE_URL:-}" ]]; then
  echo "provider smoke gate skipped: no real provider env configured"
  exit 0
fi

echo "==> provider smoke tests"
cargo test -p model-adapter --test provider_compatibility_matrix -- --ignored --nocapture
