#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/../rust/media-control-bridge"
rustup target add x86_64-unknown-linux-gnu >/dev/null 2>&1 || true
cargo build --release --target x86_64-unknown-linux-gnu
mkdir -p ../../dist
cp target/x86_64-unknown-linux-gnu/release/media-control-bridge ../../dist/media-control-bridge-x86_64-linux
