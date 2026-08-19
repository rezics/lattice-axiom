//! Storage DTO for authoritative palette-backed chunks.

use anyhow::{Context as _, Result};
use latticeaxiom_core::{BlockId, Chunk, ChunkPos};
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
struct ChunkPayloadV1 {
    palette: Vec<u32>,
    indices: Vec<u16>,
}

/// Encodes the runtime chunk into the schema-1 storage DTO.
pub fn encode(chunk: &Chunk) -> Result<Vec<u8>> {
    let dto = ChunkPayloadV1 {
        palette: chunk.palette().iter().map(|id| id.to_raw()).collect(),
        indices: chunk.palette_indices().to_vec(),
    };
    bincode::serde::encode_to_vec(&dto, bincode::config::standard())
        .context("failed to encode chunk payload schema 1")
}

/// Decodes a schema-1 storage DTO into canonical runtime state.
pub fn decode(position: ChunkPos, payload: &[u8]) -> Result<Chunk> {
    let (dto, consumed): (ChunkPayloadV1, usize) =
        bincode::serde::decode_from_slice(payload, bincode::config::standard())
            .context("failed to decode chunk payload schema 1")?;
    if consumed != payload.len() {
        anyhow::bail!(
            "chunk payload has {} trailing bytes",
            payload.len().saturating_sub(consumed)
        );
    }
    let palette = dto.palette.into_iter().map(BlockId::from_raw).collect();
    Chunk::from_palette_indices(position, palette, dto.indices)
        .context("decoded chunk payload violates canonical palette invariants")
}

#[cfg(test)]
mod tests {
    use latticeaxiom_core::LocalBlockPos;

    use super::*;

    #[test]
    fn schema_round_trips_canonical_chunk() {
        let position = ChunkPos::new(-2, 3, 0);
        let mut chunk = Chunk::empty(position);
        chunk.set_block(
            LocalBlockPos::new(31, 2, 4).expect("test coordinate is inside the chunk"),
            BlockId::from_raw(7),
        );
        let payload = encode(&chunk).expect("encoding a valid chunk must succeed");
        let decoded = decode(position, &payload).expect("encoded chunk must decode");
        assert_eq!(decoded, chunk);
    }

    #[test]
    fn trailing_bytes_are_rejected() {
        let chunk = Chunk::empty(ChunkPos::default());
        let mut payload = encode(&chunk).expect("encoding a valid chunk must succeed");
        payload.push(0);
        assert!(decode(chunk.position(), &payload).is_err());
    }
}
