//! Versioned solid/fluid palette snapshot encoding.
//!
//! Bit streams are little-endian byte / LSB-first. Local voxel order is
//! `x + 32 * (z + 32 * y)`. Unknown schema never becomes writable.

#![allow(
    clippy::too_many_lines,
    reason = "encode/decode stay in one schema owner"
)]

use std::{
    num::{NonZeroU16, NonZeroU32, NonZeroU64},
    str::FromStr,
};

use latticeaxiom_core::{PackageName, SchemaId, StableId, WorldId};
use latticeaxiom_storage::{
    ChunkKey, ChunkRevision, DimensionId, PayloadSchemaVersion, VoxelRevision, WorldRevision,
};
use serde::{Deserialize, Serialize};

use crate::snapshot::{codec_seal, encode_typed_snapshot_exact};
use crate::{
    SnapshotCodecLimits, SnapshotCodecResource, SnapshotContract, SnapshotSchemaCodec,
    ValidatedSnapshotPayload, WireResult, WireSegment, WorldWireError, WorldWireLimits,
    decode_typed_snapshot, require_numeric_tag,
};

/// First-party owner of solid/fluid palette schema version one.
pub const SOLID_FLUID_PALETTE_OWNER_V1: &str = "latticeaxiom";

/// Schema identity of solid/fluid palette schema version one.
pub const SOLID_FLUID_PALETTE_SCHEMA_ID_V1: &str = "latticeaxiom:schema/solid-fluid-palette@1";

/// Exact owner-controlled palette payload version.
pub const SOLID_FLUID_PALETTE_SCHEMA_VERSION_V1: u32 = 1;

/// Frozen fluid-state schema referenced by non-empty palette rows.
pub const FLUID_STATE_SCHEMA_V1: &str = "latticeaxiom:schema/fluid-state@1";

/// Authoritative cubic chunk edge.
pub const CHUNK_EDGE_V1: u16 = 32;

const MAX_PAYLOAD_BYTES: u64 = 64 * 1024 * 1024;
const MAX_COLLECTION_ENTRIES: u32 = 65_536;
const MAX_NESTING_DEPTH: u16 = 4;
const MAX_IDENTIFIER_BYTES: u64 = 1_024;

const FLUID_KIND_EMPTY: u16 = 0;
const FLUID_KIND_FLUID: u16 = 1;
const FLOW_STILL: u16 = 0;
const FLOW_DOWN: u16 = 1;
const FLOW_EAST: u16 = 2;
const FLOW_WEST: u16 = 3;
const FLOW_SOUTH: u16 = 4;
const FLOW_NORTH: u16 = 5;

/// Writer/open disposition for a candidate palette schema.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FluidPaletteOpenDispositionV1 {
    /// Exact v1 schema; a writer may encode.
    Writable,
    /// Structurally valid unknown schema; preserve bytes read-only.
    ReadOnlyRecovery,
    /// Not a schema identity; reject before opening a writer.
    Rejected,
}

/// Classifies a candidate schema before a writer is opened.
#[must_use]
pub fn classify_fluid_palette_open(schema: &SchemaId) -> FluidPaletteOpenDispositionV1 {
    if schema.as_str() == SOLID_FLUID_PALETTE_SCHEMA_ID_V1 {
        FluidPaletteOpenDispositionV1::Writable
    } else {
        FluidPaletteOpenDispositionV1::ReadOnlyRecovery
    }
}

/// Wire row for one solid palette entry.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct SolidPaletteWireEntryV1 {
    block: String,
    state_schema: String,
    state_schema_version: u32,
    canonical_state_bytes: Vec<u8>,
}

#[derive(Deserialize)]
struct SolidPaletteWireEntryDecodeV1 {
    block: String,
    state_schema: String,
    state_schema_version: u32,
    canonical_state_bytes: Vec<u8>,
}

impl SolidPaletteWireEntryV1 {
    /// Creates one solid palette row.
    #[must_use]
    pub fn new(
        block: impl Into<String>,
        state_schema: impl Into<String>,
        state_schema_version: u32,
        canonical_state_bytes: Vec<u8>,
    ) -> Self {
        Self {
            block: block.into(),
            state_schema: state_schema.into(),
            state_schema_version,
            canonical_state_bytes,
        }
    }

    /// Exact block identity text.
    #[must_use]
    pub fn block(&self) -> &str {
        &self.block
    }

    /// State schema identity text.
    #[must_use]
    pub fn state_schema(&self) -> &str {
        &self.state_schema
    }

    /// Positive state schema version.
    #[must_use]
    pub const fn state_schema_version(&self) -> u32 {
        self.state_schema_version
    }

    /// Canonical state bytes.
    #[must_use]
    pub fn canonical_state_bytes(&self) -> &[u8] {
        &self.canonical_state_bytes
    }
}

/// Wire row for one fluid palette entry.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct FluidPaletteWireEntryV1 {
    kind_tag: u16,
    fluid: String,
    state_schema: String,
    state_schema_version: u32,
    level: u8,
    flow_tag: u16,
}

#[derive(Deserialize)]
struct FluidPaletteWireEntryDecodeV1 {
    kind_tag: u16,
    fluid: String,
    state_schema: String,
    state_schema_version: u32,
    level: u8,
    flow_tag: u16,
}

impl FluidPaletteWireEntryV1 {
    /// Canonical empty fluid row. Must occupy index zero.
    #[must_use]
    pub fn empty() -> Self {
        Self {
            kind_tag: FLUID_KIND_EMPTY,
            fluid: String::new(),
            state_schema: String::new(),
            state_schema_version: 0,
            level: 0,
            flow_tag: FLOW_STILL,
        }
    }

    /// Non-empty fluid row.
    #[must_use]
    pub fn fluid(
        fluid: impl Into<String>,
        state_schema: impl Into<String>,
        state_schema_version: u32,
        level: u8,
        flow_tag: u16,
    ) -> Self {
        Self {
            kind_tag: FLUID_KIND_FLUID,
            fluid: fluid.into(),
            state_schema: state_schema.into(),
            state_schema_version,
            level,
            flow_tag,
        }
    }

