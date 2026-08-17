use std::{
    collections::HashSet,
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};

use parking_lot::{Mutex, RwLock};
use rand_core::{OsRng, RngCore};
use serde::{Deserialize, Serialize};
use tokio::{
    net::{TcpListener, TcpStream},
    sync::{broadcast, mpsc},
};

use crate::{
    crypto::{create_event, decrypt_chunk, encrypt_chunk, verify_and_decrypt},
    files,
    identity::{validate_public_identity, DeviceId, Identity, PublicIdentity},
    model::{
        direct_stream_id, ChatItem, DecryptedPayload, EventEnvelope, EventKind, Peer, PeerEndpoint,
        TextPayload, PROTOCOL_VERSION,
    },
    protocol::{WireChunk, WireEnvelope, WireMessage},
    storage::Store,
    transport::{
        lan::{read_frame, write_frame, LanMdnsAdapter, LanTcpTransport},
        DataTransport, DiscoveryAdapter, TransportEvent,
    },
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeConfig {
    pub data_dir: PathBuf,
    pub display_name: String,
    #[serde(default = "default_port")]
    pub listen_port: u16,
    #[serde(default = "default_discovery")]
    pub enable_mdns: bool,
}

fn default_port() -> u16 {
    47777
}
fn default_discovery() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "data", rename_all = "snake_case")]
pub enum NodeEvent {
    Ready {
        identity: PublicIdentity,
        port: u16,
    },
    PeerDiscovered(Peer),
    PeerLost {
        device_id: DeviceId,
    },
    Paired {
        peer: Peer,
    },
    Message(ChatItem),
    FileProgress {
        manifest_id: String,
        received_chunks: usize,
        total_chunks: usize,
    },
    FileComplete {
        manifest_id: String,
        name: String,
    },
    SyncComplete {
        peer_id: DeviceId,
        received_events: usize,
    },
    Error {
        message: String,
    },
}

pub struct Node {
    config: NodeConfig,
    store: Arc<Store>,
    identity: Arc<Identity>,
    display_name: RwLock<String>,
    transport: Arc<LanTcpTransport>,
    discovery: Option<Arc<LanMdnsAdapter>>,
    discovery_rx: Mutex<Option<mpsc::UnboundedReceiver<TransportEvent>>>,
    events: broadcast::Sender<NodeEvent>,
    started: AtomicBool,
}

impl std::fmt::Debug for Node {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Node")
            .field("config", &self.config)
            .field("identity", &self.identity.public())
            .finish()
    }
}

impl Node {
    pub fn new(config: NodeConfig) -> anyhow::Result<Arc<Self>> {
        let store = Arc::new(Store::open(&config.data_dir)?);
        let identity = Arc::new(Identity::load_or_create(&store, &config.display_name)?);
        let display_name = identity.public().display_name.clone();
        let (discovery_tx, discovery_rx) = mpsc::unbounded_channel();
        let discovery = if config.enable_mdns {
            Some(Arc::new(LanMdnsAdapter::new(
                identity.public(),
                config.listen_port,
                discovery_tx,
            )?))
        } else {
            None
        };
        let (events, _) = broadcast::channel(256);
        Ok(Arc::new(Self {
            config,
            store,
            identity,
            display_name: RwLock::new(display_name),
            transport: Arc::new(LanTcpTransport),
            discovery,
            discovery_rx: Mutex::new(Some(discovery_rx)),
            events,
            started: AtomicBool::new(false),
        }))
    }

    pub fn identity(&self) -> PublicIdentity {
        let mut identity = self.identity.public().clone();
        identity.display_name = self.display_name.read().clone();
        identity
    }

    pub fn set_display_name(&self, display_name: &str) -> anyhow::Result<PublicIdentity> {
        let display_name = display_name.trim();
        anyhow::ensure!(!display_name.is_empty(), "device name cannot be empty");
        anyhow::ensure!(
            display_name.chars().count() <= 40,
            "device name is too long"
        );
        let mut identity = self.identity();
        identity.display_name = display_name.to_owned();
        self.store
            .update_identity_public(&serde_json::to_string(&identity)?)?;
        *self.display_name.write() = display_name.to_owned();
        Ok(identity)
    }
    pub fn subscribe(&self) -> broadcast::Receiver<NodeEvent> {
        self.events.subscribe()
    }
    pub fn peers(&self) -> anyhow::Result<Vec<Peer>> {
        self.store.peers()
    }

