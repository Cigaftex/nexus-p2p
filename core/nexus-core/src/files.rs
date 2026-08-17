use std::{
    fs::File,
    io::{Read, Write},
    path::Path,
};

use crate::{
    model::{ChunkDescriptor, FileManifest, FILE_CHUNK_SIZE},
    storage::Store,
};

pub fn ingest_file(
    store: &Store,
    path: impl AsRef<Path>,
    media_type: &str,
) -> anyhow::Result<FileManifest> {
    let path = path.as_ref();
    let mut file = File::open(path)?;
    let mut whole_hasher = blake3::Hasher::new();
    let mut chunks = Vec::new();
    let mut total = 0_u64;
    loop {
        let mut buffer = vec![0_u8; FILE_CHUNK_SIZE];
        let count = file.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        buffer.truncate(count);
        whole_hasher.update(&buffer);
        let hash = blake3::hash(&buffer).to_hex().to_string();
        store.put_blob(&hash, &buffer)?;
        chunks.push(ChunkDescriptor {
            index: chunks.len() as u32,
            hash,
            size: count as u32,
        });
        total += count as u64;
    }
    let file_hash = whole_hasher.finalize().to_hex().to_string();
    Ok(FileManifest {
        id: ulid::Ulid::new().to_string(),
        name: path
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or("file")
            .to_owned(),
        media_type: media_type.to_owned(),
        size: total,
        chunk_size: FILE_CHUNK_SIZE as u32,
        chunks,
        file_hash,
    })
}

pub fn materialize_file(
    store: &Store,
    manifest: &FileManifest,
    destination: impl AsRef<Path>,
) -> anyhow::Result<()> {
    anyhow::ensure!(
        store.missing_chunks(&manifest.id)?.is_empty(),
        "file is not complete"
    );
    let destination = destination.as_ref();
    let temporary = destination.with_extension("nexus-partial");
    let mut output = File::create(&temporary)?;
    let mut whole_hasher = blake3::Hasher::new();
    for chunk in &manifest.chunks {
        let bytes = store.get_blob(&chunk.hash)?;
        anyhow::ensure!(
            bytes.len() == chunk.size as usize,
            "stored chunk size mismatch"
        );
        whole_hasher.update(&bytes);
        output.write_all(&bytes)?;
    }
    output.sync_all()?;
    anyhow::ensure!(
        whole_hasher.finalize().to_hex().as_str() == manifest.file_hash,
        "assembled file hash mismatch"
    );
    std::fs::rename(temporary, destination)?;
    Ok(())
}