    /// Returns whether this is the canonical empty row.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.kind_tag == FLUID_KIND_EMPTY
    }

    /// Explicit kind tag.
    #[must_use]
    pub const fn kind_tag(&self) -> u16 {
        self.kind_tag
    }

    /// Fluid identity text; empty for the empty row.
    #[must_use]
    pub fn fluid_id(&self) -> &str {
        &self.fluid
    }

    /// Level `0..=7`.
    #[must_use]
    pub const fn level(&self) -> u8 {
        self.level
    }

    /// Explicit flow tag.
    #[must_use]
    pub const fn flow_tag(&self) -> u16 {
        self.flow_tag
    }
}

/// Versioned solid/fluid palette snapshot for one cubic chunk.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct SolidFluidPaletteSnapshotV1 {
    key: ChunkKey,
    captured_world_revision: WorldRevision,
    chunk_revision: ChunkRevision,
    voxel_revision: VoxelRevision,
    chunk_edge: u16,
    solid_palette: Vec<SolidPaletteWireEntryV1>,
    fluid_palette: Vec<FluidPaletteWireEntryV1>,
    packed_solid: Vec<u8>,
    packed_fluid: Vec<u8>,
    fluid_state_schema: String,
    fluid_state_schema_version: u32,
}

#[derive(Deserialize)]
struct SolidFluidPaletteSnapshotDecodeV1 {
    key: ChunkKey,
    captured_world_revision: WorldRevision,
    chunk_revision: ChunkRevision,
    voxel_revision: VoxelRevision,
    chunk_edge: u16,
    solid_palette: Vec<SolidPaletteWireEntryDecodeV1>,
    fluid_palette: Vec<FluidPaletteWireEntryDecodeV1>,
    packed_solid: Vec<u8>,
    packed_fluid: Vec<u8>,
    fluid_state_schema: String,
    fluid_state_schema_version: u32,
}

impl SolidFluidPaletteSnapshotDecodeV1 {
    fn into_value(self) -> SolidFluidPaletteSnapshotV1 {
        SolidFluidPaletteSnapshotV1 {
            key: self.key,
            captured_world_revision: self.captured_world_revision,
            chunk_revision: self.chunk_revision,
            voxel_revision: self.voxel_revision,
            chunk_edge: self.chunk_edge,
            solid_palette: self
                .solid_palette
                .into_iter()
                .map(|entry| SolidPaletteWireEntryV1 {
                    block: entry.block,
                    state_schema: entry.state_schema,
                    state_schema_version: entry.state_schema_version,
                    canonical_state_bytes: entry.canonical_state_bytes,
                })
                .collect(),
            fluid_palette: self
                .fluid_palette
                .into_iter()
                .map(|entry| FluidPaletteWireEntryV1 {
                    kind_tag: entry.kind_tag,
                    fluid: entry.fluid,
                    state_schema: entry.state_schema,
                    state_schema_version: entry.state_schema_version,
                    level: entry.level,
                    flow_tag: entry.flow_tag,
                })
                .collect(),
            packed_solid: self.packed_solid,
            packed_fluid: self.packed_fluid,
            fluid_state_schema: self.fluid_state_schema,
            fluid_state_schema_version: self.fluid_state_schema_version,
        }
    }
}

impl SolidFluidPaletteSnapshotV1 {
    /// Compiles, canonicalizes, and bit-packs one dual-layer volume.
    ///
    /// `solid_indices` and `fluid_indices` are in canonical linear order.
    /// An all-empty fluid layer is stored as a missing packed section.
    ///
    /// # Errors
    ///
    /// Returns a typed identity, range, palette, packing, or canonicality error.
    #[allow(
        clippy::too_many_arguments,
        reason = "snapshot construction keeps key, revisions, palettes, and packed indices explicit"
    )]
    pub fn from_layers(
        key: ChunkKey,
        captured_world_revision: WorldRevision,
        chunk_revision: ChunkRevision,
        voxel_revision: VoxelRevision,
        solid_palette: Vec<SolidPaletteWireEntryV1>,
        fluid_palette: Vec<FluidPaletteWireEntryV1>,
        solid_indices: Vec<u16>,
        fluid_indices: Vec<u16>,
    ) -> WireResult<Self> {
        let (solid_palette, solid_indices) = canonicalize_solid(solid_palette, solid_indices)?;
        let (fluid_palette, fluid_indices) = canonicalize_fluid(fluid_palette, fluid_indices)?;
        let packed_solid = pack_indices(&solid_indices, solid_palette.len())?;
        let packed_fluid = if fluid_indices.iter().all(|index| *index == 0) {
            Vec::new()
        } else {
            pack_indices(&fluid_indices, fluid_palette.len())?
        };
        let snapshot = Self {
            key,
            captured_world_revision,
            chunk_revision,
            voxel_revision,
            chunk_edge: CHUNK_EDGE_V1,
            solid_palette,
            fluid_palette,
            packed_solid,
            packed_fluid,
            fluid_state_schema: FLUID_STATE_SCHEMA_V1.to_owned(),
            fluid_state_schema_version: 1,
        };
        validate_snapshot(&snapshot)?;
        Ok(snapshot)
    }

    /// Returns the chunk key, including signed Y-up coordinates.
    #[must_use]
    pub const fn key(&self) -> &ChunkKey {
        &self.key
    }

    /// Returns the compiled solid palette.
    #[must_use]
    pub fn solid_palette(&self) -> &[SolidPaletteWireEntryV1] {
        &self.solid_palette
    }

    /// Returns the compiled fluid palette.
    #[must_use]
    pub fn fluid_palette(&self) -> &[FluidPaletteWireEntryV1] {
        &self.fluid_palette
    }

    /// Unpacks solid indices in canonical linear order.
    ///
    /// # Errors
    ///
    /// Returns a malformed packed-section error.
    pub fn unpacked_solid_indices(&self) -> WireResult<Vec<u16>> {
        unpack_indices(&self.packed_solid, self.solid_palette.len())
    }

    /// Unpacks fluid indices; a missing layer is all empty.
    ///
    /// # Errors
    ///
    /// Returns a malformed packed-section error.
    pub fn unpacked_fluid_indices(&self) -> WireResult<Vec<u16>> {
        if self.packed_fluid.is_empty() {
            Ok(vec![0; cell_count()])
        } else {
            unpack_indices(&self.packed_fluid, self.fluid_palette.len())
        }
    }
}

