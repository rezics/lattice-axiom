//! Package-owned V5 natural terrain, geology, hydrology, resource, and vegetation
//! layer compiled on top of the D4 coordinator.
//!
//! The layer is optional. D4 plans omit it and keep byte-identical output.
//! Host crates never supply Terrenia concrete IDs; frozen Role bindings do.

use std::num::NonZeroU32;

use latticeaxiom_core::canonical_json_bytes;
use serde::{Deserialize, Serialize};

use crate::{
    D4BlockCatalogClosureV1, D4MaterialRoleV1, FixedCoordinateV1, FrozenRoleBindingsV1,
    GenerationDiagnosticsV1, NaturalLayerHashV1, NaturalRoleVocabularyV1, PlacementPredicateKindV1,
    PlacementPredicateReceiptV1, ProviderGenerationIdentityV1, ProviderOfferV1, ProviderSlotV1,
    RiverBasinIdV1, RoleBindingReceiptV1, TerrainStyleV1, WorldgenConfigV1, WorldgenError,
    WorldgenLimitsV1, WorldgenResult, WorldgenSeedRootV2,
    config::MAX_TERRAIN_RELIEF,
    hashes::{domain_hash, hash_u64, sample_hash_2d, sample_hash_3d},
    open_simplex_2s_2d_v1,
    provider::ResolvedProvidersV1,
    terrain_field::{DrainageFieldV3, DrainageSampleV3},
    tree_morphology::{
        MAX_TREE_HEIGHT_VOXELS, MAX_TREE_RADIUS_VOXELS, MIN_TREE_EXCLUSION_RADIUS_VOXELS,
        TreeBlueprintV1, TreeDescriptorV1, TreeMorphologySamplerV1,
    },
};

const NATURAL_LAYER_DOMAIN: &[u8] = b"latticeaxiom.natural-layer.v1\0";
const BASIN_DOMAIN: &[u8] = b"latticeaxiom.natural-basin.v1\0";
const GEOLOGY_DOMAIN: &[u8] = b"latticeaxiom.natural-geology.v1\0";
const RESOURCE_DOMAIN: &[u8] = b"latticeaxiom.natural-resource.v1\0";
const COVER_DOMAIN: &[u8] = b"latticeaxiom.natural-cover.v1\0";
const SOIL_DEPTH_DOMAIN: &[u8] = b"latticeaxiom.natural-soil-depth.v1\0";
const SOIL_DEPTH_SCALE_VOXELS: NonZeroU32 =
    NonZeroU32::new(128).expect("the authored soil-depth scale is nonzero");
const FIXED_FIELD_UNIT_Q30: i64 = 1_i64 << 30;

/// Closed integer configuration for the V5 natural layer.
///
/// Defaults materialize before hashing. Unknown JSON fields fail closed.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct NaturalLayerConfigV1 {
    /// Boreal wetland nominal surface Y.
    pub boreal_base_height: i32,
    /// Boreal wetland bounded relief amplitude.
    pub boreal_relief: u16,
    /// Coarse river-basin cell edge in voxels.
    pub river_cell_edge_voxels: u16,
    /// Channel half-width in voxels.
    pub river_width_voxels: u16,
    /// Bounded catchment count that promotes a drainage cell to a channel.
    pub river_accumulation: u16,
    /// Surface incision applied inside a channel.
    pub river_incision_voxels: u16,
    /// Chebyshev exclusion radius between accepted tree anchors.
    pub tree_exclusion_radius_voxels: u16,
    /// Boreal pine-anchor threshold in `0..=1024` hash units.
    pub pine_threshold_per_1024: u16,
    /// Moss/peat ground-cover threshold in `0..=1024` hash units.
    pub moss_threshold_per_1024: u16,
    /// Coal replacement threshold in `0..=1024` hash units.
    pub coal_threshold_per_1024: u16,
    /// Iron replacement threshold in `0..=1024` hash units.
    pub iron_threshold_per_1024: u16,
    /// Tin replacement threshold in `0..=1024` hash units.
    pub tin_threshold_per_1024: u16,
    /// Gold replacement threshold in `0..=1024` hash units.
    pub gold_threshold_per_1024: u16,
    /// Sulfur replacement threshold in `0..=1024` hash units.
    pub sulfur_threshold_per_1024: u16,
    /// Crystal replacement threshold in `0..=1024` hash units.
    pub crystal_threshold_per_1024: u16,
    /// Mid-depth granite versus slate threshold in `0..=1024` hash units.
    pub granite_threshold_per_1024: u16,
    /// Geologic intrusion threshold in `0..=1024` hash units.
    pub intrusion_threshold_per_1024: u16,
}

impl Default for NaturalLayerConfigV1 {
    fn default() -> Self {
        Self {
            boreal_base_height: 28,
            boreal_relief: 12,
            river_cell_edge_voxels: 32,
            river_width_voxels: 3,
            river_accumulation: 3,
            river_incision_voxels: 2,
            tree_exclusion_radius_voxels: MIN_TREE_EXCLUSION_RADIUS_VOXELS,
            pine_threshold_per_1024: 14,
            moss_threshold_per_1024: 80,
            coal_threshold_per_1024: 24,
            iron_threshold_per_1024: 16,
            tin_threshold_per_1024: 14,
            gold_threshold_per_1024: 8,
            sulfur_threshold_per_1024: 10,
            crystal_threshold_per_1024: 6,
            granite_threshold_per_1024: 96,
            intrusion_threshold_per_1024: 20,
        }
    }
}

impl NaturalLayerConfigV1 {
    /// Decodes a closed JSON record and validates it against the D4 spine config.
    ///
    /// # Errors
    ///
    /// Returns a config encoding or bounds error.
    pub fn from_json(bytes: &[u8], spine: &WorldgenConfigV1) -> WorldgenResult<Self> {
        let value: Self = serde_json::from_slice(bytes).map_err(|error| {
            WorldgenError::InvalidConfigEncoding {
                reason: error.to_string(),
            }
        })?;
        value.validate(spine)?;
        Ok(value)
    }