    /// Adds a peer endpoint supplied by any discovery adapter (LAN, BLE, QR, etc.).
    pub fn remember_peer(
        &self,
        identity: PublicIdentity,
        endpoint: PeerEndpoint,
    ) -> anyhow::Result<()> {
        validate_public_identity(&identity)?;
        let paired = self
            .store
            .peer(&identity.device_id)?
            .is_some_and(|peer| peer.paired);
        self.store.upsert_peer(&Peer {
            identity,
            endpoint: Some(endpoint),
            paired,
            last_seen_ms: crate::model::now_ms(),
        })
    }

    pub async fn start(self: &Arc<Self>) -> anyhow::Result<()> {
        if self.started.swap(true, Ordering::SeqCst) {
            return Ok(());
        }
        let listener = TcpListener::bind(("0.0.0.0", self.config.listen_port)).await?;
        let node = Arc::clone(self);
        tokio::spawn(async move {
            loop {
                match listener.accept().await {
                    Ok((stream, _)) => {
                        let node = Arc::clone(&node);
                        tokio::spawn(async move {
                            if let Err(error) = node.handle_connection(stream).await {
                                node.emit_error(error);
                            }
                        });
                    }
                    Err(error) => node.emit_error(error.into()),
                }
            }
        });

        if let Some(discovery) = &self.discovery {
            discovery.start().await?;
        }
        if let Some(mut receiver) = self.discovery_rx.lock().take() {
            let node = Arc::clone(self);
            tokio::spawn(async move {
                while let Some(event) = receiver.recv().await {
                    node.handle_transport_event(event).await;
                }
            });
        }
        let _ = self.events.send(NodeEvent::Ready {
            identity: self.identity(),
            port: self.config.listen_port,
        });
        Ok(())
    }

    pub async fn shutdown(&self) -> anyhow::Result<()> {
        if let Some(discovery) = &self.discovery {
            discovery.stop().await?;
        }
        Ok(())
    }

    async fn handle_transport_event(self: &Arc<Self>, event: TransportEvent) {
        match event {
            TransportEvent::Discovered {
                endpoint,
                announcement,
            } => {
                if let Err(error) = validate_public_identity(&announcement) {
                    self.emit_error(error);
                    return;
                }
                let paired = self
                    .store
                    .peer(&announcement.device_id)
                    .ok()
                    .flatten()
                    .is_some_and(|peer| peer.paired);
                let peer = Peer {
                    identity: announcement,
                    endpoint: Some(endpoint),
                    paired,
                    last_seen_ms: crate::model::now_ms(),
                };
                if let Err(error) = self.store.upsert_peer(&peer) {
                    self.emit_error(error);
                    return;
                }
                let _ = self.events.send(NodeEvent::PeerDiscovered(peer.clone()));
                if paired {
                    let node = Arc::clone(self);
                    let peer_id = peer.identity.device_id;
                    tokio::spawn(async move {
                        if let Err(error) = node.sync(&peer_id).await {
                            node.emit_error(error);
                        }
                    });
                }
            }
            TransportEvent::Lost { device_id } => {
                let _ = self.events.send(NodeEvent::PeerLost { device_id });
            }
        }
    }

    pub async fn pair(&self, peer_id: &DeviceId) -> anyhow::Result<()> {
        let peer = self.require_reachable_peer(peer_id)?;
        let mut bytes = [0_u8; 6];
        OsRng.fill_bytes(&mut bytes);
        let response = self
            .request(
                &peer,
                WireMessage::PairRequest {
                    pairing_nonce: hex::encode(bytes),
                },
            )
            .await?;
        match response.body {
            WireMessage::PairResponse { accepted: true } => {
                self.store
                    .set_peer_paired(&response.sender, peer.endpoint.as_ref())?;
                let paired = self
                    .store
                    .peer(peer_id)?
                    .ok_or_else(|| anyhow::anyhow!("paired peer missing"))?;
                let _ = self.events.send(NodeEvent::Paired { peer: paired });
                self.sync(peer_id).await?;
                Ok(())
            }
            WireMessage::PairResponse { accepted: false } => anyhow::bail!("pair request rejected"),
            _ => anyhow::bail!("unexpected pair response"),
        }
    }

