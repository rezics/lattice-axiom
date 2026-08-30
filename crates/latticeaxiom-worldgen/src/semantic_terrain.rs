//! Closed semantic terrain fields, splines, hydrology constraints, and density composition.

use std::num::NonZeroU32;

use latticeaxiom_core::{CanonicalHash, StableId, canonical_json_bytes};
use latticeaxiom_storage::DimensionId;
use serde::{Deserialize, Serialize};

use crate::{
    BoundaryAdapterDeclarationV1, GenerationEpochIdV1, HydrologicBoundaryEdgeV1,
    HydrologicBoundaryPortV1, HydrologicDomainConfigV1, HydrologicDomainGridV1,
    HydrologicDomainInputV1, HydrologicDomainPlanV1, HydrologicTopologyConfigV1,
    HydrologicTopologyPlanV1, SemanticTerrainPlanHashV1, SemanticTerrainPolicyHashV1,
    TerrainBoundaryAdapterHashV1, TerrainColumnSampleV2, TerrainConfigV2, TerrainFamilyV2,
    WorldgenError, WorldgenResult, WorldgenSeedRootV2, build_hydrologic_topology_v1,
    hashes::domain_hash, open_simplex_2f_3d_v1, open_simplex_2s_2d_v1, plan_hydrologic_domain_v1,
};
use crate::{FixedCoordinateV1, FixedFieldSampleV1};

const SEMANTIC_UNIT: i64 = 1_024;
const MAX_SPLINE_POINTS: usize = 32;
const MAX_FIELD_SCALE: u32 = 1_048_576;
const MAX_SPLINE_OUTPUT: i32 = 1_048_576;
const SEMANTIC_TERRAIN_ALGORITHM: &str = "latticeaxiom:terrain/semantic-hydrology-density@1";

const CONTINENT_DOMAIN: &[u8] = b"latticeaxiom.semantic.continentalness.v1\0";
const UPLIFT_DOMAIN: &[u8] = b"latticeaxiom.semantic.uplift.v1\0";
const LITHOLOGY_DOMAIN: &[u8] = b"latticeaxiom.semantic.lithology.v1\0";
const TEMPERATURE_DOMAIN: &[u8] = b"latticeaxiom.semantic.temperature.v1\0";
const PRECIPITATION_DOMAIN: &[u8] = b"latticeaxiom.semantic.precipitation.v1\0";
const INFILTRATION_DOMAIN: &[u8] = b"latticeaxiom.semantic.infiltration.v1\0";
const DETAIL_DOMAIN: &[u8] = b"latticeaxiom.semantic.detail.v1\0";
const TERRAIN_VOLUME_DOMAIN: &[u8] = b"latticeaxiom.semantic.terrain-volume.v1\0";
const GEOLOGIC_VOLUME_DOMAIN: &[u8] = b"latticeaxiom.semantic.geologic-volume.v1\0";

/// One bounded, independently seeded semantic field frequency and amplitude.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SemanticFieldSpecV1 {
    scale_voxels: NonZeroU32,
    amplitude_per_1024: u16,
}

impl SemanticFieldSpecV1 {
    /// Creates one field specification. Policy validation closes its bounds.
    #[must_use]
    pub const fn new(scale_voxels: NonZeroU32, amplitude_per_1024: u16) -> Self {
        Self {
            scale_voxels,
            amplitude_per_1024,
        }
    }

    /// Returns the field wavelength in voxels.
    #[must_use]
    pub const fn scale_voxels(self) -> NonZeroU32 {
        self.scale_voxels
    }

    /// Returns the normalized output amplitude.
    #[must_use]
    pub const fn amplitude_per_1024(self) -> u16 {
        self.amplitude_per_1024
    }

    fn validate(self, field: &'static str) -> WorldgenResult<()> {
        if self.scale_voxels.get() > MAX_FIELD_SCALE {
            return invalid(field, format!("scale exceeds {MAX_FIELD_SCALE}"));
        }
        if self.amplitude_per_1024 > 1_024 {
            return invalid(field, "amplitude must be in 0..=1024");
        }
        Ok(())
    }
}

/// One exact control point in a closed fixed-point spline.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ClosedSplinePointV1 {
    input_per_1024: i16,
    output_per_1024: i32,
}

impl ClosedSplinePointV1 {
    /// Creates one normalized control point.
    #[must_use]
    pub const fn new(input_per_1024: i16, output_per_1024: i32) -> Self {
        Self {
            input_per_1024,
            output_per_1024,
        }
    }

    /// Returns the normalized input coordinate.
    #[must_use]
    pub const fn input_per_1024(self) -> i16 {
        self.input_per_1024
    }

    /// Returns the normalized output value.
    #[must_use]
    pub const fn output_per_1024(self) -> i32 {
        self.output_per_1024
    }
}

/// Version-one closed linear spline with explicit endpoints and clamping.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ClosedSplineV1 {
    points: Vec<ClosedSplinePointV1>,
}

impl ClosedSplineV1 {
    /// Creates and validates a closed spline.
    ///
    /// # Errors
    ///
    /// Returns an invalid-config error unless there are 2..=32 strictly
    /// ordered points whose endpoints are exactly -1024 and 1024.
    pub fn new(points: Vec<ClosedSplinePointV1>) -> WorldgenResult<Self> {
        let spline = Self { points };
        spline.validate()?;
        Ok(spline)
    }

    /// Returns exact control points in ascending input order.
    #[must_use]
    pub fn points(&self) -> &[ClosedSplinePointV1] {
        &self.points
    }

    /// Evaluates with clamped endpoints and round-to-nearest linear segments.
    ///
    /// # Errors
    ///
    /// Returns an invalid-config error if deserialized spline data is not
    /// closed and canonical.
    pub fn evaluate(&self, input_per_1024: i32) -> WorldgenResult<i32> {
        self.validate()?;
        let input = input_per_1024.clamp(-1_024, 1_024);
        let upper = self
            .points
            .partition_point(|point| i32::from(point.input_per_1024) < input);
        if upper == 0 {
            return Ok(self.points[0].output_per_1024);
        }
        if upper == self.points.len() {
            return Ok(self.points[self.points.len() - 1].output_per_1024);
        }
        let left = self.points[upper - 1];
        let right = self.points[upper];
        let span = i64::from(right.input_per_1024) - i64::from(left.input_per_1024);
        let offset = i64::from(input) - i64::from(left.input_per_1024);
        let delta = i64::from(right.output_per_1024) - i64::from(left.output_per_1024);
        let interpolated = i64::from(left.output_per_1024)
            .saturating_add(round_divide_i64(delta.saturating_mul(offset), span));
        i32::try_from(interpolated).map_err(|_| WorldgenError::ArithmeticOverflow {
            operation: "closed semantic spline interpolation",
        })
    }