    /// Validates natural-layer numeric bounds against the compiled spine config.
    ///
    /// # Errors
    ///
    /// Returns [`WorldgenError::InvalidConfig`] when a field is outside the
    /// closed domain or the derived surface would not fit the world column.
    pub fn validate(&self, spine: &WorldgenConfigV1) -> WorldgenResult<()> {
        bounded_u16("boreal_relief", self.boreal_relief, 1, MAX_TERRAIN_RELIEF)?;
        bounded_u16(
            "river_cell_edge_voxels",
            self.river_cell_edge_voxels,
            8,
            4_096,
        )?;
        bounded_u16("river_width_voxels", self.river_width_voxels, 1, 16)?;
        bounded_u16("river_accumulation", self.river_accumulation, 1, 32)?;
        bounded_u16("river_incision_voxels", self.river_incision_voxels, 0, 64)?;
        bounded_u16(
            "tree_exclusion_radius_voxels",
            self.tree_exclusion_radius_voxels,
            MIN_TREE_EXCLUSION_RADIUS_VOXELS,
            16,
        )?;
        for (field, value) in [
            ("pine_threshold_per_1024", self.pine_threshold_per_1024),
            ("moss_threshold_per_1024", self.moss_threshold_per_1024),
            ("coal_threshold_per_1024", self.coal_threshold_per_1024),
            ("iron_threshold_per_1024", self.iron_threshold_per_1024),
            ("tin_threshold_per_1024", self.tin_threshold_per_1024),
            ("gold_threshold_per_1024", self.gold_threshold_per_1024),
            ("sulfur_threshold_per_1024", self.sulfur_threshold_per_1024),
            (
                "crystal_threshold_per_1024",
                self.crystal_threshold_per_1024,
            ),
            (
                "granite_threshold_per_1024",
                self.granite_threshold_per_1024,
            ),
            (
                "intrusion_threshold_per_1024",
                self.intrusion_threshold_per_1024,
            ),
        ] {
            bounded_u16(field, value, 0, 1_024)?;
        }

        let maximum_surface = i64::from(self.boreal_base_height)
            .saturating_add(i64::from(self.boreal_relief))
            .max(
                i64::from(spine.temperate_base_height)
                    .saturating_add(i64::from(spine.temperate_relief)),
            )
            .max(i64::from(spine.arid_base_height).saturating_add(i64::from(spine.arid_relief)));
        let minimum_surface = i64::from(self.boreal_base_height)
            .saturating_sub(i64::from(self.boreal_relief))
            .min(
                i64::from(spine.temperate_base_height)
                    .saturating_sub(i64::from(spine.temperate_relief)),
            )
            .min(i64::from(spine.arid_base_height).saturating_sub(i64::from(spine.arid_relief)))
            .saturating_sub(i64::from(self.river_incision_voxels));
        if maximum_surface.saturating_add(MAX_TREE_HEIGHT_VOXELS) > i64::from(spine.world_ceiling_y)
        {
            return Err(invalid(
                "world_ceiling_y",
                "must leave ten voxels above the natural maximum surface",
            ));
        }
        if minimum_surface.saturating_sub(8) < i64::from(spine.world_floor_y) {
            return Err(invalid(
                "world_floor_y",
                "must leave eight voxels below the natural minimum surface",
            ));
        }
        Ok(())
    }

    /// Returns canonical compact JSON with defaults materialized.
    ///
    /// # Errors
    ///
    /// Returns a canonical encoding error if serialization fails.
    pub fn canonical_bytes(&self) -> WorldgenResult<Vec<u8>> {
        canonical_json_bytes(self).map_err(|error| WorldgenError::CanonicalEncoding {
            kind: "NaturalLayerConfigV1",
            reason: error.to_string(),
        })
    }
}

/// Immutable inputs that enable the V5 natural layer on a D4 plan.
#[derive(Clone, Debug)]
pub struct NaturalLayerInputV1 {
    config: NaturalLayerConfigV1,
    vocabulary: NaturalRoleVocabularyV1,
    provider_offers: Vec<ProviderOfferV1>,
}

impl NaturalLayerInputV1 {
    /// Creates a complete natural-layer compilation input.
    #[must_use]
    pub const fn new(
        config: NaturalLayerConfigV1,
        vocabulary: NaturalRoleVocabularyV1,
        provider_offers: Vec<ProviderOfferV1>,
    ) -> Self {
        Self {
            config,
            vocabulary,
            provider_offers,
        }
    }

    /// Returns the closed natural-layer configuration.
    #[must_use]
    pub const fn config(&self) -> &NaturalLayerConfigV1 {
        &self.config
    }

    pub(crate) fn vocabulary(&self) -> &NaturalRoleVocabularyV1 {
        &self.vocabulary
    }

    pub(crate) fn provider_offers(&self) -> &[ProviderOfferV1] {
        &self.provider_offers
    }
}

/// Queryable geologic sample at one world voxel.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct GeologicSampleV1 {
    role: D4MaterialRoleV1,
    depth: i64,
    intrusion: bool,
}

impl GeologicSampleV1 {
    /// Returns the Role selected for this stratum.
    #[must_use]
    pub const fn role(self) -> D4MaterialRoleV1 {
        self.role
    }

    /// Returns depth below the river-adjusted surface.
    #[must_use]
    pub const fn depth(self) -> i64 {
        self.depth
    }

    /// Returns whether an intrusion replaced the host rock.
    #[must_use]
    pub const fn is_intrusion(self) -> bool {
        self.intrusion
    }
}

/// Locally queryable surface river sample.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RiverSampleV1 {
    basin: RiverBasinIdV1,
    in_channel: bool,
    distance_voxels: u32,
}

impl RiverSampleV1 {
    /// Returns the stable basin identity covering this column.
    #[must_use]
    pub const fn basin(self) -> RiverBasinIdV1 {
        self.basin
    }

    /// Returns whether the column is inside the planned channel.
    #[must_use]
    pub const fn in_channel(self) -> bool {
        self.in_channel
    }

    /// Returns the one-voxel-Lipschitz distance proxy to the shared drainage centerline.
    #[must_use]
    pub const fn distance_voxels(self) -> u32 {
        self.distance_voxels
    }
}

