use std::str::FromStr;

use latticeaxiom_core::{StableId, WorldId};
use latticeaxiom_storage::{ChunkCoordinate, ChunkKey, DimensionId};

use crate::{CHUNK_RECORD_KEY_MAJOR, WireResult, WireSegment, WorldWireError, WorldWireLimits};

const KEY_MAJOR_BYTES: usize = 2;
const WORLD_ID_BYTES: usize = 16;
const SEGMENT_LENGTH_BYTES: usize = 2;
const COORDINATE_BYTES: usize = 3 * 4;
const RECORD_KIND_BYTES: usize = 2;

/// Version-one interpretation of a fixed `u16` chunk-record kind tag.
///
/// The raw field is private, so an unknown tag cannot be constructed in a state
/// that aliases a known writable tag. Unknown values remain distinguishable for
/// opaque read-only handling.
#[repr(transparent)]
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct RecordKind(u16);

impl RecordKind {
    /// Complete authoritative chunk snapshot (`u16be` tag `1`).
    pub const CHUNK_SNAPSHOT: Self = Self(1);

    /// Interprets a raw `u16` tag without discarding unknown values.
    #[must_use]
    pub const fn from_raw(raw: u16) -> Self {
        Self(raw)
    }

    /// Returns the exact tag encoded in the key.
    #[must_use]
    pub const fn raw(self) -> u16 {
        self.0
    }

    /// Returns whether this tag has defined semantics under key major one.
    #[must_use]
    pub const fn is_known_v1(self) -> bool {
        self.0 == Self::CHUNK_SNAPSHOT.0
    }

    /// Returns whether version one permits producing new records of this kind.
    #[must_use]
    pub const fn is_writable_v1(self) -> bool {
        self.is_known_v1()
    }
}

/// Complete logical tuple represented by a chunk-record key.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ChunkRecordKey {
    chunk: ChunkKey,
    record_kind: RecordKind,
    owner: StableId,
}

impl ChunkRecordKey {
    /// Creates a record tuple from canonical shared identity types.
    #[must_use]
    pub const fn new(chunk: ChunkKey, record_kind: RecordKind, owner: StableId) -> Self {
        Self {
            chunk,
            record_kind,
            owner,
        }
    }

    /// Returns the world, dimension, and canonical `(x, y, z)` coordinate.
    #[must_use]
    pub const fn chunk(&self) -> &ChunkKey {
        &self.chunk
    }

    /// Returns the fixed or opaque record kind.
    #[must_use]
    pub const fn record_kind(&self) -> RecordKind {
        self.record_kind
    }

    /// Returns the canonical stable ID owning this record.
    #[must_use]
    pub const fn owner(&self) -> &StableId {
        &self.owner
    }
}

/// Lexicographic half-open range covering one encoded prefix.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct KeyRange {
    start: Vec<u8>,
    exclusive_end: Option<Vec<u8>>,
}

impl KeyRange {
    /// Returns the inclusive range start.
    #[must_use]
    pub fn start(&self) -> &[u8] {
        &self.start
    }

    /// Returns the exclusive range end, or `None` when no finite successor exists.
    #[must_use]
    pub fn exclusive_end(&self) -> Option<&[u8]> {
        self.exclusive_end.as_deref()
    }

    /// Tests whether an encoded key is inside this lexicographic range.
    #[must_use]
    pub fn contains(&self, key: &[u8]) -> bool {
        key >= self.start.as_slice()
            && self
                .exclusive_end
                .as_deref()
                .is_none_or(|exclusive_end| key < exclusive_end)
    }
}

/// Encodes a writable version-one chunk-record key from ADR 0027.
///
/// # Errors
///
/// Returns [`WorldWireError::ReadOnlyRecordKind`] for an opaque future tag, or a
/// typed length error before allocation when either textual segment exceeds its
/// active hard ceiling or the `u16` wire representation.
pub fn encode_chunk_record_key(
    key: &ChunkRecordKey,
    limits: WorldWireLimits,
) -> WireResult<Vec<u8>> {
    if !key.record_kind.is_writable_v1() {
        return Err(WorldWireError::ReadOnlyRecordKind {
            found: key.record_kind.raw(),
        });
    }
    encode_key_tuple(key, limits)
}

