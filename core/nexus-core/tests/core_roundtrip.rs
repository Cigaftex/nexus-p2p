use nexus_core::{
    crypto::{create_event, verify_and_decrypt},
    files::{ingest_file, materialize_file},
    identity::Identity,
    model::{direct_stream_id, DecryptedPayload, EventKind, TextPayload},
    storage::Store,
};

#[test]
fn identity_crypto_and_blob_roundtrip() -> anyhow::Result<()> {
    let alice_dir = tempfile::tempdir()?;
    let bob_dir = tempfile::tempdir()?;
    let alice_store = Store::open(alice_dir.path())?;
    let bob_store = Store::open(bob_dir.path())?;
    let alice = Identity::load_or_create(&alice_store, "Alice")?;
    let bob = Identity::load_or_create(&bob_store, "Bob")?;
    let stream = direct_stream_id(&alice.public().device_id, &bob.public().device_id);
    let payload = DecryptedPayload::Text(TextPayload {
        text: "offline hello".into(),
    });
    let event = create_event(&alice, bob.public(), stream, EventKind::Text, &payload)?;
    assert_eq!(
        verify_and_decrypt(&bob, alice.public(), alice.public(), &event)?,
        payload
    );

    let source = alice_dir.path().join("payload.bin");
    std::fs::write(&source, vec![0x5a; 600_000])?;
    let manifest = ingest_file(&alice_store, &source, "application/octet-stream")?;
    alice_store.save_manifest(&bob.public().device_id, &manifest)?;
    let output = alice_dir.path().join("restored.bin");
    materialize_file(&alice_store, &manifest, &output)?;
    assert_eq!(std::fs::read(source)?, std::fs::read(output)?);
    assert!(manifest.chunks.len() >= 3);
    Ok(())
}