/// Stable resource-field sample at one world voxel.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ResourceFieldSampleV1 {
    role: Option<D4MaterialRoleV1>,
}

impl ResourceFieldSampleV1 {
    /// Returns the accepted resource Role, if any.
    #[must_use]
    pub const fn role(self) -> Option<D4MaterialRoleV1> {
        self.role
    }
}

/// Compiled, allocation-light V5 sampler used by chunk materialization.
#[derive(Clone, Debug)]
pub(crate) struct NaturalSamplerV1 {
    seed_root: WorldgenSeedRootV2,
    config: NaturalLayerConfigV1,
    layer_hash: NaturalLayerHashV1,
    hydrology: ProviderGenerationIdentityV1,
    surface_material_policy: SurfaceMaterialPolicyV1,
    nonnegative_surface_descent: bool,
    basin_seed: u64,
    geology_seed: u64,
    resource_seed: u64,
    soil_depth_seed: i64,
    trees: TreeMorphologySamplerV1,
    cover_seed: u64,
    drainage: DrainageFieldV3,
    receipts: Vec<RoleBindingReceiptV1>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SurfaceMaterialPolicyV1 {
    PreliminaryHeight,
    FinalSurfaceAndSlope,
}

/// Environmental and final-topography controls for one surface profile.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct SurfaceFormationInputV1 {
    maximum_neighbor_descent: u16,
    neighbor_height_delta_sum: i16,
    precipitation_per_1024: i16,
    infiltration_per_1024: i16,
    effective_runoff_q16: u32,
}

impl SurfaceFormationInputV1 {
    pub(crate) const fn new(
        maximum_neighbor_descent: u16,
        neighbor_height_delta_sum: i16,
        precipitation_per_1024: i16,
        infiltration_per_1024: i16,
        effective_runoff_q16: u32,
    ) -> Self {
        Self {
            maximum_neighbor_descent,
            neighbor_height_delta_sum,
            precipitation_per_1024,
            infiltration_per_1024,
            effective_runoff_q16,
        }
    }
}

/// Cached boundary between surface sediment and base rock for one column.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct SurfaceMaterialProfileV1 {
    subsurface_depth_voxels: u8,
}

/// One voxel query bound to the preliminary and final surface contracts.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct MaterializedNaturalSampleV1 {
    x: i64,
    y: i64,
    z: i64,
    preliminary_surface_y: i32,
    final_surface_y: i64,
    surface_profile: SurfaceMaterialProfileV1,
    style: TerrainStyleV1,
}

impl MaterializedNaturalSampleV1 {
    pub(crate) const fn new(
        position: (i64, i64, i64),
        preliminary_surface_y: i32,
        final_surface_y: i64,
        surface_profile: SurfaceMaterialProfileV1,
        style: TerrainStyleV1,
    ) -> Self {
        Self {
            x: position.0,
            y: position.1,
            z: position.2,
            preliminary_surface_y,
            final_surface_y,
            surface_profile,
            style,
        }
    }
}

impl SurfaceMaterialProfileV1 {
    const LEGACY: Self = Self {
        subsurface_depth_voxels: 3,
    };

    pub(crate) const fn legacy() -> Self {
        Self::LEGACY
    }

    const fn minimum_resource_depth(self) -> i64 {
        let below_sediment = self.subsurface_depth_voxels as i64 + 1;
        if below_sediment < 4 {
            4
        } else {
            below_sediment
        }
    }
}

impl SurfaceMaterialPolicyV1 {
    const fn for_geology_revision(revision: u32) -> Self {
        if revision >= 3 {
            Self::FinalSurfaceAndSlope
        } else {
            Self::PreliminaryHeight
        }
    }
}

