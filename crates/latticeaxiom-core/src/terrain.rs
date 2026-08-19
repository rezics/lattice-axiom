//! Deterministic, inexpensive heightmap terrain for the first playable world.

use thiserror::Error;

use crate::{BlockId, CHUNK_EDGE, Chunk, ChunkPos, LocalBlockPos};

const NOISE_CELL_EDGE: i32 = 16;
const FADE_SCALE: i64 = 4_096;
const MAX_RELIEF: u16 = 1_024;

/// Block identities used for the three solid heightmap layers.
///
/// The core never assigns content identities. Callers resolve package keys to
/// stable [`BlockId`] values and pass the resulting layer set here.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TerrainBlocks {
    /// Topmost solid cell of a column.
    pub surface: BlockId,
    /// Shallow cells immediately below the surface.
    pub subsurface: BlockId,
    /// Remaining solid cells below the shallow layer.
    pub stone: BlockId,
}

impl TerrainBlocks {
    /// Creates a terrain layer identity set.
    #[must_use]
    pub const fn new(surface: BlockId, subsurface: BlockId, stone: BlockId) -> Self {
        Self {
            surface,
            subsurface,
            stone,
        }
    }
}

/// Configuration or generation failure for [`Heightmap`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Error)]
pub enum HeightmapError {
    /// Relief exceeds the deliberately small playable-demo bound.
    #[error("heightmap relief {relief} exceeds the supported maximum {MAX_RELIEF}")]
    ReliefTooLarge {
        /// Requested maximum deviation from the base height.
        relief: u16,
    },
    /// Base height plus or minus relief cannot fit in world coordinates.
    #[error("base height {base_height} plus/minus relief {relief} exceeds i32 world height")]
    HeightRangeOverflow {
        /// Requested center height.
        base_height: i32,
        /// Requested maximum deviation.
        relief: u16,
    },
    /// Chunk position cannot be represented in the `i32` world block domain.
    #[error("chunk position {position:?} lies outside representable world block coordinates")]
    ChunkOutsideWorld {
        /// Invalid chunk-lattice position.
        position: ChunkPos,
    },
}

/// Seeded smooth value-noise heightmap for the milestone-3 sandbox.
///
/// Sampling uses only specified integer arithmetic and wrapping hash
/// operations. Equal configuration and world coordinates therefore produce
/// identical heights without depending on registration, thread, or chunk
/// generation order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Heightmap {
    seed: u64,
    base_height: i32,
    relief: u16,
    subsurface_depth: u8,
}

impl Heightmap {
    /// Creates a deterministic heightmap.
    ///
    /// `relief` is the maximum deviation from `base_height`, and
    /// `subsurface_depth` is the number of cells below the surface that use
    /// [`TerrainBlocks::subsurface`].
    ///
    /// # Errors
    ///
    /// Returns [`HeightmapError`] when relief exceeds the demo bound or the
    /// resulting height range would overflow `i32`.
    pub fn new(
        seed: u64,
        base_height: i32,
        relief: u16,
        subsurface_depth: u8,
    ) -> Result<Self, HeightmapError> {
        if relief > MAX_RELIEF {
            return Err(HeightmapError::ReliefTooLarge { relief });
        }
        let relief_i32 = i32::from(relief);
        if base_height.checked_sub(relief_i32).is_none()
            || base_height.checked_add(relief_i32).is_none()
        {
            return Err(HeightmapError::HeightRangeOverflow {
                base_height,
                relief,
            });
        }
        Ok(Self {
            seed,
            base_height,
            relief,
            subsurface_depth,
        })
    }

    /// World seed used by the stable value-noise hash.
    #[must_use]
    pub const fn seed(self) -> u64 {
        self.seed
    }

    /// Center height around which relief varies.
    #[must_use]
    pub const fn base_height(self) -> i32 {
        self.base_height
    }

    /// Maximum height deviation from [`Self::base_height`].
    #[must_use]
    pub const fn relief(self) -> u16 {
        self.relief
    }

    /// Number of shallow subsurface cells below each column surface.
    #[must_use]
    pub const fn subsurface_depth(self) -> u8 {
        self.subsurface_depth
    }

    /// Returns the inclusive top solid block height at world column `(x, y)`.
    #[must_use]
    pub fn height_at(self, x: i32, y: i32) -> i32 {
        let grid_x = x.div_euclid(NOISE_CELL_EDGE);
        let grid_y = y.div_euclid(NOISE_CELL_EDGE);
        let local_x = x.rem_euclid(NOISE_CELL_EDGE);
        let local_y = y.rem_euclid(NOISE_CELL_EDGE);

        let southwest = self.lattice_sample(grid_x, grid_y);
        let southeast = self.lattice_sample(grid_x + 1, grid_y);
        let northwest = self.lattice_sample(grid_x, grid_y + 1);
        let northeast = self.lattice_sample(grid_x + 1, grid_y + 1);
        let weight_x = fade_weight(local_x);
        let weight_y = fade_weight(local_y);
        let south = lerp_fixed(southwest, southeast, weight_x);
        let north = lerp_fixed(northwest, northeast, weight_x);
        self.base_height + lerp_fixed(south, north, weight_y)
    }

