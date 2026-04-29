#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
CARGO_BIN="/Users/jung/.cargo/bin/cargo"

cd "$ROOT_DIR"

"$CARGO_BIN" fmt
"$CARGO_BIN" clippy --workspace --all-targets -- -D warnings
"$CARGO_BIN" install --path crates/oml-cli --force
