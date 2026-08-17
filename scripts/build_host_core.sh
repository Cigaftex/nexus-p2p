#!/usr/bin/env bash
set -euo pipefail
repo_dir="$(cd "$(dirname "$0")/.." && pwd)"
cd "$repo_dir"
cargo build -p nexus-core --release
case "$(uname -s)" in
  Darwin)
    cp target/release/libnexus_core.dylib apps/nexus_app/
    ;;
  Linux)
    mkdir -p apps/nexus_app/linux/lib
    cp target/release/libnexus_core.so apps/nexus_app/linux/lib/
    ;;
  *)
    cp target/release/nexus_core.dll apps/nexus_app/nexus_core.dll
    echo "For release packaging also copy nexus_core.dll beside the app executable."
    ;;
esac