    fn validate(&self) -> WorldgenResult<()> {
        if !(2..=MAX_SPLINE_POINTS).contains(&self.points.len()) {
            return invalid("semantic.spline.points", "point count must be in 2..=32");
        }
        if self.points.first().map(|point| point.input_per_1024) != Some(-1_024)
            || self.points.last().map(|point| point.input_per_1024) != Some(1_024)
        {
            return invalid(
                "semantic.spline.endpoints",
                "closed spline endpoints must be exactly -1024 and 1024",
            );
        }
        if self
            .points
            .windows(2)
            .any(|pair| pair[0].input_per_1024 >= pair[1].input_per_1024)
        {
            return invalid(
                "semantic.spline.order",
                "spline inputs must be strictly increasing",
            );
        }
        if self
            .points
            .iter()
            .any(|point| point.output_per_1024.unsigned_abs() > MAX_SPLINE_OUTPUT as u32)
        {
            return invalid(
                "semantic.spline.output",
                "spline output exceeds the closed fixed-point envelope",
            );
        }
        Ok(())
    }
}

/// Package-authored semantic field, spline, density, and protection policy.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SemanticTerrainPolicyV1 {
    continentalness: SemanticFieldSpecV1,
    uplift: SemanticFieldSpecV1,
    lithology: SemanticFieldSpecV1,
    temperature: SemanticFieldSpecV1,
    precipitation: SemanticFieldSpecV1,
    infiltration: SemanticFieldSpecV1,
    detail: SemanticFieldSpecV1,
    terrain_volume: SemanticFieldSpecV1,
    geologic_volume: SemanticFieldSpecV1,
    continental_height: ClosedSplineV1,
    uplift_relief: ClosedSplineV1,
    roughness: ClosedSplineV1,
    cliff_tendency: ClosedSplineV1,
    max_detail_displacement_q8: u16,
    max_terrain_volume_q8: u16,
    max_geologic_volume_q8: u16,
    water_protection_radius_q8: u16,
}

impl SemanticTerrainPolicyV1 {
    /// Creates a complete semantic-terrain policy.
    ///
    /// # Errors
    ///
    /// Returns an invalid-config error for an open spline, excessive field
    /// scale/amplitude, or zero/excessive density envelope.
    #[allow(
        clippy::too_many_arguments,
        reason = "the constructor closes every output-affecting semantic policy field"
    )]
    pub fn new(
        continentalness: SemanticFieldSpecV1,
        uplift: SemanticFieldSpecV1,
        lithology: SemanticFieldSpecV1,
        temperature: SemanticFieldSpecV1,
        precipitation: SemanticFieldSpecV1,
        infiltration: SemanticFieldSpecV1,
        detail: SemanticFieldSpecV1,
        terrain_volume: SemanticFieldSpecV1,
        geologic_volume: SemanticFieldSpecV1,
        continental_height: ClosedSplineV1,
        uplift_relief: ClosedSplineV1,
        roughness: ClosedSplineV1,
        cliff_tendency: ClosedSplineV1,
        max_detail_displacement_q8: u16,
        max_terrain_volume_q8: u16,
        max_geologic_volume_q8: u16,
        water_protection_radius_q8: u16,
    ) -> WorldgenResult<Self> {
        let policy = Self {
            continentalness,
            uplift,
            lithology,
            temperature,
            precipitation,
            infiltration,
            detail,
            terrain_volume,
            geologic_volume,
            continental_height,
            uplift_relief,
            roughness,
            cliff_tendency,
            max_detail_displacement_q8,
            max_terrain_volume_q8,
            max_geologic_volume_q8,
            water_protection_radius_q8,
        };
        policy.validate()?;
        Ok(policy)
    }

    /// Returns the canonical policy hash included in provider provenance.
    ///
    /// # Errors
    ///
    /// Returns an invalid-policy or canonical-encoding error.
    pub fn canonical_hash(&self) -> WorldgenResult<SemanticTerrainPolicyHashV1> {
        self.validate()?;
        let bytes =
            canonical_json_bytes(self).map_err(|error| WorldgenError::CanonicalEncoding {
                kind: "SemanticTerrainPolicyV1",
                reason: error.to_string(),
            })?;
        Ok(SemanticTerrainPolicyHashV1::from_hash(domain_hash(
            b"latticeaxiom.semantic-terrain.policy.v1\0",
            &[&bytes],
        )))
    }

    /// Returns the continentalness field contract used for land ownership.
    #[must_use]
    pub const fn continentalness(&self) -> SemanticFieldSpecV1 {
        self.continentalness
    }

    /// Returns the maximum protected-water distance in Q8 voxels.
    #[must_use]
    pub const fn water_protection_radius_q8(&self) -> u16 {
        self.water_protection_radius_q8
    }

    fn validate(&self) -> WorldgenResult<()> {
        for (field, spec) in [
            ("semantic.continentalness", self.continentalness),
            ("semantic.uplift", self.uplift),
            ("semantic.lithology", self.lithology),
            ("semantic.temperature", self.temperature),
            ("semantic.precipitation", self.precipitation),
            ("semantic.infiltration", self.infiltration),
            ("semantic.detail", self.detail),
            ("semantic.terrain_volume", self.terrain_volume),
            ("semantic.geologic_volume", self.geologic_volume),
        ] {
            spec.validate(field)?;
        }
        self.continental_height.validate()?;
        self.uplift_relief.validate()?;
        self.roughness.validate()?;
        self.cliff_tendency.validate()?;
        if self.max_detail_displacement_q8 == 0
            || self.max_terrain_volume_q8 == 0
            || self.max_geologic_volume_q8 == 0
            || self.water_protection_radius_q8 == 0
        {
            return invalid(
                "semantic.density_envelopes",
                "detail, volume, geology, and water envelopes must be nonzero",
            );
        }
        Ok(())
    }
}

/// Named low-frequency semantic values and their initial surface result.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SemanticFieldSampleV1 {
    continentalness_per_1024: i16,
    uplift_per_1024: i16,
    lithology_per_1024: i16,
    temperature_per_1024: i16,
    precipitation_per_1024: i16,
    infiltration_per_1024: i16,
    effective_runoff_q16: u32,
    initial_surface_y_q8: i32,
    detail_displacement_q8: i32,
    family: TerrainFamilyV2,
}