/// Result of schema-owned palette encoding.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EncodedSolidFluidPaletteV1 {
    envelope: Vec<u8>,
    postcard_payload_bytes: u64,
}

impl EncodedSolidFluidPaletteV1 {
    /// Exact envelope bytes.
    #[must_use]
    pub fn envelope(&self) -> &[u8] {
        &self.envelope
    }

    /// Consumes the receipt and returns envelope bytes.
    #[must_use]
    pub fn into_envelope(self) -> Vec<u8> {
        self.envelope
    }

    /// Uncompressed postcard length.
    #[must_use]
    pub const fn postcard_payload_bytes(&self) -> u64 {
        self.postcard_payload_bytes
    }
}

/// Sealed first-party codec for [`SolidFluidPaletteSnapshotV1`].
#[derive(Clone, Debug)]
pub struct SolidFluidPaletteSnapshotCodecV1 {
    schema: SchemaId,
    owner: PackageName,
    version: PayloadSchemaVersion,
    limits: SnapshotCodecLimits,
}

impl SolidFluidPaletteSnapshotCodecV1 {
    /// Creates the fixed first-party schema codec.
    ///
    /// # Errors
    ///
    /// Returns a typed identity/version error if a compile-time contract
    /// constant ceases to satisfy the canonical shared identity grammar.
    pub fn new() -> WireResult<Self> {
        let schema = SchemaId::from_str(SOLID_FLUID_PALETTE_SCHEMA_ID_V1).map_err(|error| {
            WorldWireError::InvalidSchemaId {
                reason: error.to_string(),
            }
        })?;
        let owner = PackageName::from_str(SOLID_FLUID_PALETTE_OWNER_V1).map_err(|error| {
            WorldWireError::InvalidSnapshotOwner {
                reason: error.to_string(),
            }
        })?;
        let version = PayloadSchemaVersion::new(SOLID_FLUID_PALETTE_SCHEMA_VERSION_V1)
            .map_err(|_| WorldWireError::InvalidSchemaVersion)?;
        let max_payload_bytes =
            NonZeroU64::new(MAX_PAYLOAD_BYTES).ok_or(WorldWireError::InvalidLimit {
                name: "solid_fluid_palette_max_payload_bytes",
                value: MAX_PAYLOAD_BYTES,
            })?;
        let max_collection_entries =
            NonZeroU32::new(MAX_COLLECTION_ENTRIES).ok_or(WorldWireError::InvalidLimit {
                name: "solid_fluid_palette_max_collection_entries",
                value: u64::from(MAX_COLLECTION_ENTRIES),
            })?;
        let max_nesting_depth =
            NonZeroU16::new(MAX_NESTING_DEPTH).ok_or(WorldWireError::InvalidLimit {
                name: "solid_fluid_palette_max_nesting_depth",
                value: u64::from(MAX_NESTING_DEPTH),
            })?;
        Ok(Self {
            schema,
            owner,
            version,
            limits: SnapshotCodecLimits::new(
                max_payload_bytes,
                max_collection_entries,
                max_nesting_depth,
            ),
        })
    }

    /// Returns the fixed schema identity.
    #[must_use]
    pub const fn schema(&self) -> &SchemaId {
        &self.schema
    }

    /// Returns the fixed owner.
    #[must_use]
    pub const fn owner(&self) -> &PackageName {
        &self.owner
    }

    /// Returns the exact payload schema version.
    #[must_use]
    pub const fn version(&self) -> PayloadSchemaVersion {
        self.version
    }
}

impl codec_seal::Sealed for SolidFluidPaletteSnapshotCodecV1 {}

impl SnapshotSchemaCodec for SolidFluidPaletteSnapshotCodecV1 {
    type Value = SolidFluidPaletteSnapshotV1;

    fn contract(&self) -> SnapshotContract<'_> {
        SnapshotContract::new(&self.schema, &self.owner, self.version)
    }

    fn limits(&self) -> SnapshotCodecLimits {
        self.limits
    }

    fn validate_for_encode(
        &self,
        value: &Self::Value,
        limits: SnapshotCodecLimits,
    ) -> WireResult<()> {
        limits.check_nesting_depth(u32::from(MAX_NESTING_DEPTH))?;
        validate_snapshot(value)
    }

    fn decode_postcard_bounded(
        &self,
        payload: ValidatedSnapshotPayload<'_>,
    ) -> WireResult<Self::Value> {
        preflight_payload(payload.bytes(), payload.limits())?;
        let (decoded, remaining): (SolidFluidPaletteSnapshotDecodeV1, &[u8]) =
            postcard::take_from_bytes(payload.bytes()).map_err(|error| {
                WorldWireError::MalformedPayload {
                    reason: error.to_string(),
                }
            })?;
        if !remaining.is_empty() {
            return Err(WorldWireError::TrailingPayloadBytes {
                remaining: remaining.len(),
            });
        }
        let decoded = decoded.into_value();
        self.validate_for_encode(&decoded, payload.limits())?;
        Ok(decoded)
    }
}

/// Encodes a solid/fluid palette snapshot through its sealed codec.
///
/// # Errors
///
/// Returns a typed contract, canonicality, budget, postcard, or envelope error.
pub fn encode_solid_fluid_palette_v1(
    value: &SolidFluidPaletteSnapshotV1,
    limits: WorldWireLimits,
) -> WireResult<EncodedSolidFluidPaletteV1> {
    let codec = SolidFluidPaletteSnapshotCodecV1::new()?;
    codec.validate_for_encode(value, codec.limits())?;
    let payload_length = encoded_payload_length(value)?;
    codec.limits().check_payload_bytes(payload_length)?;
    limits.check(WireSegment::SnapshotPayload, payload_length)?;
    let envelope = encode_typed_snapshot_exact(&codec, value, payload_length, limits)?;
    let postcard_payload_bytes =
        u64::try_from(payload_length).map_err(|_| WorldWireError::LengthOverflow {
            segment: WireSegment::SnapshotPayload,
        })?;
    Ok(EncodedSolidFluidPaletteV1 {
        envelope,
        postcard_payload_bytes,
    })
}