fn encode_key_tuple(key: &ChunkRecordKey, limits: WorldWireLimits) -> WireResult<Vec<u8>> {
    let dimension = key.chunk.dimension.as_str().as_bytes();
    let owner = key.owner.as_str().as_bytes();
    limits.check(WireSegment::DimensionId, dimension.len())?;
    limits.check(WireSegment::RecordOwnerId, owner.len())?;
    let dimension_length = wire_u16_length(WireSegment::DimensionId, dimension.len())?;
    let owner_length = wire_u16_length(WireSegment::RecordOwnerId, owner.len())?;

    let capacity = KEY_MAJOR_BYTES
        .checked_add(WORLD_ID_BYTES)
        .and_then(|value| value.checked_add(SEGMENT_LENGTH_BYTES))
        .and_then(|value| value.checked_add(dimension.len()))
        .and_then(|value| value.checked_add(COORDINATE_BYTES))
        .and_then(|value| value.checked_add(RECORD_KIND_BYTES))
        .and_then(|value| value.checked_add(SEGMENT_LENGTH_BYTES))
        .and_then(|value| value.checked_add(owner.len()))
        .ok_or(WorldWireError::LengthOverflow {
            segment: WireSegment::RecordOwnerId,
        })?;
    let mut encoded = Vec::with_capacity(capacity);
    encode_common_chunk_prefix(&mut encoded, &key.chunk, dimension, dimension_length);
    encoded.extend_from_slice(&key.record_kind.raw().to_be_bytes());
    encoded.extend_from_slice(&owner_length.to_be_bytes());
    encoded.extend_from_slice(owner);
    Ok(encoded)
}

/// Decodes one complete chunk-record key without accepting a writable alias for
/// an unknown record kind.
///
/// # Errors
///
/// Returns a typed error for unsupported majors, preflight ceiling violations,
/// truncation, malformed identities, invalid UTF-8, or trailing bytes.
pub fn decode_chunk_record_key(
    encoded: &[u8],
    limits: WorldWireLimits,
) -> WireResult<ChunkRecordKey> {
    let mut cursor = SliceCursor::new(encoded);
    let major = cursor.read_u16("chunk-record key major")?;
    if major != CHUNK_RECORD_KEY_MAJOR {
        return Err(WorldWireError::UnsupportedKeyMajor { found: major });
    }

    let world_bytes = cursor.read_array::<16>("world ID")?;
    let world =
        WorldId::from_bytes(world_bytes).map_err(|error| WorldWireError::InvalidWorldId {
            reason: error.to_string(),
        })?;
    let dimension = read_text_segment(
        &mut cursor,
        limits,
        WireSegment::DimensionId,
        "dimension ID",
    )?;
    let dimension =
        DimensionId::from_str(dimension).map_err(|error| WorldWireError::InvalidDimensionId {
            reason: error.to_string(),
        })?;
    let coordinate = ChunkCoordinate::new(
        decode_ordered_coordinate(cursor.read_u32("ordered x")?),
        decode_ordered_coordinate(cursor.read_u32("ordered y")?),
        decode_ordered_coordinate(cursor.read_u32("ordered z")?),
    );
    let record_kind = RecordKind::from_raw(cursor.read_u16("record kind")?);
    let owner = read_text_segment(
        &mut cursor,
        limits,
        WireSegment::RecordOwnerId,
        "record owner ID",
    )?;
    let owner =
        StableId::from_str(owner).map_err(|error| WorldWireError::InvalidRecordOwnerId {
            reason: error.to_string(),
        })?;
    if cursor.remaining() != 0 {
        return Err(WorldWireError::TrailingKeyBytes {
            remaining: cursor.remaining(),
        });
    }
    Ok(ChunkRecordKey::new(
        ChunkKey::new(world, dimension, coordinate),
        record_kind,
        owner,
    ))
}

/// Encodes the version and world prefix shared by every record in one world.
#[must_use]
pub fn world_key_prefix(world: WorldId) -> Vec<u8> {
    let mut prefix = Vec::with_capacity(KEY_MAJOR_BYTES + WORLD_ID_BYTES);
    prefix.extend_from_slice(&CHUNK_RECORD_KEY_MAJOR.to_be_bytes());
    prefix.extend_from_slice(&world.as_bytes());
    prefix
}

/// Encodes the prefix shared by all records in one world dimension.
///
/// # Errors
///
/// Returns a typed length error before allocation when the dimension ID exceeds
/// the configured or representable ceiling.
pub fn dimension_key_prefix(
    world: WorldId,
    dimension: &DimensionId,
    limits: WorldWireLimits,
) -> WireResult<Vec<u8>> {
    let dimension = dimension.as_str().as_bytes();
    limits.check(WireSegment::DimensionId, dimension.len())?;
    let dimension_length = wire_u16_length(WireSegment::DimensionId, dimension.len())?;
    let mut prefix = world_key_prefix(world);
    prefix.extend_from_slice(&dimension_length.to_be_bytes());
    prefix.extend_from_slice(dimension);
    Ok(prefix)
}

