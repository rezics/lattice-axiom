use latticeaxiom_core::{CanonicalHash, StableId};
use serde::{Deserialize, Serialize};

use crate::{
    GenerationInputHashV1, PlanningCellIdV1, ProviderGenerationIdentityV1, SemanticDensityColumnV1,
    TerrainConfigV2, TerrainFamilyV2, WorldgenConfigV1, WorldgenSeedRootV2,
    hashes::{domain_hash, hash_u64},
    terrain_field::{TerrainColumnSampleV2, climate_field},
    terrain_program::ResolvedTerrainProgramsV1,
};

const TERRITORY_CELL_DOMAIN: &[u8] = b"latticeaxiom.territory-cell.v1\0";
const STYLE_DOMAIN: &[u8] = b"latticeaxiom.d4-style.v1\0";
const TRANSITION_DOMAIN: &[u8] = b"latticeaxiom.d4-transition.v1\0";
const TEMPERATURE_SALT: u64 = 0xa076_1d64_78bd_642f;
const HUMIDITY_SALT: u64 = 0xe703_7ed1_a0b4_28db;

/// Deterministic terrain-style discriminants used by the D4 and natural layers.
///
/// Package-owned style identities and their registration schema remain external.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum TerrainStyleV1 {
    /// Marine shelf and ocean-basin materials with no terrestrial vegetation.
    Marine,
    /// Grass/dirt, mixed stone/limestone, and bounded oak vegetation.
    TemperateWoodland,
    /// Sand/red sand, sandstone strata, basalt, and copper resources.
    AridBadlands,
    /// Snow/peat/moss surfaces, slate, and bounded pine vegetation.
    BorealWetland,
}

impl TerrainStyleV1 {
    /// Compatibility material styles in stable order.
    pub const ALL: [Self; 4] = [
        Self::Marine,
        Self::TemperateWoodland,
        Self::AridBadlands,
        Self::BorealWetland,
    ];
}

/// Metadata for the named deterministic transition at a territory boundary.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TransitionMetadataV1 {
    provider_id: StableId,
    algorithm_revision: u32,
    width_voxels: u16,
    adjacent_style: TerrainStyleV1,
    active: bool,
    provenance: CanonicalHash,
}

impl TransitionMetadataV1 {
    /// Returns the named transition provider identity.
    #[must_use]
    pub const fn provider_id(&self) -> &StableId {
        &self.provider_id
    }

    /// Returns the owner-controlled transition algorithm revision.
    #[must_use]
    pub const fn algorithm_revision(&self) -> u32 {
        self.algorithm_revision
    }

    /// Returns the finite half-width of the transition band.
    #[must_use]
    pub const fn width_voxels(&self) -> u16 {
        self.width_voxels
    }

    /// Returns the actual style in the nearest adjacent planning cell.
    #[must_use]
    pub const fn adjacent_style(&self) -> TerrainStyleV1 {
        self.adjacent_style
    }

    /// Returns whether this coordinate is inside a differing-style band.
    #[must_use]
    pub const fn is_active(&self) -> bool {
        self.active
    }

    /// Returns deterministic transition provenance for diagnostics/receipts.
    #[must_use]
    pub const fn provenance(&self) -> &CanonicalHash {
        &self.provenance
    }
}

/// D7-compatible query shape returned by the D4 coarse two-style selector.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TerritoryQueryV1 {
    domain_id: PlanningCellIdV1,
    winner: TerrainStyleV1,
    runner_up: TerrainStyleV1,
    boundary_distance_voxels: u32,
    transition: TransitionMetadataV1,
}

impl TerritoryQueryV1 {
    /// Returns the stable coarse domain identity.
    #[must_use]
    pub const fn domain_id(&self) -> PlanningCellIdV1 {
        self.domain_id
    }

    /// Returns the selected terrain owner for the core of this cell.
    #[must_use]
    pub const fn winner(&self) -> TerrainStyleV1 {
        self.winner
    }

    /// Returns the highest-ranked available style that did not win this cell.
    #[must_use]
    pub const fn runner_up(&self) -> TerrainStyleV1 {
        self.runner_up
    }

    /// Returns distance to the nearest planning-cell boundary in voxels.
    #[must_use]
    pub const fn boundary_distance_voxels(&self) -> u32 {
        self.boundary_distance_voxels
    }

