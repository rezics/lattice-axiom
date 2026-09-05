//! Versioned dual-layer solid/fluid palette volumes.
//!
//! Integrator wiring (`lib.rs`):
//! ```ignore
//! pub use fluid::solid_fluid_volume::{
//!     CHUNK_EDGE_V1, SOLID_FLUID_VOLUME_SCHEMA_V1, SolidFluidVolumeCellV1,
//!     SolidFluidVolumeV1, VolumeChunkCoordinateV1, volume_linear_index,
//! };
//! ```
//!
//! This module owns identity-plus-state palettes and local cell indices. Bit
//! packing, envelope encoding, and writer recovery remain world-wire concerns.

use latticeaxiom_core::{CanonicalHash, canonical_json_bytes};
use serde::Serialize;

use crate::{
    CompiledFluidPaletteV1, CompiledSolidPaletteV1, ContentCatalogV1, ContentError, ContentResult,
    FluidPaletteEntryV1, PaletteLimitsV1, SolidPaletteEntryV1,
};

/// Canonical schema identity for a dual-layer palette volume.
pub const SOLID_FLUID_VOLUME_SCHEMA_V1: &str = "latticeaxiom:schema/solid-fluid-volume@1";

/// Authoritative cubic chunk edge frozen by ADR 0027.
pub const CHUNK_EDGE_V1: u16 = 32;

/// Y-up chunk coordinate captured with a palette volume.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
pub struct VolumeChunkCoordinateV1 {
    /// Horizontal chunk coordinate increasing to world right.
    pub x: i32,
    /// Vertical chunk coordinate increasing upward.
    pub y: i32,
    /// Horizontal chunk coordinate on the world depth axis.
    pub z: i32,
}

impl VolumeChunkCoordinateV1 {
    /// Creates a coordinate in canonical `(x, y, z)` order.
    #[must_use]
    pub const fn new(x: i32, y: i32, z: i32) -> Self {
        Self { x, y, z }
    }
}

/// One locally addressed dual-layer cell accepted by volume compilation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SolidFluidVolumeCellV1 {
    /// Local voxel X.
    pub x: u16,
    /// Local voxel Y, the vertical axis.
    pub y: u16,
    /// Local voxel Z.
    pub z: u16,
    /// Solid occupancy for the cell.
    pub solid: SolidPaletteEntryV1,
    /// Orthogonal fluid layer. `Empty` is canonical absence.
    pub fluid: FluidPaletteEntryV1,
}

/// Versioned dense solid/fluid palette volume for one cubic chunk.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SolidFluidVolumeV1 {
    schema: &'static str,
    chunk_x: i32,
    chunk_y: i32,
    chunk_z: i32,
    edge: u16,
    solid_palette: CompiledSolidPaletteV1,
    fluid_palette: CompiledFluidPaletteV1,
    solid_indices: Vec<u16>,
    fluid_indices: Vec<u16>,
}