impl SemanticFieldSampleV1 {
    /// Returns signed continentalness in normalized units.
    #[must_use]
    pub const fn continentalness_per_1024(self) -> i16 {
        self.continentalness_per_1024
    }

    /// Returns signed uplift in normalized units.
    #[must_use]
    pub const fn uplift_per_1024(self) -> i16 {
        self.uplift_per_1024
    }

    /// Returns signed lithology/erodibility control in normalized units.
    #[must_use]
    pub const fn lithology_per_1024(self) -> i16 {
        self.lithology_per_1024
    }

    /// Returns signed temperature in normalized units.
    #[must_use]
    pub const fn temperature_per_1024(self) -> i16 {
        self.temperature_per_1024
    }

    /// Returns signed precipitation in normalized units.
    #[must_use]
    pub const fn precipitation_per_1024(self) -> i16 {
        self.precipitation_per_1024
    }

    /// Returns signed infiltration in normalized units.
    #[must_use]
    pub const fn infiltration_per_1024(self) -> i16 {
        self.infiltration_per_1024
    }

    /// Returns bounded local effective runoff in Q16.
    #[must_use]
    pub const fn effective_runoff_q16(self) -> u32 {
        self.effective_runoff_q16
    }

    /// Returns the semantic initial DEM height in Q24.8 voxels.
    #[must_use]
    pub const fn initial_surface_y_q8(self) -> i32 {
        self.initial_surface_y_q8
    }

    /// Returns the post-macro bounded detail displacement in Q8 voxels.
    #[must_use]
    pub const fn detail_displacement_q8(self) -> i32 {
        self.detail_displacement_q8
    }

    /// Returns the macro terrain family.
    #[must_use]
    pub const fn family(self) -> TerrainFamilyV2 {
        self.family
    }
}

/// Auditable contributions to one territory-owned three-dimensional density.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SemanticDensitySampleV1 {
    base_surface_density_q8: i32,
    terrain_family_volume_q8: i32,
    geologic_volume_q8: i32,
    protected_water: bool,
    final_density_q8: i32,
}

impl SemanticDensitySampleV1 {
    /// Returns the final signed density; positive values are solid.
    #[must_use]
    pub const fn final_density_q8(self) -> i32 {
        self.final_density_q8
    }

    /// Returns whether hard water/river protection suppressed positive volume.
    #[must_use]
    pub const fn protected_water(self) -> bool {
        self.protected_water
    }

    /// Returns the bounded terrain-family volume contribution.
    #[must_use]
    pub const fn terrain_family_volume_q8(self) -> i32 {
        self.terrain_family_volume_q8
    }

    /// Returns the bounded geologic volume contribution.
    #[must_use]
    pub const fn geologic_volume_q8(self) -> i32 {
        self.geologic_volume_q8
    }
}

/// Compiled deterministic semantic field and density evaluator.
#[derive(Clone, Debug)]
pub struct SemanticTerrainFieldV1 {
    terrain: TerrainConfigV2,
    policy: SemanticTerrainPolicyV1,
    seeds: SemanticSeeds,
}

#[derive(Clone, Copy, Debug)]
struct SemanticSeeds {
    continentalness: i64,
    uplift: i64,
    lithology: i64,
    temperature: i64,
    precipitation: i64,
    infiltration: i64,
    detail: i64,
    terrain_volume: i64,
    geologic_volume: i64,
}

impl SemanticTerrainFieldV1 {
    /// Compiles independent seed domains and validates the closed policy.
    ///
    /// # Errors
    ///
    /// Returns an invalid-policy error for malformed deserialized policy data.
    pub fn new(
        seed_root: WorldgenSeedRootV2,
        terrain: TerrainConfigV2,
        policy: SemanticTerrainPolicyV1,
    ) -> WorldgenResult<Self> {
        policy.validate()?;
        let seed = |domain| semantic_seed(seed_root, domain);
        Ok(Self {
            terrain,
            policy,
            seeds: SemanticSeeds {
                continentalness: seed(CONTINENT_DOMAIN),
                uplift: seed(UPLIFT_DOMAIN),
                lithology: seed(LITHOLOGY_DOMAIN),
                temperature: seed(TEMPERATURE_DOMAIN),
                precipitation: seed(PRECIPITATION_DOMAIN),
                infiltration: seed(INFILTRATION_DOMAIN),
                detail: seed(DETAIL_DOMAIN),
                terrain_volume: seed(TERRAIN_VOLUME_DOMAIN),
                geologic_volume: seed(GEOLOGIC_VOLUME_DOMAIN),
            },
        })
    }

    /// Returns the compiled package policy.
    #[must_use]
    pub const fn policy(&self) -> &SemanticTerrainPolicyV1 {
        &self.policy
    }

