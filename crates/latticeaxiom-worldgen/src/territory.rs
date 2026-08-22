use latticeaxiom_core::{CanonicalHash, StableId};
use serde::{Deserialize, Serialize};

use crate::{
    GenerationInputHashV1, PlanningCellIdV1, ProviderGenerationIdentityV1, WorldSeedV1,
    WorldgenConfigV1,
    hashes::{domain_hash, hash_u64},
};

const TERRITORY_CELL_DOMAIN: &[u8] = b"latticeaxiom.territory-cell.v1\0";
const STYLE_DOMAIN: &[u8] = b"latticeaxiom.d4-style.v1\0";
const HEIGHT_DOMAIN: &[u8] = b"latticeaxiom.d4-height.v1\0";
const TRANSITION_DOMAIN: &[u8] = b"latticeaxiom.d4-transition.v1\0";

/// Two deterministic fixture-algorithm discriminants used by the D4 scaffold.
///
/// Package-owned style identities and their registration schema remain external.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum TerrainStyleV1 {
    /// Grass/dirt, mixed stone/limestone, and bounded oak vegetation.
    TemperateWoodland,
    /// Sand/red sand, sandstone strata, basalt, and copper resources.
    AridBadlands,
    /// Snow/peat/moss surfaces, slate, and bounded pine vegetation.
    BorealWetland,
}

impl TerrainStyleV1 {
    pub(crate) const fn discriminant(self) -> u8 {
        match self {
            Self::TemperateWoodland => 0,
            Self::AridBadlands => 1,
            Self::BorealWetland => 2,
        }
    }

    pub(crate) const fn other(self) -> Self {
        match self {
            Self::TemperateWoodland => Self::AridBadlands,
            Self::AridBadlands | Self::BorealWetland => Self::TemperateWoodland,
        }
    }
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

    /// Returns the other ranked D4 candidate.
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
pub(crate) struct BorealTerrainParamsV1 {
    pub(crate) provider: ProviderGenerationIdentityV1,
    pub(crate) base_height: i32,
    pub(crate) relief: u16,
}

#[derive(Clone, Debug)]
pub(crate) struct TerritorySamplerV1 {
    seed: WorldSeedV1,
    input_hash: GenerationInputHashV1,
    config: WorldgenConfigV1,
    selector: ProviderGenerationIdentityV1,
    transition: ProviderGenerationIdentityV1,
    temperate: ProviderGenerationIdentityV1,
    arid: ProviderGenerationIdentityV1,
    boreal: Option<BorealTerrainParamsV1>,
}

impl TerritorySamplerV1 {
    pub(crate) const fn new(
        seed: WorldSeedV1,
        input_hash: GenerationInputHashV1,
        config: WorldgenConfigV1,
        selector: ProviderGenerationIdentityV1,
        transition: ProviderGenerationIdentityV1,
        temperate: ProviderGenerationIdentityV1,
        arid: ProviderGenerationIdentityV1,
    ) -> Self {
        Self {
            seed,
            input_hash,
            config,
            selector,
            transition,
            temperate,
            arid,
            boreal: None,
        }
    }

    pub(crate) fn with_boreal(mut self, params: BorealTerrainParamsV1) -> Self {
        self.boreal = Some(params);
        self
    }

