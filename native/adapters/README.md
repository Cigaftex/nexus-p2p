# Platform transport adapters

These adapters are deliberately radio-only. They discover peers, establish a
byte stream, and report capabilities. Identity, pairing state, encryption,
events, file manifests, and sync remain in `nexus-core`.

Current MVP data path: Rust `LanTcpTransport` with native Apple Bonjour/BLE
discovery in the iOS and macOS runners. Other platforms currently use Rust
`LanMdnsAdapter` discovery.

Reserved platform paths:

- Android: BLE discovery, then Wi-Fi Aware (API 26+) or Wi-Fi Direct.
- Apple: implemented peer-to-peer Bonjour plus BLE proximity in each Runner.
- Windows: mDNS + TCP now; native BLE discovery is a future fallback.

Native adapters must never expose long-term private keys. They return a
`PeerEndpoint` and opaque connection handle to the Rust transport manager.
