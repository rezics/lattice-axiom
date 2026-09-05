//! Closed semantic terrain fields, splines, hydrology constraints, and density composition.

use std::num::NonZeroU32;

use latticeaxiom_core::{CanonicalHash, StableId, canonical_json_bytes};
use latticeaxiom_storage::DimensionId;
use serde::{Deserialize, Serialize};

use crate::{
    BoundaryAdapterDeclarationV1, GenerationEpochIdV1, HydrologicBoundaryEdgeV1,
    HydrologicBoundaryPortV1, HydrologicDomainConfigV1, HydrologicDomainGridV1,
    HydrologicDomainInputV1, HydrologicDomainPlanV1, HydrologicTopologyConfigV1,
    HydrologicTopologyPlanV1, LandscapeEvolutionConfigV1, LandscapeEvolutionEvidenceV1,
    LandscapeEvolutionInputV1, SemanticTerrainPlanHashV1, SemanticTerrainPolicyHashV1,
    TerrainBoundaryAdapterHashV1, TerrainColumnSampleV2, TerrainConfigV2, TerrainFamilyV2,
    WorldgenError, WorldgenResult, WorldgenSeedRootV2, build_hydrologic_topology_v1,
    evolve_hydrologic_landscape_v1, hashes::domain_hash, open_simplex_2f_3d_v1,
    open_simplex_2s_2d_v1, plan_hydrologic_domain_v1,
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
const MIDDLE_RELIEF_DOMAIN: &[u8] = b"latticeaxiom.semantic.middle-relief.v1\0";
const MIDDLE_RELIEF_OCTAVE_DOMAIN: &[u8] = b"latticeaxiom.semantic.middle-relief.octave-1.v1\0";
const MIDDLE_RELIEF_OCTAVE_2_DOMAIN: &[u8] = b"latticeaxiom.semantic.middle-relief.octave-2.v1\0";
const MIDDLE_RELIEF_OCTAVE_3_DOMAIN: &[u8] = b"latticeaxiom.semantic.middle-relief.octave-3.v1\0";
const MIDDLE_RELIEF_OCTAVE_4_DOMAIN: &[u8] = b"latticeaxiom.semantic.middle-relief.octave-4.v1\0";
const PLATEAU_DOMAIN: &[u8] = b"latticeaxiom.semantic.plateau.v1\0";
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

/// Optional versioned middle-scale relief, plateau, and cliff-gate policy.
///
/// Absence preserves the original semantic policy byte encoding and behavior.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SemanticMorphologyPolicyV1 {
    middle_relief: SemanticFieldSpecV1,
    plateau: SemanticFieldSpecV1,
    middle_relief_amplitude: ClosedSplineV1,
    cliff_lithology: ClosedSplineV1,
    plateau_transition_half_width_per_1024: u16,
}

impl SemanticMorphologyPolicyV1 {
    /// Creates one closed middle-scale morphology policy.
    ///
    /// # Errors
    ///
    /// Returns an invalid-config error for unbounded fields, malformed
    /// splines, or a zero/excessive plateau-transition width.
    pub fn new(
        middle_relief: SemanticFieldSpecV1,
        plateau: SemanticFieldSpecV1,
        middle_relief_amplitude: ClosedSplineV1,
        cliff_lithology: ClosedSplineV1,
        plateau_transition_half_width_per_1024: u16,
    ) -> WorldgenResult<Self> {
        let policy = Self {
            middle_relief,
            plateau,
            middle_relief_amplitude,
            cliff_lithology,
            plateau_transition_half_width_per_1024,
        };
        policy.validate()?;
        Ok(policy)
    }

    /// Returns the independent middle-scale relief field.
    #[must_use]
    pub const fn middle_relief(&self) -> SemanticFieldSpecV1 {
        self.middle_relief
    }

    /// Returns the independent plateau-coverage field.
    #[must_use]
    pub const fn plateau(&self) -> SemanticFieldSpecV1 {
        self.plateau
    }

    /// Returns the half-width of the closed plateau transition in normalized
    /// field units.
    #[must_use]
    pub const fn plateau_transition_half_width_per_1024(&self) -> u16 {
        self.plateau_transition_half_width_per_1024
    }

