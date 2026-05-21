#!/usr/bin/env bash
# Build the cef-host sidecar and bundle it into a macOS .app
# (Chromium Embedded Framework + helper subprocesses).
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
CEF_RS_PATH="${CEF_RS_PATH:-$HOME/cef-rs}"

cd "$ROOT/cef-host"

# bundle-cef-app reads `package.metadata.cef.bundle` from the *current* Cargo manifest
# (so cwd must be the cef-host crate), then invokes `cargo build --bin <name>` for both
# the main executable and the helper, and finally constructs the macOS bundle layout.
cargo run \
    --manifest-path "$CEF_RS_PATH/Cargo.toml" \
    -p cef \
    --bin bundle-cef-app \
    --features build-util \
    -- \
    cef-host \
    -o "$ROOT/cef-host/target/bundle"

echo
echo "Built: $ROOT/cef-host/target/bundle/cef-host.app"
