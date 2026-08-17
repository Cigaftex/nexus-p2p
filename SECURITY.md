# Security notes

This is an MVP, not an audited secure messenger.

The cryptographic construction uses established primitives: Ed25519 signatures,
X25519 static Diffie-Hellman, HKDF-SHA256, XChaCha20-Poly1305, and BLAKE3. Static
X25519 keys mean v1 does not provide forward secrecy. A production protocol
should use an audited Noise pattern or Double Ratchet after authenticated
pairing.

Report vulnerabilities privately to the repository owner. Do not include real
private keys, message databases, or sensitive files in a public issue.

Before production deployment: add mutual QR/SAS pairing, replay/clock bounds,
key rotation, secure key storage, encrypted backups, traffic-analysis review,
dependency audit, fuzzing of CBOR/frame parsing, and an independent protocol
review.