/// Encodes the prefix shared by every record kind and owner for one chunk.
///
/// # Errors
///
/// Returns a typed length error before allocation when the dimension ID exceeds
/// the configured or representable ceiling.
pub fn chunk_key_prefix(key: &ChunkKey, limits: WorldWireLimits) -> WireResult<Vec<u8>> {
    let dimension = key.dimension.as_str().as_bytes();
    limits.check(WireSegment::DimensionId, dimension.len())?;
    let dimension_length = wire_u16_length(WireSegment::DimensionId, dimension.len())?;
    let mut prefix = Vec::with_capacity(
        KEY_MAJOR_BYTES
            + WORLD_ID_BYTES
            + SEGMENT_LENGTH_BYTES
            + dimension.len()
            + COORDINATE_BYTES,
    );
    encode_common_chunk_prefix(&mut prefix, key, dimension, dimension_length);
    Ok(prefix)
}

/// Encodes the prefix shared by all owners for one chunk record kind.
///
/// # Errors
///
/// Returns a typed length error before allocation when the dimension ID exceeds
/// the configured or representable ceiling.
pub fn record_kind_key_prefix(
    key: &ChunkKey,
    record_kind: RecordKind,
    limits: WorldWireLimits,
) -> WireResult<Vec<u8>> {
    let mut prefix = chunk_key_prefix(key, limits)?;
    prefix.extend_from_slice(&record_kind.raw().to_be_bytes());
    Ok(prefix)
}

/// Produces the smallest half-open lexicographic range covering `prefix`.
#[must_use]
pub fn prefix_range(prefix: Vec<u8>) -> KeyRange {
    let exclusive_end = prefix_successor(&prefix);
    KeyRange {
        start: prefix,
        exclusive_end,
    }
}

fn encode_common_chunk_prefix(
    output: &mut Vec<u8>,
    key: &ChunkKey,
    dimension: &[u8],
    dimension_length: u16,
) {
    output.extend_from_slice(&CHUNK_RECORD_KEY_MAJOR.to_be_bytes());
    output.extend_from_slice(&key.world.as_bytes());
    output.extend_from_slice(&dimension_length.to_be_bytes());
    output.extend_from_slice(dimension);
    output.extend_from_slice(&encode_ordered_coordinate(key.coordinate.x).to_be_bytes());
    output.extend_from_slice(&encode_ordered_coordinate(key.coordinate.y).to_be_bytes());
    output.extend_from_slice(&encode_ordered_coordinate(key.coordinate.z).to_be_bytes());
}

const fn encode_ordered_coordinate(value: i32) -> u32 {
    value.cast_unsigned() ^ 0x8000_0000
}

const fn decode_ordered_coordinate(value: u32) -> i32 {
    (value ^ 0x8000_0000).cast_signed()
}

fn wire_u16_length(segment: WireSegment, length: usize) -> WireResult<u16> {
    u16::try_from(length).map_err(|_| WorldWireError::LengthOverflow { segment })
}

fn read_text_segment<'a>(
    cursor: &mut SliceCursor<'a>,
    limits: WorldWireLimits,
    segment: WireSegment,
    section: &'static str,
) -> WireResult<&'a str> {
    let length = usize::from(cursor.read_u16(section)?);
    limits.check(segment, length)?;
    let bytes = cursor.read(length, section)?;
    std::str::from_utf8(bytes).map_err(|_| WorldWireError::InvalidUtf8 { segment })
}

fn prefix_successor(prefix: &[u8]) -> Option<Vec<u8>> {
    let mut successor = prefix.to_vec();
    while let Some(last) = successor.last_mut() {
        if *last == u8::MAX {
            successor.pop();
        } else {
            *last += 1;
            return Some(successor);
        }
    }
    None
}

struct SliceCursor<'a> {
    bytes: &'a [u8],
    position: usize,
}

impl<'a> SliceCursor<'a> {
    const fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, position: 0 }
    }

    fn read(&mut self, length: usize, section: &'static str) -> WireResult<&'a [u8]> {
        let remaining = self.remaining();
        if length > remaining {
            return Err(WorldWireError::Truncated {
                section,
                needed: length,
                remaining,
            });
        }
        let end = self
            .position
            .checked_add(length)
            .ok_or(WorldWireError::LengthOverflow {
                segment: WireSegment::SnapshotPayload,
            })?;
        let value = &self.bytes[self.position..end];
        self.position = end;
        Ok(value)
    }

    fn read_array<const N: usize>(&mut self, section: &'static str) -> WireResult<[u8; N]> {
        let mut output = [0_u8; N];
        output.copy_from_slice(self.read(N, section)?);
        Ok(output)
    }

    fn read_u16(&mut self, section: &'static str) -> WireResult<u16> {
        Ok(u16::from_be_bytes(self.read_array(section)?))
    }

    fn read_u32(&mut self, section: &'static str) -> WireResult<u32> {
        Ok(u32::from_be_bytes(self.read_array(section)?))
    }

    const fn remaining(&self) -> usize {
        self.bytes.len() - self.position
    }
}