impl NaturalSamplerV1 {
    pub(crate) fn continuous_river_sample(&self, x: i64, z: i64) -> RiverSampleV1 {
        let drainage = self.drainage.channel_sample(x, z);
        RiverSampleV1 {
            basin: RiverBasinIdV1::from_hash(domain_hash(
                BASIN_DOMAIN,
                &[
                    self.seed_root.as_bytes(),
                    &drainage.cell_x.to_be_bytes(),
                    &drainage.cell_z.to_be_bytes(),
                ],
            )),
            in_channel: drainage.distance_voxels <= u32::from(self.config.river_width_voxels),
            distance_voxels: drainage.distance_voxels,
        }
    }
    #[allow(
        clippy::too_many_arguments,
        reason = "natural-layer compilation keeps hash inputs explicit"
    )]
    pub(crate) fn compile(
        seed_root: WorldgenSeedRootV2,
        spine: &WorldgenConfigV1,
        limits: WorldgenLimitsV1,
        layer: NaturalLayerInputV1,
        providers: &ResolvedProvidersV1,
        bindings: &FrozenRoleBindingsV1,
        catalog: &D4BlockCatalogClosureV1,
        d4_targets: &[RoleBindingReceiptV1],
    ) -> WorldgenResult<Self> {
        layer.config.validate(spine)?;
        if catalog.blocks().len() < crate::D7_NATURAL_BLOCK_COUNT {
            return Err(WorldgenError::IncompleteNaturalCatalogClosure {
                minimum: crate::D7_NATURAL_BLOCK_COUNT,
                actual: catalog.blocks().len(),
            });
        }
        let mut offers = dummy_d4_offers(providers);
        offers.extend(layer.provider_offers);
        let resolved = ResolvedProvidersV1::resolve(offers, limits)?;
        let hydrology = required_natural(&resolved, ProviderSlotV1::Hydrology)?.clone();
        let geology = required_natural(&resolved, ProviderSlotV1::Geology)?.clone();
        let resources = required_natural(&resolved, ProviderSlotV1::Resources)?.clone();
        let vegetation = required_natural(&resolved, ProviderSlotV1::Vegetation)?.clone();
        let basin_seed = natural_sample_seed(BASIN_DOMAIN, seed_root);
        let geology_seed = natural_sample_seed(GEOLOGY_DOMAIN, seed_root);
        let resource_seed = natural_sample_seed(RESOURCE_DOMAIN, seed_root);
        let soil_depth_seed = natural_sample_seed_i64(SOIL_DEPTH_DOMAIN, seed_root);
        let trees = TreeMorphologySamplerV1::new(seed_root);
        let cover_seed = natural_sample_seed(COVER_DOMAIN, seed_root);
        let drainage = DrainageFieldV3::new(seed_root, layer.config.river_cell_edge_voxels);
        let receipts = resolve_natural_roles(&layer.vocabulary, bindings, catalog, d4_targets)?;
        let receipt_bytes =
            canonical_json_bytes(&receipts).map_err(|error| WorldgenError::CanonicalEncoding {
                kind: "natural role receipts",
                reason: error.to_string(),
            })?;
        let layer_hash = NaturalLayerHashV1::from_hash(domain_hash(
            NATURAL_LAYER_DOMAIN,
            &[
                layer.config.canonical_bytes()?.as_slice(),
                receipt_bytes.as_slice(),
                hydrology.provider_stable_id().as_str().as_bytes(),
                hydrology.implementation_fingerprint().as_bytes(),
                geology.provider_stable_id().as_str().as_bytes(),
                geology.implementation_fingerprint().as_bytes(),
                resources.provider_stable_id().as_str().as_bytes(),
                resources.implementation_fingerprint().as_bytes(),
                vegetation.provider_stable_id().as_str().as_bytes(),
                vegetation.implementation_fingerprint().as_bytes(),
            ],
        ));
        Ok(Self {
            seed_root,
            config: layer.config,
            layer_hash,
            hydrology,
            surface_material_policy: SurfaceMaterialPolicyV1::for_geology_revision(
                geology.algorithm_revision(),
            ),
            nonnegative_surface_descent: geology.algorithm_revision() >= 4,
            basin_seed,
            geology_seed,
            resource_seed,
            soil_depth_seed,
            trees,
            cover_seed,
            drainage,
            receipts,
        })
    }

    pub(crate) const fn layer_hash(&self) -> NaturalLayerHashV1 {
        self.layer_hash
    }

    pub(crate) const fn config(&self) -> &NaturalLayerConfigV1 {
        &self.config
    }

    pub(crate) fn receipts(&self) -> &[RoleBindingReceiptV1] {
        &self.receipts
    }

    pub(crate) const fn hydrology_identity(&self) -> &ProviderGenerationIdentityV1 {
        &self.hydrology
    }

    pub(crate) const fn uses_final_surface_materials(&self) -> bool {
        matches!(
            self.surface_material_policy,
            SurfaceMaterialPolicyV1::FinalSurfaceAndSlope
        )
    }

    /// Geology revision 4 treats higher neighbors as zero descent. Earlier
    /// frozen worlds retain their original signed-conversion behavior.
    pub(crate) fn surface_descent(&self, surface_y: i64, neighbor_y: i64) -> u16 {
        surface_descent(surface_y, neighbor_y, self.nonnegative_surface_descent)
    }

    /// Resolves one bounded sediment/bedrock profile from coherent geology,
    /// climate, runoff, and final four-neighbor topography.
    pub(crate) fn surface_material_profile(
        &self,
        x: i64,
        z: i64,
        input: SurfaceFormationInputV1,
    ) -> WorldgenResult<SurfaceMaterialProfileV1> {
        if self.surface_material_policy == SurfaceMaterialPolicyV1::PreliminaryHeight {
            return Ok(SurfaceMaterialProfileV1::LEGACY);
        }

        let normalized = soil_depth_control_per_1024(self.soil_depth_seed, x, z)?;
        Ok(surface_material_profile_from_control(normalized, input))
    }

    pub(crate) fn river_sample(&self, x: i64, z: i64) -> RiverSampleV1 {
        let local = self.local_river_sample(x, z);
        let basin = RiverBasinIdV1::from_hash(domain_hash(
            BASIN_DOMAIN,
            &[
                self.seed_root.as_bytes(),
                &local.cell_x.to_be_bytes(),
                &local.cell_z.to_be_bytes(),
            ],
        ));
        RiverSampleV1 {
            basin,
            in_channel: local.in_channel,
            distance_voxels: local.distance_voxels,
        }
    }

    pub(crate) fn in_river_channel(&self, x: i64, z: i64) -> bool {
        self.local_river_sample(x, z).in_channel
    }

    fn local_river_sample(&self, x: i64, z: i64) -> LocalRiverSampleV1 {
        self.local_river_sample_with_drainage(x, z, None)
    }

    fn local_river_sample_with_drainage(
        &self,
        x: i64,
        z: i64,
        reused: Option<DrainageSampleV3>,
    ) -> LocalRiverSampleV1 {
        let drainage = reused
            .filter(|sample| sample.edge_voxels == self.config.river_cell_edge_voxels)
            .unwrap_or_else(|| self.drainage.sample(x, z));
        let sparse = self.basin_rank(drainage.cell_x, drainage.cell_z)
            % u64::from(self.config.river_accumulation).saturating_add(2);
        LocalRiverSampleV1 {
            cell_x: drainage.cell_x,
            cell_z: drainage.cell_z,
            in_channel: drainage.distance_voxels <= u32::from(self.config.river_width_voxels)
                && sparse != 0,
            distance_voxels: drainage.distance_voxels,
        }
    }

    pub(crate) fn adjust_height_with_drainage(
        &self,
        x: i64,
        z: i64,
        height: i32,
        drainage: Option<DrainageSampleV3>,
    ) -> i32 {
        let in_channel = self
            .local_river_sample_with_drainage(x, z, drainage)
            .in_channel;
        self.adjust_height_for_channel(height, in_channel)
    }

    pub(crate) fn adjust_height_for_channel(&self, height: i32, in_channel: bool) -> i32 {
        if in_channel {
            height.saturating_sub(i32::from(self.config.river_incision_voxels))
        } else {
            height
        }
    }

    pub(crate) fn geologic_sample(
        &self,
        x: i64,
        y: i64,
        z: i64,
        height: i32,
        style: TerrainStyleV1,
    ) -> GeologicSampleV1 {
        let depth = i64::from(height).saturating_sub(y);
        self.geologic_sample_at_depth(x, y, z, depth, style, None)
    }

    pub(crate) fn materialized_geologic_sample(
        &self,
        input: MaterializedNaturalSampleV1,
    ) -> GeologicSampleV1 {
        let depth = match self.surface_material_policy {
            SurfaceMaterialPolicyV1::PreliminaryHeight => {
                i64::from(input.preliminary_surface_y).saturating_sub(input.y)
            }
            SurfaceMaterialPolicyV1::FinalSurfaceAndSlope => {
                input.final_surface_y.saturating_sub(input.y)
            }
        };
        let profile = matches!(
            self.surface_material_policy,
            SurfaceMaterialPolicyV1::FinalSurfaceAndSlope
        )
        .then_some(input.surface_profile);
        self.geologic_sample_at_depth(input.x, input.y, input.z, depth, input.style, profile)
    }

    fn geologic_sample_at_depth(
        &self,
        x: i64,
        y: i64,
        z: i64,
        depth: i64,
        style: TerrainStyleV1,
        profile: Option<SurfaceMaterialProfileV1>,
    ) -> GeologicSampleV1 {
        let roll = sample_hash_3d(self.geology_seed, x, y, z);
        let minimum_rock_depth =
            profile.map_or(4, SurfaceMaterialProfileV1::minimum_resource_depth);
        let intrusion = depth >= minimum_rock_depth
            && roll % 1_024 < u64::from(self.config.intrusion_threshold_per_1024);
        let stratum = if intrusion {
            match roll % 3 {
                0 => D4MaterialRoleV1::Tuff,
                1 => D4MaterialRoleV1::Calcite,
                _ => D4MaterialRoleV1::Dripstone,
            }
        } else if depth <= 0 {
            surface_role(style)
        } else if depth <= profile.map_or(3, |profile| i64::from(profile.subsurface_depth_voxels)) {
            shallow_role(style, roll)
        } else if depth <= 8 {
            upper_rock_role(style, roll)
        } else if depth <= 16 {
            if roll % 1_024 < u64::from(self.config.granite_threshold_per_1024) {
                D4MaterialRoleV1::Granite
            } else {
                D4MaterialRoleV1::Slate
            }
        } else {
            D4MaterialRoleV1::Deepstone
        };
        GeologicSampleV1 {
            role: stratum,
            depth,
            intrusion,
        }
    }

    pub(crate) fn resource_sample(
        &self,
        x: i64,
        y: i64,
        z: i64,
        height: i32,
        style: TerrainStyleV1,
    ) -> ResourceFieldSampleV1 {
        let depth = i64::from(height).saturating_sub(y);
        self.resource_sample_at_depth(x, y, z, depth, 4, style)
    }

    pub(crate) fn materialized_resource_sample(
        &self,
        input: MaterializedNaturalSampleV1,
    ) -> ResourceFieldSampleV1 {
        let depth = match self.surface_material_policy {
            SurfaceMaterialPolicyV1::PreliminaryHeight => {
                i64::from(input.preliminary_surface_y).saturating_sub(input.y)
            }
            SurfaceMaterialPolicyV1::FinalSurfaceAndSlope => {
                input.final_surface_y.saturating_sub(input.y)
            }
        };
        let minimum_depth = match self.surface_material_policy {
            SurfaceMaterialPolicyV1::PreliminaryHeight => 4,
            SurfaceMaterialPolicyV1::FinalSurfaceAndSlope => {
                input.surface_profile.minimum_resource_depth()
            }
        };
        self.resource_sample_at_depth(input.x, input.y, input.z, depth, minimum_depth, input.style)
    }

    fn resource_sample_at_depth(
        &self,
        x: i64,
        y: i64,
        z: i64,
        depth: i64,
        minimum_depth: i64,
        style: TerrainStyleV1,
    ) -> ResourceFieldSampleV1 {
        if depth < minimum_depth {
            return ResourceFieldSampleV1 { role: None };
        }
        let candidates = [
            (
                D4MaterialRoleV1::CoalResource,
                self.config.coal_threshold_per_1024,
                4_i64,
                14_i64,
            ),
            (
                D4MaterialRoleV1::IronResource,
                self.config.iron_threshold_per_1024,
                8,
                24,
            ),
            (
                D4MaterialRoleV1::TinResource,
                self.config.tin_threshold_per_1024,
                6,
                18,
            ),
            (D4MaterialRoleV1::CopperResource, 18, 8, 28),
            (
                D4MaterialRoleV1::GoldResource,
                self.config.gold_threshold_per_1024,
                12,
                32,
            ),
            (
                D4MaterialRoleV1::SulfurResource,
                self.config.sulfur_threshold_per_1024,
                10,
                26,
            ),
            (
                D4MaterialRoleV1::CrystalResource,
                self.config.crystal_threshold_per_1024,
                16,
                40,
            ),
        ];
        for (role, threshold, minimum, maximum) in candidates {
            if depth < minimum || depth > maximum {
                continue;
            }
            if style == TerrainStyleV1::AridBadlands && role == D4MaterialRoleV1::CoalResource {
                continue;
            }
            if style == TerrainStyleV1::BorealWetland && role == D4MaterialRoleV1::SulfurResource {
                continue;
            }
            let kind = u64::from(role.resource_discriminant());
            let salted = sample_hash_3d(
                self.resource_seed ^ kind.wrapping_mul(0x9e37_79b9_7f4a_7c15),
                x,
                y,
                z,
            );
            if salted % 1_024 < u64::from(threshold) {
                return ResourceFieldSampleV1 { role: Some(role) };
            }
        }
        ResourceFieldSampleV1 { role: None }
    }

    pub(crate) fn channel_bed_role(
        &self,
        x: i64,
        z: i64,
        style: TerrainStyleV1,
    ) -> D4MaterialRoleV1 {
        match style {
            TerrainStyleV1::Marine | TerrainStyleV1::TemperateWoodland => D4MaterialRoleV1::Silt,
            TerrainStyleV1::AridBadlands => D4MaterialRoleV1::TemperateGravel,
            TerrainStyleV1::BorealWetland => {
                if sample_hash_3d(self.basin_seed, x, 0, z) & 1 == 0 {
                    D4MaterialRoleV1::Ice
                } else {
                    D4MaterialRoleV1::Mud
                }
            }
        }
    }

    pub(crate) fn is_exclusive_tree_anchor<F>(
        &self,
        x: i64,
        z: i64,
        style: TerrainStyleV1,
        counters: &mut NaturalWorkCountersV1,
        mut style_at: F,
    ) -> bool
    where
        F: FnMut(i64, i64) -> TerrainStyleV1,
    {
        if !self.is_tree_candidate(x, z, style) {
            return false;
        }
        counters.tree_anchor_samples = counters.tree_anchor_samples.saturating_add(1);
        let rank = self.tree_rank(x, z);
        let radius = i64::from(self.config.tree_exclusion_radius_voxels);
        for offset_z in -radius..=radius {
            for offset_x in -radius..=radius {
                if offset_x == 0 && offset_z == 0 {
                    continue;
                }
                let other_x = x.saturating_add(offset_x);
                let other_z = z.saturating_add(offset_z);
                if !self.is_tree_candidate(other_x, other_z, style_at(other_x, other_z)) {
                    continue;
                }
                counters.exclusion_samples = counters.exclusion_samples.saturating_add(1);
                let other_rank = self.tree_rank(other_x, other_z);
                if (other_rank, other_x, other_z) < (rank, x, z) {
                    counters.exclusion_rejects = counters.exclusion_rejects.saturating_add(1);
                    return false;
                }
            }
        }
        true
    }

    pub(crate) fn may_have_tree_anchor(&self, x: i64, z: i64) -> bool {
        let maximum_threshold = self.config.pine_threshold_per_1024.max(14);
        self.trees.is_eligible(x, z, maximum_threshold)
    }

    pub(crate) fn ground_cover_role(
        &self,
        x: i64,
        z: i64,
        style: TerrainStyleV1,
        in_channel: bool,
    ) -> Option<D4MaterialRoleV1> {
        if in_channel {
            return None;
        }
        match style {
            TerrainStyleV1::TemperateWoodland => {
                if Self::sample_threshold(self.cover_seed, x, 0, z, 96) {
                    Some(D4MaterialRoleV1::WoodlandGroundCover)
                } else {
                    None
                }
            }
            TerrainStyleV1::BorealWetland => {
                if Self::sample_threshold(
                    self.cover_seed,
                    x,
                    0,
                    z,
                    self.config.moss_threshold_per_1024,
                ) {
                    Some(D4MaterialRoleV1::Moss)
                } else {
                    Some(D4MaterialRoleV1::Peat)
                }
            }
            TerrainStyleV1::Marine | TerrainStyleV1::AridBadlands => None,
        }
    }

    pub(crate) fn tree_descriptor(
        &self,
        x: i64,
        z: i64,
        style: TerrainStyleV1,
    ) -> Option<TreeDescriptorV1> {
        self.trees.descriptor(x, z, style)
    }

    pub(crate) fn tree_blueprint(
        &self,
        x: i64,
        support_y: i64,
        z: i64,
        descriptor: &TreeDescriptorV1,
    ) -> Option<TreeBlueprintV1> {
        self.trees.blueprint(x, support_y, z, descriptor)
    }

    pub(crate) const fn tree_radius() -> i64 {
        MAX_TREE_RADIUS_VOXELS
    }

    pub(crate) fn placement_predicates(
        diagnostics: GenerationDiagnosticsV1,
    ) -> Vec<PlacementPredicateReceiptV1> {
        vec![
            PlacementPredicateReceiptV1::new(
                PlacementPredicateKindV1::GeologyStratum,
                vec![
                    D4MaterialRoleV1::Deepstone,
                    D4MaterialRoleV1::Granite,
                    D4MaterialRoleV1::Slate,
                    D4MaterialRoleV1::Tuff,
                    D4MaterialRoleV1::Calcite,
                    D4MaterialRoleV1::Dripstone,
                ],
                diagnostics.geology_samples,
                diagnostics.geology_samples,
            ),
            PlacementPredicateReceiptV1::new(
                PlacementPredicateKindV1::RiverChannel,
                vec![
                    D4MaterialRoleV1::Silt,
                    D4MaterialRoleV1::Mud,
                    D4MaterialRoleV1::Ice,
                    D4MaterialRoleV1::TemperateGravel,
                ],
                diagnostics.river_samples,
                diagnostics.river_samples,
            ),
            PlacementPredicateReceiptV1::new(
                PlacementPredicateKindV1::StableResource,
                vec![
                    D4MaterialRoleV1::CoalResource,
                    D4MaterialRoleV1::IronResource,
                    D4MaterialRoleV1::TinResource,
                    D4MaterialRoleV1::CopperResource,
                    D4MaterialRoleV1::GoldResource,
                    D4MaterialRoleV1::SulfurResource,
                    D4MaterialRoleV1::CrystalResource,
                ],
                diagnostics.resource_samples,
                diagnostics.resource_accepts,
            ),
            PlacementPredicateReceiptV1::new(
                PlacementPredicateKindV1::VegetationExclusion,
                vec![D4MaterialRoleV1::WoodlandLog, D4MaterialRoleV1::BorealLog],
                diagnostics.vegetation_exclusion_samples,
                diagnostics.vegetation_exclusion_rejects,
            ),
            PlacementPredicateReceiptV1::new(
                PlacementPredicateKindV1::VegetationHabitat,
                vec![
                    D4MaterialRoleV1::WoodlandLog,
                    D4MaterialRoleV1::WoodlandLeaves,
                    D4MaterialRoleV1::WoodlandGroundCover,
                    D4MaterialRoleV1::BorealLog,
                    D4MaterialRoleV1::BorealLeaves,
                    D4MaterialRoleV1::Moss,
                    D4MaterialRoleV1::Peat,
                ],
                diagnostics.vegetation_habitat_samples,
                diagnostics
                    .vegetation_habitat_samples
                    .saturating_sub(diagnostics.vegetation_habitat_rejects),
            ),
        ]
    }

    fn is_tree_candidate(&self, x: i64, z: i64, style: TerrainStyleV1) -> bool {
        let threshold = match style {
            TerrainStyleV1::TemperateWoodland => 14,
            TerrainStyleV1::BorealWetland => self.config.pine_threshold_per_1024,
            TerrainStyleV1::Marine | TerrainStyleV1::AridBadlands => return false,
        };
        self.trees.is_eligible(x, z, threshold) && !self.in_river_channel(x, z)
    }

    fn tree_rank(&self, x: i64, z: i64) -> u64 {
        self.trees.priority(x, z)
    }

    fn basin_rank(&self, cell_x: i64, cell_z: i64) -> u64 {
        sample_hash_2d(self.basin_seed, cell_x, cell_z)
    }

    fn sample_threshold(seed: u64, x: i64, y: i64, z: i64, threshold: u16) -> bool {
        sample_hash_3d(seed, x, y, z) % 1_024 < u64::from(threshold)
    }
}

