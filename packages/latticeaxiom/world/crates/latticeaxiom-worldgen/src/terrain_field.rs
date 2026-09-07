//! Integer terrain-density fields for authoritative surface generation.
//!
//! The field graph follows the same separation used by Minecraft's Overworld
//! density router: broad continental shape, erosion, ridges, and local detail
//! are sampled independently and then composed. Low-frequency domain shifts
//! break stationary noise patterns, while a shared drainage field carves
//! valleys through uplift instead of treating erosion as uniform amplitude
//! loss. Gradient selection and quintic interpolation are informed by
//! `OpenSimplex2`, upstream revision
//! `4cd120d35bfc27096698de90d1bcbf4f9d359a3b` (CC0-1.0), but this is a
//! project-owned fixed-point implementation so authoritative output does not
//! depend on platform floating-point behavior.

use serde::{Deserialize, Serialize};

use crate::{
    TerrainConfigV2, WorldgenSeedRootV2,
    hashes::{hash_u64, sample_hash_2d},
};

const CONTINENT_V2_DOMAIN: &[u8] = b"latticeaxiom.terrain.continent.v2\0";
const ISLAND_V2_DOMAIN: &[u8] = b"latticeaxiom.terrain.island.v2\0";
const EROSION_V2_DOMAIN: &[u8] = b"latticeaxiom.terrain.erosion.v2\0";
const MOUNTAIN_V2_DOMAIN: &[u8] = b"latticeaxiom.terrain.mountain.v2\0";
const HILL_V2_DOMAIN: &[u8] = b"latticeaxiom.terrain.hill.v2\0";
const PLATEAU_V2_DOMAIN: &[u8] = b"latticeaxiom.terrain.plateau.v2\0";
const DETAIL_V2_DOMAIN: &[u8] = b"latticeaxiom.terrain.detail.v2\0";
const VOLCANO_V2_DOMAIN: &[u8] = b"latticeaxiom.terrain.volcano.v2\0";
const WETLAND_V2_DOMAIN: &[u8] = b"latticeaxiom.terrain.wetland.v2\0";
const LAKE_V2_DOMAIN: &[u8] = b"latticeaxiom.terrain.lake.v2\0";
const RELIEF_WARP_X_V3_DOMAIN: &[u8] = b"latticeaxiom.terrain.relief-warp-x.v3\0";
const RELIEF_WARP_Z_V3_DOMAIN: &[u8] = b"latticeaxiom.terrain.relief-warp-z.v3\0";
const DRAINAGE_V3_DOMAIN: &[u8] = b"latticeaxiom.terrain.drainage.v3\0";
const FIELD_UNIT: i64 = 1_024;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct DrainageSampleV3 {
    pub(crate) cell_x: i64,
    pub(crate) cell_z: i64,
    pub(crate) edge_voxels: u16,
    pub(crate) distance_voxels: u32,
}

/// Seeded continuous drainage potential shared by terrain carving and river water.
#[derive(Clone, Debug)]
pub(crate) struct DrainageFieldV3 {
    seed: u64,
    edge: i64,
}

impl DrainageFieldV3 {
    pub(crate) fn new(seed_root: WorldgenSeedRootV2, edge: u16) -> Self {
        Self {
            seed: hash_u64(DRAINAGE_V3_DOMAIN, &[seed_root.as_bytes()]),
            edge: i64::from(edge).max(1),
        }
    }

    pub(crate) fn sample(&self, x: i64, z: i64) -> DrainageSampleV3 {
        self.sample_scaled(x, z, 4)
    }

    /// Distance for connected channels before the legacy valley-width reduction.
    /// Legacy terrain fields retain their previous sampling convention.
    pub(crate) fn channel_sample(&self, x: i64, z: i64) -> DrainageSampleV3 {
        self.sample_scaled(x, z, 1)
    }

    fn sample_scaled(&self, x: i64, z: i64, width_scale: i64) -> DrainageSampleV3 {
        let potential = fractal_noise(self.seed, x, z, self.edge.saturating_mul(2), 3)
            .abs()
            .min(FIELD_UNIT);
        let distance = potential
            .saturating_mul(self.edge)
            .div_euclid(FIELD_UNIT.saturating_mul(width_scale));
        DrainageSampleV3 {
            cell_x: x.div_euclid(self.edge),
            cell_z: z.div_euclid(self.edge),
            edge_voxels: u16::try_from(self.edge).unwrap_or(u16::MAX),
            distance_voxels: u32::try_from(distance).unwrap_or(u32::MAX),
        }
    }
}

/// Shape family selected independently from climate and surface material.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum TerrainFamilyV2 {
    /// Deep ocean basin.
    DeepOcean,
    /// Shallow ocean shelf.
    ShallowOcean,
    /// Coast and beach transition.
    Coast,
    /// Low-relief inland plain.
    Plains,
    /// Smooth rolling hills.
    RollingHills,
    /// Elevated broad, flatter surface.
    Plateau,
    /// Continuous ridge-driven mountain system.
    MountainRange,
    /// Flat saturated lowland.
    Wetland,
    /// Closed inland basin with a stable standing-water level.
    LakeBasin,
    /// Rare high-relief volcanic uplift.
    Volcanic,
}

/// Allocation-free macro-terrain sample for one world column.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TerrainColumnSampleV2 {
    pub(crate) height: i32,
    pub(crate) family: TerrainFamilyV2,
    pub(crate) surface_water_y: Option<i32>,
    pub(crate) drainage: Option<DrainageSampleV3>,
}