    pub(crate) fn query(&self, x: i64, z: i64) -> TerritoryQueryV1 {
        let sample = self.sample(x, z);
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
            runner_up: sample.winner.other(),
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

    #[allow(
        clippy::similar_names,
        reason = "winner/adjacent height and weight pairs mirror the blend equation"
    )]
    pub(crate) fn height(&self, x: i64, z: i64, sample: CompactTerritorySampleV1) -> i32 {
        let winner_height = self.raw_height(sample.winner, x, z);
        if !sample.transition_active {
            return winner_height;
        }
        let adjacent_height = self.raw_height(sample.adjacent_style, x, z);
        let width = i64::from(self.config.transition_width_voxels);
        let distance = i64::from(sample.boundary_distance_voxels.min(u32::from(u16::MAX)));
        let winner_weight = width.saturating_add(distance);
        let adjacent_weight = width.saturating_sub(distance);
        let denominator = width.saturating_mul(2).max(1);
        let blended = i64::from(winner_height)
            .saturating_mul(winner_weight)
            .saturating_add(i64::from(adjacent_height).saturating_mul(adjacent_weight))
            .div_euclid(denominator);
        i32::try_from(blended).unwrap_or_else(|_| {
            if blended.is_negative() {
                i32::MIN
            } else {
                i32::MAX
            }
        })
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
        let width = u64::from(self.config.transition_width_voxels);
        let distance = u64::from(sample.boundary_distance_voxels);
        let winner_weight = width.saturating_add(distance);
        let total = width.saturating_mul(2).max(1);
        let roll = hash_u64(
            TRANSITION_DOMAIN,
            &[
                self.input_hash.as_bytes(),
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
                self.seed.as_bytes(),
                self.input_hash.as_bytes(),
                &cell_x.to_be_bytes(),
                &cell_z.to_be_bytes(),
            ],
        ))
    }

    fn style_for_cell(&self, cell_x: i64, cell_z: i64) -> TerrainStyleV1 {
        let roll = hash_u64(
            STYLE_DOMAIN,
            &[
                self.seed.as_bytes(),
                self.input_hash.as_bytes(),
                self.selector.provider_stable_id().as_str().as_bytes(),
                self.selector.implementation_fingerprint().as_bytes(),
                &cell_x.to_be_bytes(),
                &cell_z.to_be_bytes(),
            ],
        );
        if self.boreal.is_some() {
            match roll % 3 {
                0 => TerrainStyleV1::TemperateWoodland,
                1 => TerrainStyleV1::AridBadlands,
                _ => TerrainStyleV1::BorealWetland,
            }
        } else if roll & 1 == 0 {
            TerrainStyleV1::TemperateWoodland
        } else {
            TerrainStyleV1::AridBadlands
        }
    }

    fn raw_height(&self, style: TerrainStyleV1, x: i64, z: i64) -> i32 {
        let (base, relief, provider) = match style {
            TerrainStyleV1::TemperateWoodland => (
                self.config.temperate_base_height,
                self.config.temperate_relief,
                &self.temperate,
            ),
            TerrainStyleV1::AridBadlands => (
                self.config.arid_base_height,
                self.config.arid_relief,
                &self.arid,
            ),
            TerrainStyleV1::BorealWetland => {
                let boreal = self.boreal.as_ref();
                (
                    boreal.map_or(self.config.temperate_base_height, |params| {
                        params.base_height
                    }),
                    boreal.map_or(self.config.temperate_relief, |params| params.relief),
                    boreal.map_or(&self.temperate, |params| &params.provider),
                )
            }
        };
        let scale = i64::from(self.config.height_noise_scale_voxels);
        let grid_x = x.div_euclid(scale);
        let grid_z = z.div_euclid(scale);
        let local_x = x.rem_euclid(scale);
        let local_z = z.rem_euclid(scale);
        let n00 = self.height_corner(provider, style, grid_x, grid_z);
        let n10 = self.height_corner(provider, style, grid_x.saturating_add(1), grid_z);
        let n01 = self.height_corner(provider, style, grid_x, grid_z.saturating_add(1));
        let n11 = self.height_corner(
            provider,
            style,
            grid_x.saturating_add(1),
            grid_z.saturating_add(1),
        );
        let nx0 = lerp_fixed(n00, n10, local_x, scale);
        let nx1 = lerp_fixed(n01, n11, local_x, scale);
        let noise = lerp_fixed(nx0, nx1, local_z, scale);
        let displacement = noise.saturating_mul(i64::from(relief)).div_euclid(1_024);
        i32::try_from(i64::from(base).saturating_add(displacement)).unwrap_or(base)
    }

    fn height_corner(
        &self,
        provider: &ProviderGenerationIdentityV1,
        style: TerrainStyleV1,
        grid_x: i64,
        grid_z: i64,
    ) -> i64 {
        let style_byte = [style.discriminant()];
        let roll = hash_u64(
            HEIGHT_DOMAIN,
            &[
                self.seed.as_bytes(),
                self.input_hash.as_bytes(),
                provider.provider_stable_id().as_str().as_bytes(),
                provider.implementation_fingerprint().as_bytes(),
                &style_byte,
                &grid_x.to_be_bytes(),
                &grid_z.to_be_bytes(),
            ],
        );
        i64::try_from(roll % 2_049).unwrap_or_default() - 1_024
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

fn lerp_fixed(left: i64, right: i64, numerator: i64, denominator: i64) -> i64 {
    left.saturating_mul(denominator.saturating_sub(numerator))
        .saturating_add(right.saturating_mul(numerator))
        .div_euclid(denominator.max(1))
}