/// Decodes a solid/fluid palette envelope through the sealed codec.
///
/// # Errors
///
/// Returns a typed envelope, exact-contract, unknown-schema, budget, or
/// canonicality error. Unknown schemas never decode as writable v1.
pub fn decode_solid_fluid_palette_v1(
    encoded: &[u8],
    limits: WorldWireLimits,
) -> WireResult<SolidFluidPaletteSnapshotV1> {
    let codec = SolidFluidPaletteSnapshotCodecV1::new()?;
    decode_typed_snapshot(encoded, &codec, limits)
}

/// Canonical linear index `x + 32 * (z + 32 * y)`.
#[must_use]
#[allow(
    clippy::cast_possible_truncation,
    reason = "local u16 coordinates are already bounded by the frozen 32-edge"
)]
pub const fn linear_index(x: u16, y: u16, z: u16) -> usize {
    (x as usize) + 32 * ((z as usize) + 32 * (y as usize))
}

fn cell_count() -> usize {
    32 * 32 * 32
}

fn palette_bit_width(len: usize) -> u32 {
    let max_index = len.saturating_sub(1).max(1);
    max_index.ilog2().saturating_add(1)
}

fn pack_indices(indices: &[u16], palette_len: usize) -> WireResult<Vec<u8>> {
    if indices.len() != cell_count() {
        return Err(WorldWireError::NonCanonicalPayload {
            reason: "packed index count is not a 32³ volume".to_owned(),
        });
    }
    let width = palette_bit_width(palette_len) as usize;
    let bit_len = cell_count()
        .checked_mul(width)
        .ok_or(WorldWireError::LengthOverflow {
            segment: WireSegment::SnapshotPayload,
        })?;
    let byte_len = bit_len.div_ceil(8);
    let mut packed = vec![0_u8; byte_len];
    for (cell, index) in indices.iter().enumerate() {
        if usize::from(*index) >= palette_len {
            return Err(WorldWireError::NonCanonicalPayload {
                reason: "palette index exceeds palette length".to_owned(),
            });
        }
        let bit_offset = cell
            .checked_mul(width)
            .ok_or(WorldWireError::LengthOverflow {
                segment: WireSegment::SnapshotPayload,
            })?;
        for bit in 0..width {
            if *index & (1_u16 << bit) != 0 {
                let position = bit_offset + bit;
                packed[position / 8] |= 1 << (position % 8);
            }
        }
    }
    Ok(packed)
}

fn unpack_indices(packed: &[u8], palette_len: usize) -> WireResult<Vec<u16>> {
    let width = palette_bit_width(palette_len) as usize;
    let bit_len = cell_count()
        .checked_mul(width)
        .ok_or(WorldWireError::LengthOverflow {
            segment: WireSegment::SnapshotPayload,
        })?;
    let byte_len = bit_len.div_ceil(8);
    if packed.len() != byte_len {
        return Err(WorldWireError::NonCanonicalPayload {
            reason: "packed section length does not match palette bit width".to_owned(),
        });
    }
    let mut indices = vec![0_u16; cell_count()];
    for (cell, index) in indices.iter_mut().enumerate() {
        let bit_offset = cell * width;
        let mut value = 0_u16;
        for bit in 0..width {
            let position = bit_offset + bit;
            if packed[position / 8] & (1 << (position % 8)) != 0 {
                value |= 1_u16 << bit;
            }
        }
        if usize::from(value) >= palette_len {
            return Err(WorldWireError::NonCanonicalPayload {
                reason: "decoded palette index exceeds palette length".to_owned(),
            });
        }
        *index = value;
    }
    Ok(indices)
}

fn canonicalize_solid(
    mut palette: Vec<SolidPaletteWireEntryV1>,
    indices: Vec<u16>,
) -> WireResult<(Vec<SolidPaletteWireEntryV1>, Vec<u16>)> {
    if palette.is_empty() {
        return Err(WorldWireError::NonCanonicalPayload {
            reason: "solid palette must contain at least one entry".to_owned(),
        });
    }
    let order = sort_key_order(&mut palette, |entry| {
        (
            entry.block.clone(),
            entry.state_schema.clone(),
            entry.state_schema_version,
            entry.canonical_state_bytes.clone(),
        )
    })?;
    Ok((palette, remap_indices(indices, &order)?))
}

fn canonicalize_fluid(
    palette: Vec<FluidPaletteWireEntryV1>,
    indices: Vec<u16>,
) -> WireResult<(Vec<FluidPaletteWireEntryV1>, Vec<u16>)> {
    if palette.is_empty() || !palette[0].is_empty() {
        return Err(WorldWireError::NonCanonicalPayload {
            reason: "fluid palette index 0 must be Empty".to_owned(),
        });
    }
    if palette
        .iter()
        .skip(1)
        .any(FluidPaletteWireEntryV1::is_empty)
    {
        return Err(WorldWireError::NonCanonicalPayload {
            reason: "Empty fluid may appear only at index zero".to_owned(),
        });
    }
    let empty = palette[0].clone();
    let mut rest = palette.into_iter().skip(1).collect::<Vec<_>>();
    let rest_remap = sort_key_order(&mut rest, |entry| {
        (
            entry.fluid.clone(),
            entry.state_schema.clone(),
            u32::from(entry.level),
            u32::from(entry.flow_tag),
        )
    })?;
    let mut remap = vec![0_u16; rest_remap.len().saturating_add(1)];
    remap[0] = 0;
    for (old_rest, new_rest) in rest_remap.iter().enumerate() {
        let old = old_rest.saturating_add(1);
        let new = new_rest
            .checked_add(1)
            .ok_or(WorldWireError::LengthOverflow {
                segment: WireSegment::SnapshotPayload,
            })?;
        if old >= remap.len() {
            return Err(WorldWireError::NonCanonicalPayload {
                reason: "fluid palette remap lost an entry".to_owned(),
            });
        }
        remap[old] = new;
    }
    rest.insert(0, empty);
    Ok((rest, remap_indices(indices, &remap)?))
}

