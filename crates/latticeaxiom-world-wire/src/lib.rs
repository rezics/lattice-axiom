//! Explicit `world-wire@1` keys and snapshot envelope codecs.
//!
//! The codecs in this crate are the portable persistence boundary. They never
//! serialize Rust layouts for keys or envelopes, and they reuse the canonical
//! identity and storage model types from `latticeaxiom-core` and
//! `latticeaxiom-storage`. Postcard DTOs are explicit, versioned, sealed, and
//! budgeted; deriving a serialization trait for a domain type does not make it
//! a wire contract.
//!
//! Scope is limited to the WORLD-01 chunk-record key and WORLD-06 outer
//! envelope foundation. This crate does not define `ChunkSnapshotV1`, palette
//! canonicalization, voxel/fluid bit packing, database checkpoints, or headers.

mod error;
mod key;
mod limits;
mod persisted_chunk;
mod snapshot;

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

/// The only chunk-record key major written by this crate.
pub const CHUNK_RECORD_KEY_MAJOR: u16 = 1;

/// The only snapshot envelope major written by this crate.
pub const SNAPSHOT_ENVELOPE_MAJOR: u16 = 1;

/// The fixed eight-byte snapshot envelope marker.
pub const SNAPSHOT_MAGIC: [u8; 8] = *b"LAXWSNP\0";