    /// Generates one canonical chunk from the heightmap and caller-owned IDs.
    ///
    /// # Errors
    ///
    /// Returns [`HeightmapError::ChunkOutsideWorld`] when the chunk origin or
    /// its positive edge cannot fit in `i32` block coordinates.
    pub fn generate_chunk(
        self,
        position: ChunkPos,
        blocks: TerrainBlocks,
    ) -> Result<Chunk, HeightmapError> {
        let Some(origin) = position.origin() else {
            return Err(HeightmapError::ChunkOutsideWorld { position });
        };
        if origin.x.checked_add(CHUNK_EDGE - 1).is_none()
            || origin.y.checked_add(CHUNK_EDGE - 1).is_none()
            || origin.z.checked_add(CHUNK_EDGE - 1).is_none()
        {
            return Err(HeightmapError::ChunkOutsideWorld { position });
        }

        let mut chunk = Chunk::empty(position);
        for x in 0_u8..32 {
            for y in 0_u8..32 {
                let world_x = origin.x + i32::from(x);
                let world_y = origin.y + i32::from(y);
                let height = self.height_at(world_x, world_y);
                for z in 0_u8..32 {
                    let world_z = origin.z + i32::from(z);
                    let depth = height.saturating_sub(world_z);
                    let block = if world_z > height {
                        BlockId::AIR
                    } else if depth == 0 {
                        blocks.surface
                    } else if depth <= i32::from(self.subsurface_depth) {
                        blocks.subsurface
                    } else {
                        blocks.stone
                    };
                    if !block.is_air() {
                        let Ok(local) = LocalBlockPos::new(x, y, z) else {
                            unreachable!("0..32 loop coordinates fit a chunk");
                        };
                        chunk.set_block(local, block);
                    }
                }
            }
        }
        Ok(chunk)
    }

    fn lattice_sample(self, x: i32, y: i32) -> i32 {
        if self.relief == 0 {
            return 0;
        }
        let x_bits = u64::from(u32::from_be_bytes(x.to_be_bytes()));
        let y_bits = u64::from(u32::from_be_bytes(y.to_be_bytes()));
        let mixed = splitmix64(
            self.seed
                ^ x_bits.wrapping_mul(0x9e37_79b9_7f4a_7c15)
                ^ y_bits.wrapping_mul(0xbf58_476d_1ce4_e5b9),
        );
        let relief = i64::from(self.relief);
        let width = u64::try_from(relief * 2 + 1).unwrap_or(1);
        let sample = i64::try_from(mixed % width).unwrap_or(0) - relief;
        i32::try_from(sample).unwrap_or(0)
    }
}

fn splitmix64(mut value: u64) -> u64 {
    value = value.wrapping_add(0x9e37_79b9_7f4a_7c15);
    value = (value ^ (value >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    value = (value ^ (value >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    value ^ (value >> 31)
}

fn fade_weight(local: i32) -> i64 {
    let value = i64::from(local) * FADE_SCALE / i64::from(NOISE_CELL_EDGE);
    value * value * (3 * FADE_SCALE - 2 * value) / (FADE_SCALE * FADE_SCALE)
}

fn lerp_fixed(from: i32, to: i32, weight: i64) -> i32 {
    let from = i64::from(from);
    let to = i64::from(to);
    let interpolated = (from * (FADE_SCALE - weight) + to * weight) / FADE_SCALE;
    i32::try_from(interpolated).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    fn generator(seed: u64) -> Heightmap {
        Heightmap::new(seed, 12, 6, 3).expect("test configuration fits the world height range")
    }

    fn layers() -> TerrainBlocks {
        TerrainBlocks::new(
            BlockId::from_raw(3),
            BlockId::from_raw(2),
            BlockId::from_raw(1),
        )
    }

    #[test]
    fn same_seed_and_column_are_repeatable() {
        let terrain = generator(0xfeed_beef);
        assert_eq!(terrain.height_at(-37, 91), terrain.height_at(-37, 91));
        assert_eq!(
            terrain
                .generate_chunk(ChunkPos::new(-1, 0, 0), layers())
                .expect("near-origin chunk is representable"),
            terrain
                .generate_chunk(ChunkPos::new(-1, 0, 0), layers())
                .expect("near-origin chunk is representable")
        );
    }

    #[test]
    fn generated_column_uses_surface_subsurface_and_stone() {
        let terrain = Heightmap::new(7, 10, 0, 2)
            .expect("flat test configuration fits the world height range");
        let chunk = terrain
            .generate_chunk(ChunkPos::new(0, 0, 0), layers())
            .expect("origin chunk is representable");
        let at = |z| {
            chunk.block(LocalBlockPos::new(0, 0, z).expect("test z coordinate is within the chunk"))
        };
        assert_eq!(at(11), BlockId::AIR);
        assert_eq!(at(10), layers().surface);
        assert_eq!(at(9), layers().subsurface);
        assert_eq!(at(8), layers().subsurface);
        assert_eq!(at(7), layers().stone);
    }

    proptest! {
        #[test]
        fn height_stays_within_configured_relief(
            seed in any::<u64>(),
            x in -1_000_000i32..1_000_000,
            y in -1_000_000i32..1_000_000,
        ) {
            let terrain = generator(seed);
            let height = terrain.height_at(x, y);
            prop_assert!((6..=18).contains(&height));
        }

        #[test]
        fn generation_order_does_not_change_chunks(seed in any::<u64>()) {
            let terrain = generator(seed);
            let west_pos = ChunkPos::new(-1, 0, 0);
            let east_pos = ChunkPos::new(0, 0, 0);
            let west_first = terrain.generate_chunk(west_pos, layers())
                .expect("test chunk is representable");
            let east_second = terrain.generate_chunk(east_pos, layers())
                .expect("test chunk is representable");
            let east_first = terrain.generate_chunk(east_pos, layers())
                .expect("test chunk is representable");
            let west_second = terrain.generate_chunk(west_pos, layers())
                .expect("test chunk is representable");
            prop_assert_eq!(west_first, west_second);
            prop_assert_eq!(east_first, east_second);
        }
    }
}