impl SolidFluidVolumeV1 {
    /// Compiles a dense dual-layer volume from sparse cells.
    ///
    /// Unspecified cells receive `empty_solid` and canonical empty fluid.
    /// Discovery order cannot change palette bytes: palettes sort by stable
    /// identity plus canonical state, and cells are stored in
    /// `x + edge * (z + edge * y)` order.
    ///
    /// # Errors
    ///
    /// Returns a catalog, palette, duplicate-cell, or range error.
    pub fn compile(
        catalog: &ContentCatalogV1,
        chunk: VolumeChunkCoordinateV1,
        empty_solid: SolidPaletteEntryV1,
        cells: Vec<SolidFluidVolumeCellV1>,
        palette_limits: PaletteLimitsV1,
    ) -> ContentResult<Self> {
        let edge = CHUNK_EDGE_V1;
        let cell_count = volume_cell_count(edge)?;
        let mut occupied = vec![false; cell_count];
        let mut solid_slots = vec![empty_solid.clone(); cell_count];
        let mut fluid_slots = vec![FluidPaletteEntryV1::Empty; cell_count];
        let mut solid_rows = vec![empty_solid];
        let mut fluid_rows = vec![FluidPaletteEntryV1::Empty];

        for cell in cells {
            let index = volume_linear_index(edge, cell.x, cell.y, cell.z)?;
            if occupied[index] {
                return Err(ContentError::InvalidSolidFluidVolume {
                    reason: "duplicate volume cell coordinate",
                });
            }
            occupied[index] = true;
            solid_slots[index] = cell.solid.clone();
            fluid_slots[index] = cell.fluid.clone();
            solid_rows.push(cell.solid);
            fluid_rows.push(cell.fluid);
        }

        let mut keyed_solids = Vec::with_capacity(solid_rows.len());
        for entry in solid_rows {
            let state_key = entry.state.canonical_bytes()?;
            keyed_solids.push((entry.block, state_key, entry.state));
        }
        keyed_solids.sort_by(|left, right| left.0.cmp(&right.0).then_with(|| left.1.cmp(&right.1)));
        keyed_solids.dedup_by(|left, right| left.0 == right.0 && left.1 == right.1);
        let solid_rows = keyed_solids
            .into_iter()
            .map(|(block, _, state)| SolidPaletteEntryV1 { block, state })
            .collect::<Vec<_>>();
        fluid_rows.sort_by(compare_fluid_entries);
        fluid_rows.dedup();

        let solid_palette = CompiledSolidPaletteV1::compile(catalog, solid_rows, palette_limits)?;
        let fluid_palette = CompiledFluidPaletteV1::compile(catalog, fluid_rows, palette_limits)?;
        let mut solid_indices = Vec::with_capacity(cell_count);
        let mut fluid_indices = Vec::with_capacity(cell_count);
        for index in 0..cell_count {
            solid_indices.push(solid_index(&solid_palette, &solid_slots[index])?);
            fluid_indices.push(fluid_index(&fluid_palette, &fluid_slots[index])?);
        }

        Ok(Self {
            schema: SOLID_FLUID_VOLUME_SCHEMA_V1,
            chunk_x: chunk.x,
            chunk_y: chunk.y,
            chunk_z: chunk.z,
            edge,
            solid_palette,
            fluid_palette,
            solid_indices,
            fluid_indices,
        })
    }

    /// Returns the frozen volume schema identity.
    #[must_use]
    pub const fn schema(&self) -> &'static str {
        self.schema
    }

    /// Returns the Y-up chunk coordinate captured with this volume.
    #[must_use]
    pub const fn chunk(&self) -> VolumeChunkCoordinateV1 {
        VolumeChunkCoordinateV1::new(self.chunk_x, self.chunk_y, self.chunk_z)
    }

    /// Returns the cubic edge.
    #[must_use]
    pub const fn edge(&self) -> u16 {
        self.edge
    }

    /// Returns the compiled solid palette.
    #[must_use]
    pub const fn solid_palette(&self) -> &CompiledSolidPaletteV1 {
        &self.solid_palette
    }

    /// Returns the compiled fluid palette with empty at index zero.
    #[must_use]
    pub const fn fluid_palette(&self) -> &CompiledFluidPaletteV1 {
        &self.fluid_palette
    }

    /// Returns solid palette indices in canonical linear order.
    #[must_use]
    pub fn solid_indices(&self) -> &[u16] {
        &self.solid_indices
    }

    /// Returns fluid palette indices in canonical linear order.
    #[must_use]
    pub fn fluid_indices(&self) -> &[u16] {
        &self.fluid_indices
    }

    /// Returns canonical compact JSON for the volume.
    ///
    /// # Errors
    ///
    /// Returns a canonical encoding error if serialization fails.
    pub fn canonical_bytes(&self) -> ContentResult<Vec<u8>> {
        canonical_json_bytes(self).map_err(ContentError::from)
    }

    /// Returns the canonical volume hash.
    ///
    /// # Errors
    ///
    /// Returns a canonical encoding error if serialization fails.
    pub fn canonical_hash(&self) -> ContentResult<CanonicalHash> {
        self.canonical_bytes().map(CanonicalHash::digest)
    }
}