impl TerrainColumnSampleV2 {
    pub(crate) const fn from_semantic(height: i32, family: TerrainFamilyV2) -> Self {
        Self {
            height,
            family,
            surface_water_y: None,
            drainage: None,
        }
    }

    pub(crate) const fn from_semantic_with_water(
        height: i32,
        family: TerrainFamilyV2,
        surface_water_y: Option<i32>,
    ) -> Self {
        Self {
            height,
            family,
            surface_water_y,
            drainage: None,
        }
    }

    /// Returns the river-adjusted solid surface height.
    #[must_use]
    pub const fn height(self) -> i32 {
        self.height
    }

    /// Returns the macro shape family independently from climate materials.
    #[must_use]
    pub const fn family(self) -> TerrainFamilyV2 {
        self.family
    }

    /// Returns the inclusive standing-water level of an inland lake basin.
    #[must_use]
    pub const fn surface_water_y(self) -> Option<i32> {
        self.surface_water_y
    }
}

#[derive(Clone, Copy, Debug)]
struct BaseTerrainColumnV2 {
    height: i64,
    family: TerrainFamilyV2,
    drainage: Option<DrainageSampleV3>,
}

#[derive(Clone, Copy, Debug)]
struct LakeBasinV2 {
    coverage_per_1024: i64,
    water_level_y: i64,
}

/// Compiled stable seeds and resolved parameters for macro terrain.
#[derive(Clone, Debug)]
pub(crate) struct TerrainFieldV2 {
    config: TerrainConfigV2,
    continent_seed: u64,
    island_seed: u64,
    erosion_seed: u64,
    mountain_seed: u64,
    hill_seed: u64,
    plateau_seed: u64,
    detail_seed: u64,
    volcano_seed: u64,
    wetland_seed: u64,
    lake_seed: u64,
    relief_warp_seeds: [u64; 2],
    drainage: DrainageFieldV3,
}

impl TerrainFieldV2 {
    pub(crate) fn new(seed_root: WorldgenSeedRootV2, config: TerrainConfigV2) -> Self {
        let seed = |domain| hash_u64(domain, &[seed_root.as_bytes()]);
        Self {
            config,
            continent_seed: seed(CONTINENT_V2_DOMAIN),
            island_seed: seed(ISLAND_V2_DOMAIN),
            erosion_seed: seed(EROSION_V2_DOMAIN),
            mountain_seed: seed(MOUNTAIN_V2_DOMAIN),
            hill_seed: seed(HILL_V2_DOMAIN),
            plateau_seed: seed(PLATEAU_V2_DOMAIN),
            detail_seed: seed(DETAIL_V2_DOMAIN),
            volcano_seed: seed(VOLCANO_V2_DOMAIN),
            wetland_seed: seed(WETLAND_V2_DOMAIN),
            lake_seed: seed(LAKE_V2_DOMAIN),
            relief_warp_seeds: [seed(RELIEF_WARP_X_V3_DOMAIN), seed(RELIEF_WARP_Z_V3_DOMAIN)],
            drainage: DrainageFieldV3::new(seed_root, config.water.river_spacing_voxels),
        }
    }

    pub(crate) fn sample(&self, x: i64, z: i64) -> TerrainColumnSampleV2 {
        let base = self.sample_base(x, z);
        let Some(lake) = self.lake_basin(x, z, base.height) else {
            return TerrainColumnSampleV2 {
                height: self.clamp_height(base.height),
                family: base.family,
                surface_water_y: None,
                drainage: base.drainage,
            };
        };
        let maximum_depth = i64::from(self.config.water.river_depth_voxels).saturating_add(4);
        let depth = 1_i64.saturating_add(
            maximum_depth
                .saturating_mul(lake.coverage_per_1024)
                .div_euclid(FIELD_UNIT),
        );
        let bed = base.height.min(lake.water_level_y.saturating_sub(depth));
        let height = self.clamp_height(bed);
        let surface_water_y = self
            .clamp_height(lake.water_level_y)
            .max(height.saturating_add(1));
        TerrainColumnSampleV2 {
            height,
            family: TerrainFamilyV2::LakeBasin,
            surface_water_y: Some(surface_water_y),
            drainage: base.drainage,
        }
    }

    pub(crate) fn is_land(&self, x: i64, z: i64) -> bool {
        self.landmass_value(x, z) >= 0
    }

    fn landmass_value(&self, x: i64, z: i64) -> i64 {
        let landmass = self.config.landmass;
        let continent = amplify(fractal_noise(
            self.continent_seed,
            x,
            z,
            i64::from(landmass.continent_scale_voxels),
            5,
        ));
        let island_noise = amplify(fractal_noise(
            self.island_seed,
            x,
            z,
            i64::from(landmass.island_scale_voxels),
            4,
        ));
        let island_peak = island_noise
            .saturating_sub(320)
            .max(0)
            .saturating_mul(i64::from(landmass.island_weight_per_1024))
            .div_euclid(FIELD_UNIT);
        continent
            .saturating_sub(i64::from(landmass.ocean_bias_per_1024))
            .saturating_add(island_peak)
            .clamp(-FIELD_UNIT, FIELD_UNIT)
    }