    /// Samples all named fields and the initial semantic DEM.
    ///
    /// # Errors
    ///
    /// Returns a fixed-field coordinate or arithmetic error outside the
    /// supported world coordinate envelope.
    #[allow(
        clippy::too_many_lines,
        reason = "one auditable pass samples independent fields and applies the closed spline/detail envelope"
    )]
    pub fn sample(&self, world_x: i64, world_z: i64) -> WorldgenResult<SemanticFieldSampleV1> {
        let continentalness = sample_2d(
            self.seeds.continentalness,
            self.policy.continentalness,
            world_x,
            world_z,
        )?;
        let uplift = sample_2d(self.seeds.uplift, self.policy.uplift, world_x, world_z)?;
        let lithology = sample_2d(
            self.seeds.lithology,
            self.policy.lithology,
            world_x,
            world_z,
        )?;
        let temperature = sample_2d(
            self.seeds.temperature,
            self.policy.temperature,
            world_x,
            world_z,
        )?;
        let precipitation = sample_2d(
            self.seeds.precipitation,
            self.policy.precipitation,
            world_x,
            world_z,
        )?;
        let infiltration = sample_2d(
            self.seeds.infiltration,
            self.policy.infiltration,
            world_x,
            world_z,
        )?;
        let detail = sample_2d(self.seeds.detail, self.policy.detail, world_x, world_z)?;
        let base_curve = self
            .policy
            .continental_height
            .evaluate(i32::from(continentalness))?;
        let uplift_curve = self.policy.uplift_relief.evaluate(i32::from(uplift))?;
        let roughness = self.policy.roughness.evaluate(i32::from(lithology))?;
        let sea_q8 = i64::from(self.terrain.world.sea_level_y).saturating_mul(256);
        let macro_q8 = if continentalness < 0 {
            sea_q8.saturating_add(
                i64::from(self.terrain.landmass.ocean_depth_voxels)
                    .saturating_mul(256)
                    .saturating_mul(i64::from(base_curve))
                    .div_euclid(SEMANTIC_UNIT),
            )
        } else {
            sea_q8
                .saturating_add(i64::from(self.terrain.relief.base_height_voxels) * 256)
                .saturating_add(
                    i64::from(self.terrain.relief.continental_lift_voxels)
                        .saturating_mul(256)
                        .saturating_mul(i64::from(base_curve))
                        .div_euclid(SEMANTIC_UNIT),
                )
                .saturating_add(
                    i64::from(self.terrain.relief.mountain_height_voxels)
                        .saturating_mul(256)
                        .saturating_mul(i64::from(uplift_curve.max(0)))
                        .div_euclid(SEMANTIC_UNIT),
                )
        };
        let coast_width = i64::from(self.terrain.landmass.coast_width_per_1024).max(1);
        let coast_mask = i64::from(continentalness)
            .unsigned_abs()
            .saturating_mul(1_024)
            .div_euclid(coast_width.cast_unsigned())
            .min(1_024)
            .cast_signed();
        let detail_q8 = i64::from(detail)
            .saturating_mul(i64::from(self.policy.max_detail_displacement_q8))
            .saturating_mul(i64::from(roughness.clamp(0, 1_024)))
            .saturating_mul(coast_mask)
            .div_euclid(SEMANTIC_UNIT * SEMANTIC_UNIT * SEMANTIC_UNIT);
        let minimum_q8 = i64::from(self.terrain.world.floor_y)
            .saturating_add(1)
            .saturating_mul(256);
        let maximum_q8 = i64::from(self.terrain.world.ceiling_y)
            .saturating_sub(16)
            .saturating_mul(256);
        let initial_surface_y_q8 = i32::try_from(
            macro_q8
                .saturating_add(detail_q8)
                .clamp(minimum_q8, maximum_q8),
        )
        .map_err(|_| WorldgenError::ArithmeticOverflow {
            operation: "semantic terrain surface",
        })?;
        let effective_runoff_q16 = effective_runoff(precipitation, infiltration);
        let family = semantic_family(continentalness, uplift, lithology);
        Ok(SemanticFieldSampleV1 {
            continentalness_per_1024: continentalness,
            uplift_per_1024: uplift,
            lithology_per_1024: lithology,
            temperature_per_1024: temperature,
            precipitation_per_1024: precipitation,
            infiltration_per_1024: infiltration,
            effective_runoff_q16,
            initial_surface_y_q8,
            detail_displacement_q8: i32::try_from(detail_q8).unwrap_or(
                if detail_q8.is_negative() {
                    i32::MIN
                } else {
                    i32::MAX
                },
            ),
            family,
        })
    }

    /// Samples the compatibility column shape without random lake/river blobs.
    ///
    /// # Errors
    ///
    /// Returns the same coordinate and arithmetic errors as [`Self::sample`].
    pub fn terrain_column(
        &self,
        world_x: i64,
        world_z: i64,
    ) -> WorldgenResult<TerrainColumnSampleV2> {
        let sample = self.sample(world_x, world_z)?;
        Ok(TerrainColumnSampleV2::from_semantic(
            sample.initial_surface_y_q8.div_euclid(256),
            sample.family,
        ))
    }

    /// Composes bounded 3D territory and geology volumes around an approved surface.
    ///
    /// Positive volume is suppressed when `protected_water` is true, so local
    /// detail cannot fill an approved river, lake, coast, or portal corridor.
    ///
    /// # Errors
    ///
    /// Returns a fixed-field coordinate or arithmetic error outside the
    /// supported world coordinate envelope.
    pub fn density(
        &self,
        world_x: i64,
        world_y: i64,
        world_z: i64,
        approved_surface_y: i32,
        protected_water: bool,
    ) -> WorldgenResult<SemanticDensitySampleV1> {
        let semantic = self.sample(world_x, world_z)?;
        let cliff = self
            .policy
            .cliff_tendency
            .evaluate(i32::from(semantic.uplift_per_1024))?
            .clamp(0, 1_024);
        let terrain_noise = sample_3d(
            self.seeds.terrain_volume,
            self.policy.terrain_volume,
            world_x,
            world_y,
            world_z,
        )?;
        let geology_noise = sample_3d(
            self.seeds.geologic_volume,
            self.policy.geologic_volume,
            world_x,
            world_y,
            world_z,
        )?;
        let terrain_volume = i64::from(terrain_noise)
            .saturating_mul(i64::from(self.policy.max_terrain_volume_q8))
            .saturating_mul(i64::from(cliff))
            .div_euclid(SEMANTIC_UNIT * SEMANTIC_UNIT);
        let geology_volume = i64::from(geology_noise)
            .saturating_mul(i64::from(self.policy.max_geologic_volume_q8))
            .div_euclid(SEMANTIC_UNIT);
        let base = i64::from(approved_surface_y)
            .saturating_sub(world_y)
            .saturating_mul(256);
        let unconstrained = base
            .saturating_add(terrain_volume)
            .saturating_add(geology_volume);
        let final_density = if protected_water {
            unconstrained.min(base)
        } else {
            unconstrained
        };
        Ok(SemanticDensitySampleV1 {
            base_surface_density_q8: clamp_i64_i32(base),
            terrain_family_volume_q8: clamp_i64_i32(terrain_volume),
            geologic_volume_q8: clamp_i64_i32(geology_volume),
            protected_water,
            final_density_q8: clamp_i64_i32(final_density),
        })
    }

    /// Returns whether the macro ownership field selects land.
    ///
    /// # Errors
    ///
    /// Returns a fixed-field coordinate error outside the supported envelope.
    pub fn is_land(&self, world_x: i64, world_z: i64) -> WorldgenResult<bool> {
        Ok(self.sample(world_x, world_z)?.continentalness_per_1024 >= 0)
    }
}