    /// Returns named transition metadata for the nearest boundary.
    #[must_use]
    pub const fn transition(&self) -> &TransitionMetadataV1 {
        &self.transition
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
enum BoundarySide {
    West,
    East,
    North,
    South,
}

/// Allocation-free territory decision used by the voxel hot path.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct CompactTerritorySampleV1 {
    cell_x: i64,
    cell_z: i64,
    neighbor_x: i64,
    neighbor_z: i64,
    side: BoundarySide,
    winner: TerrainStyleV1,
    adjacent_style: TerrainStyleV1,
    boundary_distance_voxels: u32,
    transition_active: bool,
}

#[derive(Clone, Debug)]
pub(crate) struct TerritorySamplerV1 {
    seed_root: WorldgenSeedRootV2,
    input_hash: GenerationInputHashV1,
    config: WorldgenConfigV1,
    terrain_config: TerrainConfigV2,
    terrain_programs: ResolvedTerrainProgramsV1,
    temperature_seed: u64,
    humidity_seed: u64,
    material_transition_seed: u64,
    transition: ProviderGenerationIdentityV1,
}

impl TerritorySamplerV1 {
    pub(crate) fn new(
        seed_root: WorldgenSeedRootV2,
        input_hash: GenerationInputHashV1,
        config: WorldgenConfigV1,
        terrain_config: TerrainConfigV2,
        terrain_programs: ResolvedTerrainProgramsV1,
        transition: ProviderGenerationIdentityV1,
    ) -> Self {
        let climate_seed = hash_u64(STYLE_DOMAIN, &[seed_root.as_bytes()]);
        Self {
            seed_root,
            input_hash,
            config,
            terrain_config,
            terrain_programs,
            temperature_seed: climate_seed ^ TEMPERATURE_SALT,
            humidity_seed: climate_seed ^ HUMIDITY_SALT,
            material_transition_seed: hash_u64(
                TRANSITION_DOMAIN,
                &[seed_root.as_bytes(), b"coherent-materials-v2"],
            ),
            transition,
        }
    }

    pub(crate) fn query(&self, x: i64, z: i64) -> TerritoryQueryV1 {
        let sample = self.sample(x, z);
        let runner_up = if sample.adjacent_style == sample.winner {
            self.runner_up_style(sample.cell_x, sample.cell_z, sample.winner)
        } else {
            sample.adjacent_style
        };
        let side_byte = [side_discriminant(sample.side)];
        let provenance = domain_hash(
            TRANSITION_DOMAIN,
            &[
                self.input_hash.as_bytes(),
                self.transition.provider_stable_id().as_str().as_bytes(),
                self.transition.implementation_fingerprint().as_bytes(),
                &sample.cell_x.to_be_bytes(),
                &sample.cell_z.to_be_bytes(),
                &sample.neighbor_x.to_be_bytes(),
                &sample.neighbor_z.to_be_bytes(),
                &side_byte,
            ],
        );
        TerritoryQueryV1 {
            domain_id: self.cell_id(sample.cell_x, sample.cell_z),
            winner: sample.winner,
            runner_up,
            boundary_distance_voxels: sample.boundary_distance_voxels,
            transition: TransitionMetadataV1 {
                provider_id: self.transition.provider_stable_id().clone(),
                algorithm_revision: self.transition.algorithm_revision(),
                width_voxels: self.config.transition_width_voxels,
                adjacent_style: sample.adjacent_style,
                active: sample.transition_active,
                provenance,
            },
        }
    }

