#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

echo "==> production safety guards"
cargo test -p client-substrate production_builder_rejects_test_only_components --test commercial_client_substrate
cargo test -p identity-core production

echo "==> approval receipt smoke"
cargo test -p capability-policy --test approval_receipt

echo "==> diagnostics/redaction smoke"
cargo test -p agentos-observability redaction
cargo test -p agentos-observability --test diagnostic_bundle
cargo test -p client-substrate secret

if command -v cargo-deny >/dev/null 2>&1; then
  echo "==> cargo deny"
  cargo deny check
else
  echo "==> cargo-deny not installed; skipping dependency/license audit smoke"
fi

if command -v cargo-sbom >/dev/null 2>&1; then
  echo "==> cargo sbom"
  cargo sbom --output-format spdx_json_2_3 >/tmp/agentos-sbom.spdx.json
else
  echo "==> cargo-sbom not installed; skipping SBOM generation smoke"
fi
