use nexus_core::{Node, NodeConfig, NodeEvent};

fn free_port() -> u16 {
    std::net::TcpListener::bind(("127.0.0.1", 0))
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

/// Requires a host that permits multicast DNS; CI and sandboxed runners often do not.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "requires a multicast-capable local network"]
async fn two_mdns_nodes_discover_each_other() -> anyhow::Result<()> {
    let alice_dir = tempfile::tempdir()?;
    let bob_dir = tempfile::tempdir()?;
    let alice = Node::new(NodeConfig {
        data_dir: alice_dir.path().into(),
        display_name: "mDNS Alice".into(),
        listen_port: free_port(),
        enable_mdns: true,
    })?;
    let bob = Node::new(NodeConfig {
        data_dir: bob_dir.path().into(),
        display_name: "mDNS Bob".into(),
        listen_port: free_port(),
        enable_mdns: true,
    })?;
    let bob_id = bob.identity().device_id.clone();
    let mut events = alice.subscribe();
    let result = tokio::time::timeout(std::time::Duration::from_secs(10), async {
        alice.start().await?;
        bob.start().await?;
        loop {
            if let NodeEvent::PeerDiscovered(peer) = events.recv().await? {
                if peer.identity.device_id == bob_id {
                    return anyhow::Ok(());
                }
            }
        }
    })
    .await;
    alice.shutdown().await?;
    bob.shutdown().await?;
    result??;
    Ok(())
}