    fn sample_base(&self, x: i64, z: i64) -> BaseTerrainColumnV2 {
        let landmass = self.config.landmass;
        let relief = self.config.relief;
        let sea = i64::from(self.config.world.sea_level_y);
        // Gradient fractals spend most samples near zero. Expanding the two
        // land-mask fields before thresholding gives the presets meaningful
        // continental interiors and ocean basins without changing their
        // smooth interpolation or introducing hard cell boundaries.
        let land = self.landmass_value(x, z);
        let coast = i64::from(landmass.coast_width_per_1024).max(1);

        if land < 0 {
            return sample_ocean(landmass, sea, land, coast);
        }

        let inland = land
            .saturating_sub(coast)
            .max(0)
            .saturating_mul(FIELD_UNIT)
            .div_euclid(FIELD_UNIT.saturating_sub(coast).max(1))
            .clamp(0, FIELD_UNIT);
        let (relief_x, relief_z) = self.relief_coordinates(x, z);
        let erosion = normalized(fractal_noise(
            self.erosion_seed,
            relief_x,
            relief_z,
            i64::from(landmass.continent_scale_voxels)
                .div_euclid(3)
                .max(64),
            4,
        ));
        let erosion_loss = erosion
            .saturating_mul(i64::from(relief.erosion_strength_per_1024))
            .div_euclid(FIELD_UNIT);
        let preserved_relief = FIELD_UNIT
            .saturating_sub(erosion_loss)
            .clamp(96, FIELD_UNIT);

        let (mountain_mask, mountain_lift) =
            self.mountain_lift(relief_x, relief_z, inland, preserved_relief);
        let (plateau_mask, plateau_lift) = self.plateau_lift(relief_x, relief_z, inland);
        let regional_ruggedness =
            smoothstep_fixed(normalized_above(FIELD_UNIT.saturating_sub(erosion), 384))
                .max(mountain_mask)
                .max(plateau_mask.div_euclid(2));
        let (hill, hill_lift, detail) =
            self.hill_and_detail(relief_x, relief_z, preserved_relief, regional_ruggedness);
        let (volcano_mask, volcano_lift) = self.volcano_lift(relief_x, relief_z, inland);

        let coast_rise = land
            .min(coast)
            .saturating_mul(i64::from(relief.base_height_voxels))
            .div_euclid(coast);
        let continental_lift = inland
            .saturating_mul(i64::from(relief.continental_lift_voxels))
            .div_euclid(FIELD_UNIT);
        let uncarved_height = sea
            .saturating_add(coast_rise)
            .saturating_add(continental_lift)
            .saturating_add(hill_lift)
            .saturating_add(plateau_lift)
            .saturating_add(mountain_lift)
            .saturating_add(volcano_lift)
            .saturating_add(detail);
        let positive_relief = mountain_lift
            .saturating_add(plateau_lift)
            .saturating_add(hill_lift.max(0))
            .saturating_add(volcano_lift);
        let (valley_incision, drainage) = self.valley_incision(x, z, land.max(0), positive_relief);
        let height = uncarved_height.saturating_sub(valley_incision);
        let wetland_rank = normalized(fractal_noise(self.wetland_seed, relief_x, relief_z, 768, 3));
        let wetland = height <= sea.saturating_add(i64::from(relief.base_height_voxels) / 2)
            && wetland_rank
                < i64::from(self.config.water.wetland_amount_per_1024).clamp(0, FIELD_UNIT);
        let family = if volcano_mask > 256 {
            TerrainFamilyV2::Volcanic
        } else if mountain_mask > 192 {
            TerrainFamilyV2::MountainRange
        } else if plateau_mask > 256 {
            TerrainFamilyV2::Plateau
        } else if wetland {
            TerrainFamilyV2::Wetland
        } else if hill.abs() > 220 {
            TerrainFamilyV2::RollingHills
        } else if land <= coast {
            TerrainFamilyV2::Coast
        } else {
            TerrainFamilyV2::Plains
        };
        BaseTerrainColumnV2 {
            height,
            family,
            drainage: Some(drainage),
        }
    }

    fn relief_coordinates(&self, x: i64, z: i64) -> (i64, i64) {
        let mountain_scale = i64::from(self.config.relief.mountain_scale_voxels).max(64);
        let warp_scale = mountain_scale.saturating_mul(2);
        let warp_strength = mountain_scale.div_euclid(5).max(1);
        let shift_x = gradient_noise(self.relief_warp_seeds[0], x, z, warp_scale)
            .saturating_mul(warp_strength)
            .div_euclid(FIELD_UNIT);
        let shift_z = gradient_noise(self.relief_warp_seeds[1], x, z, warp_scale)
            .saturating_mul(warp_strength)
            .div_euclid(FIELD_UNIT);
        (x.saturating_add(shift_x), z.saturating_add(shift_z))
    }

    fn valley_incision(
        &self,
        x: i64,
        z: i64,
        land_weight: i64,
        positive_relief: i64,
    ) -> (i64, DrainageSampleV3) {
        let water = self.config.water;
        let spacing = i64::from(water.river_spacing_voxels).max(64);
        let valley_width = i64::from(water.river_width_voxels)
            .saturating_mul(2)
            .max(spacing.div_euclid(32))
            .max(1);
        let drainage = self.drainage.sample(x, z);
        let approximate_distance = i64::from(drainage.distance_voxels);
        if approximate_distance >= valley_width {
            return (0, drainage);
        }
        let linear_profile = valley_width
            .saturating_sub(approximate_distance)
            .saturating_mul(FIELD_UNIT)
            .div_euclid(valley_width);
        let profile = smoothstep_fixed(linear_profile);
        let erosion_coupling = positive_relief
            .saturating_mul(i64::from(self.config.relief.erosion_strength_per_1024))
            .div_euclid(FIELD_UNIT)
            .div_euclid(2);
        let maximum_incision = i64::from(water.river_depth_voxels)
            .saturating_mul(2)
            .saturating_add(erosion_coupling);
        let incision = maximum_incision
            .saturating_mul(profile)
            .div_euclid(FIELD_UNIT)
            .saturating_mul(land_weight.clamp(0, FIELD_UNIT))
            .div_euclid(FIELD_UNIT);
        (incision, drainage)
    }