/// Complete bounded input used to build one semantic hydrologic terrain plan.
#[derive(Clone, Debug)]
pub struct SemanticHydrologicTerrainInputV1 {
    dimension: DimensionId,
    seed_root: WorldgenSeedRootV2,
    generation_epoch: GenerationEpochIdV1,
    domain_x: i64,
    domain_z: i64,
    parent_domain: Option<crate::HydrologicDomainIdV1>,
    provenance: CanonicalHash,
    grid: HydrologicDomainGridV1,
    base_level_q8: i32,
    ports: Vec<HydrologicBoundaryPortV1>,
    terrain: TerrainConfigV2,
    policy: SemanticTerrainPolicyV1,
    domain_config: HydrologicDomainConfigV1,
    topology_config: HydrologicTopologyConfigV1,
}

impl SemanticHydrologicTerrainInputV1 {
    /// Creates one finite semantic/hydrology planning request.
    #[allow(
        clippy::too_many_arguments,
        reason = "the input closes identity, geometry, policy, ports, and both bounded algorithm configs"
    )]
    #[must_use]
    pub fn new(
        dimension: DimensionId,
        seed_root: WorldgenSeedRootV2,
        generation_epoch: GenerationEpochIdV1,
        domain_x: i64,
        domain_z: i64,
        parent_domain: Option<crate::HydrologicDomainIdV1>,
        provenance: CanonicalHash,
        grid: HydrologicDomainGridV1,
        base_level_q8: i32,
        ports: Vec<HydrologicBoundaryPortV1>,
        terrain: TerrainConfigV2,
        policy: SemanticTerrainPolicyV1,
        domain_config: HydrologicDomainConfigV1,
        topology_config: HydrologicTopologyConfigV1,
    ) -> Self {
        Self {
            dimension,
            seed_root,
            generation_epoch,
            domain_x,
            domain_z,
            parent_domain,
            provenance,
            grid,
            base_level_q8,
            ports,
            terrain,
            policy,
            domain_config,
            topology_config,
        }
    }
}

/// Immutable semantic DEM, hydrologic domain, and connected topology artifact.
#[derive(Clone, Debug)]
pub struct SemanticHydrologicTerrainPlanV1 {
    algorithm: &'static str,
    policy_hash: SemanticTerrainPolicyHashV1,
    domain: HydrologicDomainPlanV1,
    topology: HydrologicTopologyPlanV1,
}

impl SemanticHydrologicTerrainPlanV1 {
    /// Returns the semantic policy hash.
    #[must_use]
    pub const fn policy_hash(&self) -> SemanticTerrainPolicyHashV1 {
        self.policy_hash
    }

    /// Returns the immutable hydrologic-domain plan.
    #[must_use]
    pub const fn domain(&self) -> &HydrologicDomainPlanV1 {
        &self.domain
    }

    /// Returns the connected river/lake topology.
    #[must_use]
    pub const fn topology(&self) -> &HydrologicTopologyPlanV1 {
        &self.topology
    }

    /// Returns stable bytes for the persisted semantic-plan boundary.
    ///
    /// # Errors
    ///
    /// Returns a canonical-encoding error if a nested artifact cannot encode.
    pub fn canonical_bytes(&self) -> WorldgenResult<Vec<u8>> {
        #[derive(Serialize)]
        struct CanonicalPlan<'a> {
            algorithm: &'static str,
            policy_hash: SemanticTerrainPolicyHashV1,
            domain: &'a HydrologicDomainPlanV1,
            topology: &'a HydrologicTopologyPlanV1,
        }
        canonical_json_bytes(&CanonicalPlan {
            algorithm: self.algorithm,
            policy_hash: self.policy_hash,
            domain: &self.domain,
            topology: &self.topology,
        })
        .map_err(|error| WorldgenError::CanonicalEncoding {
            kind: "SemanticHydrologicTerrainPlanV1",
            reason: error.to_string(),
        })
    }

    /// Returns the canonical semantic terrain plan hash.
    ///
    /// # Errors
    ///
    /// Returns a canonical-encoding error if the plan cannot encode.
    pub fn canonical_hash(&self) -> WorldgenResult<SemanticTerrainPlanHashV1> {
        Ok(SemanticTerrainPlanHashV1::from_hash(domain_hash(
            b"latticeaxiom.semantic-terrain.plan.v1\0",
            &[&self.canonical_bytes()?],
        )))
    }
}

/// Builds semantic macro fields, an initial DEM, runoff, and connected water topology.
///
/// # Errors
///
/// Returns policy, field-coordinate, domain, topology, arithmetic, or hard-budget errors.
pub fn build_semantic_hydrologic_terrain_plan_v1(
    input: SemanticHydrologicTerrainInputV1,
) -> WorldgenResult<SemanticHydrologicTerrainPlanV1> {
    let policy_hash = input.policy.canonical_hash()?;
    let field = SemanticTerrainFieldV1::new(input.seed_root, input.terrain, input.policy)?;
    let mut elevations = Vec::with_capacity(input.grid.sample_count());
    let mut runoff = Vec::with_capacity(input.grid.sample_count());
    for index in 0..input.grid.sample_count() {
        let coordinate = input.grid.coordinate_of(index).ok_or_else(|| {
            WorldgenError::InvalidHydrologicDomain {
                field: "semantic.grid",
                reason: "row-major index lies outside semantic grid".to_owned(),
            }
        })?;
        let (world_x, world_z) = input.grid.world_coordinate(coordinate)?;
        let sample = field.sample(world_x, world_z)?;
        elevations.push(sample.initial_surface_y_q8);
        runoff.push(sample.effective_runoff_q16);
    }
    let domain_input = HydrologicDomainInputV1::new(
        input.dimension,
        input.generation_epoch,
        input.domain_x,
        input.domain_z,
        input.parent_domain,
        input.provenance,
        input.grid,
        input.base_level_q8,
        elevations,
        runoff,
        input.ports,
        input.domain_config,
    )?;
    let domain = plan_hydrologic_domain_v1(&domain_input)?;
    let topology = build_hydrologic_topology_v1(&domain, &input.topology_config)?;
    Ok(SemanticHydrologicTerrainPlanV1 {
        algorithm: SEMANTIC_TERRAIN_ALGORITHM,
        policy_hash,
        domain,
        topology,
    })
}

/// Runtime sampler binding semantic density to one immutable hydrologic plan.
#[derive(Clone, Debug)]
pub struct HydrologyConstrainedTerrainSamplerV1 {
    field: SemanticTerrainFieldV1,
    plan: SemanticHydrologicTerrainPlanV1,
}