fn sort_key_order<T, K: Ord>(rows: &mut [T], key: impl Fn(&T) -> K) -> WireResult<Vec<u16>> {
    let mut keyed = rows
        .iter()
        .enumerate()
        .map(|(index, row)| {
            u16::try_from(index)
                .map(|old| (key(row), old))
                .map_err(|_| WorldWireError::LengthOverflow {
                    segment: WireSegment::SnapshotPayload,
                })
        })
        .collect::<WireResult<Vec<_>>>()?;
    keyed.sort_by(|left, right| left.0.cmp(&right.0));
    for pair in keyed.windows(2) {
        if pair[0].0 == pair[1].0 {
            return Err(WorldWireError::NonCanonicalPayload {
                reason: "duplicate palette entry".to_owned(),
            });
        }
    }
    let mut remap = vec![0_u16; keyed.len()];
    for (new, (_, old)) in keyed.iter().enumerate() {
        remap[usize::from(*old)] =
            u16::try_from(new).map_err(|_| WorldWireError::LengthOverflow {
                segment: WireSegment::SnapshotPayload,
            })?;
    }
    rows.sort_by_cached_key(key);
    Ok(remap)
}

fn remap_indices(indices: Vec<u16>, old_to_new: &[u16]) -> WireResult<Vec<u16>> {
    if indices.len() != cell_count() {
        return Err(WorldWireError::NonCanonicalPayload {
            reason: "index buffer is not a 32³ volume".to_owned(),
        });
    }
    indices
        .into_iter()
        .map(|old| {
            old_to_new
                .get(usize::from(old))
                .copied()
                .ok_or(WorldWireError::NonCanonicalPayload {
                    reason: "palette index exceeds palette length".to_owned(),
                })
        })
        .collect()
}

fn validate_snapshot(value: &SolidFluidPaletteSnapshotV1) -> WireResult<()> {
    if value.chunk_edge != CHUNK_EDGE_V1 {
        return Err(WorldWireError::NonCanonicalPayload {
            reason: "chunk edge must be 32".to_owned(),
        });
    }
    if value.chunk_revision.get() > value.captured_world_revision.get() {
        return Err(WorldWireError::NonCanonicalPayload {
            reason: "chunk revision exceeds its captured world revision".to_owned(),
        });
    }
    if value.voxel_revision.get() > value.chunk_revision.get() {
        return Err(WorldWireError::NonCanonicalPayload {
            reason: "voxel revision exceeds the complete chunk revision".to_owned(),
        });
    }
    if value.fluid_state_schema != FLUID_STATE_SCHEMA_V1 {
        return Err(WorldWireError::UnknownSchema {
            found: SchemaId::from_str(&value.fluid_state_schema).map_err(|error| {
                WorldWireError::InvalidSchemaId {
                    reason: error.to_string(),
                }
            })?,
        });
    }
    if value.fluid_state_schema_version != 1 {
        return Err(WorldWireError::UnsupportedSchemaVersion {
            found: value.fluid_state_schema_version,
            expected: 1,
        });
    }
    check_identifier_bytes(value.key.dimension.as_str().len())?;
    check_identifier_bytes(value.fluid_state_schema.len())?;
    validate_solid_palette(&value.solid_palette)?;
    validate_fluid_palette(&value.fluid_palette)?;
    let solid_indices = unpack_indices(&value.packed_solid, value.solid_palette.len())?;
    let fluid_indices = if value.packed_fluid.is_empty() {
        vec![0; cell_count()]
    } else {
        unpack_indices(&value.packed_fluid, value.fluid_palette.len())?
    };
    if value.packed_fluid.is_empty() && fluid_indices.iter().any(|index| *index != 0) {
        return Err(WorldWireError::NonCanonicalPayload {
            reason: "missing fluid layer is reserved for all-empty volumes".to_owned(),
        });
    }
    if !value.packed_fluid.is_empty() && fluid_indices.iter().all(|index| *index == 0) {
        return Err(WorldWireError::NonCanonicalPayload {
            reason: "all-empty fluid must omit the packed fluid section".to_owned(),
        });
    }
    let _ = solid_indices;
    Ok(())
}

fn validate_solid_palette(palette: &[SolidPaletteWireEntryV1]) -> WireResult<()> {
    if palette.is_empty() {
        return Err(WorldWireError::NonCanonicalPayload {
            reason: "solid palette must contain at least one entry".to_owned(),
        });
    }
    let mut previous: Option<(&str, &str, u32, &[u8])> = None;
    for entry in palette {
        check_identifier_bytes(entry.block.len())?;
        check_identifier_bytes(entry.state_schema.len())?;
        let block =
            StableId::from_str(&entry.block).map_err(|error| WorldWireError::MalformedPayload {
                reason: format!("invalid solid palette block: {error}"),
            })?;
        if block.kind() != "block" || block.major().is_some() {
            return Err(WorldWireError::NonCanonicalPayload {
                reason: "solid palette block must be an exact block identity".to_owned(),
            });
        }
        let schema = SchemaId::from_str(&entry.state_schema).map_err(|error| {
            WorldWireError::InvalidSchemaId {
                reason: error.to_string(),
            }
        })?;
        let _ = schema;
        if entry.state_schema_version == 0 {
            return Err(WorldWireError::InvalidSchemaVersion);
        }
        let key = (
            entry.block.as_str(),
            entry.state_schema.as_str(),
            entry.state_schema_version,
            entry.canonical_state_bytes.as_slice(),
        );
        if previous.is_some_and(|previous| previous >= key) {
            return Err(WorldWireError::NonCanonicalPayload {
                reason: "solid palette is not strictly ordered".to_owned(),
            });
        }
        previous = Some(key);
    }
    Ok(())
}

