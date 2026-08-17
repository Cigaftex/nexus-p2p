#!/usr/bin/env bash
set -euo pipefail
repo_dir="$(cd "$(dirname "$0")/.." && pwd)"
cd "$repo_dir"
cargo ndk -t arm64-v8a -t armeabi-v7a -t x86_64 \
  -o apps/nexus_app/android/app/src/main/jniLibs \
  build -p nexus-core --release
