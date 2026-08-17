#!/usr/bin/env bash
set -euo pipefail
repo_dir="$(cd "$(dirname "$0")/.." && pwd)"
cd "$repo_dir"
rustup target add aarch64-apple-ios
cargo build -p nexus-core --release --target aarch64-apple-ios
cp target/aarch64-apple-ios/release/libnexus_core.a apps/nexus_app/ios/libnexus_core.a
echo "Device library copied to apps/nexus_app/ios/libnexus_core.a."