    pub async fn send_text(&self, peer_id: &DeviceId, text: &str) -> anyhow::Result<ChatItem> {
        let peer = self.require_paired_peer(peer_id)?;
        let stream_id = direct_stream_id(&self.identity().device_id, peer_id);
        let payload = DecryptedPayload::Text(TextPayload {
            text: text.to_owned(),
        });
        let event = create_event(
            &self.identity,
            &peer.identity,
            stream_id,
            EventKind::Text,
            &payload,
        )?;
        self.store.insert_event(&event)?;
        let response = self
            .request(
                &peer,
                WireMessage::PushEvent {
                    event: event.clone(),
                },
            )
            .await?;
        anyhow::ensure!(
            matches!(response.body, WireMessage::Ack { ref event_id } if event_id == &event.id),
            "message was not acknowledged"
        );
        let item = self.chat_item(&peer.identity, &event)?;
        let _ = self.events.send(NodeEvent::Message(item.clone()));
        Ok(item)
    }

    pub async fn send_file(
        &self,
        peer_id: &DeviceId,
        path: impl AsRef<std::path::Path>,
        media_type: &str,
    ) -> anyhow::Result<ChatItem> {
        let peer = self.require_paired_peer(peer_id)?;
        let manifest = files::ingest_file(&self.store, path, media_type)?;
        self.store.save_manifest(peer_id, &manifest)?;
        let payload = DecryptedPayload::FileManifest(manifest.clone());
        let event = create_event(
            &self.identity,
            &peer.identity,
            direct_stream_id(&self.identity().device_id, peer_id),
            EventKind::FileManifest,
            &payload,
        )?;
        self.store.insert_event(&event)?;
        let response = self
            .request(
                &peer,
                WireMessage::PushEvent {
                    event: event.clone(),
                },
            )
            .await?;
        anyhow::ensure!(
            matches!(response.body, WireMessage::Ack { ref event_id } if event_id == &event.id),
            "file manifest was not acknowledged"
        );
        for batch in manifest.chunks.chunks(16) {
            let stream_id = direct_stream_id(&self.identity().device_id, peer_id);
            let chunks = batch
                .iter()
                .map(|chunk| {
                    let (nonce, bytes) = encrypt_chunk(
                        &self.identity,
                        &peer.identity,
                        &stream_id,
                        &manifest.id,
                        &chunk.hash,
                        &self.store.get_blob(&chunk.hash)?,
                    )?;
                    Ok(WireChunk {
                        hash: chunk.hash.clone(),
                        nonce,
                        bytes,
                    })
                })
                .collect::<anyhow::Result<Vec<_>>>()?;
            let response = self
                .request(
                    &peer,
                    WireMessage::PushChunks {
                        manifest_id: manifest.id.clone(),
                        chunks,
                    },
                )
                .await?;
            anyhow::ensure!(
                matches!(response.body, WireMessage::Ack { .. }),
                "file chunks were not acknowledged"
            );
        }
        self.chat_item(&peer.identity, &event)
    }

    pub fn chat(&self, peer_id: &DeviceId) -> anyhow::Result<Vec<ChatItem>> {
        let peer = self.require_paired_peer(peer_id)?;
        let stream = direct_stream_id(&self.identity().device_id, peer_id);
        self.store
            .events(&stream)?
            .iter()
            .map(|event| self.chat_item(&peer.identity, event))
            .collect()
    }

    pub fn materialize_file(
        &self,
        manifest_id: &str,
        destination: impl AsRef<std::path::Path>,
    ) -> anyhow::Result<()> {
        let manifest = self
            .store
            .manifest(manifest_id)?
            .ok_or_else(|| anyhow::anyhow!("unknown file manifest"))?;
        files::materialize_file(&self.store, &manifest, destination)
    }