fn soil_depth_control_per_1024(seed: i64, x: i64, z: i64) -> WorldgenResult<i64> {
    let noise = open_simplex_2s_2d_v1(
        seed,
        FixedCoordinateV1::from_voxel(x, SOIL_DEPTH_SCALE_VOXELS)?,
        FixedCoordinateV1::from_voxel(z, SOIL_DEPTH_SCALE_VOXELS)?,
    )?;
    Ok(i64::from(noise.raw_q30())
        .saturating_mul(1_024)
        .div_euclid(FIXED_FIELD_UNIT_Q30)
        .clamp(-1_024, 1_024))
}

fn surface_material_profile_from_control(
    normalized: i64,
    input: SurfaceFormationInputV1,
) -> SurfaceMaterialProfileV1 {
    let mut depth = match normalized {
        ..=-257 => 1_i64,
        -256..=-1 => 2,
        0..=255 => 3,
        _ => 4,
    };

    let climate = i64::from(input.precipitation_per_1024)
        .saturating_add(i64::from(input.infiltration_per_1024));
    if climate >= 768 {
        depth = depth.saturating_add(1);
    } else if climate <= -768 {
        depth = depth.saturating_sub(1);
    }
    if input.effective_runoff_q16 >= 49_152 {
        depth = depth.saturating_sub(1);
    }

    if input.neighbor_height_delta_sum >= 2 {
        depth = depth.saturating_add(1);
    } else if input.neighbor_height_delta_sum <= -2 {
        depth = depth.saturating_sub(1);
    }
    depth = depth.clamp(0, 5);

    let subsurface_depth_voxels = match input.maximum_neighbor_descent {
        0 => u8::try_from(depth).unwrap_or(5),
        1 => u8::try_from(depth.min(1)).unwrap_or(1),
        _ => 0,
    };
    SurfaceMaterialProfileV1 {
        subsurface_depth_voxels,
    }
}

