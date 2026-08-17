# Nexus protocol v1

Every TCP request is one big-endian `u32` length followed by a CBOR
`WireEnvelope`; every request receives exactly one response. The 8 MiB frame
limit and 16-chunk batches bound memory use.

## Identity and pairing

A device ID is the first 128 bits of BLAKE3 over its Ed25519 public key. The
advertisement contains device ID, display name, Ed25519 key, X25519 key,
protocol version, and TCP port. The receiver recomputes the ID before trusting
the announcement.

MVP pairing is explicit on the initiating device and trust-on-first-use on the
receiver. Production pairing must replace receiver auto-accept with a QR code
or short authentication string displayed on both devices.

## Encrypted events

Direct stream IDs are deterministic from two sorted device IDs. X25519 creates
the shared secret; HKDF-SHA256 with `nexus-p2p-v1` and the stream ID derives a
32-byte conversation key. XChaCha20-Poly1305 encrypts the payload. Event
metadata is authenticated as AAD and the complete encrypted envelope is signed
with Ed25519.

The append-only event log is a grow-only CRDT: event ID is a ULID, insertion is
idempotent, and display order is `(created_at_ms, event_id)`. Deletion and group
membership CRDTs are intentionally deferred.

## Reconnect sync

Peers exchange known event IDs and locally authored missing events. Duplicates
are ignored by SQLite's primary key. Discovery of an already paired peer
automatically triggers this exchange.

## Files

Files are split into 256 KiB chunks. Every wire chunk uses the conversation key
with an independent XChaCha20-Poly1305 nonce; manifest ID and plaintext BLAKE3
hash are authenticated as AAD. BLAKE3 validates decrypted content before atomic
placement in the blob store. The encrypted manifest includes
ordered chunk hashes, sizes, whole-file hash, media type, and name. Initial
transfer pushes batches; reconnect sync requests only missing hashes. Final
materialization rechecks every chunk and the whole-file hash.
