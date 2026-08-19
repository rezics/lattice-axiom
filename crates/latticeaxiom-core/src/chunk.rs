//! Fixed-size chunks and canonical palette-backed voxel storage.

use thiserror::Error;

use crate::{BlockId, space::BlockPos};

/// Fixed chunk edge length in blocks.
pub const CHUNK_EDGE: i32 = 32;
/// Number of block cells stored by one chunk.
pub const CHUNK_VOLUME: usize = 32 * 32 * 32;
const CHUNK_EDGE_U8: u8 = 32;
const CHUNK_EDGE_USIZE: usize = 32;

/// Integer position of a chunk in the infinite chunk lattice.
///
/// Coordinates follow the world `(x, y, z)` order and use Euclidean splitting,
/// so the block immediately west of the origin belongs to chunk `x = -1`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ChunkPos {
    /// East-west chunk coordinate.
    pub x: i32,
    /// North-south chunk coordinate.
    pub y: i32,
    /// Vertical chunk coordinate.
    pub z: i32,
}

impl ChunkPos {
    /// Creates a chunk position from its components.
    #[must_use]
    pub const fn new(x: i32, y: i32, z: i32) -> Self {
        Self { x, y, z }
    }

    /// Returns the chunk origin in world block coordinates when representable.
    #[must_use]
    pub fn origin(self) -> Option<BlockPos> {
        Some(BlockPos::new(
            self.x.checked_mul(CHUNK_EDGE)?,
            self.y.checked_mul(CHUNK_EDGE)?,
            self.z.checked_mul(CHUNK_EDGE)?,
        ))
    }
}

/// A validated block coordinate local to a chunk.
///
/// Each component is in `0..CHUNK_EDGE`. The compact `u8` representation is
/// never exposed as a persistence format; use the component accessors.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct LocalBlockPos {
    x: u8,
    y: u8,
    z: u8,
}

impl LocalBlockPos {
    /// Validates and creates a chunk-local block position.
    ///
    /// # Errors
    ///
    /// Returns [`LocalBlockPosError`] if any component is outside the fixed
    /// chunk edge.
    pub const fn new(x: u8, y: u8, z: u8) -> Result<Self, LocalBlockPosError> {
        if x >= CHUNK_EDGE_U8 || y >= CHUNK_EDGE_U8 || z >= CHUNK_EDGE_U8 {
            Err(LocalBlockPosError { x, y, z })
        } else {
            Ok(Self { x, y, z })
        }
    }

    /// East-west local component.
    #[must_use]
    pub const fn x(self) -> u8 {
        self.x
    }

    /// North-south local component.
    #[must_use]
    pub const fn y(self) -> u8 {
        self.y
    }

    /// Vertical local component.
    #[must_use]
    pub const fn z(self) -> u8 {
        self.z
    }

    /// Returns the x-fastest linear storage index.
    #[must_use]
    pub fn linear_index(self) -> usize {
        let edge = CHUNK_EDGE_USIZE;
        usize::from(self.x) + edge * (usize::from(self.y) + edge * usize::from(self.z))
    }

    /// Converts an x-fastest linear storage index into a local position.
    #[must_use]
    #[allow(
        clippy::cast_possible_truncation,
        reason = "components are remainders modulo the 32-block chunk edge"
    )]
    pub fn from_linear_index(index: usize) -> Option<Self> {
        if index >= CHUNK_VOLUME {
            return None;
        }
        let edge = 32_usize;
        let x = (index % edge) as u8;
        let y = ((index / edge) % edge) as u8;
        let z = (index / (edge * edge)) as u8;
        Some(Self { x, y, z })
    }

    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "Euclidean remainders are proven to be in 0..CHUNK_EDGE"
    )]
    const fn from_remainders(x: i32, y: i32, z: i32) -> Self {
        debug_assert!(x >= 0 && x < CHUNK_EDGE);
        debug_assert!(y >= 0 && y < CHUNK_EDGE);
        debug_assert!(z >= 0 && z < CHUNK_EDGE);
        Self {
            x: x as u8,
            y: y as u8,
            z: z as u8,
        }
    }
}

/// Error returned for a chunk-local coordinate outside the fixed edge.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Error)]
#[error("local block coordinates must each be within 0..{CHUNK_EDGE}, got ({x}, {y}, {z})")]
pub struct LocalBlockPosError {
    x: u8,
    y: u8,
    z: u8,
}

/// A world block split into its chunk and chunk-local coordinates.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ChunkAddress {
    /// Chunk containing the block.
    pub chunk: ChunkPos,
    /// Coordinate inside [`Self::chunk`].
    pub local: LocalBlockPos,
}

impl ChunkAddress {
    /// Splits a world block with Euclidean division on every axis.
    #[must_use]
    pub const fn from_block(block: BlockPos) -> Self {
        let chunk = ChunkPos::new(
            block.x.div_euclid(CHUNK_EDGE),
            block.y.div_euclid(CHUNK_EDGE),
            block.z.div_euclid(CHUNK_EDGE),
        );
        let local = LocalBlockPos::from_remainders(
            block.x.rem_euclid(CHUNK_EDGE),
            block.y.rem_euclid(CHUNK_EDGE),
            block.z.rem_euclid(CHUNK_EDGE),
        );
        Self { chunk, local }
    }