#[derive(Clone, Copy)]
struct LocalRiverSampleV1 {
    cell_x: i64,
    cell_z: i64,
    in_channel: bool,
    distance_voxels: u32,
}

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct NaturalWorkCountersV1 {
    pub(crate) geology_samples: u64,
    pub(crate) river_samples: u64,
    pub(crate) resource_samples: u64,
    pub(crate) resource_accepts: u64,
    pub(crate) tree_anchor_samples: u64,
    pub(crate) tree_anchor_accepts: u64,
    pub(crate) exclusion_samples: u64,
    pub(crate) exclusion_rejects: u64,
    pub(crate) habitat_samples: u64,
    pub(crate) habitat_rejects: u64,
    pub(crate) ground_cover_samples: u64,
    pub(crate) ground_cover_accepts: u64,
}

fn natural_sample_seed(domain: &[u8], seed_root: WorldgenSeedRootV2) -> u64 {
    hash_u64(domain, &[seed_root.as_bytes()])
}

fn natural_sample_seed_i64(domain: &[u8], seed_root: WorldgenSeedRootV2) -> i64 {
    i64::from_be_bytes(natural_sample_seed(domain, seed_root).to_be_bytes())
}

fn required_natural(
    providers: &ResolvedProvidersV1,
    slot: ProviderSlotV1,
) -> WorldgenResult<&ProviderGenerationIdentityV1> {
    providers
        .try_identity(slot)
        .ok_or(WorldgenError::MissingProvider { slot })
}

