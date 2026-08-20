use std::num::{NonZeroU16, NonZeroU32, NonZeroU64};

use latticeaxiom_core::{CanonicalHash, canonical_json_bytes};
use serde::{Deserialize, Serialize};

use crate::{WorldgenConfigHashV1, WorldgenError, WorldgenResult};

const MIN_CHUNK_EDGE: u16 = 8;
const MAX_CHUNK_EDGE: u16 = 64;
const MAX_THRESHOLD: u16 = 1_024;

/// Closed, integer-only configuration for the version-one D4 generator.
///
/// Missing JSON fields materialize the documented defaults before hashing and
/// unknown fields are rejected. Every output-affecting number is bounded and
/// integral, avoiding target-specific floating-point behavior.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct WorldgenConfigV1 {
    /// Edge of one cubic chunk in voxels.
    pub chunk_edge_voxels: u16,
    /// Edge of one square planning cell in chunks.
    pub planning_cell_edge_chunks: u16,
    /// Named transition half-width in voxels.
    pub transition_width_voxels: u16,
    /// Horizontal scale used by deterministic height intent.
    pub height_noise_scale_voxels: u16,
    /// Coarse three-dimensional cave sample cell edge.
    pub cave_cell_edge_voxels: u16,
    /// Inclusive lowest materializable world voxel Y.
    pub world_floor_y: i32,
    /// Inclusive highest materializable world voxel Y.
    pub world_ceiling_y: i32,
    /// Temperate woodland nominal surface Y.
    pub temperate_base_height: i32,
    /// Temperate woodland bounded relief amplitude.
    pub temperate_relief: u16,
    /// Arid badlands nominal surface Y.
    pub arid_base_height: i32,
    /// Arid badlands bounded relief amplitude.
    pub arid_relief: u16,
    /// Minimum number of solid voxels above a cave void.
    pub cave_minimum_cover: u16,
    /// Cave selection threshold in `0..=1024` hash units.
    pub cave_threshold_per_1024: u16,
    /// Copper replacement threshold in `0..=1024` hash units.
    pub copper_threshold_per_1024: u16,
    /// Woodland tree-anchor threshold in `0..=1024` hash units.
    pub tree_threshold_per_1024: u16,
    /// Woodland ground-cover threshold in `0..=1024` hash units.
    pub ground_cover_threshold_per_1024: u16,
}

impl Default for WorldgenConfigV1 {
    fn default() -> Self {
        Self {
            chunk_edge_voxels: 16,
            planning_cell_edge_chunks: 8,
            transition_width_voxels: 8,
            height_noise_scale_voxels: 16,
            cave_cell_edge_voxels: 8,
            world_floor_y: -64,
            world_ceiling_y: 127,
            temperate_base_height: 24,
            temperate_relief: 10,
            arid_base_height: 18,
            arid_relief: 14,
            cave_minimum_cover: 6,
            cave_threshold_per_1024: 132,
            copper_threshold_per_1024: 18,
            tree_threshold_per_1024: 14,
            ground_cover_threshold_per_1024: 96,
        }
    }
}

impl WorldgenConfigV1 {
    /// Decodes a closed JSON record, materializes defaults, and validates it.
    ///
    /// # Errors
    ///
    /// Returns [`WorldgenError::InvalidConfigEncoding`] for malformed or
    /// unknown JSON fields and [`WorldgenError::InvalidConfig`] for bounded
    /// value violations.
    pub fn from_json(bytes: &[u8]) -> WorldgenResult<Self> {
        let value: Self = serde_json::from_slice(bytes).map_err(|error| {
            WorldgenError::InvalidConfigEncoding {
                reason: error.to_string(),
            }
        })?;
        value.validate()?;
        Ok(value)
    }