    fn chat_item(&self, peer: &PublicIdentity, event: &EventEnvelope) -> anyhow::Result<ChatItem> {
        let author = if event.author == self.identity().device_id {
            self.identity()
        } else {
            peer.clone()
        };
        let payload = verify_and_decrypt(&self.identity, peer, &author, event)?;
        Ok(ChatItem {
            event_id: event.id.clone(),
            author: event.author.clone(),
            created_at_ms: event.created_at_ms,
            payload,
        })
    }

    pub async fn sync(&self, peer_id: &DeviceId) -> anyhow::Result<()> {
        let peer = self.require_paired_peer(peer_id)?;
        let stream_id = direct_stream_id(&self.identity().device_id, peer_id);
        let known_ids: Vec<_> = self.store.event_ids(&stream_id)?.into_iter().collect();
        let events = self
            .store
            .events(&stream_id)?
            .into_iter()
            .filter(|event| event.author == self.identity().device_id)
            .collect();
        let response = self
            .request(
                &peer,
                WireMessage::SyncExchange {
                    stream_id,
                    known_ids,
                    events,
                },
            )
            .await?;
        let WireMessage::SyncResult { events } = response.body else {
            anyhow::bail!("unexpected sync response")
        };
        let mut received = 0;
        for event in events {
            if event.author != peer.identity.device_id {
                continue;
            }
            self.chat_item(&peer.identity, &event)?;
            if self.store.insert_event(&event)? {
                received += 1;
                self.process_manifest(&peer.identity.device_id, &peer.identity, &event)?;
            }
        }
        self.resume_files(&peer).await?;
        let _ = self.events.send(NodeEvent::SyncComplete {
            peer_id: peer_id.clone(),
            received_events: received,
        });
        Ok(())
    }