    /// Reconstructs the world block when all components fit in `i32`.
    #[must_use]
    pub fn to_block(self) -> Option<BlockPos> {
        Some(BlockPos::new(
            self.chunk
                .x
                .checked_mul(CHUNK_EDGE)?
                .checked_add(i32::from(self.local.x))?,
            self.chunk
                .y
                .checked_mul(CHUNK_EDGE)?
                .checked_add(i32::from(self.local.y))?,
            self.chunk
                .z
                .checked_mul(CHUNK_EDGE)?
                .checked_add(i32::from(self.local.z))?,
        ))
    }
}

impl From<BlockPos> for ChunkAddress {
    fn from(value: BlockPos) -> Self {
        Self::from_block(value)
    }
}

/// Failure to reconstruct a chunk from persisted palette data.
#[derive(Clone, Debug, PartialEq, Eq, Error)]
pub enum ChunkDataError {
    /// Palette is missing the reserved air entry.
    #[error("chunk palette must start with BlockId::AIR")]
    MissingAir,
    /// Palette values are not strictly increasing.
    #[error("chunk palette must contain unique BlockId values in ascending order")]
    NonCanonicalPalette,
    /// Index buffer has the wrong fixed length.
    #[error("chunk index buffer has length {actual}; expected {CHUNK_VOLUME}")]
    WrongIndexCount {
        /// Actual number of palette indices supplied.
        actual: usize,
    },
    /// An index points outside the supplied palette.
    #[error("palette index {index} at block offset {offset} exceeds palette length {palette_len}")]
    InvalidPaletteIndex {
        /// Linear block offset containing the bad index.
        offset: usize,
        /// Invalid palette index.
        index: u16,
        /// Number of palette entries available.
        palette_len: usize,
    },
    /// A non-air palette entry is not referenced by the index buffer.
    #[error("non-air palette entry {palette_index} is unused")]
    UnusedPaletteEntry {
        /// Unused index within the palette.
        palette_index: usize,
    },
}

/// One fixed-size chunk of canonical palette-backed block data.
///
/// The palette is always sorted by [`BlockId`], begins with [`BlockId::AIR`],
/// and contains no unused non-air entries. Therefore logically equal chunks
/// have equal palette/index persistence views regardless of edit order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Chunk {
    position: ChunkPos,
    palette: Vec<BlockId>,
    indices: Vec<u16>,
}

impl Chunk {
    /// Creates an all-air chunk at `position`.
    #[must_use]
    pub fn empty(position: ChunkPos) -> Self {
        Self {
            position,
            palette: vec![BlockId::AIR],
            indices: vec![0; CHUNK_VOLUME],
        }
    }

    /// Reconstructs a chunk from its canonical persistence views.
    ///
    /// # Errors
    ///
    /// Returns [`ChunkDataError`] when the palette or index buffer violates
    /// any documented chunk invariant.
    pub fn from_palette_indices(
        position: ChunkPos,
        palette: Vec<BlockId>,
        indices: Vec<u16>,
    ) -> Result<Self, ChunkDataError> {
        if palette.first() != Some(&BlockId::AIR) {
            return Err(ChunkDataError::MissingAir);
        }
        if palette.windows(2).any(|pair| pair[0] >= pair[1]) {
            return Err(ChunkDataError::NonCanonicalPalette);
        }
        if indices.len() != CHUNK_VOLUME {
            return Err(ChunkDataError::WrongIndexCount {
                actual: indices.len(),
            });
        }

        let mut used = vec![false; palette.len()];
        for (offset, &index) in indices.iter().enumerate() {
            let palette_index = usize::from(index);
            if palette_index >= palette.len() {
                return Err(ChunkDataError::InvalidPaletteIndex {
                    offset,
                    index,
                    palette_len: palette.len(),
                });
            }
            used[palette_index] = true;
        }
        if let Some(palette_index) = used
            .iter()
            .enumerate()
            .skip(1)
            .find_map(|(index, &is_used)| (!is_used).then_some(index))
        {
            return Err(ChunkDataError::UnusedPaletteEntry { palette_index });
        }

        Ok(Self {
            position,
            palette,
            indices,
        })
    }

    /// Chunk-lattice position of this data.
    #[must_use]
    pub const fn position(&self) -> ChunkPos {
        self.position
    }

    /// Canonical block palette for persistence or meshing snapshots.
    #[must_use]
    pub fn palette(&self) -> &[BlockId] {
        &self.palette
    }

    /// X-fastest palette index buffer for persistence or meshing snapshots.
    #[must_use]
    pub fn palette_indices(&self) -> &[u16] {
        &self.indices
    }

    /// Reads one local block in constant time.
    #[must_use]
    pub fn block(&self, local: LocalBlockPos) -> BlockId {
        let palette_index = usize::from(self.indices[local.linear_index()]);
        self.palette[palette_index]
    }