fn validate_fluid_palette(palette: &[FluidPaletteWireEntryV1]) -> WireResult<()> {
    if palette.is_empty() || !palette[0].is_empty() {
        return Err(WorldWireError::NonCanonicalPayload {
            reason: "fluid palette index 0 must be Empty".to_owned(),
        });
    }
    validate_empty_row(&palette[0])?;
    let mut previous: Option<(&str, u8, u16)> = None;
    for entry in palette.iter().skip(1) {
        require_numeric_tag("fluid.kind", entry.kind_tag, &[FLUID_KIND_FLUID])?;
        require_numeric_tag(
            "fluid.flow",
            entry.flow_tag,
            &[
                FLOW_STILL, FLOW_DOWN, FLOW_EAST, FLOW_WEST, FLOW_SOUTH, FLOW_NORTH,
            ],
        )?;
        if entry.level > 7 {
            return Err(WorldWireError::NonCanonicalPayload {
                reason: "fluid level is outside 0..=7".to_owned(),
            });
        }
        check_identifier_bytes(entry.fluid.len())?;
        check_identifier_bytes(entry.state_schema.len())?;
        let fluid =
            StableId::from_str(&entry.fluid).map_err(|error| WorldWireError::MalformedPayload {
                reason: format!("invalid fluid palette identity: {error}"),
            })?;
        if fluid.kind() != "fluid" || fluid.major().is_some() {
            return Err(WorldWireError::NonCanonicalPayload {
                reason: "fluid palette identity must be an exact fluid identity".to_owned(),
            });
        }
        if entry.state_schema != FLUID_STATE_SCHEMA_V1 {
            return Err(WorldWireError::UnknownSchema {
                found: SchemaId::from_str(&entry.state_schema).map_err(|error| {
                    WorldWireError::InvalidSchemaId {
                        reason: error.to_string(),
                    }
                })?,
            });
        }
        if entry.state_schema_version != 1 {
            return Err(WorldWireError::UnsupportedSchemaVersion {
                found: entry.state_schema_version,
                expected: 1,
            });
        }
        let key = (entry.fluid.as_str(), entry.level, entry.flow_tag);
        if previous.is_some_and(|previous| previous >= key) {
            return Err(WorldWireError::NonCanonicalPayload {
                reason: "fluid palette is not strictly ordered".to_owned(),
            });
        }
        previous = Some(key);
    }
    Ok(())
}

fn validate_empty_row(entry: &FluidPaletteWireEntryV1) -> WireResult<()> {
    require_numeric_tag("fluid.kind", entry.kind_tag, &[FLUID_KIND_EMPTY])?;
    if !entry.fluid.is_empty()
        || !entry.state_schema.is_empty()
        || entry.state_schema_version != 0
        || entry.level != 0
        || entry.flow_tag != FLOW_STILL
    {
        return Err(WorldWireError::NonCanonicalPayload {
            reason: "empty fluid row must not carry identity or state".to_owned(),
        });
    }
    Ok(())
}

fn check_identifier_bytes(actual: usize) -> WireResult<()> {
    let actual = u64::try_from(actual).map_err(|_| WorldWireError::LengthOverflow {
        segment: WireSegment::SchemaId,
    })?;
    if actual > MAX_IDENTIFIER_BYTES {
        Err(WorldWireError::CodecBudgetExceeded {
            resource: SnapshotCodecResource::IdentifierBytes,
            actual,
            maximum: MAX_IDENTIFIER_BYTES,
        })
    } else {
        Ok(())
    }
}

fn encoded_payload_length(value: &SolidFluidPaletteSnapshotV1) -> WireResult<usize> {
    let mut meter = LengthMeter::default();
    meter.text(&value.key.world.to_string())?;
    meter.text(value.key.dimension.as_str())?;
    meter.signed_i32(value.key.coordinate.x)?;
    meter.signed_i32(value.key.coordinate.y)?;
    meter.signed_i32(value.key.coordinate.z)?;
    meter.unsigned(value.captured_world_revision.get())?;
    meter.unsigned(value.chunk_revision.get())?;
    meter.unsigned(value.voxel_revision.get())?;
    meter.unsigned(u64::from(value.chunk_edge))?;
    meter.unsigned(u64::try_from(value.solid_palette.len()).unwrap_or(u64::MAX))?;
    for entry in &value.solid_palette {
        meter.text(&entry.block)?;
        meter.text(&entry.state_schema)?;
        meter.unsigned(u64::from(entry.state_schema_version))?;
        meter.bytes(&entry.canonical_state_bytes)?;
    }
    meter.unsigned(u64::try_from(value.fluid_palette.len()).unwrap_or(u64::MAX))?;
    for entry in &value.fluid_palette {
        meter.unsigned(u64::from(entry.kind_tag))?;
        meter.text(&entry.fluid)?;
        meter.text(&entry.state_schema)?;
        meter.unsigned(u64::from(entry.state_schema_version))?;
        meter.add(1)?;
        meter.unsigned(u64::from(entry.flow_tag))?;
    }
    meter.bytes(&value.packed_solid)?;
    meter.bytes(&value.packed_fluid)?;
    meter.text(&value.fluid_state_schema)?;
    meter.unsigned(u64::from(value.fluid_state_schema_version))?;
    Ok(meter.length)
}

fn preflight_payload(payload: &[u8], limits: SnapshotCodecLimits) -> WireResult<()> {
    limits.check_payload_bytes(payload.len())?;
    limits.check_nesting_depth(u32::from(MAX_NESTING_DEPTH))?;
    let mut cursor = PayloadCursor::new(payload);
    scan_world_id(&mut cursor)?;
    scan_dimension_id(&mut cursor)?;
    cursor.read_i32("chunk x")?;
    cursor.read_i32("chunk y")?;
    cursor.read_i32("chunk z")?;
    cursor.read_u64("captured world revision")?;
    cursor.read_u64("chunk revision")?;
    cursor.read_u64("voxel revision")?;
    cursor.read_u16("chunk edge")?;
    let solid_count = cursor.read_length("solid palette")?;
    limits.check_collection_elements(u64::try_from(solid_count).unwrap_or(u64::MAX))?;
    for _ in 0..solid_count {
        cursor.read_identifier_text("solid block")?;
        cursor.read_identifier_text("solid state schema")?;
        cursor.read_u32("solid state schema version")?;
        let bytes = cursor.read_length("solid state bytes")?;
        cursor.read_exact(bytes, "solid state bytes")?;
    }
    let fluid_count = cursor.read_length("fluid palette")?;
    let total =
        solid_count
            .checked_add(fluid_count)
            .ok_or(WorldWireError::CodecBudgetExceeded {
                resource: SnapshotCodecResource::CollectionElements,
                actual: u64::MAX,
                maximum: u64::from(limits.max_collection_elements()),
            })?;
    limits.check_collection_elements(u64::try_from(total).unwrap_or(u64::MAX))?;
    for _ in 0..fluid_count {
        cursor.read_u16("fluid kind")?;
        cursor.read_identifier_text("fluid identity")?;
        cursor.read_identifier_text("fluid state schema")?;
        cursor.read_u32("fluid state schema version")?;
        cursor.read_exact(1, "fluid level")?;
        cursor.read_u16("fluid flow")?;
    }
    let packed_solid = cursor.read_length("packed solid")?;
    cursor.read_exact(packed_solid, "packed solid")?;
    let packed_fluid = cursor.read_length("packed fluid")?;
    cursor.read_exact(packed_fluid, "packed fluid")?;
    cursor.read_identifier_text("fluid state schema")?;
    cursor.read_u32("fluid state schema version")?;
    if cursor.remaining() != 0 {
        return Err(WorldWireError::TrailingPayloadBytes {
            remaining: cursor.remaining(),
        });
    }
    Ok(())
}