impl HydrologyConstrainedTerrainSamplerV1 {
    /// Binds an exact policy/field to its semantic plan.
    ///
    /// # Errors
    ///
    /// Returns a mismatch error if policy hashes differ.
    pub fn new(
        seed_root: WorldgenSeedRootV2,
        terrain: TerrainConfigV2,
        policy: SemanticTerrainPolicyV1,
        plan: SemanticHydrologicTerrainPlanV1,
    ) -> WorldgenResult<Self> {
        if policy.canonical_hash()? != plan.policy_hash {
            return invalid(
                "semantic.plan.policy_hash",
                "sampler policy differs from the planned semantic DEM",
            );
        }
        Ok(Self {
            field: SemanticTerrainFieldV1::new(seed_root, terrain, policy)?,
            plan,
        })
    }

    /// Returns the immutable semantic/hydrologic source plan.
    #[must_use]
    pub const fn plan(&self) -> &SemanticHydrologicTerrainPlanV1 {
        &self.plan
    }

    /// Returns whether a world column maps to this finite domain artifact.
    #[must_use]
    pub fn contains_column(&self, world_x: i64, world_z: i64) -> bool {
        self.plan
            .domain
            .grid()
            .nearest_coordinate(world_x, world_z)
            .is_some()
    }

    /// Samples the approved hydrology-constrained compatibility column.
    ///
    /// # Errors
    ///
    /// Returns a field, query-domain, or topology error.
    pub fn terrain_column(
        &self,
        world_x: i64,
        world_z: i64,
    ) -> WorldgenResult<TerrainColumnSampleV2> {
        let semantic = self.field.sample(world_x, world_z)?;
        let domain = &self.plan.domain;
        let topology = &self.plan.topology;
        let coordinate = domain.grid().nearest_coordinate(world_x, world_z);
        let planned_surface_q8 = coordinate
            .and_then(|coordinate| domain.grid().index_of(coordinate))
            .map_or(semantic.initial_surface_y_q8, |index| {
                domain.initial_elevation_q8()[index]
            });
        let water = topology.static_water_column(domain, world_x, world_z)?;
        let bed_q8 = water.map_or(planned_surface_q8, crate::StaticWaterColumnV1::bed_y_q8);
        Ok(TerrainColumnSampleV2::from_semantic_with_water(
            bed_q8.div_euclid(256),
            semantic.family,
            water.map(|column| column.surface_y_q8().div_euclid(256)),
        ))
    }

    /// Samples bounded density with hard river/lake/ocean protection.
    ///
    /// # Errors
    ///
    /// Returns a field, query-domain, topology, or arithmetic error.
    pub fn density(
        &self,
        world_x: i64,
        world_y: i64,
        world_z: i64,
    ) -> WorldgenResult<SemanticDensitySampleV1> {
        let column = self.terrain_column(world_x, world_z)?;
        let river = self
            .plan
            .topology
            .river_sdf_sample(&self.plan.domain, world_x, world_z)?;
        let protected = river.is_some_and(|sample| {
            sample.signed_distance_q8() <= i64::from(self.field.policy.water_protection_radius_q8())
        }) || column.surface_water_y().is_some();
        self.field
            .density(world_x, world_y, world_z, column.height(), protected)
    }
}

/// Direction-independent measured evidence for joining legacy and semantic epochs.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TerrainBoundaryAdapterEvidenceV1 {
    edge: HydrologicBoundaryEdgeV1,
    epoch_a: GenerationEpochIdV1,
    epoch_b: GenerationEpochIdV1,
    signature_a: CanonicalHash,
    signature_b: CanonicalHash,
    sample_count: u16,
    max_absolute_delta_q8: u32,
    adapter_hash: TerrainBoundaryAdapterHashV1,
}

impl TerrainBoundaryAdapterEvidenceV1 {
    /// Measures and hashes two same-length boundary profiles.
    ///
    /// # Errors
    ///
    /// Returns an invalid-config error for equal epochs or profile lengths
    /// outside 2..=4096.
    pub fn measure(
        edge: HydrologicBoundaryEdgeV1,
        first_epoch: GenerationEpochIdV1,
        first_profile_q8: &[i32],
        second_epoch: GenerationEpochIdV1,
        second_profile_q8: &[i32],
    ) -> WorldgenResult<Self> {
        if first_epoch == second_epoch {
            return invalid("semantic.boundary.epochs", "epochs must differ");
        }
        if first_profile_q8.len() != second_profile_q8.len()
            || !(2..=4_096).contains(&first_profile_q8.len())
        {
            return invalid(
                "semantic.boundary.samples",
                "profiles must have the same length in 2..=4096",
            );
        }
        let edge_byte = [boundary_edge_tag(edge)];
        let first_bytes = canonical_json_bytes(first_profile_q8).map_err(|error| {
            WorldgenError::CanonicalEncoding {
                kind: "legacy terrain boundary profile",
                reason: error.to_string(),
            }
        })?;
        let second_bytes = canonical_json_bytes(second_profile_q8).map_err(|error| {
            WorldgenError::CanonicalEncoding {
                kind: "semantic terrain boundary profile",
                reason: error.to_string(),
            }
        })?;
        let first_signature = domain_hash(
            b"latticeaxiom.terrain-boundary.profile.v1\0",
            &[first_epoch.as_bytes(), &edge_byte, &first_bytes],
        );
        let second_signature = domain_hash(
            b"latticeaxiom.terrain-boundary.profile.v1\0",
            &[second_epoch.as_bytes(), &edge_byte, &second_bytes],
        );
        let ((epoch_a, signature_a), (epoch_b, signature_b)) = if first_epoch < second_epoch {
            (
                (first_epoch, first_signature),
                (second_epoch, second_signature),
            )
        } else {
            (
                (second_epoch, second_signature),
                (first_epoch, first_signature),
            )
        };
        let max_absolute_delta = first_profile_q8
            .iter()
            .zip(second_profile_q8)
            .map(|(&first, &second)| i64::from(first).abs_diff(i64::from(second)))
            .max()
            .unwrap_or_default()
            .min(u64::from(u32::MAX));
        let max_absolute_delta_q8 = u32::try_from(max_absolute_delta).unwrap_or(u32::MAX);
        let sample_count = u16::try_from(first_profile_q8.len()).map_err(|_| {
            WorldgenError::ArithmeticOverflow {
                operation: "terrain boundary sample count",
            }
        })?;
        let adapter_hash = TerrainBoundaryAdapterHashV1::from_hash(domain_hash(
            b"latticeaxiom.terrain-boundary.adapter.v1\0",
            &[
                &edge_byte,
                epoch_a.as_bytes(),
                signature_a.as_bytes(),
                epoch_b.as_bytes(),
                signature_b.as_bytes(),
                &sample_count.to_be_bytes(),
                &max_absolute_delta_q8.to_be_bytes(),
            ],
        ));
        Ok(Self {
            edge,
            epoch_a,
            epoch_b,
            signature_a,
            signature_b,
            sample_count,
            max_absolute_delta_q8,
            adapter_hash,
        })
    }