    /// Validates all cross-field and numeric bounds.
    ///
    /// # Errors
    ///
    /// Returns [`WorldgenError::InvalidConfig`] when a field is outside the
    /// version-one closed domain or when a derived size would be invalid.
    pub fn validate(&self) -> WorldgenResult<()> {
        bounded_u16(
            "chunk_edge_voxels",
            self.chunk_edge_voxels,
            MIN_CHUNK_EDGE,
            MAX_CHUNK_EDGE,
        )?;
        bounded_u16(
            "planning_cell_edge_chunks",
            self.planning_cell_edge_chunks,
            1,
            1_024,
        )?;
        bounded_u16(
            "transition_width_voxels",
            self.transition_width_voxels,
            1,
            u16::MAX,
        )?;
        bounded_u16(
            "height_noise_scale_voxels",
            self.height_noise_scale_voxels,
            1,
            4_096,
        )?;
        bounded_u16("cave_cell_edge_voxels", self.cave_cell_edge_voxels, 2, 128)?;
        bounded_u16("temperate_relief", self.temperate_relief, 1, 64)?;
        bounded_u16("arid_relief", self.arid_relief, 1, 64)?;
        bounded_u16("cave_minimum_cover", self.cave_minimum_cover, 1, 64)?;
        threshold("cave_threshold_per_1024", self.cave_threshold_per_1024)?;
        threshold("copper_threshold_per_1024", self.copper_threshold_per_1024)?;
        threshold("tree_threshold_per_1024", self.tree_threshold_per_1024)?;
        threshold(
            "ground_cover_threshold_per_1024",
            self.ground_cover_threshold_per_1024,
        )?;

        if self.world_floor_y >= self.world_ceiling_y {
            return Err(invalid(
                "world_floor_y",
                "must be lower than world_ceiling_y",
            ));
        }

        let maximum_surface = i64::from(self.temperate_base_height)
            .saturating_add(i64::from(self.temperate_relief))
            .max(i64::from(self.arid_base_height).saturating_add(i64::from(self.arid_relief)));
        let minimum_surface = i64::from(self.temperate_base_height)
            .saturating_sub(i64::from(self.temperate_relief))
            .min(i64::from(self.arid_base_height).saturating_sub(i64::from(self.arid_relief)));
        if maximum_surface.saturating_add(7) > i64::from(self.world_ceiling_y) {
            return Err(invalid(
                "world_ceiling_y",
                "must leave seven voxels above the maximum surface for bounded vegetation",
            ));
        }
        if minimum_surface.saturating_sub(8) < i64::from(self.world_floor_y) {
            return Err(invalid(
                "world_floor_y",
                "must leave eight voxels below the minimum surface for caves and rock",
            ));
        }

        let planning_edge = u64::from(self.chunk_edge_voxels)
            .checked_mul(u64::from(self.planning_cell_edge_chunks))
            .ok_or(WorldgenError::ArithmeticOverflow {
                operation: "planning-cell voxel edge",
            })?;
        if u64::from(self.transition_width_voxels).saturating_mul(2) >= planning_edge {
            return Err(invalid(
                "transition_width_voxels",
                "twice the transition width must be smaller than a planning cell",
            ));
        }

        Ok(())
    }

    /// Returns canonical compact JSON with all defaults materialized.
    ///
    /// # Errors
    ///
    /// Returns a canonical encoding error if serialization fails.
    pub fn canonical_bytes(&self) -> WorldgenResult<Vec<u8>> {
        self.validate()?;
        canonical_json_bytes(self).map_err(|error| WorldgenError::CanonicalEncoding {
            kind: "WorldgenConfigV1",
            reason: error.to_string(),
        })
    }

    /// Returns the hash of [`Self::canonical_bytes`].
    ///
    /// # Errors
    ///
    /// Returns any validation or canonical encoding error.
    pub fn canonical_hash(&self) -> WorldgenResult<WorldgenConfigHashV1> {
        self.canonical_bytes()
            .map(|bytes| WorldgenConfigHashV1::from_hash(CanonicalHash::digest(bytes)))
    }
}

/// Hard preflight limits for one compiled plan and one chunk generation call.
///
/// Limits gate work and allocation. They are not hidden output inputs: a call
/// below both limits has identical output regardless of the larger allowance.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WorldgenLimitsV1 {
    /// Maximum number of provider offers accepted at plan compilation.
    pub max_provider_offers: NonZeroU16,
    /// Maximum number of frozen role bindings accepted at compilation.
    pub max_role_bindings: NonZeroU16,
    /// Maximum number of block definitions in the supplied D4 closure.
    pub max_catalog_blocks: NonZeroU16,
    /// Maximum number of adjacent epoch records per generation request.
    pub max_adjacent_epochs: NonZeroU16,
    /// Maximum number of boundary adapter declarations per generation request.
    pub max_boundary_adapters: NonZeroU16,
    /// Maximum UTF-8 bytes in any one `StableId` accepted by plan compilation.
    pub max_identity_bytes: NonZeroU32,
    /// Maximum aggregate identity/config bytes inspected by plan compilation.
    pub max_plan_input_bytes: NonZeroU64,
    /// Maximum conservatively estimated live bytes during one generation call.
    pub max_live_generation_bytes: NonZeroU64,
    /// Maximum cubic chunk edge accepted by this host profile.
    pub max_chunk_edge_voxels: NonZeroU16,
    /// Maximum exact voxel count in one generated draft.
    pub max_voxels_per_chunk: NonZeroU32,
    /// Maximum deterministic sampling work units in one request.
    pub max_work_units: NonZeroU64,
    /// Maximum canonical snapshot candidate byte length.
    pub max_snapshot_bytes: NonZeroU64,
}