    async fn request(&self, peer: &Peer, body: WireMessage) -> anyhow::Result<WireEnvelope> {
        let endpoint = peer
            .endpoint
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("peer is offline"))?;
        let response = self
            .transport
            .request(
                endpoint,
                &WireEnvelope {
                    version: PROTOCOL_VERSION,
                    sender: self.identity().clone(),
                    body,
                },
            )
            .await?;
        anyhow::ensure!(
            response.version == PROTOCOL_VERSION,
            "unsupported protocol version"
        );
        validate_public_identity(&response.sender)?;
        anyhow::ensure!(
            response.sender.device_id == peer.identity.device_id,
            "response identity mismatch"
        );
        if let WireMessage::Error { ref message } = response.body {
            anyhow::bail!(message.clone());
        }
        Ok(response)
    }

    async fn handle_connection(&self, mut stream: TcpStream) -> anyhow::Result<()> {
        let request = read_frame(&mut stream).await?;
        anyhow::ensure!(
            request.version == PROTOCOL_VERSION,
            "unsupported protocol version"
        );
        validate_public_identity(&request.sender)?;
        let body = self
            .handle_message(&request.sender, request.body)
            .await
            .unwrap_or_else(|error| WireMessage::Error {
                message: error.to_string(),
            });
        write_frame(
            &mut stream,
            &WireEnvelope {
                version: PROTOCOL_VERSION,
                sender: self.identity().clone(),
                body,
            },
        )
        .await?;
        Ok(())
    }

    async fn handle_message(
        &self,
        sender: &PublicIdentity,
        body: WireMessage,
    ) -> anyhow::Result<WireMessage> {
        match body {
            WireMessage::Hello => Ok(WireMessage::Hello),
            WireMessage::PairRequest { pairing_nonce: _ } => {
                // MVP policy: discovery is local-network scoped and the receiver performs TOFU auto-accept.
                // Production adapters should surface a SAS/QR approval before calling this transition.
                let endpoint = self
                    .store
                    .peer(&sender.device_id)?
                    .and_then(|peer| peer.endpoint);
                self.store.set_peer_paired(sender, endpoint.as_ref())?;
                let peer = self
                    .store
                    .peer(&sender.device_id)?
                    .expect("just stored peer");
                let _ = self.events.send(NodeEvent::Paired { peer });
                Ok(WireMessage::PairResponse { accepted: true })
            }
            WireMessage::PushEvent { event } => {
                let peer = self.require_sender_paired(sender)?;
                anyhow::ensure!(
                    event.author == sender.device_id,
                    "forwarded events are not accepted"
                );
                self.chat_item(&peer.identity, &event)?;
                if self.store.insert_event(&event)? {
                    self.process_manifest(&sender.device_id, sender, &event)?;
                    let _ = self
                        .events
                        .send(NodeEvent::Message(self.chat_item(sender, &event)?));
                }
                Ok(WireMessage::Ack { event_id: event.id })
            }
            WireMessage::SyncExchange {
                stream_id,
                known_ids,
                events,
            } => {
                let peer = self.require_sender_paired(sender)?;
                anyhow::ensure!(
                    stream_id == direct_stream_id(&self.identity().device_id, &sender.device_id),
                    "invalid stream id"
                );
                for event in events {
                    if event.author != sender.device_id {
                        continue;
                    }
                    self.chat_item(sender, &event)?;
                    if self.store.insert_event(&event)? {
                        self.process_manifest(&sender.device_id, sender, &event)?;
                    }
                }
                let known: HashSet<_> = known_ids.into_iter().collect();
                let missing = self
                    .store
                    .events(&stream_id)?
                    .into_iter()
                    .filter(|event| {
                        event.author == self.identity().device_id && !known.contains(&event.id)
                    })
                    .collect();
                let _ = peer;
                Ok(WireMessage::SyncResult { events: missing })
            }
            WireMessage::ChunkRequest {
                manifest_id,
                hashes,
            } => {
                let peer = self.require_sender_paired(sender)?;
                let manifest = self
                    .store
                    .manifest(&manifest_id)?
                    .ok_or_else(|| anyhow::anyhow!("unknown manifest"))?;
                let allowed: HashSet<_> = manifest
                    .chunks
                    .iter()
                    .map(|chunk| chunk.hash.as_str())
                    .collect();
                let mut chunks = Vec::new();
                let stream_id = direct_stream_id(&self.identity().device_id, &sender.device_id);
                for hash in hashes.into_iter().take(16) {
                    anyhow::ensure!(
                        allowed.contains(hash.as_str()),
                        "chunk is not part of manifest"
                    );
                    let (nonce, bytes) = encrypt_chunk(
                        &self.identity,
                        &peer.identity,
                        &stream_id,
                        &manifest_id,
                        &hash,
                        &self.store.get_blob(&hash)?,
                    )?;
                    chunks.push(WireChunk { bytes, nonce, hash });
                }
                Ok(WireMessage::ChunkResult {
                    manifest_id,
                    chunks,
                })
            }
            WireMessage::PushChunks {
                manifest_id,
                chunks,
            } => {
                let peer = self.require_sender_paired(sender)?;
                let manifest = self
                    .store
                    .manifest(&manifest_id)?
                    .ok_or_else(|| anyhow::anyhow!("unknown manifest"))?;
                let allowed: HashSet<_> = manifest
                    .chunks
                    .iter()
                    .map(|chunk| chunk.hash.as_str())
                    .collect();
                let stream_id = direct_stream_id(&self.identity().device_id, &sender.device_id);
                for chunk in chunks {
                    anyhow::ensure!(
                        allowed.contains(chunk.hash.as_str()),
                        "chunk is not part of manifest"
                    );
                    let cleartext = decrypt_chunk(
                        &self.identity,
                        &peer.identity,
                        &stream_id,
                        &manifest_id,
                        &chunk.hash,
                        &chunk.nonce,
                        &chunk.bytes,
                    )?;
                    self.store.put_blob(&chunk.hash, &cleartext)?;
                }
                let remaining = self.store.missing_chunks(&manifest_id)?.len();
                let _ = self.events.send(NodeEvent::FileProgress {
                    manifest_id: manifest_id.clone(),
                    received_chunks: manifest.chunks.len() - remaining,
                    total_chunks: manifest.chunks.len(),
                });
                if self.store.refresh_manifest_complete(&manifest_id)? {
                    let _ = self.events.send(NodeEvent::FileComplete {
                        manifest_id: manifest_id.clone(),
                        name: manifest.name,
                    });
                }
                Ok(WireMessage::Ack {
                    event_id: manifest_id,
                })
            }
            WireMessage::PairResponse { .. }
            | WireMessage::Ack { .. }
            | WireMessage::SyncResult { .. }
            | WireMessage::ChunkResult { .. }
            | WireMessage::Error { .. } => anyhow::bail!("response message used as a request"),
        }
    }

    fn process_manifest(
        &self,
        peer_id: &DeviceId,
        peer: &PublicIdentity,
        event: &EventEnvelope,
    ) -> anyhow::Result<()> {
        if event.kind != EventKind::FileManifest {
            return Ok(());
        }
        if let DecryptedPayload::FileManifest(manifest) = self.chat_item(peer, event)?.payload {
            self.store.save_manifest(peer_id, &manifest)?;
        }
        Ok(())
    }

    async fn resume_files(&self, peer: &Peer) -> anyhow::Result<()> {
        for manifest in self
            .store
            .incomplete_manifests_for_peer(&peer.identity.device_id)?
        {
            loop {
                let missing = self.store.missing_chunks(&manifest.id)?;
                if missing.is_empty() {
                    break;
                }
                let requested: Vec<_> = missing.into_iter().take(16).collect();
                let response = self
                    .request(
                        peer,
                        WireMessage::ChunkRequest {
                            manifest_id: manifest.id.clone(),
                            hashes: requested,
                        },
                    )
                    .await?;
                let WireMessage::ChunkResult {
                    manifest_id,
                    chunks,
                } = response.body
                else {
                    anyhow::bail!("unexpected chunk response")
                };
                anyhow::ensure!(manifest_id == manifest.id, "manifest response mismatch");
                anyhow::ensure!(!chunks.is_empty(), "peer returned no chunks");
                let stream_id =
                    direct_stream_id(&self.identity().device_id, &peer.identity.device_id);
                for chunk in chunks {
                    let cleartext = decrypt_chunk(
                        &self.identity,
                        &peer.identity,
                        &stream_id,
                        &manifest_id,
                        &chunk.hash,
                        &chunk.nonce,
                        &chunk.bytes,
                    )?;
                    self.store.put_blob(&chunk.hash, &cleartext)?;
                }
                let remaining = self.store.missing_chunks(&manifest.id)?.len();
                let _ = self.events.send(NodeEvent::FileProgress {
                    manifest_id: manifest.id.clone(),
                    received_chunks: manifest.chunks.len() - remaining,
                    total_chunks: manifest.chunks.len(),
                });
            }
            if self.store.refresh_manifest_complete(&manifest.id)? {
                let _ = self.events.send(NodeEvent::FileComplete {
                    manifest_id: manifest.id.clone(),
                    name: manifest.name.clone(),
                });
            }
        }
        Ok(())
    }

    fn require_reachable_peer(&self, id: &DeviceId) -> anyhow::Result<Peer> {
        let peer = self
            .store
            .peer(id)?
            .ok_or_else(|| anyhow::anyhow!("unknown peer {id}"))?;
        anyhow::ensure!(peer.endpoint.is_some(), "peer {id} is offline");
        Ok(peer)
    }

    fn require_paired_peer(&self, id: &DeviceId) -> anyhow::Result<Peer> {
        let peer = self.require_reachable_peer(id)?;
        anyhow::ensure!(peer.paired, "peer {id} is not paired");
        Ok(peer)
    }

    fn require_sender_paired(&self, sender: &PublicIdentity) -> anyhow::Result<Peer> {
        let peer = self
            .store
            .peer(&sender.device_id)?
            .ok_or_else(|| anyhow::anyhow!("unknown sender"))?;
        anyhow::ensure!(peer.paired, "sender is not paired");
        anyhow::ensure!(peer.identity == *sender, "paired identity changed");
        Ok(peer)
    }

    fn emit_error(&self, error: anyhow::Error) {
        let _ = self.events.send(NodeEvent::Error {
            message: error.to_string(),
        });
    }
}