    fn mountain_lift(&self, x: i64, z: i64, inland: i64, preserved: i64) -> (i64, i64) {
        let relief = self.config.relief;
        let ridge = FIELD_UNIT.saturating_sub(
            fractal_noise(
                self.mountain_seed,
                x,
                z,
                i64::from(relief.mountain_scale_voxels),
                4,
            )
            .abs()
            .min(FIELD_UNIT),
        );
        let threshold = FIELD_UNIT
            .saturating_sub(i64::from(relief.mountain_amount_per_1024))
            .clamp(0, FIELD_UNIT.saturating_sub(1));
        let mask = normalized_above(ridge, threshold)
            .saturating_mul(inland)
            .div_euclid(FIELD_UNIT);
        let lift = mask
            .saturating_mul(mask)
            .div_euclid(FIELD_UNIT)
            .saturating_mul(i64::from(relief.mountain_height_voxels))
            .div_euclid(FIELD_UNIT)
            .saturating_mul(preserved)
            .div_euclid(FIELD_UNIT);
        (mask, lift)
    }

    fn plateau_lift(&self, x: i64, z: i64, inland: i64) -> (i64, i64) {
        let relief = self.config.relief;
        let field = normalized(fractal_noise(
            self.plateau_seed,
            x,
            z,
            i64::from(relief.mountain_scale_voxels).saturating_mul(2),
            3,
        ));
        let threshold = FIELD_UNIT
            .saturating_sub(i64::from(relief.plateau_amount_per_1024))
            .clamp(0, FIELD_UNIT.saturating_sub(1));
        let mask = normalized_above(field, threshold)
            .saturating_mul(inland)
            .div_euclid(FIELD_UNIT);
        let lift = mask
            .saturating_mul(i64::from(relief.plateau_height_voxels))
            .div_euclid(FIELD_UNIT);
        (mask, lift)
    }

    fn hill_and_detail(
        &self,
        x: i64,
        z: i64,
        preserved: i64,
        regional_ruggedness: i64,
    ) -> (i64, i64, i64) {
        if regional_ruggedness == 0 {
            return (0, 0, 0);
        }
        let relief = self.config.relief;
        let hill = fractal_noise(
            self.hill_seed,
            x,
            z,
            i64::from(relief.mountain_scale_voxels)
                .div_euclid(5)
                .max(64),
            4,
        );
        let hill_lift = hill
            .saturating_mul(i64::from(relief.hill_height_voxels))
            .div_euclid(FIELD_UNIT)
            .saturating_mul(preserved)
            .div_euclid(FIELD_UNIT)
            .saturating_mul(regional_ruggedness)
            .div_euclid(FIELD_UNIT);
        let detail_scale = i64::from(relief.mountain_scale_voxels)
            .div_euclid(16)
            .clamp(64, 192);
        let detail_amplitude = i64::from(relief.hill_height_voxels)
            .div_euclid(2)
            .saturating_add(8);
        let detail = fractal_noise(self.detail_seed, x, z, detail_scale, 3)
            .saturating_mul(i64::from(relief.roughness_per_1024))
            .div_euclid(FIELD_UNIT)
            .saturating_mul(detail_amplitude)
            .div_euclid(FIELD_UNIT)
            .saturating_mul(regional_ruggedness)
            .div_euclid(FIELD_UNIT);
        (
            hill.saturating_mul(regional_ruggedness)
                .div_euclid(FIELD_UNIT),
            hill_lift,
            detail,
        )
    }

    fn volcano_lift(&self, x: i64, z: i64, inland: i64) -> (i64, i64) {
        let relief = self.config.relief;
        let field = normalized(fractal_noise(
            self.volcano_seed,
            x,
            z,
            i64::from(relief.mountain_scale_voxels).saturating_mul(2),
            3,
        ));
        let threshold = FIELD_UNIT
            .saturating_sub(i64::from(relief.volcano_amount_per_1024))
            .clamp(0, FIELD_UNIT.saturating_sub(1));
        let mask = normalized_above(field, threshold)
            .saturating_mul(inland)
            .div_euclid(FIELD_UNIT);
        let lift = mask
            .saturating_mul(mask)
            .div_euclid(FIELD_UNIT)
            .saturating_mul(i64::from(relief.mountain_height_voxels))
            .div_euclid(FIELD_UNIT);
        (mask, lift)
    }

