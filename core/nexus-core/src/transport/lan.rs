use std::{
    collections::HashMap,
    net::{IpAddr, Ipv4Addr},
    sync::Arc,
};

use async_trait::async_trait;
use mdns_sd::{ServiceDaemon, ServiceEvent, ServiceInfo};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
    sync::mpsc,
};

use crate::{
    identity::{DeviceId, PublicIdentity},
    model::{PeerEndpoint, PROTOCOL_VERSION},
    protocol::{decode, encode, WireEnvelope},
};

use super::{DataTransport, DiscoveryAdapter, TransportEvent};

pub const SERVICE_TYPE: &str = "_nexus-p2p._tcp.local.";
const MAX_FRAME: usize = 8 * 1024 * 1024;

pub struct LanMdnsAdapter {
    mdns: ServiceDaemon,
    service: ServiceInfo,
    local_id: DeviceId,
    events: mpsc::UnboundedSender<TransportEvent>,
}

impl LanMdnsAdapter {
    pub fn new(
        identity: &PublicIdentity,
        port: u16,
        events: mpsc::UnboundedSender<TransportEvent>,
    ) -> anyhow::Result<Self> {
        let mdns = ServiceDaemon::new()?;
        let host = format!("{}.local.", identity.device_id.0);
        let instance = format!("nexus-{}", &identity.device_id.0[..8]);
        let properties: HashMap<String, String> = [
            ("id".into(), identity.device_id.0.clone()),
            ("name".into(), identity.display_name.clone()),
            ("sign".into(), hex::encode(identity.signing_public_key)),
            ("exchange".into(), hex::encode(identity.exchange_public_key)),
            ("v".into(), PROTOCOL_VERSION.to_string()),
        ]
        .into_iter()
        .collect();
        // mdns-sd selects suitable interface addresses when an unspecified address is supplied.
        let service = ServiceInfo::new(
            SERVICE_TYPE,
            &instance,
            &host,
            IpAddr::V4(Ipv4Addr::UNSPECIFIED),
            port,
            properties,
        )?
        .enable_addr_auto();
        Ok(Self {
            mdns,
            service,
            local_id: identity.device_id.clone(),
            events,
        })
    }
}

#[async_trait]
impl DiscoveryAdapter for LanMdnsAdapter {
    async fn start(&self) -> anyhow::Result<()> {
        self.mdns.register(self.service.clone())?;
        let receiver = self.mdns.browse(SERVICE_TYPE)?;
        let events = self.events.clone();
        let local_id = self.local_id.clone();
        tokio::task::spawn_blocking(move || {
            while let Ok(event) = receiver.recv() {
                match event {
                    ServiceEvent::ServiceResolved(info) => {
                        let property =
                            |key: &str| info.get_property_val_str(key).map(ToOwned::to_owned);
                        let Some(id) = property("id") else { continue };
                        if id == local_id.0 {
                            continue;
                        }
                        let (Some(name), Some(sign), Some(exchange)) =
                            (property("name"), property("sign"), property("exchange"))
                        else {
                            continue;
                        };
                        let (Ok(sign), Ok(exchange)) = (hex::decode(sign), hex::decode(exchange))
                        else {
                            continue;
                        };
                        let (Ok(signing_public_key), Ok(exchange_public_key)) =
                            (sign.try_into(), exchange.try_into())
                        else {
                            continue;
                        };
                        let host = info.get_hostname().trim_end_matches('.').to_owned();
                        if host.is_empty() {
                            continue;
                        }
                        let announcement = PublicIdentity {
                            device_id: DeviceId(id),
                            display_name: name,
                            signing_public_key,
                            exchange_public_key,
                        };
                        let endpoint = PeerEndpoint {
                            // Keep the Bonjour hostname so the OS can choose the correct
                            // interface and fall back between IPv6 and IPv4 addresses.
                            host,
                            port: info.get_port(),
                        };
                        let _ = events.send(TransportEvent::Discovered {
                            endpoint,
                            announcement,
                        });
                    }
                    ServiceEvent::ServiceRemoved(_, fullname) => {
                        if let Some(short) = fullname
                            .strip_prefix("nexus-")
                            .and_then(|s| s.split('.').next())
                        {
                            let _ = events.send(TransportEvent::Lost {
                                device_id: DeviceId(short.to_owned()),
                            });
                        }
                    }
                    _ => {}
                }
            }
        });
        Ok(())
    }

    async fn stop(&self) -> anyhow::Result<()> {
        if let Ok(receiver) = self.mdns.unregister(self.service.get_fullname()) {
            let _ = tokio::task::spawn_blocking(move || {
                receiver.recv_timeout(std::time::Duration::from_secs(2))
            })
            .await;
        }
        let receiver = self.mdns.shutdown()?;
        let _ = tokio::task::spawn_blocking(move || {
            receiver.recv_timeout(std::time::Duration::from_secs(2))
        })
        .await;
        Ok(())
    }
}

#[derive(Debug, Default)]
pub struct LanTcpTransport;

#[async_trait]
impl DataTransport for LanTcpTransport {
    fn name(&self) -> &'static str {
        "lan-tcp"
    }

    async fn request(
        &self,
        endpoint: &PeerEndpoint,
        message: &WireEnvelope,
    ) -> anyhow::Result<WireEnvelope> {
        let address = format_address(endpoint);
        let mut stream = tokio::time::timeout(
            std::time::Duration::from_secs(8),
            TcpStream::connect(&address),
        )
        .await??;
        let payload = encode(message)?;
        anyhow::ensure!(payload.len() <= MAX_FRAME, "wire frame too large");
        stream.write_u32(payload.len() as u32).await?;
        stream.write_all(&payload).await?;
        stream.flush().await?;
        let length = stream.read_u32().await? as usize;
        anyhow::ensure!(length <= MAX_FRAME, "peer frame too large");
        let mut response = vec![0_u8; length];
        stream.read_exact(&mut response).await?;
        decode(&response)
    }
}

pub async fn read_frame(stream: &mut TcpStream) -> anyhow::Result<WireEnvelope> {
    let length = stream.read_u32().await? as usize;
    anyhow::ensure!(length <= MAX_FRAME, "incoming frame too large");
    let mut bytes = vec![0_u8; length];
    stream.read_exact(&mut bytes).await?;
    decode(&bytes)
}

pub async fn write_frame(stream: &mut TcpStream, message: &WireEnvelope) -> anyhow::Result<()> {
    let bytes = encode(message)?;
    anyhow::ensure!(bytes.len() <= MAX_FRAME, "outgoing frame too large");
    stream.write_u32(bytes.len() as u32).await?;
    stream.write_all(&bytes).await?;
    stream.flush().await?;
    Ok(())
}

pub fn format_address(endpoint: &PeerEndpoint) -> String {
    if endpoint.host.contains(':') {
        format!("[{}]:{}", endpoint.host, endpoint.port)
    } else {
        format!("{}:{}", endpoint.host, endpoint.port)
    }
}

pub type SharedLanTransport = Arc<LanTcpTransport>;