    /// Returns the direction-independent adapter evidence hash.
    #[must_use]
    pub const fn adapter_hash(&self) -> TerrainBoundaryAdapterHashV1 {
        self.adapter_hash
    }

    /// Returns the measured maximum profile delta in Q8 voxels.
    #[must_use]
    pub const fn max_absolute_delta_q8(&self) -> u32 {
        self.max_absolute_delta_q8
    }

    /// Creates the existing epoch-boundary declaration consumed by publication.
    ///
    /// # Errors
    ///
    /// Returns an invalid declaration error for an invalid adapter identity,
    /// transition width, epoch pair, or portal set.
    pub fn declaration(
        &self,
        adapter_id: StableId,
        transition_width: NonZeroU32,
        required_cave_portals: Vec<CanonicalHash>,
    ) -> WorldgenResult<BoundaryAdapterDeclarationV1> {
        BoundaryAdapterDeclarationV1::new(
            adapter_id,
            NonZeroU32::MIN,
            *self.adapter_hash.as_hash(),
            self.epoch_a,
            self.epoch_b,
            transition_width,
            domain_hash(
                b"latticeaxiom.terrain-boundary.join-signature.v1\0",
                &[self.signature_a.as_bytes(), self.signature_b.as_bytes()],
            ),
            required_cave_portals,
        )
    }
}

fn sample_2d(
    seed: i64,
    spec: SemanticFieldSpecV1,
    world_x: i64,
    world_z: i64,
) -> WorldgenResult<i16> {
    let x = FixedCoordinateV1::from_voxel(world_x, spec.scale_voxels)?;
    let z = FixedCoordinateV1::from_voxel(world_z, spec.scale_voxels)?;
    let sample = open_simplex_2s_2d_v1(seed, x, z)?;
    normalized_sample(sample, spec.amplitude_per_1024)
}

fn sample_3d(
    seed: i64,
    spec: SemanticFieldSpecV1,
    world_x: i64,
    world_y: i64,
    world_z: i64,
) -> WorldgenResult<i16> {
    let x = FixedCoordinateV1::from_voxel(world_x, spec.scale_voxels)?;
    let y = FixedCoordinateV1::from_voxel(world_y, spec.scale_voxels)?;
    let z = FixedCoordinateV1::from_voxel(world_z, spec.scale_voxels)?;
    let sample = open_simplex_2f_3d_v1(seed, x, y, z)?;
    normalized_sample(sample, spec.amplitude_per_1024)
}

fn normalized_sample(sample: FixedFieldSampleV1, amplitude: u16) -> WorldgenResult<i16> {
    let scaled = round_divide_i64(
        i64::from(sample.raw_q30()).saturating_mul(i64::from(amplitude)),
        1_i64 << 30,
    )
    .clamp(-1_024, 1_024);
    i16::try_from(scaled).map_err(|_| WorldgenError::ArithmeticOverflow {
        operation: "semantic normalized field",
    })
}

fn effective_runoff(precipitation: i16, infiltration: i16) -> u32 {
    let available = i64::from(precipitation)
        .saturating_add(1_024)
        .clamp(0, 2_048);
    let retained = i64::from(infiltration)
        .saturating_add(1_024)
        .clamp(0, 2_048);
    let runoff = available
        .saturating_mul(2_048_i64.saturating_sub(retained))
        .saturating_mul(65_536)
        .div_euclid(2_048 * 2_048)
        .max(256);
    u32::try_from(runoff).unwrap_or(u32::MAX)
}

fn semantic_family(continentalness: i16, uplift: i16, lithology: i16) -> TerrainFamilyV2 {
    if continentalness < -512 {
        TerrainFamilyV2::DeepOcean
    } else if continentalness < -96 {
        TerrainFamilyV2::ShallowOcean
    } else if continentalness < 96 {
        TerrainFamilyV2::Coast
    } else if uplift > 560 {
        TerrainFamilyV2::MountainRange
    } else if uplift > 240 && lithology > 0 {
        TerrainFamilyV2::Plateau
    } else if uplift > 64 {
        TerrainFamilyV2::RollingHills
    } else if lithology < -560 {
        TerrainFamilyV2::Wetland
    } else {
        TerrainFamilyV2::Plains
    }
}

fn semantic_seed(seed_root: WorldgenSeedRootV2, domain: &[u8]) -> i64 {
    let hash = domain_hash(domain, &[seed_root.as_bytes()]);
    let mut bytes = [0_u8; 8];
    bytes.copy_from_slice(&hash.as_bytes()[..8]);
    i64::from_be_bytes(bytes)
}

fn round_divide_i64(numerator: i64, denominator: i64) -> i64 {
    if denominator <= 0 {
        return 0;
    }
    let sign = numerator.signum();
    numerator
        .unsigned_abs()
        .saturating_add(denominator.cast_unsigned() / 2)
        .checked_div(denominator.cast_unsigned())
        .and_then(|value| i64::try_from(value).ok())
        .unwrap_or(i64::MAX)
        .saturating_mul(sign)
}

fn clamp_i64_i32(value: i64) -> i32 {
    i32::try_from(value).unwrap_or(if value.is_negative() {
        i32::MIN
    } else {
        i32::MAX
    })
}

const fn boundary_edge_tag(edge: HydrologicBoundaryEdgeV1) -> u8 {
    match edge {
        HydrologicBoundaryEdgeV1::North => 0,
        HydrologicBoundaryEdgeV1::East => 1,
        HydrologicBoundaryEdgeV1::South => 2,
        HydrologicBoundaryEdgeV1::West => 3,
    }
}

fn invalid<T>(field: &'static str, reason: impl Into<String>) -> WorldgenResult<T> {
    Err(WorldgenError::InvalidConfig {
        field,
        reason: reason.into(),
    })
}

#[cfg(test)]
mod tests {
    use std::{num::NonZeroU16, str::FromStr};

    use latticeaxiom_core::CanonicalHash;
    use latticeaxiom_storage::DimensionId;

