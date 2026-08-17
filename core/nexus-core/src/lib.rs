//! Offline-first peer-to-peer core for Nexus.
//!
//! The crate deliberately keeps UI and platform radio APIs outside the core.
//! LAN/mDNS is the reference transport and the same protocol can be carried by
//! BLE, Wi-Fi Aware, Wi-Fi Direct, MultipeerConnectivity, or Network.framework.

pub mod crypto;
pub mod ffi;
pub mod files;
pub mod identity;
pub mod model;
pub mod node;
pub mod protocol;
pub mod storage;
pub mod transport;

pub use identity::{DeviceId, Identity, PublicIdentity};
pub use model::{ChatItem, Peer, PeerEndpoint};
pub use node::{Node, NodeConfig, NodeEvent};