/// Returns the canonical linear index `x + edge * (z + edge * y)`.
///
/// # Errors
///
/// Returns [`ContentError::InvalidSolidFluidVolume`] when a local coordinate is
/// outside the cubic edge.
pub fn volume_linear_index(edge: u16, x: u16, y: u16, z: u16) -> ContentResult<usize> {
    if x >= edge || y >= edge || z >= edge {
        return Err(ContentError::InvalidSolidFluidVolume {
            reason: "local volume coordinate exceeds the cubic edge",
        });
    }
    let edge = usize::from(edge);
    Ok(usize::from(x) + edge * (usize::from(z) + edge * usize::from(y)))
}

fn volume_cell_count(edge: u16) -> ContentResult<usize> {
    let edge = usize::from(edge);
    edge.checked_mul(edge)
        .and_then(|area| area.checked_mul(edge))
        .ok_or(ContentError::InvalidSolidFluidVolume {
            reason: "volume cell count overflowed",
        })
}

fn compare_fluid_entries(
    left: &FluidPaletteEntryV1,
    right: &FluidPaletteEntryV1,
) -> std::cmp::Ordering {
    match (left, right) {
        (FluidPaletteEntryV1::Empty, FluidPaletteEntryV1::Empty) => std::cmp::Ordering::Equal,
        (FluidPaletteEntryV1::Empty, FluidPaletteEntryV1::Fluid { .. }) => std::cmp::Ordering::Less,
        (FluidPaletteEntryV1::Fluid { .. }, FluidPaletteEntryV1::Empty) => {
            std::cmp::Ordering::Greater
        }
        (
            FluidPaletteEntryV1::Fluid {
                fluid: left_id,
                state: left_state,
            },
            FluidPaletteEntryV1::Fluid {
                fluid: right_id,
                state: right_state,
            },
        ) => left_id
            .cmp(right_id)
            .then_with(|| left_state.level.get().cmp(&right_state.level.get()))
            .then_with(|| left_state.flow.cmp(&right_state.flow)),
    }
}

fn solid_index(
    palette: &CompiledSolidPaletteV1,
    entry: &SolidPaletteEntryV1,
) -> ContentResult<u16> {
    palette
        .entries()
        .iter()
        .position(|candidate| {
            candidate.block() == &entry.block && candidate.state() == &entry.state
        })
        .and_then(|index| u16::try_from(index).ok())
        .ok_or(ContentError::InvalidSolidFluidVolume {
            reason: "solid palette lost a volume entry",
        })
}