    use super::*;
    use crate::{TerrainConfigV2, WorldSeedV1};

    fn spline(points: &[(i16, i32)]) -> ClosedSplineV1 {
        ClosedSplineV1::new(
            points
                .iter()
                .map(|&(input, output)| ClosedSplinePointV1::new(input, output))
                .collect(),
        )
        .expect("fixture spline is closed")
    }

    fn policy() -> SemanticTerrainPolicyV1 {
        let field = |scale| {
            SemanticFieldSpecV1::new(
                NonZeroU32::new(scale).expect("fixture field scale is nonzero"),
                1_024,
            )
        };
        SemanticTerrainPolicyV1::new(
            field(8_192),
            field(2_048),
            field(1_024),
            field(4_096),
            field(3_072),
            field(2_560),
            field(256),
            field(96),
            field(64),
            spline(&[(-1_024, -1_024), (0, 0), (1_024, 1_024)]),
            spline(&[(-1_024, 0), (0, 96), (512, 640), (1_024, 1_024)]),
            spline(&[(-1_024, 128), (0, 384), (1_024, 1_024)]),
            spline(&[(-1_024, 0), (256, 0), (768, 768), (1_024, 1_024)]),
            384,
            768,
            256,
            1_024,
        )
        .expect("fixture semantic policy is valid")
    }

    fn field() -> SemanticTerrainFieldV1 {
        SemanticTerrainFieldV1::new(
            WorldgenSeedRootV2::from_world_seed(WorldSeedV1::from_integer(73)),
            TerrainConfigV2::representative_test_baseline(),
            policy(),
        )
        .expect("fixture semantic field compiles")
    }

    #[test]
    fn closed_splines_reject_open_duplicate_and_boundary_plus_one() {
        assert!(
            ClosedSplineV1::new(vec![
                ClosedSplinePointV1::new(-512, 0),
                ClosedSplinePointV1::new(1_024, 1)
            ])
            .is_err()
        );
        assert!(
            ClosedSplineV1::new(vec![
                ClosedSplinePointV1::new(-1_024, 0),
                ClosedSplinePointV1::new(-1_024, 1),
                ClosedSplinePointV1::new(1_024, 2)
            ])
            .is_err()
        );
        assert!(
            ClosedSplineV1::new(vec![
                ClosedSplinePointV1::new(-1_024, MAX_SPLINE_OUTPUT + 1),
                ClosedSplinePointV1::new(1_024, 0)
            ])
            .is_err()
        );
        let valid = spline(&[(-1_024, -1_024), (0, 0), (1_024, 1_024)]);
        assert_eq!(valid.evaluate(-2_048).ok(), Some(-1_024));
        assert_eq!(valid.evaluate(512).ok(), Some(512));
        assert_eq!(valid.evaluate(2_048).ok(), Some(1_024));
    }

    #[test]
    fn named_fields_are_repeatable_independent_and_bounded() {
        let field = field();
        let first = field.sample(-17_123, 8_991).expect("sample fits");
        assert_eq!(first, field.sample(-17_123, 8_991).expect("sample repeats"));
        assert_ne!(first.continentalness_per_1024, first.uplift_per_1024);
        assert!(first.effective_runoff_q16 >= 256);
        assert!(first.detail_displacement_q8.unsigned_abs() <= 384);
    }

    #[test]
    fn protected_density_never_adds_solid_above_approved_surface() {
        let field = field();
        for y in 65..=80 {
            let protected = field
                .density(113, y, -277, 64, true)
                .expect("protected density samples");
            assert!(i64::from(protected.final_density_q8) <= (64 - y) * 256);
        }
    }

    #[test]
    fn semantic_plan_routes_package_fields_into_connected_topology() {
        let seed_root = WorldgenSeedRootV2::from_world_seed(WorldSeedV1::from_integer(73));
        let terrain = TerrainConfigV2::representative_test_baseline();
        let input = SemanticHydrologicTerrainInputV1::new(
            DimensionId::from_str("latticeaxiom:dimension/terrenia")
                .expect("fixture dimension is valid"),
            seed_root,
            GenerationEpochIdV1::from_hash(CanonicalHash::digest("semantic-epoch")),
            0,
            0,
            None,
            CanonicalHash::digest("semantic-provenance"),
            HydrologicDomainGridV1::new(
                -64,
                -64,
                NonZeroU32::new(8).expect("spacing is nonzero"),
                NonZeroU16::new(17).expect("width is nonzero"),
                NonZeroU16::new(17).expect("height is nonzero"),
                2,
            ),
            terrain.world.sea_level_y * 256,
            Vec::new(),
            terrain,
            policy(),
            HydrologicDomainConfigV1::default(),
            HydrologicTopologyConfigV1::default(),
        );
        let plan = build_semantic_hydrologic_terrain_plan_v1(input)
            .expect("semantic hydrologic plan builds");
        assert_eq!(plan.domain.routing().len(), 17 * 17);
        assert_eq!(
            plan.topology.accounting().source_runoff_q16(),
            plan.topology.accounting().terminal_runoff_q16()
        );
        assert!(plan.canonical_hash().is_ok());
    }

    #[test]
    fn boundary_evidence_is_direction_independent_and_mints_declaration() {
        let old = GenerationEpochIdV1::from_hash(CanonicalHash::digest("old"));
        let new = GenerationEpochIdV1::from_hash(CanonicalHash::digest("new"));
        let old_profile = [1_024, 1_040, 1_060, 1_080];
        let new_profile = [1_024, 1_042, 1_058, 1_080];
        let forward = TerrainBoundaryAdapterEvidenceV1::measure(
            HydrologicBoundaryEdgeV1::East,
            old,
            &old_profile,
            new,
            &new_profile,
        )
        .expect("boundary evidence measures");
        let reverse = TerrainBoundaryAdapterEvidenceV1::measure(
            HydrologicBoundaryEdgeV1::East,
            new,
            &new_profile,
            old,
            &old_profile,
        )
        .expect("reverse boundary evidence measures");
        assert_eq!(forward.adapter_hash(), reverse.adapter_hash());
        assert_eq!(forward.max_absolute_delta_q8(), 2);
        assert!(
            forward
                .declaration(
                    StableId::from_str("terrenia:worldgen-boundary-adapter/legacy-semantic@1")
                        .expect("adapter identity is valid"),
                    NonZeroU32::new(32).expect("transition width is nonzero"),
                    Vec::new(),
                )
                .is_ok()
        );
    }
}