    fn lake_basin(&self, x: i64, z: i64, local_height: i64) -> Option<LakeBasinV2> {
        let amount = u64::from(self.config.water.lake_amount_per_1024);
        if amount == 0 {
            return None;
        }
        let edge = i64::from(self.config.water.river_spacing_voxels)
            .saturating_mul(2)
            .max(128);
        let cell_x = x.div_euclid(edge);
        let cell_z = z.div_euclid(edge);
        let mut selected: Option<LakeBasinV2> = None;
        for candidate_z in cell_z.saturating_sub(1)..=cell_z.saturating_add(1) {
            for candidate_x in cell_x.saturating_sub(1)..=cell_x.saturating_add(1) {
                let rank = sample_hash_2d(self.lake_seed, candidate_x, candidate_z);
                if rank % 1_024 >= amount {
                    continue;
                }
                let jitter_span = edge.div_euclid(2).max(1);
                let center_x = candidate_x
                    .saturating_mul(edge)
                    .saturating_add(edge.div_euclid(4))
                    .saturating_add(
                        i64::try_from((rank >> 10) % u64::try_from(jitter_span).unwrap_or(1))
                            .unwrap_or_default(),
                    );
                let center_z = candidate_z
                    .saturating_mul(edge)
                    .saturating_add(edge.div_euclid(4))
                    .saturating_add(
                        i64::try_from((rank >> 32) % u64::try_from(jitter_span).unwrap_or(1))
                            .unwrap_or_default(),
                    );
                let radius_span = edge.div_euclid(8).max(1);
                let radius = edge.div_euclid(8).saturating_add(
                    i64::try_from((rank >> 48) % u64::try_from(radius_span).unwrap_or(1))
                        .unwrap_or_default(),
                );
                let delta_x = x.saturating_sub(center_x).abs();
                let delta_z = z.saturating_sub(center_z).abs();
                let distance = delta_x
                    .max(delta_z)
                    .saturating_add(delta_x.min(delta_z).div_euclid(2));
                if distance >= radius {
                    continue;
                }
                let anchor = self.sample_base(center_x, center_z);
                if anchor.height <= i64::from(self.config.world.sea_level_y).saturating_add(4)
                    || matches!(
                        anchor.family,
                        TerrainFamilyV2::MountainRange | TerrainFamilyV2::Volcanic
                    )
                {
                    continue;
                }
                let water_level_y = anchor.height.saturating_sub(1).div_euclid(4) * 4;
                if local_height > water_level_y.saturating_add(3)
                    || local_height < water_level_y.saturating_sub(12)
                {
                    continue;
                }
                let coverage_per_1024 = radius
                    .saturating_sub(distance)
                    .saturating_mul(FIELD_UNIT)
                    .div_euclid(radius.max(1));
                let basin = LakeBasinV2 {
                    coverage_per_1024,
                    water_level_y,
                };
                if selected
                    .is_none_or(|current| basin.coverage_per_1024 > current.coverage_per_1024)
                {
                    selected = Some(basin);
                }
            }
        }
        selected
    }

    fn clamp_height(&self, height: i64) -> i32 {
        let minimum = i64::from(self.config.world.floor_y).saturating_add(1);
        let maximum = i64::from(self.config.world.ceiling_y).saturating_sub(8);
        i32::try_from(height.clamp(minimum, maximum)).unwrap_or(self.config.world.sea_level_y)
    }
}

fn sample_ocean(
    landmass: crate::LandmassConfigV2,
    sea: i64,
    land: i64,
    coast: i64,
) -> BaseTerrainColumnV2 {
    let depth_factor = land
        .saturating_neg()
        .saturating_mul(FIELD_UNIT)
        .div_euclid(FIELD_UNIT)
        .clamp(0, FIELD_UNIT);
    let maximum_depth = i64::from(landmass.ocean_depth_voxels);
    let depth = 2_i64.saturating_add(
        maximum_depth
            .saturating_mul(depth_factor)
            .div_euclid(FIELD_UNIT),
    );
    let height = sea.saturating_sub(depth);
    let family = if depth > maximum_depth.div_euclid(2).max(1) {
        TerrainFamilyV2::DeepOcean
    } else if land > coast.saturating_neg() {
        TerrainFamilyV2::Coast
    } else {
        TerrainFamilyV2::ShallowOcean
    };
    BaseTerrainColumnV2 {
        height,
        family,
        drainage: None,
    }
}

fn normalized(value: i64) -> i64 {
    value
        .saturating_add(FIELD_UNIT)
        .div_euclid(2)
        .clamp(0, FIELD_UNIT)
}

fn amplify(value: i64) -> i64 {
    value.saturating_mul(2).clamp(-FIELD_UNIT, FIELD_UNIT)
}

fn normalized_above(value: i64, threshold: i64) -> i64 {
    value
        .saturating_sub(threshold)
        .saturating_mul(FIELD_UNIT)
        .div_euclid(FIELD_UNIT.saturating_sub(threshold).max(1))
        .clamp(0, FIELD_UNIT)
}

fn smoothstep_fixed(value: i64) -> i64 {
    let value = value.clamp(0, FIELD_UNIT);
    value
        .saturating_mul(value)
        .div_euclid(FIELD_UNIT)
        .saturating_mul(
            FIELD_UNIT
                .saturating_mul(3)
                .saturating_sub(value.saturating_mul(2)),
        )
        .div_euclid(FIELD_UNIT)
}

const UNIT: i64 = 1_024;
const DIAGONAL: i64 = 724;

const DETAIL_SALT: u64 = 0x9e37_79b9_7f4a_7c15;

/// Samples a low-frequency correlated climate field over planning-cell coordinates.
pub(crate) fn climate_field(seed: u64, cell_x: i64, cell_z: i64, scale_cells: u16) -> i64 {
    gradient_noise(seed, cell_x, cell_z, i64::from(scale_cells.max(1)))
}

