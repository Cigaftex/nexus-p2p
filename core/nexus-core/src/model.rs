use serde::{Deserialize, Serialize};

use crate::identity::{DeviceId, PublicIdentity};

pub const PROTOCOL_VERSION: u16 = 1;
pub const FILE_CHUNK_SIZE: usize = 256 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PeerEndpoint {
    pub host: String,
    pub port: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Peer {
    pub identity: PublicIdentity,
    pub endpoint: Option<PeerEndpoint>,
    pub paired: bool,
    pub last_seen_ms: i64,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[repr(i32)]
pub enum EventKind {
    Text = 1,
    FileManifest = 2,
    System = 3,
}

impl TryFrom<i32> for EventKind {
    type Error = anyhow::Error;

    fn try_from(value: i32) -> Result<Self, Self::Error> {
        match value {
            1 => Ok(Self::Text),
            2 => Ok(Self::FileManifest),
            3 => Ok(Self::System),
            _ => anyhow::bail!("unknown event kind {value}"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct EventEnvelope {
    pub id: String,
    pub stream_id: String,
    pub author: DeviceId,
    pub created_at_ms: i64,
    pub kind: EventKind,
    pub nonce: [u8; 24],
    pub ciphertext: Vec<u8>,
    pub signature: Vec<u8>,
}

#[derive(Debug, Serialize)]
pub(crate) struct SignableEvent<'a> {
    pub id: &'a str,
    pub stream_id: &'a str,
    pub author: &'a DeviceId,
    pub created_at_ms: i64,
    pub kind: EventKind,
    pub nonce: &'a [u8; 24],
    pub ciphertext: &'a [u8],
}

impl EventEnvelope {
    pub(crate) fn signable(&self) -> SignableEvent<'_> {
        SignableEvent {
            id: &self.id,
            stream_id: &self.stream_id,
            author: &self.author,
            created_at_ms: self.created_at_ms,
            kind: self.kind,
            nonce: &self.nonce,
            ciphertext: &self.ciphertext,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TextPayload {
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FileManifest {
    pub id: String,
    pub name: String,
    pub media_type: String,
    pub size: u64,
    pub chunk_size: u32,
    pub chunks: Vec<ChunkDescriptor>,
    pub file_hash: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ChunkDescriptor {
    pub index: u32,
    pub hash: String,
    pub size: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", content = "data", rename_all = "snake_case")]
pub enum DecryptedPayload {
    Text(TextPayload),
    FileManifest(FileManifest),
    System(String),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ChatItem {
    pub event_id: String,
    pub author: DeviceId,
    pub created_at_ms: i64,
    pub payload: DecryptedPayload,
}

pub fn direct_stream_id(a: &DeviceId, b: &DeviceId) -> String {
    let (first, second) = if a <= b { (a, b) } else { (b, a) };
    format!("dm:{first}:{second}")
}

pub fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}