    #[allow(
        clippy::similar_names,
        reason = "paired X/Z and winner/adjacent values are domain terms"
    )]
    pub(crate) fn sample(&self, x: i64, z: i64) -> CompactTerritorySampleV1 {
        let edge = self.planning_edge_voxels();
        let cell_x = x.div_euclid(edge);
        let cell_z = z.div_euclid(edge);
        let local_x = x.rem_euclid(edge);
        let local_z = z.rem_euclid(edge);
        let (side, distance) = nearest_boundary(local_x, local_z, edge);
        let (neighbor_x, neighbor_z) = neighbor(cell_x, cell_z, side);
        let winner = self.style_for_cell(cell_x, cell_z);
        let adjacent_style = self.style_for_cell(neighbor_x, neighbor_z);
        let boundary_distance_voxels = u32::try_from(distance).unwrap_or(u32::MAX);
        let transition_active = adjacent_style != winner
            && boundary_distance_voxels <= u32::from(self.config.transition_width_voxels);
        CompactTerritorySampleV1 {
            cell_x,
            cell_z,
            neighbor_x,
            neighbor_z,
            side,
            winner,
            adjacent_style,
            boundary_distance_voxels,
            transition_active,
        }
    }

    pub(crate) fn terrain_column(&self, x: i64, z: i64) -> TerrainColumnSampleV2 {
        let edge = self.planning_edge_voxels();
        let cell_x = x.div_euclid(edge);
        let cell_z = z.div_euclid(edge);
        let blend_x = axis_blend(
            x.rem_euclid(edge),
            edge,
            self.config.transition_width_voxels,
        );
        let blend_z = axis_blend(
            z.rem_euclid(edge),
            edge,
            self.config.transition_width_voxels,
        );
        let neighbor_x = cell_x.saturating_add(blend_x.neighbor_offset);
        let neighbor_z = cell_z.saturating_add(blend_z.neighbor_offset);

        let current = self
            .terrain_programs
            .sample(self.style_for_cell(cell_x, cell_z), x, z);
        if blend_x.neighbor_offset == 0 && blend_z.neighbor_offset == 0 {
            return current;
        }
        let across_x = self
            .terrain_programs
            .sample(self.style_for_cell(neighbor_x, cell_z), x, z);
        if blend_z.neighbor_offset == 0 {
            return blend_terrain_columns(current, across_x, blend_x.current_weight, blend_x.total);
        }
        let across_z = self
            .terrain_programs
            .sample(self.style_for_cell(cell_x, neighbor_z), x, z);
        if blend_x.neighbor_offset == 0 {
            return blend_terrain_columns(current, across_z, blend_z.current_weight, blend_z.total);
        }
        let diagonal =
            self.terrain_programs
                .sample(self.style_for_cell(neighbor_x, neighbor_z), x, z);
        let near_z =
            blend_terrain_columns(current, across_x, blend_x.current_weight, blend_x.total);
        let far_z =
            blend_terrain_columns(across_z, diagonal, blend_x.current_weight, blend_x.total);
        blend_terrain_columns(near_z, far_z, blend_z.current_weight, blend_z.total)
    }

    pub(crate) fn family(&self, x: i64, z: i64) -> TerrainFamilyV2 {
        self.terrain_column(x, z).family
    }

    pub(crate) fn prepare_density_column(
        &self,
        style: TerrainStyleV1,
        x: i64,
        z: i64,
        approved_surface_y: i32,
        protected_water: bool,
    ) -> Option<SemanticDensityColumnV1> {
        self.terrain_programs.prepare_density_column(
            style,
            x,
            z,
            approved_surface_y,
            protected_water,
        )
    }

    pub(crate) fn maximum_density_displacement_voxels(&self) -> u32 {
        self.terrain_programs.semantic_policy().map_or(
            0,
            crate::SemanticTerrainPolicyV1::maximum_density_displacement_voxels,
        )
    }

    pub(crate) fn choose_material_style(
        &self,
        x: i64,
        z: i64,
        sample: CompactTerritorySampleV1,
    ) -> TerrainStyleV1 {
        if !sample.transition_active {
            return sample.winner;
        }
        if self.transition.algorithm_revision() >= 9 {
            return coherent_material_style(
                self.material_transition_seed,
                x,
                z,
                self.config.transition_width_voxels,
                sample,
            );
        }
        let width = u64::from(self.config.transition_width_voxels);
        let distance = u64::from(sample.boundary_distance_voxels);
        let winner_weight = width.saturating_add(distance);
        let total = width.saturating_mul(2).max(1);
        let roll = hash_u64(
            TRANSITION_DOMAIN,
            &[
                self.seed_root.as_bytes(),
                &x.to_be_bytes(),
                &z.to_be_bytes(),
            ],
        ) % total;
        if roll < winner_weight {
            sample.winner
        } else {
            sample.adjacent_style
        }
    }

    pub(crate) fn planning_edge_voxels(&self) -> i64 {
        i64::from(self.config.chunk_edge_voxels)
            .saturating_mul(i64::from(self.config.planning_cell_edge_chunks))
    }

    fn cell_id(&self, cell_x: i64, cell_z: i64) -> PlanningCellIdV1 {
        PlanningCellIdV1::from_hash(domain_hash(
            TERRITORY_CELL_DOMAIN,
            &[
                self.seed_root.as_bytes(),
                &cell_x.to_be_bytes(),
                &cell_z.to_be_bytes(),
            ],
        ))
    }

    fn style_for_cell(&self, cell_x: i64, cell_z: i64) -> TerrainStyleV1 {
        let (temperature, humidity) = self.climate_for_cell(cell_x, cell_z);
        let edge = self.planning_edge_voxels();
        let center_x = cell_x
            .saturating_mul(edge)
            .saturating_add(edge.div_euclid(2));
        let center_z = cell_z
            .saturating_mul(edge)
            .saturating_add(edge.div_euclid(2));
        self.terrain_programs
            .select_style(center_x, center_z, temperature, humidity)
    }

    fn runner_up_style(&self, cell_x: i64, cell_z: i64, winner: TerrainStyleV1) -> TerrainStyleV1 {
        let (temperature, humidity) = self.climate_for_cell(cell_x, cell_z);
        let edge = self.planning_edge_voxels();
        let center_x = cell_x
            .saturating_mul(edge)
            .saturating_add(edge.div_euclid(2));
        let center_z = cell_z
            .saturating_mul(edge)
            .saturating_add(edge.div_euclid(2));
        self.terrain_programs
            .runner_up_style(winner, center_x, center_z, temperature, humidity)
    }

    fn climate_for_cell(&self, cell_x: i64, cell_z: i64) -> (i64, i64) {
        let edge = self.planning_edge_voxels().max(1);
        let scale_cells = i64::from(self.terrain_config.climate.scale_voxels)
            .div_euclid(edge)
            .clamp(1, i64::from(u16::MAX));
        let scale_cells = u16::try_from(scale_cells).unwrap_or(u16::MAX);
        let temperature = climate_field(self.temperature_seed, cell_x, cell_z, scale_cells)
            .saturating_mul(i64::from(
                self.terrain_config.climate.temperature_variance_per_1024,
            ))
            .div_euclid(1_024);
        let humidity = climate_field(self.humidity_seed, cell_x, cell_z, scale_cells)
            .saturating_mul(i64::from(
                self.terrain_config.climate.humidity_variance_per_1024,
            ))
            .div_euclid(1_024);
        (temperature, humidity)
    }
}

