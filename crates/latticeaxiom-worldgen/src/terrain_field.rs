//! Integer terrain-density fields for authoritative surface generation.
//!
//! The field graph follows the same separation used by Minecraft's Overworld
//! density router: broad continental shape, erosion, ridges, and local detail
//! are sampled independently and then composed. Gradient selection and
//! quintic interpolation are informed by `OpenSimplex2`, upstream revision
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
const FIELD_UNIT: i64 = 1_024;

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
}

impl TerrainColumnSampleV2 {
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
        }
    }

    pub(crate) fn sample(&self, x: i64, z: i64) -> TerrainColumnSampleV2 {
        let base = self.sample_base(x, z);
        let Some(lake) = self.lake_basin(x, z, base.height) else {
            return TerrainColumnSampleV2 {
                height: self.clamp_height(base.height),
                family: base.family,
                surface_water_y: None,
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
        let erosion = normalized(fractal_noise(
            self.erosion_seed,
            x,
            z,
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

        let (mountain_mask, mountain_lift) = self.mountain_lift(x, z, inland, preserved_relief);
        let (plateau_mask, plateau_lift) = self.plateau_lift(x, z, inland);
        let (hill, hill_lift, detail) = self.hill_and_detail(x, z, preserved_relief);
        let (volcano_mask, volcano_lift) = self.volcano_lift(x, z, inland);

        let coast_rise = land
            .min(coast)
            .saturating_mul(i64::from(relief.base_height_voxels))
            .div_euclid(coast);
        let continental_lift = inland
            .saturating_mul(i64::from(relief.continental_lift_voxels))
            .div_euclid(FIELD_UNIT);
        let height = sea
            .saturating_add(coast_rise)
            .saturating_add(continental_lift)
            .saturating_add(hill_lift)
            .saturating_add(plateau_lift)
            .saturating_add(mountain_lift)
            .saturating_add(volcano_lift)
            .saturating_add(detail);
        let wetland_rank = normalized(fractal_noise(self.wetland_seed, x, z, 768, 3));
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
        BaseTerrainColumnV2 { height, family }
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

    fn hill_and_detail(&self, x: i64, z: i64, preserved: i64) -> (i64, i64, i64) {
        let relief = self.config.relief;
        let hill = fractal_noise(
            self.hill_seed,
            x,
            z,
            i64::from(relief.mountain_scale_voxels)
                .div_euclid(3)
                .max(64),
            4,
        );
        let hill_lift = hill
            .saturating_mul(i64::from(relief.hill_height_voxels))
            .div_euclid(FIELD_UNIT)
            .saturating_mul(preserved)
            .div_euclid(FIELD_UNIT);
        let detail = fractal_noise(self.detail_seed, x, z, 96, 3)
            .saturating_mul(i64::from(relief.roughness_per_1024))
            .div_euclid(FIELD_UNIT)
            .saturating_mul(8)
            .div_euclid(FIELD_UNIT);
        (hill, hill_lift, detail)
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
    BaseTerrainColumnV2 { height, family }
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
}
