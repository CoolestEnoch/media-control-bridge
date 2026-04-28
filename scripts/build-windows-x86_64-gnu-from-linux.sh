#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/../rust/media-control-bridge"
rustup target add x86_64-pc-windows-gnu >/dev/null 2>&1 || true
cargo build --release --target x86_64-pc-windows-gnu
mkdir -p ../../dist
cp target/x86_64-pc-windows-gnu/release/media-control-bridge.exe ../../dist/media-control-bridge-x86_64-windows-gnu.exe