fn dummy_d4_offers(providers: &ResolvedProvidersV1) -> Vec<ProviderOfferV1> {
    ProviderSlotV1::ALL
        .into_iter()
        .map(|slot| ProviderOfferV1::new(slot, providers.identity(slot).clone()))
        .collect()
}

fn resolve_natural_roles(
    vocabulary: &NaturalRoleVocabularyV1,
    bindings: &FrozenRoleBindingsV1,
    catalog: &D4BlockCatalogClosureV1,
    d4_targets: &[RoleBindingReceiptV1],
) -> WorldgenResult<Vec<RoleBindingReceiptV1>> {
    let mut receipts = Vec::with_capacity(D4MaterialRoleV1::NATURAL.len());
    let mut used = d4_targets
        .iter()
        .map(|receipt| receipt.block_id().clone())
        .collect::<std::collections::BTreeSet<_>>();
    for (purpose, role) in vocabulary.iter() {
        let target = bindings
            .target(role)
            .ok_or_else(|| WorldgenError::MissingRoleBinding { role: role.clone() })?;
        if !catalog.contains(target) {
            return Err(WorldgenError::RoleTargetOutsideCatalog {
                role: role.clone(),
                target: Box::new(target.clone()),
            });
        }
        if !used.insert(target.clone()) {
            let first = d4_targets
                .iter()
                .find(|receipt| receipt.block_id() == target)
                .map_or("natural-duplicate", |receipt| receipt.purpose().as_str());
            return Err(WorldgenError::DuplicateRequiredRoleTarget {
                first_purpose: first,
                second_purpose: purpose.as_str(),
                target: Box::new(target.clone()),
            });
        }
        receipts.push(RoleBindingReceiptV1::from_parts(
            purpose,
            role.clone(),
            target.clone(),
        ));
    }
    Ok(receipts)
}

