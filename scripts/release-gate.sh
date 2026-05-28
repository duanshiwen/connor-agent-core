#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

echo "==> release contract check"
test -f schemas/release-contract.toml
grep -q "gate_script = \"./scripts/release-gate.sh\"" schemas/release-contract.toml
grep -q "stable_crates" schemas/release-contract.toml

echo "==> cargo check host examples"
cargo check -p agentos-kernel --example minimal-cli-host
cargo check -p agentos-kernel --example minimal-server-host
cargo check -p agentos-kernel --example minimal-desktop-host

echo "==> cargo check client substrate"
cargo check -p client-substrate --all-targets
cargo check -p client-substrate --example minimal-commercial-client-host

./scripts/api-compat-gate.sh
./scripts/client-substrate-gate.sh
./scripts/bridge-gate.sh
./scripts/security-smoke-gate.sh

echo "==> cargo fmt --all --check"
cargo fmt --all --check

echo "==> cargo clippy --workspace --all-targets -- -D warnings"
cargo clippy --workspace --all-targets -- -D warnings

echo "==> cargo test --workspace"
cargo test --workspace

echo "==> perf smoke gate"
./scripts/perf-smoke-gate.sh

echo "release gate passed"