fn fractal_noise(seed: u64, x: i64, z: i64, scale: i64, octaves: u8) -> i64 {
    let mut weighted = 0_i64;
    let mut total_weight = 0_i64;
    let mut octave_scale = scale.max(1);
    let mut weight = 1_i64 << octaves.saturating_sub(1);
    for octave in 0..octaves {
        weighted = weighted.saturating_add(
            gradient_noise(
                seed ^ u64::from(octave).wrapping_mul(DETAIL_SALT),
                x,
                z,
                octave_scale,
            )
            .saturating_mul(weight),
        );
        total_weight = total_weight.saturating_add(weight);
        octave_scale = octave_scale.saturating_div(2).max(1);
        weight = weight.saturating_div(2).max(1);
    }
    weighted.div_euclid(total_weight.max(1)).clamp(-UNIT, UNIT)
}

fn gradient_noise(seed: u64, x: i64, z: i64, scale: i64) -> i64 {
    let scale = scale.max(1);
    let cell_x = x.div_euclid(scale);
    let cell_z = z.div_euclid(scale);
    let local_x = x.rem_euclid(scale).saturating_mul(UNIT).div_euclid(scale);
    let local_z = z.rem_euclid(scale).saturating_mul(UNIT).div_euclid(scale);
    let fade_x = quintic_fade(local_x);
    let fade_z = quintic_fade(local_z);

    let south_west = gradient_dot(seed, cell_x, cell_z, local_x, local_z);
    let south_east = gradient_dot(
        seed,
        cell_x.saturating_add(1),
        cell_z,
        local_x.saturating_sub(UNIT),
        local_z,
    );
    let north_west = gradient_dot(
        seed,
        cell_x,
        cell_z.saturating_add(1),
        local_x,
        local_z.saturating_sub(UNIT),
    );
    let north_east = gradient_dot(
        seed,
        cell_x.saturating_add(1),
        cell_z.saturating_add(1),
        local_x.saturating_sub(UNIT),
        local_z.saturating_sub(UNIT),
    );
    let south = lerp_fixed(south_west, south_east, fade_x);
    let north = lerp_fixed(north_west, north_east, fade_x);
    lerp_fixed(south, north, fade_z).clamp(-UNIT, UNIT)
}

fn gradient_dot(seed: u64, cell_x: i64, cell_z: i64, dx: i64, dz: i64) -> i64 {
    let (gradient_x, gradient_z) = match sample_hash_2d(seed, cell_x, cell_z) & 7 {
        0 => (UNIT, 0),
        1 => (-UNIT, 0),
        2 => (0, UNIT),
        3 => (0, -UNIT),
        4 => (DIAGONAL, DIAGONAL),
        5 => (-DIAGONAL, DIAGONAL),
        6 => (DIAGONAL, -DIAGONAL),
        _ => (-DIAGONAL, -DIAGONAL),
    };
    gradient_x
        .saturating_mul(dx)
        .saturating_add(gradient_z.saturating_mul(dz))
        .div_euclid(UNIT)
}

fn quintic_fade(value: i64) -> i64 {
    let square = multiply_fixed(value, value);
    let cube = multiply_fixed(square, value);
    let fourth = multiply_fixed(cube, value);
    let fifth = multiply_fixed(fourth, value);
    fifth
        .saturating_mul(6)
        .saturating_sub(fourth.saturating_mul(15))
        .saturating_add(cube.saturating_mul(10))
        .clamp(0, UNIT)
}

fn multiply_fixed(left: i64, right: i64) -> i64 {
    left.saturating_mul(right).div_euclid(UNIT)
}

fn lerp_fixed(left: i64, right: i64, amount: i64) -> i64 {
    left.saturating_add(multiply_fixed(right.saturating_sub(left), amount))
}

#[cfg(test)]
mod tests {
    use super::{
        TerrainFamilyV2, TerrainFieldV2, UNIT, climate_field, gradient_noise, quintic_fade,
    };
    use crate::{TerrainConfigV2, WorldSeedV1, WorldgenSeedRootV2};
    use latticeaxiom_core::CanonicalHash;

    #[derive(Debug, Eq, PartialEq)]
    struct LegacyTerrainEvidence {
        map_hash: String,
        drainage_hash: String,
        minimum_height: i32,
        maximum_height: i32,
        land_columns: u64,
        ocean_columns: u64,
        lake_columns: u64,
        plateau_steps: u64,
        axis_energy_x: u128,
        axis_energy_z: u128,
        diagonal_energy_northeast: u128,
        diagonal_energy_southeast: u128,
        drainage_center_columns: u64,
    }

    #[test]
    fn quintic_fade_has_exact_endpoints() {
        assert_eq!(quintic_fade(0), 0);
        assert_eq!(quintic_fade(UNIT), UNIT);
        assert_eq!(quintic_fade(UNIT / 2), UNIT / 2);
    }

    #[test]
    fn gradient_noise_is_continuous_across_signed_lattice_edges() {
        let seed = 0x5eed_5eed_d15c_a11e;
        for boundary in -8_i64..=8 {
            let x = boundary.saturating_mul(16);
            let left = gradient_noise(seed, x.saturating_sub(1), -7, 16);
            let center = gradient_noise(seed, x, -7, 16);
            let right = gradient_noise(seed, x.saturating_add(1), -7, 16);
            assert!((center - left).abs() <= 128, "left edge jump at {x}");
            assert!((right - center).abs() <= 128, "right edge jump at {x}");
        }
    }