    fn validate(&self) -> WorldgenResult<()> {
        self.middle_relief
            .validate("semantic.morphology.middle_relief")?;
        self.plateau.validate("semantic.morphology.plateau")?;
        self.middle_relief_amplitude.validate()?;
        self.cliff_lithology.validate()?;
        if !(1..=512).contains(&self.plateau_transition_half_width_per_1024) {
            return invalid(
                "semantic.morphology.plateau_transition_half_width_per_1024",
                "must be in 1..=512",
            );
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    morphology: Option<SemanticMorphologyPolicyV1>,
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
            morphology: None,
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

    /// Adds a versioned middle-scale morphology extension.
    ///
    /// # Errors
    ///
    /// Returns an invalid-config error when the extension violates its closed
    /// field, spline, or transition-width bounds.
    pub fn with_morphology(
        mut self,
        morphology: SemanticMorphologyPolicyV1,
    ) -> WorldgenResult<Self> {
        morphology.validate()?;
        self.morphology = Some(morphology);
        self.validate()?;
        Ok(self)
    }

    /// Returns the optional versioned middle-scale morphology policy.
    #[must_use]
    pub const fn morphology(&self) -> Option<&SemanticMorphologyPolicyV1> {
        self.morphology.as_ref()
    }

    /// Returns the closed vertical displacement envelope of both 3D density
    /// contributors, rounded up to whole voxels.
    ///
    /// A caller may inspect only this many voxels above or below the approved
    /// surface when looking for the final density boundary. Cave arbitration
    /// can still reject every candidate inside that bounded envelope.
    #[must_use]
    pub const fn maximum_density_displacement_voxels(&self) -> u32 {
        let displacement_q8 =
            (self.max_terrain_volume_q8 as u32).saturating_add(self.max_geologic_volume_q8 as u32);
        displacement_q8.saturating_add(255) / 256
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
        if let Some(morphology) = &self.morphology {
            morphology.validate()?;
        }
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
    middle_relief_per_1024: i16,
    plateau_per_1024: i16,
    plateau_weight_per_1024: u16,
    plateau_transition_per_1024: u16,
    middle_relief_displacement_q8: i32,
    plateau_displacement_q8: i32,
    morphology_displacement_q8: i32,
    family: TerrainFamilyV2,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct SemanticDensityControlsV1 {
    continentalness: i16,
    uplift: i16,
    lithology: i16,
    plateau: i16,
}

impl From<SemanticFieldSampleV1> for SemanticDensityControlsV1 {
    fn from(sample: SemanticFieldSampleV1) -> Self {
        Self {
            continentalness: sample.continentalness_per_1024(),
            uplift: sample.uplift_per_1024(),
            lithology: sample.lithology_per_1024(),
            plateau: sample.plateau_per_1024(),
        }
    }
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

    /// Returns the independent middle-scale relief field, or zero when the
    /// policy predates the morphology extension.
    #[must_use]
    pub const fn middle_relief_per_1024(self) -> i16 {
        self.middle_relief_per_1024
    }

    /// Returns the independent plateau-coverage field, or zero when the
    /// policy predates the morphology extension.
    #[must_use]
    pub const fn plateau_per_1024(self) -> i16 {
        self.plateau_per_1024
    }

    /// Returns plateau membership in normalized `0..=1024` units.
    #[must_use]
    pub const fn plateau_weight_per_1024(self) -> u16 {
        self.plateau_weight_per_1024
    }

    /// Returns the triangular plateau-edge signal in normalized `0..=1024`
    /// units. It is zero in plateau interiors and exteriors.
    #[must_use]
    pub const fn plateau_transition_per_1024(self) -> u16 {
        self.plateau_transition_per_1024
    }

    /// Returns the middle-relief contribution in Q8 voxels, or zero for the
    /// original semantic policy.
    #[must_use]
    pub const fn middle_relief_displacement_q8(self) -> i32 {
        self.middle_relief_displacement_q8
    }

    /// Returns the plateau-profile contribution in Q8 voxels, or zero for the
    /// original semantic policy.
    #[must_use]
    pub const fn plateau_displacement_q8(self) -> i32 {
        self.plateau_displacement_q8
    }

    /// Returns the combined middle-relief and plateau displacement in Q8
    /// voxels, or zero for the original semantic policy.
    #[must_use]
    pub const fn morphology_displacement_q8(self) -> i32 {
        self.morphology_displacement_q8
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

/// One coordinate-bound semantic density evaluator prepared for a whole
/// vertical column.
///
/// The two-dimensional cliff controls are sampled once when this value is
/// created. Its private fields bind the coordinates, approved surface,
/// protection decision, seed domains, and policy constants together so a
/// caller cannot accidentally combine pieces from different columns.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SemanticDensityColumnV1 {
    world_x: i64,
    world_z: i64,
    approved_surface_y: i32,
    protected_water: bool,
    cliff_per_1024: i32,
    terrain_volume_seed: i64,
    geologic_volume_seed: i64,
    terrain_volume: SemanticFieldSpecV1,
    geologic_volume: SemanticFieldSpecV1,
    max_terrain_volume_q8: u16,
    max_geologic_volume_q8: u16,
}

impl SemanticDensityColumnV1 {
    /// Samples one vertical coordinate with the column's prepared controls.
    ///
    /// # Errors
    ///
    /// Returns a fixed-field coordinate or arithmetic error outside the
    /// supported world coordinate envelope.
    pub fn density_at(&self, world_y: i64) -> WorldgenResult<SemanticDensitySampleV1> {
        let terrain_noise = sample_3d(
            self.terrain_volume_seed,
            self.terrain_volume,
            self.world_x,
            world_y,
            self.world_z,
        )?;
        let geology_noise = sample_3d(
            self.geologic_volume_seed,
            self.geologic_volume,
            self.world_x,
            world_y,
            self.world_z,
        )?;
        let terrain_volume = i64::from(terrain_noise)
            .saturating_mul(i64::from(self.max_terrain_volume_q8))
            .saturating_mul(i64::from(self.cliff_per_1024))
            .div_euclid(SEMANTIC_UNIT * SEMANTIC_UNIT);
        let geology_volume = i64::from(geology_noise)
            .saturating_mul(i64::from(self.max_geologic_volume_q8))
            .div_euclid(SEMANTIC_UNIT);
        let base = i64::from(self.approved_surface_y)
            .saturating_sub(world_y)
            .saturating_mul(256);
        let unconstrained = base
            .saturating_add(terrain_volume)
            .saturating_add(geology_volume);
        let final_density = if self.protected_water {
            unconstrained.min(base)
        } else {
            unconstrained
        };
        Ok(SemanticDensitySampleV1 {
            base_surface_density_q8: clamp_i64_i32(base),
            terrain_family_volume_q8: clamp_i64_i32(terrain_volume),
            geologic_volume_q8: clamp_i64_i32(geology_volume),
            protected_water: self.protected_water,
            final_density_q8: clamp_i64_i32(final_density),
        })
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
    middle_relief: i64,
    middle_relief_octave: i64,
    middle_relief_octave_2: i64,
    middle_relief_octave_3: i64,
    middle_relief_octave_4: i64,
    plateau: i64,
    terrain_volume: i64,
    geologic_volume: i64,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct SemanticMorphologySample {
    middle_relief_per_1024: i16,
    plateau_per_1024: i16,
    plateau_weight_per_1024: u16,
    plateau_transition_per_1024: u16,
    middle_relief_displacement_q8: i32,
    plateau_displacement_q8: i32,
    displacement_q8: i32,
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
                middle_relief: seed(MIDDLE_RELIEF_DOMAIN),
                middle_relief_octave: seed(MIDDLE_RELIEF_OCTAVE_DOMAIN),
                middle_relief_octave_2: seed(MIDDLE_RELIEF_OCTAVE_2_DOMAIN),
                middle_relief_octave_3: seed(MIDDLE_RELIEF_OCTAVE_3_DOMAIN),
                middle_relief_octave_4: seed(MIDDLE_RELIEF_OCTAVE_4_DOMAIN),
                plateau: seed(PLATEAU_DOMAIN),
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
        let coast_width = i64::from(self.terrain.landmass.coast_width_per_1024).max(1);
        let coast_mask = i64::from(continentalness)
            .unsigned_abs()
            .saturating_mul(1_024)
            .div_euclid(coast_width.cast_unsigned())
            .min(1_024)
            .cast_signed();
        let land_blend = land_blend_per_1024(continentalness, coast_width);
        let macro_q8 = if continentalness < 0 {
            sea_q8.saturating_add(
                i64::from(self.terrain.landmass.ocean_depth_voxels)
                    .saturating_mul(256)
                    .saturating_mul(i64::from(base_curve))
                    .div_euclid(SEMANTIC_UNIT),
            )
        } else {
            let land_contribution_q8 = i64::from(self.terrain.relief.base_height_voxels)
                .saturating_mul(256)
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
                );
            if self.policy.morphology.is_some() {
                sea_q8.saturating_add(round_divide_i64(
                    land_contribution_q8.saturating_mul(land_blend),
                    SEMANTIC_UNIT,
                ))
            } else {
                sea_q8.saturating_add(land_contribution_q8)
            }
        };
        let morphology = self.sample_morphology(
            world_x,
            world_z,
            continentalness,
            uplift,
            lithology,
            land_blend,
        )?;
        let detail_mask = if self.policy.morphology.is_some() && continentalness >= 0 {
            land_blend
        } else {
            coast_mask
        };
        let detail_q8 = i64::from(detail)
            .saturating_mul(i64::from(self.policy.max_detail_displacement_q8))
            .saturating_mul(i64::from(roughness.clamp(0, 1_024)))
            .saturating_mul(detail_mask)
            .div_euclid(SEMANTIC_UNIT * SEMANTIC_UNIT * SEMANTIC_UNIT);
        let minimum_q8 = i64::from(self.terrain.world.floor_y)
            .saturating_add(1)
            .saturating_mul(256);
        let maximum_q8 = i64::from(self.terrain.world.ceiling_y)
            .saturating_sub(16)
            .saturating_mul(256);
        let initial_surface_y_q8 = i32::try_from(
            macro_q8
                .saturating_add(i64::from(morphology.displacement_q8))
                .saturating_add(detail_q8)
                .clamp(minimum_q8, maximum_q8),
        )
        .map_err(|_| WorldgenError::ArithmeticOverflow {
            operation: "semantic terrain surface",
        })?;
        let effective_runoff_q16 = effective_runoff(precipitation, infiltration);
        let mut family = semantic_family(continentalness, uplift, lithology);
        if continentalness >= 96
            && morphology.plateau_weight_per_1024 >= 512
            && family != TerrainFamilyV2::MountainRange
        {
            family = TerrainFamilyV2::Plateau;
        }
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
            middle_relief_per_1024: morphology.middle_relief_per_1024,
            plateau_per_1024: morphology.plateau_per_1024,
            plateau_weight_per_1024: morphology.plateau_weight_per_1024,
            plateau_transition_per_1024: morphology.plateau_transition_per_1024,
            middle_relief_displacement_q8: morphology.middle_relief_displacement_q8,
            plateau_displacement_q8: morphology.plateau_displacement_q8,
            morphology_displacement_q8: morphology.displacement_q8,
            family,
        })
    }

    fn sample_middle_relief_octaves(
        &self,
        policy: &SemanticMorphologyPolicyV1,
        world_x: i64,
        world_z: i64,
    ) -> WorldgenResult<i16> {
        let base_scale = policy.middle_relief.scale_voxels().get();
        let amplitude = policy.middle_relief.amplitude_per_1024();
        let octaves = [
            (self.seeds.middle_relief, 1_u32, 20_i64),
            (self.seeds.middle_relief_octave, 2, 15),
            (self.seeds.middle_relief_octave_2, 4, 10),
            (self.seeds.middle_relief_octave_3, 8, 5),
            (self.seeds.middle_relief_octave_4, 16, 4),
        ];
        let mut weighted_sum = 0_i64;
        for (seed, divisor, weight) in octaves {
            let scale =
                NonZeroU32::new(base_scale.div_euclid(divisor).max(1)).unwrap_or(NonZeroU32::MIN);
            let sample = sample_2d(
                seed,
                SemanticFieldSpecV1::new(scale, amplitude),
                world_x,
                world_z,
            )?;
            weighted_sum = weighted_sum.saturating_add(i64::from(sample).saturating_mul(weight));
        }
        i16::try_from(round_divide_i64(weighted_sum, 54).clamp(-1_024, 1_024)).map_err(|_| {
            WorldgenError::ArithmeticOverflow {
                operation: "semantic middle-relief octave composition",
            }
        })
    }

    fn sample_morphology(
        &self,
        world_x: i64,
        world_z: i64,
        continentalness: i16,
        uplift: i16,
        lithology: i16,
        land_blend_per_1024: i64,
    ) -> WorldgenResult<SemanticMorphologySample> {
        let Some(policy) = self.policy.morphology.as_ref() else {
            return Ok(SemanticMorphologySample::default());
        };
        let middle_relief = self.sample_middle_relief_octaves(policy, world_x, world_z)?;
        let plateau = sample_2d(self.seeds.plateau, policy.plateau, world_x, world_z)?;
        let plateau_weight = plateau_weight_per_1024(
            plateau,
            self.terrain.relief.plateau_amount_per_1024,
            policy.plateau_transition_half_width_per_1024,
        );
        let plateau_transition = plateau_transition_per_1024(plateau_weight);
        let (middle_relief_displacement_q8, plateau_displacement_q8) = if continentalness < 0 {
            (0, 0)
        } else {
            let amplitude = i64::from(
                policy
                    .middle_relief_amplitude
                    .evaluate(i32::from(uplift))?
                    .clamp(0, 2_048),
            );
            let erosion_retention =
                2_048_i64.saturating_sub(i64::from(self.terrain.relief.erosion_strength_per_1024));
            let middle_relief_control = signed_ease_out_per_1024(i64::from(middle_relief));
            let middle_q8 = round_divide_i64(
                middle_relief_control
                    .saturating_mul(i64::from(self.terrain.relief.hill_height_voxels))
                    .saturating_mul(256)
                    .saturating_mul(amplitude)
                    .saturating_mul(land_blend_per_1024)
                    .saturating_mul(erosion_retention),
                SEMANTIC_UNIT
                    .saturating_mul(SEMANTIC_UNIT)
                    .saturating_mul(SEMANTIC_UNIT)
                    .saturating_mul(2_048),
            );
            let plateau_cliff = policy
                .cliff_lithology
                .evaluate(i32::from(lithology))?
                .clamp(0, 1_024);
            let plateau_displacement_weight =
                plateau_displacement_weight(plateau_weight, plateau_cliff);
            let plateau_q8 = round_divide_i64(
                i64::from(self.terrain.relief.plateau_height_voxels)
                    .saturating_mul(256)
                    .saturating_mul(i64::from(plateau_displacement_weight))
                    .saturating_mul(land_blend_per_1024),
                SEMANTIC_UNIT.saturating_mul(SEMANTIC_UNIT),
            );
            (clamp_i64_i32(middle_q8), clamp_i64_i32(plateau_q8))
        };
        let displacement_q8 = middle_relief_displacement_q8.saturating_add(plateau_displacement_q8);
        Ok(SemanticMorphologySample {
            middle_relief_per_1024: middle_relief,
            plateau_per_1024: plateau,
            plateau_weight_per_1024: plateau_weight,
            plateau_transition_per_1024: plateau_transition,
            middle_relief_displacement_q8,
            plateau_displacement_q8,
            displacement_q8,
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
        self.prepare_density_column(world_x, world_z, approved_surface_y, protected_water)?
            .density_at(world_y)
    }

    /// Prepares coordinate-bound two-dimensional controls once for a complete
    /// vertical density column.
    ///
    /// # Errors
    ///
    /// Returns a fixed-field coordinate or arithmetic error outside the
    /// supported world coordinate envelope.
    pub fn prepare_density_column(
        &self,
        world_x: i64,
        world_z: i64,
        approved_surface_y: i32,
        protected_water: bool,
    ) -> WorldgenResult<SemanticDensityColumnV1> {
        let uplift_per_1024 = sample_2d(self.seeds.uplift, self.policy.uplift, world_x, world_z)?;
        let (continentalness_per_1024, lithology_per_1024, plateau_per_1024) =
            if let Some(morphology) = self.policy.morphology.as_ref() {
                (
                    sample_2d(
                        self.seeds.continentalness,
                        self.policy.continentalness,
                        world_x,
                        world_z,
                    )?,
                    sample_2d(
                        self.seeds.lithology,
                        self.policy.lithology,
                        world_x,
                        world_z,
                    )?,
                    sample_2d(self.seeds.plateau, morphology.plateau, world_x, world_z)?,
                )
            } else {
                (0, 0, 0)
            };
        self.prepare_density_column_from_controls(
            world_x,
            world_z,
            approved_surface_y,
            protected_water,
            SemanticDensityControlsV1 {
                continentalness: continentalness_per_1024,
                uplift: uplift_per_1024,
                lithology: lithology_per_1024,
                plateau: plateau_per_1024,
            },
        )
    }

    pub(crate) fn prepare_density_column_from_semantic(
        &self,
        world_x: i64,
        world_z: i64,
        approved_surface_y: i32,
        protected_water: bool,
        semantic: SemanticFieldSampleV1,
    ) -> WorldgenResult<SemanticDensityColumnV1> {
        self.prepare_density_column_from_controls(
            world_x,
            world_z,
            approved_surface_y,
            protected_water,
            semantic.into(),
        )
    }

    fn prepare_density_column_from_controls(
        &self,
        world_x: i64,
        world_z: i64,
        approved_surface_y: i32,
        protected_water: bool,
        controls: SemanticDensityControlsV1,
    ) -> WorldgenResult<SemanticDensityColumnV1> {
        let uplift_cliff = self
            .policy
            .cliff_tendency
            .evaluate(i32::from(controls.uplift))?
            .clamp(0, 1_024);
        let cliff = if let Some(morphology) = self.policy.morphology.as_ref() {
            let plateau_weight = plateau_weight_per_1024(
                controls.plateau,
                self.terrain.relief.plateau_amount_per_1024,
                morphology.plateau_transition_half_width_per_1024,
            );
            let plateau_transition = i32::from(plateau_transition_per_1024(plateau_weight));
            let lithology_cliff = morphology
                .cliff_lithology
                .evaluate(i32::from(controls.lithology))?
                .clamp(0, 1_024);
            let cliff = (uplift_cliff
                .saturating_mul(2)
                .saturating_add(lithology_cliff)
                .saturating_add(plateau_transition.saturating_mul(2)))
            .div_euclid(5);
            let coast_width = i64::from(self.terrain.landmass.coast_width_per_1024).max(1);
            i32::try_from(round_divide_i64(
                i64::from(cliff)
                    .saturating_mul(land_blend_per_1024(controls.continentalness, coast_width)),
                SEMANTIC_UNIT,
            ))
            .unwrap_or(i32::MAX)
        } else {
            uplift_cliff
        };
        Ok(SemanticDensityColumnV1 {
            world_x,
            world_z,
            approved_surface_y,
            protected_water,
            cliff_per_1024: cliff,
            terrain_volume_seed: self.seeds.terrain_volume,
            geologic_volume_seed: self.seeds.geologic_volume,
            terrain_volume: self.policy.terrain_volume,
            geologic_volume: self.policy.geologic_volume,
            max_terrain_volume_q8: self.policy.max_terrain_volume_q8,
            max_geologic_volume_q8: self.policy.max_geologic_volume_q8,
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
    landscape_evolution: Option<LandscapeEvolutionConfigV1>,
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
            landscape_evolution: None,
        }
    }

    /// Enables bounded deterministic erosion before the final topology is published.
    #[must_use]
    pub fn with_landscape_evolution(
        mut self,
        landscape_evolution: LandscapeEvolutionConfigV1,
    ) -> Self {
        self.landscape_evolution = Some(landscape_evolution);
        self
    }
}

/// Immutable semantic DEM, hydrologic domain, and connected topology artifact.
#[derive(Clone, Debug)]
pub struct SemanticHydrologicTerrainPlanV1 {
    algorithm: &'static str,
    policy_hash: SemanticTerrainPolicyHashV1,
    domain: HydrologicDomainPlanV1,
    topology: HydrologicTopologyPlanV1,
    evolution: Option<LandscapeEvolutionEvidenceV1>,
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

    /// Returns bounded landscape-evolution evidence when erosion was enabled.
    #[must_use]
    pub const fn evolution(&self) -> Option<&LandscapeEvolutionEvidenceV1> {
        self.evolution.as_ref()
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
            #[serde(skip_serializing_if = "Option::is_none")]
            evolution: Option<&'a LandscapeEvolutionEvidenceV1>,
        }
        canonical_json_bytes(&CanonicalPlan {
            algorithm: self.algorithm,
            policy_hash: self.policy_hash,
            domain: &self.domain,
            topology: &self.topology,
            evolution: self.evolution.as_ref(),
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
    let (domain, topology, evolution) = if let Some(config) = input.landscape_evolution {
        let evolved = evolve_hydrologic_landscape_v1(&LandscapeEvolutionInputV1::new(
            domain_input,
            input.topology_config,
            config,
        ))?;
        let (domain, topology, evidence) = evolved.into_parts();
        (domain, topology, Some(evidence))
    } else {
        let domain = plan_hydrologic_domain_v1(&domain_input)?;
        let topology = build_hydrologic_topology_v1(&domain, &input.topology_config)?;
        (domain, topology, None)
    };
    Ok(SemanticHydrologicTerrainPlanV1 {
        algorithm: SEMANTIC_TERRAIN_ALGORITHM,
        policy_hash,
        domain,
        topology,
        evolution,
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

    /// Returns the closed vertical displacement envelope of the bound density
    /// policy, rounded up to whole voxels.
    #[must_use]
    pub const fn maximum_density_displacement_voxels(&self) -> u32 {
        self.field.policy().maximum_density_displacement_voxels()
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
        self.terrain_column_from_semantic(world_x, world_z, semantic)
    }

    fn terrain_column_from_semantic(
        &self,
        world_x: i64,
        world_z: i64,
        semantic: SemanticFieldSampleV1,
    ) -> WorldgenResult<TerrainColumnSampleV2> {
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

    pub(crate) fn terrain_column_with_semantic(
        &self,
        world_x: i64,
        world_z: i64,
    ) -> WorldgenResult<(TerrainColumnSampleV2, SemanticFieldSampleV1)> {
        let semantic = self.field.sample(world_x, world_z)?;
        let column = self.terrain_column_from_semantic(world_x, world_z, semantic)?;
        Ok((column, semantic))
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
        self.terrain_column_with_density(world_x, world_z)?
            .1
            .density_at(world_y)
    }

    pub(crate) fn terrain_column_with_density(
        &self,
        world_x: i64,
        world_z: i64,
    ) -> WorldgenResult<(TerrainColumnSampleV2, SemanticDensityColumnV1)> {
        let (column, semantic) = self.terrain_column_with_semantic(world_x, world_z)?;
        let density = self.density_column_from_semantic(world_x, world_z, column, semantic)?;
        Ok((column, density))
    }

    pub(crate) fn density_column_from_semantic(
        &self,
        world_x: i64,
        world_z: i64,
        column: TerrainColumnSampleV2,
        semantic: SemanticFieldSampleV1,
    ) -> WorldgenResult<SemanticDensityColumnV1> {
        let river = self
            .plan
            .topology
            .river_sdf_sample(&self.plan.domain, world_x, world_z)?;
        let protected = river.is_some_and(|sample| {
            sample.signed_distance_q8() <= i64::from(self.field.policy.water_protection_radius_q8())
        }) || column.surface_water_y().is_some();
        self.field.prepare_density_column_from_semantic(
            world_x,
            world_z,
            column.height(),
            protected,
            semantic,
        )
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

fn plateau_weight_per_1024(
    plateau: i16,
    amount_per_1024: u16,
    transition_half_width_per_1024: u16,
) -> u16 {
    let plateau = signed_ease_out_per_1024(i64::from(plateau));
    let threshold = 1_024_i64.saturating_sub(i64::from(amount_per_1024).saturating_mul(2));
    let half_width = i64::from(transition_half_width_per_1024);
    let lower = threshold.saturating_sub(half_width);
    let upper = threshold.saturating_add(half_width);
    let weight = if plateau <= lower {
        0
    } else if plateau >= upper {
        1_024
    } else {
        round_divide_i64(
            plateau.saturating_sub(lower).saturating_mul(SEMANTIC_UNIT),
            upper.saturating_sub(lower),
        )
    };
    u16::try_from(weight.clamp(0, 1_024)).unwrap_or(1_024)
}

fn signed_ease_out_per_1024(value: i64) -> i64 {
    let magnitude = value.unsigned_abs().min(1_024).cast_signed();
    let eased = magnitude
        .saturating_mul(SEMANTIC_UNIT.saturating_mul(2).saturating_sub(magnitude))
        .div_euclid(SEMANTIC_UNIT);
    eased.saturating_mul(value.signum())
}

fn smoothstep_per_1024(value: i64) -> i64 {
    let value = value.clamp(0, SEMANTIC_UNIT);
    round_divide_i64(
        value.saturating_mul(value).saturating_mul(
            SEMANTIC_UNIT
                .saturating_mul(3)
                .saturating_sub(value.saturating_mul(2)),
        ),
        SEMANTIC_UNIT.saturating_mul(SEMANTIC_UNIT),
    )
    .clamp(0, SEMANTIC_UNIT)
}

fn land_blend_per_1024(continentalness: i16, coast_width: i64) -> i64 {
    if continentalness <= 0 {
        return 0;
    }
    let linear = i64::from(continentalness)
        .saturating_mul(SEMANTIC_UNIT)
        .div_euclid(coast_width.max(1));
    smoothstep_per_1024(linear)
}

fn plateau_transition_per_1024(plateau_weight_per_1024: u16) -> u16 {
    let centered = i32::from(plateau_weight_per_1024)
        .saturating_mul(2)
        .saturating_sub(1_024)
        .unsigned_abs();
    u16::try_from(1_024_u32.saturating_sub(centered)).unwrap_or_default()
}

fn plateau_displacement_weight(plateau_weight_per_1024: u16, cliff_per_1024: i32) -> u16 {
    let smooth = smoothstep_per_1024(i64::from(plateau_weight_per_1024));
    let sharpened = smoothstep_per_1024(smoothstep_per_1024(smooth));
    let cliff = i64::from(cliff_per_1024).clamp(0, SEMANTIC_UNIT);
    let weight = round_divide_i64(
        smooth
            .saturating_mul(SEMANTIC_UNIT.saturating_sub(cliff))
            .saturating_add(sharpened.saturating_mul(cliff)),
        SEMANTIC_UNIT,
    );
    u16::try_from(weight.clamp(0, SEMANTIC_UNIT)).unwrap_or(1_024)
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
    use std::{collections::BTreeSet, num::NonZeroU16, str::FromStr};

    use latticeaxiom_core::CanonicalHash;
    use latticeaxiom_storage::DimensionId;

    use super::*;
    use crate::{TerrainConfigV2, WorldSeedV1};

    #[test]
    fn coast_and_plateau_profiles_are_continuous_without_fixed_terrace_levels() {
        assert_eq!(land_blend_per_1024(-1, 192), 0);
        assert_eq!(land_blend_per_1024(0, 192), 0);
        assert_eq!(land_blend_per_1024(192, 192), 1_024);
        assert_eq!(land_blend_per_1024(1_024, 192), 1_024);
        let coast_weights: Vec<_> = (0_i16..=192)
            .map(|continentalness| land_blend_per_1024(continentalness, 192))
            .collect();
        assert!(coast_weights.windows(2).all(|pair| pair[0] <= pair[1]));

        for cliff in [0, 512, 1_024] {
            let weights: Vec<_> = (0_u16..=1_024)
                .map(|weight| plateau_displacement_weight(weight, cliff))
                .collect();
            assert_eq!(weights.first(), Some(&0));
            assert_eq!(weights.last(), Some(&1_024));
            assert!(weights.windows(2).all(|pair| pair[0] <= pair[1]));
            assert!(weights.iter().copied().collect::<BTreeSet<_>>().len() > 256);
        }
        assert!(plateau_displacement_weight(256, 1_024) < plateau_displacement_weight(256, 0));
        assert!(plateau_displacement_weight(768, 1_024) > plateau_displacement_weight(768, 0));
    }

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
    fn retained_semantic_controls_prepare_the_same_density_column() {
        let field = field();
        let semantic = field.sample(113, -277).expect("semantic sample fits");
        let sampled = field
            .prepare_density_column(113, -277, 64, false)
            .expect("sampled controls prepare density");
        let retained = field
            .prepare_density_column_from_semantic(113, -277, 64, false, semantic)
            .expect("retained controls prepare density");
        assert_eq!(sampled, retained);
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
    fn semantic_plan_can_publish_the_final_rerouted_evolved_dem() {
        let seed_root = WorldgenSeedRootV2::from_world_seed(WorldSeedV1::from_integer(73));
        let terrain = TerrainConfigV2::representative_test_baseline();
        let input = SemanticHydrologicTerrainInputV1::new(
            DimensionId::from_str("latticeaxiom:dimension/terrenia")
                .expect("fixture dimension is valid"),
            seed_root,
            GenerationEpochIdV1::from_hash(CanonicalHash::digest("evolved-semantic-epoch")),
            0,
            0,
            None,
            CanonicalHash::digest("evolved-semantic-provenance"),
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
        )
        .with_landscape_evolution(LandscapeEvolutionConfigV1::new(
            1,
            1,
            1,
            65_536,
            0,
            1 << 26,
            4_096,
            0,
            10_000_000,
            16 * 1024 * 1024,
            64 * 1024 * 1024,
        ));
        let plan = build_semantic_hydrologic_terrain_plan_v1(input)
            .expect("evolved semantic hydrologic plan builds");
        let evidence = plan.evolution().expect("evolution evidence is retained");
        assert_eq!(evidence.accounting().iterations_completed(), 1);
        assert_ne!(
            evidence.initial_elevation_q8(),
            plan.domain().initial_elevation_q8()
        );
        assert_eq!(
            plan.topology().accounting().source_runoff_q16(),
            plan.topology().accounting().terminal_runoff_q16()
        );
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
