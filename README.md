# Nexus MVP

Nexus is an offline-first nearby P2P messenger for Android, iOS, Windows, and
macOS. It uses Flutter for UI, Rust for identity/protocol/crypto/storage/sync,
and narrow native adapter contracts for platform radio APIs. It has no central
server and no blockchain.

The runnable MVP uses mDNS discovery and direct TCP. On Apple platforms the
native Bonjour adapter includes peer-to-peer Bluetooth/Wi-Fi discovery even
without a shared router, while BLE RSSI supplies a coarse proximity indicator.
Android Wi-Fi Aware/Wi-Fi Direct remain the next native bearer adapters.

## What works

- Persistent local Ed25519 identity and X25519 exchange key.
- Unified discovery/data transport traits with working mDNS + TCP.
- Nearby pairing (initiator confirmation + MVP receiver TOFU).
- Ed25519-signed, XChaCha20-Poly1305 encrypted direct text events.
- SQLite WAL append-only event log with deterministic CRDT merge order.
- 256 KiB content-addressed file chunks, encrypted manifest, BLAKE3 chunk and
  whole-file verification, atomic writes, batching, and resume by missing hash.
- Automatic two-way event sync and incomplete-file recovery when a paired peer
  is rediscovered.
- Flutter nearby-device and chat/file UI through a compact C JSON FFI.
- Persistent custom device names and coarse BLE near/nearby/far status on Apple.
- A CLI and an automated two-node loopback test for headless verification.

## Repository map

```text
apps/nexus_app/             Flutter UI + Dart FFI bridge
core/nexus-core/            Rust identity, crypto, protocol, store, sync, FFI
native/adapters/            Kotlin/Swift radio adapter contracts
tools/nexus-cli/            Headless node for real two-device LAN testing
docs/protocol.md            Wire protocol and security model
scripts/                    Native library build helpers
```

## Prerequisites

- Rust 1.85+ (`rustup` honors `rust-toolchain.toml`)
- Flutter stable with the desired platform toolchains
- Two devices with Wi-Fi enabled; Apple peer-to-peer discovery can work without
  a shared router. LAN testing remains useful for Android/Windows today.

## Verify the core

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

`core/nexus-core/tests/two_nodes.rs` starts two real TCP listeners, pairs them,
exchanges encrypted text in both directions, transfers a multi-chunk file,
materializes and verifies it, and performs a sync.

On a multicast-capable development machine, verify real mDNS discovery with:

```bash
cargo test -p nexus-core --test mdns_smoke -- --ignored
```

## Run two CLI devices

Open two terminals on two machines (or use ports `47777` and `47778` locally):

```bash
cargo run -p nexus-cli -- ./data/alice Alice 47777
cargo run -p nexus-cli -- ./data/bob Bob 47778
```

Use `peers`, then `pair DEVICE_ID`, `say DEVICE_ID hello`, or
`file DEVICE_ID /absolute/path/to/file`. Paired devices sync automatically when
mDNS sees them again.

On macOS, allow Local Network access and incoming connections if prompted. On
Windows, allow the executable on Private networks. Routers with client/AP
isolation prevent peer reachability even though both devices show Wi-Fi.

If `xcrun` cannot find `xcodebuild` even though Xcode is installed, the active
developer directory may still point to Command Line Tools. Prefix commands with
`DEVELOPER_DIR=/Applications/Xcode.app/Contents/Developer`, or deliberately
change the global selection with `sudo xcode-select -s
/Applications/Xcode.app/Contents/Developer`.

## Run Flutter

The Android, iOS, macOS, and Windows runners are included. Install packages:

```bash
cd apps/nexus_app
flutter pub get
```

On Apple hosts whose global developer directory still points to Command Line
Tools, apply the `DEVELOPER_DIR` prefix to `flutter pub get`, `flutter build`,
and `flutter run` so Flutter native-asset hooks use the full Xcode SDK.

Build the Rust library for the host and place it where the runner can load it:

```bash
# macOS
cd ../..
./scripts/build_host_core.sh
cd apps/nexus_app
DEVELOPER_DIR=/Applications/Xcode.app/Contents/Developer flutter run -d macos

# Windows (run from a Developer PowerShell/Git Bash)
cargo build -p nexus-core --release
# Copy target/release/nexus_core.dll beside the built app executable.
flutter run -d windows
```

The macOS Xcode project embeds and signs `libnexus_core.dylib` from the Flutter
app directory. For Android, install `cargo-ndk`, add Rust Android targets, then run
`scripts/build_android_core.sh`; it writes ABI libraries under
`android/app/src/main/jniLibs`. For iOS, use `scripts/build_apple_core.sh` on
macOS. The Runner target already force-loads the generated device static
library and exports the C FFI symbols. Select your own Apple Development team
in Xcode before deploying to a device. Simulator archives are intentionally not
built by the MVP helper because the checked-in runner links the device archive.
The C surface is declared in `core/nexus-core/include/nexus_core.h`.

Required app permissions/descriptions after runner generation:

- Android: `INTERNET`, `ACCESS_NETWORK_STATE`, `CHANGE_WIFI_MULTICAST_STATE`,
  and Android-version-appropriate nearby Wi-Fi/Bluetooth permissions.
- iOS/macOS: `NSLocalNetworkUsageDescription`, Bonjour service
  `_nexus-p2p._tcp`, Bluetooth usage descriptions, and the macOS Bluetooth
  sandbox entitlement are already configured.

## Storage layout

Each app support directory contains `nexus.db` (WAL SQLite) and
`blobs/<prefix>/<blake3>`. Private key bytes never cross the Rust FFI. This MVP
stores them in the local database; production must wrap them with Keychain/
Secure Enclave on Apple and Android Keystore, and DPAPI on Windows.

## Known MVP constraints

- Receiver-side pairing auto-accepts a valid locally discovered identity (TOFU).
  Add a symmetric QR/SAS confirmation before treating pairing as production-safe.
- Direct messages are a grow-only event set. Tombstones, edits, group membership,
  causal vectors, retention, and compaction are not in v1.
- TCP is the implemented bulk bearer. Apple peer-to-peer Bonjour can supply a
  direct Wi-Fi/Bluetooth route; BLE is used for discovery/proximity rather than
  large file payloads. Android Wi-Fi Aware/Direct remains an adapter contract.
- FFI calls are synchronous. Move large file operations to a Dart isolate before
  shipping a polished mobile experience.
- Ordinary LAN mDNS can be blocked by guest Wi-Fi isolation; Apple peer-to-peer
  discovery also requires Wi-Fi/Bluetooth and permission to remain enabled.

## Iteration backlog

1. Replace TOFU auto-accept with QR/SAS mutual confirmation and key-change UI.
2. Store private keys behind platform secure hardware/credential stores.
3. Wire Kotlin BLE discovery, Wi-Fi Aware, then Wi-Fi Direct into
   `PlatformRadioAdapter`; add Android foreground transfer service.
4. Wire CoreBluetooth discovery and Network.framework `includePeerToPeer`, with
   MultipeerConnectivity fallback; add iOS background-state UX.
5. Add QUIC multiplexing, adaptive bearer selection, congestion/backpressure,
   cancellation, and transfer speed/ETA events.
6. Add group membership CRDT, delivery/read receipts, event tombstones,
   snapshots/compaction, and bounded sync summaries (Bloom/IBLT or Merkle ranges).
7. Harden metadata privacy, rate limits, replay windows, encrypted-at-rest fields,
   fuzzing, protocol compatibility fixtures, and external security review.
8. Add Linux packaging after the four primary targets are stable.

See [the protocol notes](docs/protocol.md) and [security notes](SECURITY.md).
