use chacha20poly1305::{
    aead::{Aead, KeyInit, Payload},
    XChaCha20Poly1305, XNonce,
};
use hkdf::Hkdf;
use rand_core::{OsRng, RngCore};
use sha2::Sha256;

use crate::{
    identity::{verify, Identity, PublicIdentity},
    model::{DecryptedPayload, EventEnvelope, EventKind},
};

pub fn conversation_key(
    identity: &Identity,
    peer: &PublicIdentity,
    stream_id: &str,
) -> anyhow::Result<[u8; 32]> {
    let shared = identity.shared_secret(&peer.exchange_public_key);
    let hk = Hkdf::<Sha256>::new(Some(b"nexus-p2p-v1"), &shared);
    let mut key = [0_u8; 32];
    hk.expand(stream_id.as_bytes(), &mut key)
        .map_err(|_| anyhow::anyhow!("HKDF output length is invalid"))?;
    Ok(key)
}

fn aad(id: &str, stream_id: &str, author: &str, created_at_ms: i64, kind: EventKind) -> Vec<u8> {
    format!(
        "nexus/v1/{id}/{stream_id}/{author}/{created_at_ms}/{}",
        kind as i32
    )
    .into_bytes()
}

pub fn create_event(
    identity: &Identity,
    peer: &PublicIdentity,
    stream_id: String,
    kind: EventKind,
    payload: &DecryptedPayload,
) -> anyhow::Result<EventEnvelope> {
    let id = ulid::Ulid::new().to_string();
    let created_at_ms = crate::model::now_ms();
    let key = conversation_key(identity, peer, &stream_id)?;
    let cipher = XChaCha20Poly1305::new((&key).into());
    let mut nonce = [0_u8; 24];
    OsRng.fill_bytes(&mut nonce);
    let mut cleartext = Vec::new();
    ciborium::ser::into_writer(payload, &mut cleartext)?;
    let ciphertext = cipher
        .encrypt(
            XNonce::from_slice(&nonce),
            Payload {
                msg: &cleartext,
                aad: &aad(
                    &id,
                    &stream_id,
                    &identity.public().device_id.0,
                    created_at_ms,
                    kind,
                ),
            },
        )
        .map_err(|_| anyhow::anyhow!("event encryption failed"))?;
    let mut event = EventEnvelope {
        id,
        stream_id,
        author: identity.public().device_id.clone(),
        created_at_ms,
        kind,
        nonce,
        ciphertext,
        signature: Vec::new(),
    };
    event.signature = identity.sign(&bincode::serialize(&event.signable())?);
    Ok(event)
}

pub fn verify_and_decrypt(
    identity: &Identity,
    peer: &PublicIdentity,
    author: &PublicIdentity,
    event: &EventEnvelope,
) -> anyhow::Result<DecryptedPayload> {
    anyhow::ensure!(event.author == author.device_id, "event author mismatch");
    verify(
        author,
        &bincode::serialize(&event.signable())?,
        &event.signature,
    )?;
    let key = conversation_key(identity, peer, &event.stream_id)?;
    let cipher = XChaCha20Poly1305::new((&key).into());
    let cleartext = cipher
        .decrypt(
            XNonce::from_slice(&event.nonce),
            Payload {
                msg: &event.ciphertext,
                aad: &aad(
                    &event.id,
                    &event.stream_id,
                    &event.author.0,
                    event.created_at_ms,
                    event.kind,
                ),
            },
        )
        .map_err(|_| anyhow::anyhow!("event authentication failed"))?;
    Ok(ciborium::de::from_reader(cleartext.as_slice())?)
}

pub fn encrypt_chunk(
    identity: &Identity,
    peer: &PublicIdentity,
    stream_id: &str,
    manifest_id: &str,
    hash: &str,
    cleartext: &[u8],
) -> anyhow::Result<([u8; 24], Vec<u8>)> {
    let key = conversation_key(identity, peer, stream_id)?;
    let cipher = XChaCha20Poly1305::new((&key).into());
    let mut nonce = [0_u8; 24];
    OsRng.fill_bytes(&mut nonce);
    let bytes = cipher
        .encrypt(
            XNonce::from_slice(&nonce),
            Payload {
                msg: cleartext,
                aad: format!("nexus/v1/chunk/{manifest_id}/{hash}").as_bytes(),
            },
        )
        .map_err(|_| anyhow::anyhow!("chunk encryption failed"))?;
    Ok((nonce, bytes))
}

pub fn decrypt_chunk(
    identity: &Identity,
    peer: &PublicIdentity,
    stream_id: &str,
    manifest_id: &str,
    hash: &str,
    nonce: &[u8; 24],
    ciphertext: &[u8],
) -> anyhow::Result<Vec<u8>> {
    let key = conversation_key(identity, peer, stream_id)?;
    let cipher = XChaCha20Poly1305::new((&key).into());
    cipher
        .decrypt(
            XNonce::from_slice(nonce),
            Payload {
                msg: ciphertext,
                aad: format!("nexus/v1/chunk/{manifest_id}/{hash}").as_bytes(),
            },
        )
        .map_err(|_| anyhow::anyhow!("chunk authentication failed"))
}
