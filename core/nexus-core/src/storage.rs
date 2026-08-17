use std::{
    collections::HashSet,
    path::{Path, PathBuf},
};

use parking_lot::Mutex;
use rusqlite::{params, Connection, OptionalExtension};

use crate::{
    identity::{DeviceId, PublicIdentity},
    model::{EventEnvelope, EventKind, FileManifest, Peer, PeerEndpoint},
};

pub struct Store {
    conn: Mutex<Connection>,
    blob_dir: PathBuf,
}

impl std::fmt::Debug for Store {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Store")
            .field("blob_dir", &self.blob_dir)
            .finish()
    }
}

impl Store {
    pub fn open(data_dir: impl AsRef<Path>) -> anyhow::Result<Self> {
        let data_dir = data_dir.as_ref();
        std::fs::create_dir_all(data_dir)?;
        let blob_dir = data_dir.join("blobs");
        std::fs::create_dir_all(&blob_dir)?;
        let conn = Connection::open(data_dir.join("nexus.db"))?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        conn.execute_batch(
            r#"
            CREATE TABLE IF NOT EXISTS local_identity (
                singleton INTEGER PRIMARY KEY CHECK(singleton = 1),
                public_json TEXT NOT NULL,
                secret_json TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS peers (
                device_id TEXT PRIMARY KEY,
                identity_json TEXT NOT NULL,
                paired INTEGER NOT NULL DEFAULT 0,
                host TEXT,
                port INTEGER,
                last_seen_ms INTEGER NOT NULL
            );
            CREATE TABLE IF NOT EXISTS events (
                id TEXT PRIMARY KEY,
                stream_id TEXT NOT NULL,
                author TEXT NOT NULL,
                created_at_ms INTEGER NOT NULL,
                kind INTEGER NOT NULL,
                nonce BLOB NOT NULL,
                ciphertext BLOB NOT NULL,
                signature BLOB NOT NULL
            );
            CREATE INDEX IF NOT EXISTS events_stream_time
                ON events(stream_id, created_at_ms, id);
            CREATE TABLE IF NOT EXISTS manifests (
                id TEXT PRIMARY KEY,
                peer_id TEXT NOT NULL,
                manifest_json TEXT NOT NULL,
                complete INTEGER NOT NULL DEFAULT 0
            );
            CREATE TABLE IF NOT EXISTS manifest_chunks (
                manifest_id TEXT NOT NULL,
                chunk_index INTEGER NOT NULL,
                hash TEXT NOT NULL,
                present INTEGER NOT NULL DEFAULT 0,
                PRIMARY KEY(manifest_id, chunk_index),
                FOREIGN KEY(manifest_id) REFERENCES manifests(id) ON DELETE CASCADE
            );
            "#,
        )?;
        Ok(Self {
            conn: Mutex::new(conn),
            blob_dir,
        })
    }

    pub fn load_identity(&self) -> anyhow::Result<Option<(String, String)>> {
        Ok(self
            .conn
            .lock()
            .query_row(
                "SELECT public_json, secret_json FROM local_identity WHERE singleton=1",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?)
    }

    pub fn save_identity(&self, public_json: &str, secret_json: &str) -> anyhow::Result<()> {
        self.conn.lock().execute(
            "INSERT OR REPLACE INTO local_identity(singleton, public_json, secret_json) VALUES(1, ?1, ?2)",
            params![public_json, secret_json],
        )?;
        Ok(())
    }

    pub fn update_identity_public(&self, public_json: &str) -> anyhow::Result<()> {
        let changed = self.conn.lock().execute(
            "UPDATE local_identity SET public_json=?1 WHERE singleton=1",
            params![public_json],
        )?;
        anyhow::ensure!(changed == 1, "local identity is missing");
        Ok(())
    }

    pub fn upsert_peer(&self, peer: &Peer) -> anyhow::Result<()> {
        let (host, port) = peer
            .endpoint
            .as_ref()
            .map(|e| (Some(e.host.as_str()), Some(e.port as i64)))
            .unwrap_or((None, None));
        self.conn.lock().execute(
            r#"INSERT INTO peers(device_id, identity_json, paired, host, port, last_seen_ms)
               VALUES(?1, ?2, ?3, ?4, ?5, ?6)
               ON CONFLICT(device_id) DO UPDATE SET
                 identity_json=excluded.identity_json,
                 paired=MAX(peers.paired, excluded.paired),
                 host=COALESCE(excluded.host, peers.host),
                 port=COALESCE(excluded.port, peers.port),
                 last_seen_ms=excluded.last_seen_ms"#,
            params![
                peer.identity.device_id.0,
                serde_json::to_string(&peer.identity)?,
                i32::from(peer.paired),
                host,
                port,
                peer.last_seen_ms,
            ],
        )?;
        Ok(())
    }

    pub fn set_peer_paired(
        &self,
        identity: &PublicIdentity,
        endpoint: Option<&PeerEndpoint>,
    ) -> anyhow::Result<()> {
        self.upsert_peer(&Peer {
            identity: identity.clone(),
            endpoint: endpoint.cloned(),
            paired: true,
            last_seen_ms: crate::model::now_ms(),
        })
    }

    pub fn peer(&self, id: &DeviceId) -> anyhow::Result<Option<Peer>> {
        Ok(self.conn.lock().query_row(
            "SELECT identity_json, paired, host, port, last_seen_ms FROM peers WHERE device_id=?1",
            params![id.0],
            |row| {
                let identity_json: String = row.get(0)?;
                let host: Option<String> = row.get(2)?;
                let port: Option<u16> = row.get(3)?;
                Ok(Peer {
                    identity: serde_json::from_str(&identity_json).map_err(|e| rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(e)))?,
                    paired: row.get::<_, i32>(1)? != 0,
                    endpoint: host.zip(port).map(|(host, port)| PeerEndpoint { host, port }),
                    last_seen_ms: row.get(4)?,
                })
            },
        ).optional()?)
    }

    pub fn peers(&self) -> anyhow::Result<Vec<Peer>> {
        let conn = self.conn.lock();
        let mut stmt = conn.prepare("SELECT device_id FROM peers ORDER BY last_seen_ms DESC")?;
        let ids = stmt
            .query_map([], |row| Ok(DeviceId(row.get(0)?)))?
            .collect::<Result<Vec<_>, _>>()?;
        drop(stmt);
        drop(conn);
        ids.into_iter()
            .map(|id| {
                self.peer(&id)?
                    .ok_or_else(|| anyhow::anyhow!("peer disappeared"))
            })
            .collect()
    }

    pub fn insert_event(&self, event: &EventEnvelope) -> anyhow::Result<bool> {
        let changed = self.conn.lock().execute(
            r#"INSERT OR IGNORE INTO events
               (id, stream_id, author, created_at_ms, kind, nonce, ciphertext, signature)
               VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)"#,
            params![
                event.id,
                event.stream_id,
                event.author.0,
                event.created_at_ms,
                event.kind as i32,
                event.nonce.as_slice(),
                event.ciphertext,
                event.signature,
            ],
        )?;
        Ok(changed > 0)
    }

