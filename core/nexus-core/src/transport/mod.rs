use async_trait::async_trait;

use crate::{identity::DeviceId, model::PeerEndpoint, protocol::WireEnvelope};

pub mod lan;

#[derive(Debug, Clone)]
pub enum TransportEvent {
    Discovered {
        endpoint: PeerEndpoint,
        announcement: crate::identity::PublicIdentity,
    },
    Lost {
        device_id: DeviceId,
    },
}

#[async_trait]
pub trait DiscoveryAdapter: Send + Sync {
    async fn start(&self) -> anyhow::Result<()>;
    async fn stop(&self) -> anyhow::Result<()>;
}

#[async_trait]
pub trait DataTransport: Send + Sync {
    fn name(&self) -> &'static str;
    async fn request(
        &self,
        endpoint: &PeerEndpoint,
        message: &WireEnvelope,
    ) -> anyhow::Result<WireEnvelope>;
}

/// Implemented by Kotlin/Swift adapters through the FFI bridge in later phases.
#[async_trait]
pub trait PlatformRadioAdapter: DiscoveryAdapter + DataTransport {
    fn capabilities(&self) -> RadioCapabilities;
}

#[derive(Debug, Clone, Copy)]
pub struct RadioCapabilities {
    pub discovery_only: bool,
    pub metered: bool,
    pub max_frame_size: usize,
}
