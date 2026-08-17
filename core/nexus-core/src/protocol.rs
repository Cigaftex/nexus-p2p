use serde::{Deserialize, Serialize};

use crate::{identity::PublicIdentity, model::EventEnvelope};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WireEnvelope {
    pub version: u16,
    pub sender: PublicIdentity,
    pub body: WireMessage,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum WireMessage {
    Hello,
    PairRequest {
        pairing_nonce: String,
    },
    PairResponse {
        accepted: bool,
    },
    PushEvent {
        event: EventEnvelope,
    },
    Ack {
        event_id: String,
    },
    SyncExchange {
        stream_id: String,
        known_ids: Vec<String>,
        events: Vec<EventEnvelope>,
    },
    SyncResult {
        events: Vec<EventEnvelope>,
    },
    ChunkRequest {
        manifest_id: String,
        hashes: Vec<String>,
    },
    ChunkResult {
        manifest_id: String,
        chunks: Vec<WireChunk>,
    },
    PushChunks {
        manifest_id: String,
        chunks: Vec<WireChunk>,
    },
    Error {
        message: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WireChunk {
    pub hash: String,
    pub nonce: [u8; 24],
    pub bytes: Vec<u8>,
}

pub fn encode(message: &WireEnvelope) -> anyhow::Result<Vec<u8>> {
    let mut output = Vec::new();
    ciborium::ser::into_writer(message, &mut output)?;
    Ok(output)
}

pub fn decode(bytes: &[u8]) -> anyhow::Result<WireEnvelope> {
    Ok(ciborium::de::from_reader(bytes)?)
}
