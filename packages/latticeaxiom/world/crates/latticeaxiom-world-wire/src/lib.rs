//! Explicit `world-wire@1` keys and snapshot envelope codecs.
//!
//! The codecs in this crate are the portable persistence boundary. They never
//! serialize Rust layouts for keys or envelopes, and they reuse the canonical
//! identity and storage model types from `latticeaxiom-core` and
//! `latticeaxiom-storage`. Postcard DTOs are explicit, versioned, sealed, and
//! budgeted; deriving a serialization trait for a domain type does not make it
//! a wire contract.
//!
//! Scope is the WORLD-01 chunk-record key, WORLD-06 outer envelope, and the
//! versioned solid/fluid palette snapshot. This crate does not define
//! `ChunkSnapshotV1`, database checkpoints, or world headers.

mod error;
mod key;
mod limits;
mod persisted_chunk;
mod snapshot;
mod solid_fluid_palette;

pub use error::{WireResult, WorldWireError};
pub use key::{
    ChunkRecordKey, KeyRange, RecordKind, chunk_key_prefix, decode_chunk_record_key,
    dimension_key_prefix, encode_chunk_record_key, prefix_range, record_kind_key_prefix,
    world_key_prefix,
};
pub use limits::{SnapshotCodecLimits, SnapshotCodecResource, WireSegment, WorldWireLimits};
pub use persisted_chunk::{
    EncodedPersistedChunkSnapshotV1, PERSISTED_CHUNK_SNAPSHOT_OWNER_V1,
    PERSISTED_CHUNK_SNAPSHOT_SCHEMA_ID_V1, PERSISTED_CHUNK_SNAPSHOT_SCHEMA_VERSION_V1,
    PersistedChunkDomainRevisionsV1, PersistedChunkSnapshotCodecV1, PersistedChunkSnapshotV1,
    decode_persisted_chunk_snapshot_v1, encode_persisted_chunk_snapshot_v1,
};
pub use snapshot::{
    BorrowedSnapshotEnvelope, OwnedSnapshotEnvelope, SnapshotContract, SnapshotSchemaCodec,
    ValidatedSnapshotPayload, decode_typed_snapshot, encode_snapshot, encode_typed_snapshot,
    preflight_snapshot, preflight_snapshot_for_contract, read_snapshot, read_snapshot_for_contract,
    read_typed_snapshot, require_numeric_tag, write_snapshot,
};
pub use solid_fluid_palette::{
    CHUNK_EDGE_V1, EncodedSolidFluidPaletteV1, FLUID_STATE_SCHEMA_V1,
    FluidPaletteOpenDispositionV1, FluidPaletteWireEntryV1, SOLID_FLUID_PALETTE_OWNER_V1,
    SOLID_FLUID_PALETTE_SCHEMA_ID_V1, SOLID_FLUID_PALETTE_SCHEMA_VERSION_V1,
    SolidFluidPaletteSnapshotCodecV1, SolidFluidPaletteSnapshotV1, SolidPaletteWireEntryV1,
    classify_fluid_palette_open, decode_solid_fluid_palette_v1, encode_solid_fluid_palette_v1,
    linear_index,
};

/// The only chunk-record key major written by this crate.
pub const CHUNK_RECORD_KEY_MAJOR: u16 = 1;

/// The only snapshot envelope major written by this crate.
pub const SNAPSHOT_ENVELOPE_MAJOR: u16 = 1;

/// The fixed eight-byte snapshot envelope marker.
pub const SNAPSHOT_MAGIC: [u8; 8] = *b"LAXWSNP\0";