fn scan_world_id(cursor: &mut PayloadCursor<'_>) -> WireResult<()> {
    let text = cursor.read_identifier_text("chunk world ID")?;
    WorldId::from_str(text).map_err(|error| WorldWireError::InvalidWorldId {
        reason: error.to_string(),
    })?;
    Ok(())
}

fn scan_dimension_id(cursor: &mut PayloadCursor<'_>) -> WireResult<()> {
    let text = cursor.read_identifier_text("chunk dimension ID")?;
    DimensionId::from_str(text).map_err(|error| WorldWireError::InvalidDimensionId {
        reason: error.to_string(),
    })?;
    Ok(())
}

struct PayloadCursor<'a> {
    bytes: &'a [u8],
    position: usize,
}

impl<'a> PayloadCursor<'a> {
    const fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, position: 0 }
    }

    fn read_exact(&mut self, length: usize, section: &'static str) -> WireResult<&'a [u8]> {
        let end =
            self.position
                .checked_add(length)
                .ok_or_else(|| WorldWireError::MalformedPayload {
                    reason: format!("{section} length overflow"),
                })?;
        let Some(value) = self.bytes.get(self.position..end) else {
            return Err(WorldWireError::MalformedPayload {
                reason: format!("truncated {section}"),
            });
        };
        self.position = end;
        Ok(value)
    }

    fn read_varint(&mut self, section: &'static str, maximum: u64) -> WireResult<u64> {
        let mut value = 0_u64;
        for index in 0..10_u32 {
            let byte = self.read_exact(1, section)?[0];
            let low = u64::from(byte & 0x7f);
            if index == 9 && low > 1 {
                return Err(WorldWireError::MalformedPayload {
                    reason: format!("{section} varint overflows u64"),
                });
            }
            value |= low << (index * 7);
            if byte & 0x80 == 0 {
                if index > 0 && low == 0 {
                    return Err(WorldWireError::NonCanonicalPayload {
                        reason: format!("{section} uses a redundant varint byte"),
                    });
                }
                if value > maximum {
                    return Err(WorldWireError::MalformedPayload {
                        reason: format!("{section} exceeds its fixed integer width"),
                    });
                }
                return Ok(value);
            }
        }
        Err(WorldWireError::MalformedPayload {
            reason: format!("unterminated {section} varint"),
        })
    }

    fn read_u16(&mut self, section: &'static str) -> WireResult<u16> {
        let value = self.read_varint(section, u64::from(u16::MAX))?;
        u16::try_from(value).map_err(|_| WorldWireError::MalformedPayload {
            reason: format!("{section} exceeds u16"),
        })
    }

    fn read_u32(&mut self, section: &'static str) -> WireResult<u32> {
        let value = self.read_varint(section, u64::from(u32::MAX))?;
        u32::try_from(value).map_err(|_| WorldWireError::MalformedPayload {
            reason: format!("{section} exceeds u32"),
        })
    }

    fn read_u64(&mut self, section: &'static str) -> WireResult<u64> {
        self.read_varint(section, u64::MAX)
    }

    fn read_i32(&mut self, section: &'static str) -> WireResult<()> {
        self.read_varint(section, u64::from(u32::MAX))?;
        Ok(())
    }

    fn read_length(&mut self, section: &'static str) -> WireResult<usize> {
        let length = self.read_u64(section)?;
        usize::try_from(length).map_err(|_| WorldWireError::LengthOverflow {
            segment: WireSegment::SnapshotPayload,
        })
    }

    fn read_identifier_text(&mut self, section: &'static str) -> WireResult<&'a str> {
        let length = self.read_u64(section)?;
        if length > MAX_IDENTIFIER_BYTES {
            return Err(WorldWireError::CodecBudgetExceeded {
                resource: SnapshotCodecResource::IdentifierBytes,
                actual: length,
                maximum: MAX_IDENTIFIER_BYTES,
            });
        }
        let length = usize::try_from(length).map_err(|_| WorldWireError::LengthOverflow {
            segment: WireSegment::SchemaId,
        })?;
        let bytes = self.read_exact(length, section)?;
        std::str::from_utf8(bytes).map_err(|_| WorldWireError::MalformedPayload {
            reason: format!("{section} is not UTF-8"),
        })
    }

    const fn remaining(&self) -> usize {
        self.bytes.len() - self.position
    }
}

#[derive(Default)]
struct LengthMeter {
    length: usize,
}

impl LengthMeter {
    fn add(&mut self, additional: usize) -> WireResult<()> {
        self.length =
            self.length
                .checked_add(additional)
                .ok_or(WorldWireError::LengthOverflow {
                    segment: WireSegment::SnapshotPayload,
                })?;
        Ok(())
    }

    fn unsigned(&mut self, value: u64) -> WireResult<()> {
        self.add(varint_length(value))
    }

    fn signed_i32(&mut self, value: i32) -> WireResult<()> {
        let sign_mask = u32::from(value < 0).wrapping_neg();
        let zigzag = value.cast_unsigned().wrapping_shl(1) ^ sign_mask;
        self.unsigned(u64::from(zigzag))
    }

    fn text(&mut self, value: &str) -> WireResult<()> {
        self.unsigned(
            u64::try_from(value.len()).map_err(|_| WorldWireError::LengthOverflow {
                segment: WireSegment::SchemaId,
            })?,
        )?;
        self.add(value.len())
    }

    fn bytes(&mut self, value: &[u8]) -> WireResult<()> {
        self.unsigned(
            u64::try_from(value.len()).map_err(|_| WorldWireError::LengthOverflow {
                segment: WireSegment::SnapshotPayload,
            })?,
        )?;
        self.add(value.len())
    }
}

const fn varint_length(mut value: u64) -> usize {
    let mut length = 1;
    while value >= 0x80 {
        value >>= 7;
        length += 1;
    }
    length
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::encode_snapshot;
    use latticeaxiom_storage::{ChunkCoordinate, VersionedPayload};
    use std::error::Error;

    fn key(x: i32, y: i32, z: i32) -> Result<ChunkKey, Box<dyn Error>> {
        Ok(ChunkKey::new(
            WorldId::from_str("018f1e2d-3c4b-4a59-8c6d-7e8f9012abcd")?,
            DimensionId::from_str("fixture:dimension/playable")?,
            ChunkCoordinate::new(x, y, z),
        ))
    }

    fn air() -> SolidPaletteWireEntryV1 {
        SolidPaletteWireEntryV1::new(
            "fixture:block/air",
            "latticeaxiom:schema/block-state@1",
            1,
            br#"{"values":{}}"#.to_vec(),
        )
    }

    fn stone() -> SolidPaletteWireEntryV1 {
        SolidPaletteWireEntryV1::new(
            "fixture:block/stone",
            "latticeaxiom:schema/block-state@1",
            1,
            br#"{"values":{}}"#.to_vec(),
        )
    }

    fn water(level: u8, flow: u16) -> FluidPaletteWireEntryV1 {
        FluidPaletteWireEntryV1::fluid("fixture:fluid/alpha", FLUID_STATE_SCHEMA_V1, 1, level, flow)
    }

    fn snapshot(
        x: i32,
        y: i32,
        z: i32,
        boundary: bool,
    ) -> Result<SolidFluidPaletteSnapshotV1, Box<dyn Error>> {
        let mut solid_indices = vec![0_u16; cell_count()];
        let mut fluid_indices = vec![0_u16; cell_count()];
        let stone_index = linear_index(0, 4, 31);
        solid_indices[stone_index] = 1;
        if boundary {
            fluid_indices[linear_index(31, 0, 0)] = 1;
        } else {
            fluid_indices[linear_index(1, 0, 0)] = 1;
        }
        Ok(SolidFluidPaletteSnapshotV1::from_layers(
            key(x, y, z)?,
            WorldRevision::new(9),
            ChunkRevision::new(4),
            VoxelRevision::new(4),
            vec![air(), stone()],
            vec![FluidPaletteWireEntryV1::empty(), water(0, FLOW_EAST)],
            solid_indices,
            fluid_indices,
        )?)
    }

    #[test]
    fn positive_and_negative_chunks_round_trip_and_canonicalize_palette_order()
    -> Result<(), Box<dyn Error>> {
        let positive = snapshot(3, 2, 1, true)?;
        let negative = snapshot(-4, -8, -2, true)?;
        let positive_bytes =
            encode_solid_fluid_palette_v1(&positive, WorldWireLimits::default())?.into_envelope();
        let negative_bytes =
            encode_solid_fluid_palette_v1(&negative, WorldWireLimits::default())?.into_envelope();
        let decoded_positive =
            decode_solid_fluid_palette_v1(&positive_bytes, WorldWireLimits::default())?;
        let decoded_negative =
            decode_solid_fluid_palette_v1(&negative_bytes, WorldWireLimits::default())?;
        assert_eq!(
            decoded_positive.key().coordinate,
            ChunkCoordinate::new(3, 2, 1)
        );
        assert_eq!(
            decoded_negative.key().coordinate,
            ChunkCoordinate::new(-4, -8, -2)
        );
        assert!(decoded_positive.fluid_palette()[0].is_empty());
        assert_eq!(
            decoded_positive.solid_palette()[0].block(),
            "fixture:block/air"
        );
        assert_eq!(
            decoded_positive.unpacked_solid_indices()?,
            decoded_negative.unpacked_solid_indices()?
        );
        assert_eq!(
            decoded_positive.unpacked_fluid_indices()?[linear_index(31, 0, 0)],
            1
        );
        assert_ne!(positive_bytes, negative_bytes);
        Ok(())
    }

    #[test]
    fn unknown_schema_is_not_writable_and_enters_read_only_recovery() -> Result<(), Box<dyn Error>>
    {
        let known = SchemaId::from_str(SOLID_FLUID_PALETTE_SCHEMA_ID_V1)?;
        let unknown = SchemaId::from_str("other:schema/solid-fluid-palette@1")?;
        assert_eq!(
            classify_fluid_palette_open(&known),
            FluidPaletteOpenDispositionV1::Writable
        );
        assert_eq!(
            classify_fluid_palette_open(&unknown),
            FluidPaletteOpenDispositionV1::ReadOnlyRecovery
        );
        let codec = SolidFluidPaletteSnapshotCodecV1::new()?;
        let encoded = encode_snapshot(
            codec.owner(),
            &VersionedPayload::new(unknown.clone(), codec.version(), vec![1, 2, 3]),
            WorldWireLimits::default(),
        )?;
        assert!(matches!(
            decode_solid_fluid_palette_v1(&encoded, WorldWireLimits::default()),
            Err(WorldWireError::UnknownSchema { found }) if found == unknown
        ));
        Ok(())
    }

    #[test]
    fn unknown_fluid_state_schema_fails_closed_before_writer_publish() {
        let mut value = snapshot(0, 0, 0, false).unwrap_or_else(|error| panic!("{error}"));
        value.fluid_state_schema = "other:schema/fluid-state@1".to_owned();
        assert!(matches!(
            encode_solid_fluid_palette_v1(&value, WorldWireLimits::default()),
            Err(WorldWireError::UnknownSchema { .. })
        ));
    }
}