fn fluid_index(
    palette: &CompiledFluidPaletteV1,
    entry: &FluidPaletteEntryV1,
) -> ContentResult<u16> {
    let index = match entry {
        FluidPaletteEntryV1::Empty => palette
            .entries()
            .iter()
            .position(crate::CompiledFluidPaletteEntryV1::is_empty),
        FluidPaletteEntryV1::Fluid { fluid, state } => {
            palette.entries().iter().position(|candidate| {
                candidate.fluid() == Some(fluid) && candidate.state() == Some(state)
            })
        }
    };
    index
        .and_then(|value| u16::try_from(value).ok())
        .ok_or(ContentError::InvalidSolidFluidVolume {
            reason: "fluid palette lost a volume entry",
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        BlockStateV1, ContentCatalogInputV1, ContentCatalogLimitsV1, FluidFlowV1, FluidLevelV1,
        FluidStateV1,
    };
    use latticeaxiom_core::StableId;

    const FIXTURE: &str = include_str!("../fixtures/terrenia/representative-catalog-v1.json");

    fn stable_id(value: &str) -> StableId {
        value
            .parse()
            .unwrap_or_else(|error| panic!("fixture StableId `{value}` is invalid: {error}"))
    }

    fn catalog() -> ContentCatalogV1 {
        let input = serde_json::from_str::<ContentCatalogInputV1>(FIXTURE)
            .unwrap_or_else(|error| panic!("representative catalog fixture is invalid: {error}"));
        ContentCatalogV1::compile(input, ContentCatalogLimitsV1::default())
            .unwrap_or_else(|error| panic!("representative catalog did not compile: {error}"))
    }

    fn air() -> SolidPaletteEntryV1 {
        SolidPaletteEntryV1 {
            block: stable_id("terrenia:block/air"),
            state: BlockStateV1::empty(),
        }
    }

    fn stone() -> SolidPaletteEntryV1 {
        SolidPaletteEntryV1 {
            block: stable_id("terrenia:block/stone"),
            state: BlockStateV1::empty(),
        }
    }

    fn water(level: u8, flow: FluidFlowV1) -> FluidPaletteEntryV1 {
        FluidPaletteEntryV1::Fluid {
            fluid: stable_id("terrenia:fluid/water"),
            state: FluidStateV1 {
                level: FluidLevelV1::new(level)
                    .unwrap_or_else(|error| panic!("level {level} is invalid: {error}")),
                flow,
            },
        }
    }

    fn compile(
        chunk: VolumeChunkCoordinateV1,
        cells: Vec<SolidFluidVolumeCellV1>,
    ) -> SolidFluidVolumeV1 {
        SolidFluidVolumeV1::compile(&catalog(), chunk, air(), cells, PaletteLimitsV1::default())
            .unwrap_or_else(|error| panic!("volume compile failed: {error}"))
    }

    #[test]
    fn linear_index_is_x_fastest_z_then_y() {
        assert_eq!(
            volume_linear_index(32, 1, 0, 0).unwrap_or_else(|error| panic!("{error}")),
            1
        );
        assert_eq!(
            volume_linear_index(32, 0, 0, 1).unwrap_or_else(|error| panic!("{error}")),
            32
        );
        assert_eq!(
            volume_linear_index(32, 0, 1, 0).unwrap_or_else(|error| panic!("{error}")),
            32 * 32
        );
        assert!(volume_linear_index(32, 32, 0, 0).is_err());
    }

    #[test]
    fn positive_and_negative_chunks_round_trip_equal_palette_bytes() {
        let cells = vec![
            SolidFluidVolumeCellV1 {
                x: 31,
                y: 0,
                z: 0,
                solid: air(),
                fluid: water(0, FluidFlowV1::East),
            },
            SolidFluidVolumeCellV1 {
                x: 0,
                y: 4,
                z: 31,
                solid: stone(),
                fluid: FluidPaletteEntryV1::Empty,
            },
        ];
        let positive = compile(VolumeChunkCoordinateV1::new(3, 2, 1), cells.clone());
        let negative = compile(VolumeChunkCoordinateV1::new(-4, -8, -2), cells);
        assert_eq!(positive.schema(), SOLID_FLUID_VOLUME_SCHEMA_V1);
        assert!(positive.fluid_palette().entries()[0].is_empty());
        assert_eq!(
            positive.solid_palette().entries().len(),
            negative.solid_palette().entries().len()
        );
        assert_eq!(positive.solid_indices(), negative.solid_indices());
        assert_eq!(positive.fluid_indices(), negative.fluid_indices());
        let boundary = volume_linear_index(32, 31, 0, 0)
            .unwrap_or_else(|error| panic!("boundary index failed: {error}"));
        assert_ne!(positive.fluid_indices()[boundary], 0);
        assert_ne!(
            positive
                .canonical_hash()
                .unwrap_or_else(|error| panic!("{error}")),
            negative
                .canonical_hash()
                .unwrap_or_else(|error| panic!("{error}"))
        );
    }

    #[test]
    fn cell_discovery_order_does_not_change_volume_bytes() {
        let forward = vec![
            SolidFluidVolumeCellV1 {
                x: 1,
                y: 0,
                z: 0,
                solid: air(),
                fluid: water(1, FluidFlowV1::West),
            },
            SolidFluidVolumeCellV1 {
                x: 0,
                y: 0,
                z: 0,
                solid: air(),
                fluid: water(0, FluidFlowV1::Still),
            },
        ];
        let mut reversed = forward.clone();
        reversed.reverse();
        let first = compile(VolumeChunkCoordinateV1::new(0, 0, 0), forward);
        let second = compile(VolumeChunkCoordinateV1::new(0, 0, 0), reversed);
        assert_eq!(
            first
                .canonical_hash()
                .unwrap_or_else(|error| panic!("{error}")),
            second
                .canonical_hash()
                .unwrap_or_else(|error| panic!("{error}"))
        );
    }
}