    #[test]
    fn climate_field_correlates_neighboring_planning_cells() {
        let mut minimum = UNIT;
        let mut maximum = -UNIT;
        let mut maximum_step = 0_i64;
        for z in -64_i64..=64 {
            for x in -64_i64..=64 {
                let sample = climate_field(0xc11a_7e42, x, z, 8);
                minimum = minimum.min(sample);
                maximum = maximum.max(sample);
                maximum_step = maximum_step
                    .max((sample - climate_field(0xc11a_7e42, x + 1, z, 8)).abs())
                    .max((sample - climate_field(0xc11a_7e42, x, z + 1, 8)).abs());
            }
        }
        assert!(minimum < -256);
        assert!(maximum > 256);
        assert!(
            maximum_step <= 512,
            "climate step is too abrupt: {maximum_step}"
        );
    }

    #[test]
    fn balanced_macro_field_contains_land_ocean_and_mountain_systems() {
        let config = TerrainConfigV2::representative_test_baseline();
        let field = TerrainFieldV2::new(
            WorldgenSeedRootV2::from_world_seed(WorldSeedV1::from_integer(42)),
            config,
        );
        let mut families = std::collections::BTreeSet::new();
        let mut lake_columns = 0_u32;
        let mut minimum = config.world.ceiling_y;
        let mut maximum = config.world.floor_y;
        for z in (-16_384_i64..=16_384).step_by(128) {
            for x in (-16_384_i64..=16_384).step_by(128) {
                let sample = field.sample(x, z);
                families.insert(sample.family);
                lake_columns = lake_columns.saturating_add(u32::from(
                    sample
                        .surface_water_y
                        .is_some_and(|water| water > sample.height),
                ));
                minimum = minimum.min(sample.height);
                maximum = maximum.max(sample.height);
            }
        }
        assert!(minimum < config.world.sea_level_y);
        assert!(
            maximum > config.world.sea_level_y + 96,
            "balanced maximum {maximum}, families {families:?}"
        );
        assert!(families.contains(&TerrainFamilyV2::DeepOcean));
        assert!(families.contains(&TerrainFamilyV2::Plains));
        assert!(families.contains(&TerrainFamilyV2::MountainRange));
        assert!(families.contains(&TerrainFamilyV2::LakeBasin));
        assert!(lake_columns > 0);
    }

    #[test]
    fn balanced_field_separates_quiet_plains_from_rugged_regions() {
        let config = TerrainConfigV2::representative_test_baseline();
        let field = TerrainFieldV2::new(
            WorldgenSeedRootV2::from_world_seed(WorldSeedV1::from_integer(42)),
            config,
        );
        let mut land_columns = 0_u32;
        let mut quiet_columns = 0_u32;
        let mut rugged_columns = 0_u32;
        for z in (-16_384_i64..=16_384).step_by(128) {
            for x in (-16_384_i64..=16_384).step_by(128) {
                let center = field.sample(x, z);
                if matches!(
                    center.family,
                    TerrainFamilyV2::DeepOcean | TerrainFamilyV2::ShallowOcean
                ) {
                    continue;
                }
                let heights = [
                    center.height,
                    field.sample(x.saturating_sub(64), z).height,
                    field.sample(x.saturating_add(64), z).height,
                    field.sample(x, z.saturating_sub(64)).height,
                    field.sample(x, z.saturating_add(64)).height,
                ];
                let minimum = heights.into_iter().min().unwrap_or(center.height);
                let maximum = heights.into_iter().max().unwrap_or(center.height);
                let local_relief = maximum.saturating_sub(minimum);
                land_columns = land_columns.saturating_add(1);
                quiet_columns = quiet_columns.saturating_add(u32::from(local_relief <= 4));
                rugged_columns = rugged_columns.saturating_add(u32::from(local_relief >= 12));
            }
        }
        assert!(
            quiet_columns > land_columns / 5,
            "quiet plains {quiet_columns}/{land_columns}"
        );
        assert!(
            rugged_columns > land_columns / 100,
            "rugged regions {rugged_columns}/{land_columns}"
        );
    }

    #[test]
    fn drainage_potential_is_lipschitz_and_contains_channel_centers() {
        let field = super::DrainageFieldV3::new(
            WorldgenSeedRootV2::from_world_seed(WorldSeedV1::from_integer(42)),
            384,
        );
        let mut minimum = u32::MAX;
        let mut maximum = 0_u32;
        for z in (-2_048_i64..=2_048).step_by(32) {
            for x in (-2_048_i64..=2_048).step_by(32) {
                let here = field.sample(x, z).distance_voxels;
                let east = field.sample(x.saturating_add(1), z).distance_voxels;
                let south = field.sample(x, z.saturating_add(1)).distance_voxels;
                minimum = minimum.min(here);
                maximum = maximum.max(here);
                assert!(here.abs_diff(east) <= 1, "X step at ({x}, {z})");
                assert!(here.abs_diff(south) <= 1, "Z step at ({x}, {z})");
            }
        }
        assert!(minimum <= 1, "minimum drainage distance {minimum}");
        assert!(maximum >= 32, "maximum drainage distance {maximum}");
    }