/// Move one shared boundary with a continuous field. Canonical style ordering
/// gives both sides the same displacement sign; per-voxel random selection
/// would fragment entire transition bands into isolated grass/sand columns.
fn coherent_material_style(
    seed: u64,
    x: i64,
    z: i64,
    width: u16,
    sample: CompactTerritorySampleV1,
) -> TerrainStyleV1 {
    let (low, high) = if sample.winner < sample.adjacent_style {
        (sample.winner, sample.adjacent_style)
    } else {
        (sample.adjacent_style, sample.winner)
    };
    let distance = i64::from(sample.boundary_distance_voxels)
        .saturating_mul(2)
        .saturating_add(1);
    let signed_distance = if sample.winner == low {
        distance
    } else {
        -distance
    };
    let noise = climate_field(seed, x, z, width.clamp(16, 96));
    let score = signed_distance
        .saturating_mul(1024)
        .saturating_add(noise.saturating_mul(i64::from(width)).saturating_mul(2));
    if score >= 0 { low } else { high }
}

#[derive(Clone, Copy)]
struct AxisBlend {
    neighbor_offset: i64,
    current_weight: i64,
    total: i64,
}

fn axis_blend(local: i64, edge: i64, width: u16) -> AxisBlend {
    let width = i64::from(width).max(1);
    let total = width.saturating_mul(2);
    let east_distance = edge.saturating_sub(1).saturating_sub(local);
    if local <= width {
        AxisBlend {
            neighbor_offset: -1,
            current_weight: width.saturating_add(local),
            total,
        }
    } else if east_distance <= width {
        AxisBlend {
            neighbor_offset: 1,
            current_weight: width.saturating_add(east_distance),
            total,
        }
    } else {
        AxisBlend {
            neighbor_offset: 0,
            current_weight: total,
            total,
        }
    }
}