impl Default for WorldgenLimitsV1 {
    fn default() -> Self {
        Self {
            max_provider_offers: NonZeroU16::new(64).unwrap_or(NonZeroU16::MIN),
            max_role_bindings: NonZeroU16::new(256).unwrap_or(NonZeroU16::MIN),
            max_catalog_blocks: NonZeroU16::new(512).unwrap_or(NonZeroU16::MIN),
            max_adjacent_epochs: NonZeroU16::new(4).unwrap_or(NonZeroU16::MIN),
            max_boundary_adapters: NonZeroU16::new(32).unwrap_or(NonZeroU16::MIN),
            max_identity_bytes: NonZeroU32::new(1_024).unwrap_or(NonZeroU32::MIN),
            max_plan_input_bytes: NonZeroU64::new(8_388_608).unwrap_or(NonZeroU64::MIN),
            max_live_generation_bytes: NonZeroU64::new(134_217_728).unwrap_or(NonZeroU64::MIN),
            max_chunk_edge_voxels: NonZeroU16::new(MAX_CHUNK_EDGE).unwrap_or(NonZeroU16::MIN),
            max_voxels_per_chunk: NonZeroU32::new(262_144).unwrap_or(NonZeroU32::MIN),
            max_work_units: NonZeroU64::new(8_388_608).unwrap_or(NonZeroU64::MIN),
            max_snapshot_bytes: NonZeroU64::new(67_108_864).unwrap_or(NonZeroU64::MIN),
        }
    }
}

fn bounded_u16(field: &'static str, value: u16, minimum: u16, maximum: u16) -> WorldgenResult<()> {
    if (minimum..=maximum).contains(&value) {
        Ok(())
    } else {
        Err(invalid(
            field,
            format!("must be in {minimum}..={maximum}, got {value}"),
        ))
    }
}

fn threshold(field: &'static str, value: u16) -> WorldgenResult<()> {
    bounded_u16(field, value, 0, MAX_THRESHOLD)
}

fn invalid(field: &'static str, reason: impl Into<String>) -> WorldgenError {
    WorldgenError::InvalidConfig {
        field,
        reason: reason.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn omitted_defaults_and_explicit_defaults_hash_identically() {
        let implicit = WorldgenConfigV1::from_json(b"{}").ok();
        let explicit_bytes = serde_json::to_vec(&WorldgenConfigV1::default()).unwrap_or_default();
        let explicit = WorldgenConfigV1::from_json(&explicit_bytes).ok();
        assert_eq!(implicit, explicit);
        assert_eq!(
            implicit.and_then(|config| config.canonical_hash().ok()),
            explicit.and_then(|config| config.canonical_hash().ok())
        );
    }

    #[test]
    fn unknown_fields_are_rejected() {
        let error = WorldgenConfigV1::from_json(br#"{"mystery":1}"#);
        assert!(matches!(
            error,
            Err(WorldgenError::InvalidConfigEncoding { .. })
        ));
    }

    #[test]
    fn transition_must_fit_inside_cell() {
        let config = WorldgenConfigV1 {
            chunk_edge_voxels: 8,
            planning_cell_edge_chunks: 1,
            transition_width_voxels: 4,
            ..WorldgenConfigV1::default()
        };
        assert!(matches!(
            config.validate(),
            Err(WorldgenError::InvalidConfig {
                field: "transition_width_voxels",
                ..
            })
        ));
    }

    #[test]
    fn transition_width_must_be_nonzero() {
        let config = WorldgenConfigV1 {
            transition_width_voxels: 0,
            ..WorldgenConfigV1::default()
        };
        assert!(matches!(
            config.validate(),
            Err(WorldgenError::InvalidConfig {
                field: "transition_width_voxels",
                ..
            })
        ));
    }
}