    /// Sets one local block and returns its previous identity.
    ///
    /// Palette insertion/removal keeps the persistence representation
    /// canonical. The per-block lookup remains constant time.
    #[allow(
        clippy::cast_possible_truncation,
        reason = "a 32^3 chunk cannot exhaust the u16 palette index space"
    )]
    pub fn set_block(&mut self, local: LocalBlockPos, block: BlockId) -> BlockId {
        let linear_index = local.linear_index();
        let old_block = self.block(local);
        if old_block == block {
            return old_block;
        }

        let new_palette_index = match self.palette.binary_search(&block) {
            Ok(index) => index as u16,
            Err(insertion_index) => {
                self.palette.insert(insertion_index, block);
                for index in &mut self.indices {
                    if usize::from(*index) >= insertion_index {
                        *index += 1;
                    }
                }
                insertion_index as u16
            }
        };

        let old_palette_index = self.indices[linear_index];
        self.indices[linear_index] = new_palette_index;

        if !old_block.is_air() && !self.indices.contains(&old_palette_index) {
            self.palette.remove(usize::from(old_palette_index));
            for index in &mut self.indices {
                if *index > old_palette_index {
                    *index -= 1;
                }
            }
        }

        old_block
    }

    /// Replaces every cell with `block` and discards unused palette entries.
    pub fn fill(&mut self, block: BlockId) {
        if block.is_air() {
            self.palette = vec![BlockId::AIR];
            self.indices.fill(0);
        } else {
            self.palette = vec![BlockId::AIR, block];
            self.indices.fill(1);
        }
    }

    /// Iterates all block identities in x-fastest storage order.
    #[must_use]
    pub fn blocks(&self) -> impl ExactSizeIterator<Item = BlockId> + '_ {
        self.indices
            .iter()
            .map(|&index| self.palette[usize::from(index)])
    }
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    fn local(x: u8, y: u8, z: u8) -> LocalBlockPos {
        LocalBlockPos::new(x, y, z).expect("test coordinates stay within the chunk")
    }

    #[test]
    fn negative_world_coordinates_split_euclideanly() {
        let address = ChunkAddress::from_block(BlockPos::new(-1, -32, -33));
        assert_eq!(address.chunk, ChunkPos::new(-1, -1, -2));
        assert_eq!(address.local, local(31, 0, 31));
        assert_eq!(address.to_block(), Some(BlockPos::new(-1, -32, -33)));
    }

    #[test]
    fn palette_is_canonical_independent_of_edit_order() {
        let stone = BlockId::from_raw(8);
        let grass = BlockId::from_raw(3);
        let a = local(1, 2, 3);
        let b = local(4, 5, 6);

        let mut first = Chunk::empty(ChunkPos::default());
        first.set_block(a, stone);
        first.set_block(b, grass);

        let mut second = Chunk::empty(ChunkPos::default());
        second.set_block(b, grass);
        second.set_block(a, stone);

        assert_eq!(first, second);
        assert_eq!(first.palette(), &[BlockId::AIR, grass, stone]);
    }

    #[test]
    fn replacing_last_use_removes_palette_entry() {
        let mut chunk = Chunk::empty(ChunkPos::default());
        let point = local(0, 0, 0);
        let stone = BlockId::from_raw(7);
        chunk.set_block(point, stone);
        assert_eq!(chunk.palette(), &[BlockId::AIR, stone]);
        assert_eq!(chunk.set_block(point, BlockId::AIR), stone);
        assert_eq!(chunk.palette(), &[BlockId::AIR]);
    }

    #[test]
    fn persisted_views_round_trip() {
        let mut chunk = Chunk::empty(ChunkPos::new(-2, 4, 1));
        chunk.set_block(local(31, 0, 9), BlockId::from_raw(19));
        let rebuilt = Chunk::from_palette_indices(
            chunk.position(),
            chunk.palette().to_vec(),
            chunk.palette_indices().to_vec(),
        )
        .expect("views emitted by Chunk satisfy its reconstruction contract");
        assert_eq!(rebuilt, chunk);
    }

    #[test]
    fn reconstruction_rejects_bad_index() {
        let result = Chunk::from_palette_indices(
            ChunkPos::default(),
            vec![BlockId::AIR],
            vec![1; CHUNK_VOLUME],
        );
        assert!(matches!(
            result,
            Err(ChunkDataError::InvalidPaletteIndex { offset: 0, .. })
        ));
    }

    proptest! {
        #[test]
        fn address_round_trips_every_world_block(x in any::<i32>(), y in any::<i32>(), z in any::<i32>()) {
            let block = BlockPos::new(x, y, z);
            let address = ChunkAddress::from_block(block);
            prop_assert_eq!(address.to_block(), Some(block));
            prop_assert!(i32::from(address.local.x()) < CHUNK_EDGE);
            prop_assert!(i32::from(address.local.y()) < CHUNK_EDGE);
            prop_assert!(i32::from(address.local.z()) < CHUNK_EDGE);
        }

        #[test]
        fn local_linearization_round_trips(index in 0usize..CHUNK_VOLUME) {
            let local = LocalBlockPos::from_linear_index(index)
                .expect("generated index is within the fixed chunk volume");
            prop_assert_eq!(local.linear_index(), index);
        }
    }
}