fn blend_terrain_columns(
    winner: TerrainColumnSampleV2,
    adjacent: TerrainColumnSampleV2,
    winner_weight: i64,
    total: i64,
) -> TerrainColumnSampleV2 {
    let adjacent_weight = total.saturating_sub(winner_weight);
    let height = i64::from(winner.height)
        .saturating_mul(winner_weight)
        .saturating_add(i64::from(adjacent.height).saturating_mul(adjacent_weight))
        .div_euclid(total);
    let surface_water_y = match (winner.surface_water_y, adjacent.surface_water_y) {
        (Some(left), Some(right)) => Some(
            i64::from(left)
                .saturating_mul(winner_weight)
                .saturating_add(i64::from(right).saturating_mul(adjacent_weight))
                .div_euclid(total)
                .try_into()
                .unwrap_or(left),
        ),
        (left, right) => {
            if winner_weight >= adjacent_weight {
                left
            } else {
                right
            }
        }
    };
    TerrainColumnSampleV2 {
        height: i32::try_from(height).unwrap_or(winner.height),
        family: if winner_weight >= adjacent_weight {
            winner.family
        } else {
            adjacent.family
        },
        surface_water_y,
        drainage: winner.drainage,
    }
}

fn nearest_boundary(local_x: i64, local_z: i64, edge: i64) -> (BoundarySide, i64) {
    let candidates = [
        (local_x, BoundarySide::West),
        (
            edge.saturating_sub(1).saturating_sub(local_x),
            BoundarySide::East,
        ),
        (local_z, BoundarySide::North),
        (
            edge.saturating_sub(1).saturating_sub(local_z),
            BoundarySide::South,
        ),
    ];
    let (distance, side) = candidates
        .into_iter()
        .min()
        .unwrap_or((0, BoundarySide::West));
    (side, distance)
}

const fn neighbor(cell_x: i64, cell_z: i64, side: BoundarySide) -> (i64, i64) {
    match side {
        BoundarySide::West => (cell_x.saturating_sub(1), cell_z),
        BoundarySide::East => (cell_x.saturating_add(1), cell_z),
        BoundarySide::North => (cell_x, cell_z.saturating_sub(1)),
        BoundarySide::South => (cell_x, cell_z.saturating_add(1)),
    }
}

const fn side_discriminant(side: BoundarySide) -> u8 {
    match side {
        BoundarySide::West => 0,
        BoundarySide::East => 1,
        BoundarySide::North => 2,
        BoundarySide::South => 3,
    }
}

#[cfg(test)]
mod coherent_material_tests {
    use super::*;

    fn sample(winner: TerrainStyleV1, adjacent_style: TerrainStyleV1) -> CompactTerritorySampleV1 {
        CompactTerritorySampleV1 {
            cell_x: -1,
            cell_z: 0,
            neighbor_x: 0,
            neighbor_z: 0,
            side: BoundarySide::East,
            winner,
            adjacent_style,
            boundary_distance_voxels: 0,
            transition_active: true,
        }
    }

    #[test]
    fn transition_forms_contiguous_patches_across_signed_coordinates() {
        let west = sample(
            TerrainStyleV1::TemperateWoodland,
            TerrainStyleV1::AridBadlands,
        );
        let east = sample(
            TerrainStyleV1::AridBadlands,
            TerrainStyleV1::TemperateWoodland,
        );
        let line: Vec<_> = (-256..256)
            .map(|z| coherent_material_style(42, -1, z, 48, west))
            .collect();
        let transitions = line.windows(2).filter(|pair| pair[0] != pair[1]).count();
        assert!(
            transitions > 0 && transitions < 32,
            "boundary must meander in broad patches: {transitions}"
        );
        let disagreements = (-256..256)
            .filter(|z| {
                coherent_material_style(42, -1, *z, 48, west)
                    != coherent_material_style(42, 0, *z, 48, east)
            })
            .count();
        assert!(
            disagreements < 64,
            "both sides must share the displaced boundary: {disagreements}"
        );
    }
}
