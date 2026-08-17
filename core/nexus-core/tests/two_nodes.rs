use nexus_core::{Node, NodeConfig, PeerEndpoint};

fn free_port() -> u16 {
    std::net::TcpListener::bind(("127.0.0.1", 0))
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn pair_chat_file_and_sync_two_nodes() -> anyhow::Result<()> {
    let alice_dir = tempfile::tempdir()?;
    let bob_dir = tempfile::tempdir()?;
    let alice_port = free_port();
    let bob_port = free_port();
    let alice = Node::new(NodeConfig {
        data_dir: alice_dir.path().into(),
        display_name: "Alice".into(),
        listen_port: alice_port,
        enable_mdns: false,
    })?;
    let bob = Node::new(NodeConfig {
        data_dir: bob_dir.path().into(),
        display_name: "Bob".into(),
        listen_port: bob_port,
        enable_mdns: false,
    })?;
    alice.remember_peer(
        bob.identity().clone(),
        PeerEndpoint {
            host: "127.0.0.1".into(),
            port: bob_port,
        },
    )?;
    bob.remember_peer(
        alice.identity().clone(),
        PeerEndpoint {
            host: "127.0.0.1".into(),
            port: alice_port,
        },
    )?;
    alice.start().await?;
    bob.start().await?;

    let bob_id = bob.identity().device_id.clone();
    let alice_id = alice.identity().device_id.clone();
    alice.pair(&bob_id).await?;
    alice.send_text(&bob_id, "hello over encrypted LAN").await?;
    assert_eq!(bob.chat(&alice_id)?.len(), 1);

    let source = alice_dir.path().join("photo.bin");
    std::fs::write(&source, vec![0x42; 700_000])?;
    let sent = alice
        .send_file(&bob_id, &source, "application/octet-stream")
        .await?;
    let manifest_id = match sent.payload {
        nexus_core::model::DecryptedPayload::FileManifest(value) => value.id,
        _ => anyhow::bail!("expected manifest"),
    };
    let restored = bob_dir.path().join("received.bin");
    bob.materialize_file(&manifest_id, &restored)?;
    assert_eq!(std::fs::read(source)?, std::fs::read(restored)?);

    bob.send_text(&alice_id, "reply while offline-first")
        .await?;
    alice.sync(&bob_id).await?;
    assert_eq!(alice.chat(&bob_id)?.len(), 3);
    Ok(())
}