    pub fn events(&self, stream_id: &str) -> anyhow::Result<Vec<EventEnvelope>> {
        let conn = self.conn.lock();
        let mut stmt = conn.prepare(
            "SELECT id, author, created_at_ms, kind, nonce, ciphertext, signature FROM events WHERE stream_id=?1 ORDER BY created_at_ms, id",
        )?;
        let values = stmt
            .query_map(params![stream_id], |row| {
                let nonce: Vec<u8> = row.get(4)?;
                let nonce: [u8; 24] = nonce.try_into().map_err(|_| {
                    rusqlite::Error::InvalidColumnType(
                        4,
                        "nonce".into(),
                        rusqlite::types::Type::Blob,
                    )
                })?;
                let kind_raw: i32 = row.get(3)?;
                let kind = EventKind::try_from(kind_raw).map_err(|e| {
                    rusqlite::Error::FromSqlConversionFailure(
                        3,
                        rusqlite::types::Type::Integer,
                        e.into(),
                    )
                })?;
                Ok(EventEnvelope {
                    id: row.get(0)?,
                    stream_id: stream_id.to_owned(),
                    author: DeviceId(row.get(1)?),
                    created_at_ms: row.get(2)?,
                    kind,
                    nonce,
                    ciphertext: row.get(5)?,
                    signature: row.get(6)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(values)
    }

    pub fn event_ids(&self, stream_id: &str) -> anyhow::Result<HashSet<String>> {
        Ok(self
            .events(stream_id)?
            .into_iter()
            .map(|event| event.id)
            .collect())
    }

    pub fn save_manifest(&self, peer: &DeviceId, manifest: &FileManifest) -> anyhow::Result<()> {
        let mut conn = self.conn.lock();
        let tx = conn.transaction()?;
        tx.execute(
            "INSERT OR IGNORE INTO manifests(id, peer_id, manifest_json, complete) VALUES(?1, ?2, ?3, 0)",
            params![manifest.id, peer.0, serde_json::to_string(manifest)?],
        )?;
        for chunk in &manifest.chunks {
            tx.execute(
                "INSERT OR IGNORE INTO manifest_chunks(manifest_id, chunk_index, hash, present) VALUES(?1, ?2, ?3, ?4)",
                params![manifest.id, chunk.index, chunk.hash, i32::from(self.blob_path(&chunk.hash).exists())],
            )?;
        }
        tx.commit()?;
        drop(conn);
        self.refresh_manifest_complete(&manifest.id)?;
        Ok(())
    }

    pub fn manifest(&self, id: &str) -> anyhow::Result<Option<FileManifest>> {
        let json: Option<String> = self
            .conn
            .lock()
            .query_row(
                "SELECT manifest_json FROM manifests WHERE id=?1",
                params![id],
                |row| row.get(0),
            )
            .optional()?;
        json.map(|value| Ok(serde_json::from_str(&value)?))
            .transpose()
    }

    pub fn missing_chunks(&self, manifest_id: &str) -> anyhow::Result<Vec<String>> {
        let conn = self.conn.lock();
        let mut stmt = conn.prepare("SELECT hash FROM manifest_chunks WHERE manifest_id=?1 AND present=0 ORDER BY chunk_index")?;
        let hashes = stmt
            .query_map(params![manifest_id], |row| row.get(0))?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(hashes)
    }

    pub fn mark_chunk_present(&self, hash: &str) -> anyhow::Result<()> {
        self.conn.lock().execute(
            "UPDATE manifest_chunks SET present=1 WHERE hash=?1",
            params![hash],
        )?;
        Ok(())
    }

    pub fn refresh_manifest_complete(&self, id: &str) -> anyhow::Result<bool> {
        let missing: i64 = self.conn.lock().query_row(
            "SELECT COUNT(*) FROM manifest_chunks WHERE manifest_id=?1 AND present=0",
            params![id],
            |row| row.get(0),
        )?;
        self.conn.lock().execute(
            "UPDATE manifests SET complete=?2 WHERE id=?1",
            params![id, i32::from(missing == 0)],
        )?;
        Ok(missing == 0)
    }

    pub fn incomplete_manifests_for_peer(
        &self,
        peer: &DeviceId,
    ) -> anyhow::Result<Vec<FileManifest>> {
        let conn = self.conn.lock();
        let mut stmt =
            conn.prepare("SELECT manifest_json FROM manifests WHERE peer_id=?1 AND complete=0")?;
        let jsons = stmt
            .query_map(params![peer.0], |row| row.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        jsons
            .into_iter()
            .map(|json| Ok(serde_json::from_str(&json)?))
            .collect()
    }

    pub fn blob_path(&self, hash: &str) -> PathBuf {
        self.blob_dir.join(&hash[..2]).join(hash)
    }

    pub fn put_blob(&self, expected_hash: &str, bytes: &[u8]) -> anyhow::Result<()> {
        let actual = blake3::hash(bytes).to_hex().to_string();
        anyhow::ensure!(actual == expected_hash, "chunk hash mismatch");
        let path = self.blob_path(expected_hash);
        if !path.exists() {
            std::fs::create_dir_all(path.parent().expect("blob parent"))?;
            let temporary = path.with_extension("partial");
            std::fs::write(&temporary, bytes)?;
            std::fs::rename(temporary, &path)?;
        }
        self.mark_chunk_present(expected_hash)?;
        Ok(())
    }

    pub fn get_blob(&self, hash: &str) -> anyhow::Result<Vec<u8>> {
        Ok(std::fs::read(self.blob_path(hash))?)
    }
}