fn surface_descent(surface_y: i64, neighbor_y: i64, nonnegative: bool) -> u16 {
    let delta = surface_y.saturating_sub(neighbor_y);
    u16::try_from(if nonnegative { delta.max(0) } else { delta }).unwrap_or(u16::MAX)
}

fn surface_role(style: TerrainStyleV1) -> D4MaterialRoleV1 {
    match style {
        TerrainStyleV1::Marine => D4MaterialRoleV1::TemperateGravel,
        TerrainStyleV1::TemperateWoodland => D4MaterialRoleV1::TemperateSurface,
        TerrainStyleV1::AridBadlands => D4MaterialRoleV1::AridSand,
        TerrainStyleV1::BorealWetland => D4MaterialRoleV1::Snow,
    }
}

fn shallow_role(style: TerrainStyleV1, roll: u64) -> D4MaterialRoleV1 {
    match style {
        TerrainStyleV1::Marine => {
            if roll & 1 == 0 {
                D4MaterialRoleV1::Silt
            } else {
                D4MaterialRoleV1::TemperateGravel
            }
        }
        TerrainStyleV1::TemperateWoodland => match roll % 8 {
            0 => D4MaterialRoleV1::CoarseDirt,
            1 => D4MaterialRoleV1::RootedDirt,
            2 => D4MaterialRoleV1::TemperateClay,
            _ => D4MaterialRoleV1::TemperateSubsurface,
        },
        TerrainStyleV1::AridBadlands => {
            if roll & 1 == 0 {
                D4MaterialRoleV1::AridSandstone
            } else {
                D4MaterialRoleV1::AridRedSandstone
            }
        }
        TerrainStyleV1::BorealWetland => {
            if roll & 1 == 0 {
                D4MaterialRoleV1::Peat
            } else {
                D4MaterialRoleV1::Mud
            }
        }
    }
}

fn upper_rock_role(style: TerrainStyleV1, roll: u64) -> D4MaterialRoleV1 {
    match style {
        TerrainStyleV1::Marine => D4MaterialRoleV1::TemperateBaseRock,
        TerrainStyleV1::TemperateWoodland => {
            if roll.is_multiple_of(7) {
                D4MaterialRoleV1::TemperateSecondaryRock
            } else {
                D4MaterialRoleV1::TemperateBaseRock
            }
        }
        TerrainStyleV1::AridBadlands => D4MaterialRoleV1::AridBaseRock,
        TerrainStyleV1::BorealWetland => D4MaterialRoleV1::Slate,
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

fn invalid(field: &'static str, reason: impl Into<String>) -> WorldgenError {
    WorldgenError::InvalidConfig {
        field,
        reason: reason.into(),
    }
}

impl D4MaterialRoleV1 {
    const fn resource_discriminant(self) -> u8 {
        match self {
            Self::CoalResource => 0,
            Self::IronResource => 1,
            Self::TinResource => 2,
            Self::CopperResource => 3,
            Self::GoldResource => 4,
            Self::SulfurResource => 5,
            Self::CrystalResource => 6,
            _ => 255,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::{
        SurfaceFormationInputV1, soil_depth_control_per_1024, surface_material_profile_from_control,
    };

    const fn formation(
        descent: u16,
        neighbor_delta_sum: i16,
        precipitation: i16,
        infiltration: i16,
        runoff: u32,
    ) -> SurfaceFormationInputV1 {
        SurfaceFormationInputV1::new(
            descent,
            neighbor_delta_sum,
            precipitation,
            infiltration,
            runoff,
        )
    }

    #[test]
    fn coherent_control_produces_multiple_bounded_soil_depths() {
        let mut depths = BTreeSet::new();
        for z in (-1_024_i64..=1_024).step_by(128) {
            for x in (-1_024_i64..=1_024).step_by(128) {
                let control = soil_depth_control_per_1024(0x0003_0511, x, z)
                    .expect("fixed corpus lies inside the field envelope");
                let profile =
                    surface_material_profile_from_control(control, formation(0, 0, 0, 0, 32_768));
                assert!(profile.subsurface_depth_voxels <= 5);
                depths.insert(profile.subsurface_depth_voxels);
            }
        }
        assert!(
            depths.len() >= 3,
            "coherent fixed corpus collapsed to {depths:?}"
        );
    }

    #[test]
    fn climate_runoff_and_curvature_adjust_but_bound_the_profile() {
        let wet_concave =
            surface_material_profile_from_control(0, formation(0, 4, 768, 768, 16_384));
        let dry_exposed =
            surface_material_profile_from_control(0, formation(0, -4, -768, -768, 65_536));
        assert_eq!(wet_concave.subsurface_depth_voxels, 5);
        assert_eq!(dry_exposed.subsurface_depth_voxels, 0);
    }

    #[test]
    fn final_surface_descent_thins_soil_without_replacing_the_biome_top() {
        let neutral =
            |descent| surface_material_profile_from_control(0, formation(descent, 0, 0, 0, 32_768));
        assert_eq!(neutral(0).subsurface_depth_voxels, 3);
        assert_eq!(neutral(1).subsurface_depth_voxels, 1);
        assert_eq!(neutral(2).subsurface_depth_voxels, 0);
        assert_eq!(neutral(3).subsurface_depth_voxels, 0);
    }
}

#[cfg(test)]
mod descent_tests {
    use super::surface_descent;

    #[test]
    fn uphill_and_flat_neighbors_are_not_cliffs() {
        assert_eq!(surface_descent(64, 65, true), 0);
        assert_eq!(surface_descent(-10, -9, true), 0);
        assert_eq!(surface_descent(64, 64, true), 0);
        assert_eq!(surface_descent(64, 63, true), 1);
        assert_eq!(surface_descent(64, 60, true), 4);
        assert_eq!(surface_descent(i64::MAX, i64::MIN, true), u16::MAX);
        assert_eq!(surface_descent(i64::MIN, i64::MAX, true), 0);
    }

    #[test]
    fn revision_three_keeps_frozen_world_generation() {
        assert_eq!(surface_descent(64, 65, false), u16::MAX);
        assert_eq!(surface_descent(64, 60, false), 4);
    }
}