    #[test]
    fn legacy_fixed_seed_map_morphology_and_directional_energy_are_frozen() {
        let actual = legacy_terrain_evidence();
        let expected = LegacyTerrainEvidence {
            map_hash: "4b48cf41fb05290798cc8931c839ae63a29d15b01698f20403cd9f5b681f1236".to_owned(),
            drainage_hash: "bf28bc2d11783b8e3140a24eba6c86eda83594ec1039c00e8895943e8966760a"
                .to_owned(),
            minimum_height: -1,
            maximum_height: 168,
            land_columns: 48_579,
            ocean_columns: 17_470,
            lake_columns: 319,
            plateau_steps: 33_704,
            axis_energy_x: 215_370,
            axis_energy_z: 210_911,
            diagonal_energy_northeast: 384_855,
            diagonal_energy_southeast: 355_511,
            drainage_center_columns: 34_139,
        };
        assert_eq!(actual, expected);
    }

    fn legacy_terrain_evidence() -> LegacyTerrainEvidence {
        const EDGE: usize = 257;
        const ORIGIN: i64 = -8_192;
        const SPACING: i64 = 64;

        let config = TerrainConfigV2::representative_test_baseline();
        let field = TerrainFieldV2::new(
            WorldgenSeedRootV2::from_world_seed(WorldSeedV1::from_integer(42)),
            config,
        );
        let mut heights = vec![0_i32; EDGE * EDGE];
        let mut map_bytes = Vec::with_capacity(EDGE * EDGE * 5);
        let mut drainage_bytes = Vec::with_capacity(EDGE * EDGE * 4);
        let mut minimum_height = i32::MAX;
        let mut maximum_height = i32::MIN;
        let mut land_columns = 0_u64;
        let mut ocean_columns = 0_u64;
        let mut lake_columns = 0_u64;
        let mut drainage_center_columns = 0_u64;

        for z_index in 0..EDGE {
            for x_index in 0..EDGE {
                let x = ORIGIN.saturating_add(
                    i64::try_from(x_index)
                        .unwrap_or(i64::MAX)
                        .saturating_mul(SPACING),
                );
                let z = ORIGIN.saturating_add(
                    i64::try_from(z_index)
                        .unwrap_or(i64::MAX)
                        .saturating_mul(SPACING),
                );
                let sample = field.sample(x, z);
                heights[z_index * EDGE + x_index] = sample.height;
                map_bytes.extend_from_slice(&sample.height.to_le_bytes());
                map_bytes.push(family_tag(sample.family));
                minimum_height = minimum_height.min(sample.height);
                maximum_height = maximum_height.max(sample.height);
                if matches!(
                    sample.family,
                    TerrainFamilyV2::DeepOcean | TerrainFamilyV2::ShallowOcean
                ) {
                    ocean_columns = ocean_columns.saturating_add(1);
                } else {
                    land_columns = land_columns.saturating_add(1);
                }
                lake_columns = lake_columns.saturating_add(u64::from(
                    sample
                        .surface_water_y
                        .is_some_and(|level| level > sample.height),
                ));
                let drainage = field.drainage.sample(x, z).distance_voxels;
                drainage_bytes.extend_from_slice(&drainage.to_le_bytes());
                drainage_center_columns = drainage_center_columns.saturating_add(u64::from(
                    drainage <= u32::from(config.water.river_width_voxels),
                ));
            }
        }

        let mut plateau_steps = 0_u64;
        let mut axis_energy_x = 0_u128;
        let mut axis_energy_z = 0_u128;
        let mut diagonal_energy_northeast = 0_u128;
        let mut diagonal_energy_southeast = 0_u128;
        for z in 0..EDGE - 1 {
            for x in 0..EDGE - 1 {
                let center = heights[z * EDGE + x];
                let east = heights[z * EDGE + x + 1];
                let south = heights[(z + 1) * EDGE + x];
                let southeast = heights[(z + 1) * EDGE + x + 1];
                let dx = i128::from(east) - i128::from(center);
                let dz = i128::from(south) - i128::from(center);
                let dne = i128::from(east) - i128::from(south);
                let dse = i128::from(southeast) - i128::from(center);
                plateau_steps = plateau_steps
                    .saturating_add(u64::from(dx == 0))
                    .saturating_add(u64::from(dz == 0));
                axis_energy_x = axis_energy_x.saturating_add(dx.unsigned_abs().pow(2));
                axis_energy_z = axis_energy_z.saturating_add(dz.unsigned_abs().pow(2));
                diagonal_energy_northeast =
                    diagonal_energy_northeast.saturating_add(dne.unsigned_abs().pow(2));
                diagonal_energy_southeast =
                    diagonal_energy_southeast.saturating_add(dse.unsigned_abs().pow(2));
            }
        }

        LegacyTerrainEvidence {
            map_hash: CanonicalHash::digest(map_bytes).to_string(),
            drainage_hash: CanonicalHash::digest(drainage_bytes).to_string(),
            minimum_height,
            maximum_height,
            land_columns,
            ocean_columns,
            lake_columns,
            plateau_steps,
            axis_energy_x,
            axis_energy_z,
            diagonal_energy_northeast,
            diagonal_energy_southeast,
            drainage_center_columns,
        }
    }

    const fn family_tag(family: TerrainFamilyV2) -> u8 {
        match family {
            TerrainFamilyV2::DeepOcean => 0,
            TerrainFamilyV2::ShallowOcean => 1,
            TerrainFamilyV2::Coast => 2,
            TerrainFamilyV2::Plains => 3,
            TerrainFamilyV2::RollingHills => 4,
            TerrainFamilyV2::Plateau => 5,
            TerrainFamilyV2::MountainRange => 6,
            TerrainFamilyV2::Wetland => 7,
            TerrainFamilyV2::LakeBasin => 8,
            TerrainFamilyV2::Volcanic => 9,
        }
    }
}
