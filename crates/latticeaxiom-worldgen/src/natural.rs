//! Package-owned V5 natural terrain, geology, hydrology, resource, and vegetation
//! layer compiled on top of the D4 coordinator.
//!
//! The layer is optional. D4 plans omit it and keep byte-identical output.
//! Host crates never supply Terrenia concrete IDs; frozen Role bindings do.

use latticeaxiom_core::canonical_json_bytes;
use serde::{Deserialize, Serialize};

use crate::{
    D4BlockCatalogClosureV1, D4MaterialRoleV1, FrozenRoleBindingsV1, NaturalLayerHashV1,
    NaturalRoleVocabularyV1, PlacementPredicateKindV1, PlacementPredicateReceiptV1,
    ProviderGenerationIdentityV1, ProviderOfferV1, ProviderSlotV1, RiverBasinIdV1,
    RoleBindingReceiptV1, TerrainStyleV1, WorldgenConfigV1, WorldgenError, WorldgenLimitsV1,
    WorldgenResult, WorldgenSeedRootV2,
    config::MAX_TERRAIN_RELIEF,
    hashes::{domain_hash, hash_u64, sample_hash_2d, sample_hash_3d},
    provider::ResolvedProvidersV1,
    territory::BorealTerrainParamsV1,
};

const NATURAL_LAYER_DOMAIN: &[u8] = b"latticeaxiom.natural-layer.v1\0";
const BASIN_DOMAIN: &[u8] = b"latticeaxiom.natural-basin.v1\0";
const GEOLOGY_DOMAIN: &[u8] = b"latticeaxiom.natural-geology.v1\0";
const RESOURCE_DOMAIN: &[u8] = b"latticeaxiom.natural-resource.v1\0";
const TREE_DOMAIN: &[u8] = b"latticeaxiom.natural-tree.v1\0";
const COVER_DOMAIN: &[u8] = b"latticeaxiom.natural-cover.v1\0";
const MAX_TREE_RADIUS: i64 = 2;
const MAX_TREE_HEIGHT: i64 = 6;

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
            tree_exclusion_radius_voxels: 5,
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
            1,
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
        if maximum_surface.saturating_add(7) > i64::from(spine.world_ceiling_y) {
            return Err(invalid(
                "world_ceiling_y",
                "must leave seven voxels above the natural maximum surface",
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

    /// Returns Chebyshev distance to the nearest channel centerline.
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
    basin_seed: u64,
    geology_seed: u64,
    resource_seed: u64,
    tree_seed: u64,
    cover_seed: u64,
    river_warp_seeds: [u64; 2],
    boreal: BorealTerrainParamsV1,
    receipts: Vec<RoleBindingReceiptV1>,
}

impl NaturalSamplerV1 {
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
        let boreal_provider = required_natural(&resolved, ProviderSlotV1::BorealTerrain)?.clone();
        let basin_seed = natural_sample_seed(BASIN_DOMAIN, seed_root);
        let geology_seed = natural_sample_seed(GEOLOGY_DOMAIN, seed_root);
        let resource_seed = natural_sample_seed(RESOURCE_DOMAIN, seed_root);
        let tree_seed = natural_sample_seed(TREE_DOMAIN, seed_root);
        let cover_seed = natural_sample_seed(COVER_DOMAIN, seed_root);
        let river_warp_seeds = [
            hash_u64(BASIN_DOMAIN, &[seed_root.as_bytes(), b"river-warp-z"]),
            hash_u64(BASIN_DOMAIN, &[seed_root.as_bytes(), b"river-warp-x"]),
        ];
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
                boreal_provider.provider_stable_id().as_str().as_bytes(),
                boreal_provider.implementation_fingerprint().as_bytes(),
            ],
        ));
        Ok(Self {
            seed_root,
            config: layer.config,
            layer_hash,
            hydrology,
            basin_seed,
            geology_seed,
            resource_seed,
            tree_seed,
            cover_seed,
            river_warp_seeds,
            boreal: BorealTerrainParamsV1,
            receipts,
        })
    }

    pub(crate) const fn layer_hash(&self) -> NaturalLayerHashV1 {
        self.layer_hash
    }

    pub(crate) const fn config(&self) -> &NaturalLayerConfigV1 {
        &self.config
    }

    pub(crate) fn boreal_params(&self) -> BorealTerrainParamsV1 {
        self.boreal.clone()
    }

    pub(crate) fn receipts(&self) -> &[RoleBindingReceiptV1] {
        &self.receipts
    }

    pub(crate) const fn hydrology_identity(&self) -> &ProviderGenerationIdentityV1 {
        &self.hydrology
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
        let edge = i64::from(self.config.river_cell_edge_voxels.max(1));
        let width = i64::from(self.config.river_width_voxels);
        let cell = coarse_cell(x, z, edge);
        let warp_z = interpolated_river_warp(self.river_warp_seeds[0], z, edge);
        let warp_x = interpolated_river_warp(self.river_warp_seeds[1], x, edge);
        let east_west = (x.saturating_add(warp_z)).rem_euclid(edge);
        let north_south = (z.saturating_add(warp_x)).rem_euclid(edge);
        let dist_ew = east_west.min(edge.saturating_sub(east_west));
        let dist_ns = north_south.min(edge.saturating_sub(north_south));
        let nearest = dist_ew.min(dist_ns);
        let distance = u32::try_from(nearest.max(0)).unwrap_or(u32::MAX);
        let sparse = self.basin_rank(cell.0, cell.1)
            % u64::from(self.config.river_accumulation).saturating_add(2);
        LocalRiverSampleV1 {
            cell_x: cell.0,
            cell_z: cell.1,
            in_channel: nearest <= width && sparse != 0,
            distance_voxels: distance,
        }
    }

    pub(crate) fn adjust_height(&self, x: i64, z: i64, height: i32) -> i32 {
        self.adjust_height_for_channel(height, self.in_river_channel(x, z))
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
        let roll = sample_hash_3d(self.geology_seed, x, y, z);
        let intrusion =
            depth >= 4 && roll % 1_024 < u64::from(self.config.intrusion_threshold_per_1024);
        let stratum = if intrusion {
            match roll % 3 {
                0 => D4MaterialRoleV1::Tuff,
                1 => D4MaterialRoleV1::Calcite,
                _ => D4MaterialRoleV1::Dripstone,
            }
        } else if depth <= 0 {
            surface_role(style)
        } else if depth <= 3 {
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
        if depth < 4 {
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
            TerrainStyleV1::TemperateWoodland => D4MaterialRoleV1::Silt,
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

    pub(crate) fn is_exclusive_tree_anchor(
        &self,
        x: i64,
        z: i64,
        style: TerrainStyleV1,
        counters: &mut NaturalWorkCountersV1,
    ) -> bool {
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
                if !self.is_tree_candidate(other_x, other_z, style) {
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
        counters.tree_anchor_accepts = counters.tree_anchor_accepts.saturating_add(1);
        true
    }

    pub(crate) fn may_have_tree_anchor(&self, x: i64, z: i64) -> bool {
        let maximum_threshold = self.config.pine_threshold_per_1024.max(14);
        Self::sample_threshold(self.tree_seed, x, 0, z, maximum_threshold)
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
            TerrainStyleV1::AridBadlands => None,
        }
    }

    pub(crate) fn tree_roles(
        style: TerrainStyleV1,
    ) -> Option<(D4MaterialRoleV1, D4MaterialRoleV1)> {
        match style {
            TerrainStyleV1::TemperateWoodland => Some((
                D4MaterialRoleV1::WoodlandLog,
                D4MaterialRoleV1::WoodlandLeaves,
            )),
            TerrainStyleV1::BorealWetland => {
                Some((D4MaterialRoleV1::BorealLog, D4MaterialRoleV1::BorealLeaves))
            }
            TerrainStyleV1::AridBadlands => None,
        }
    }

    pub(crate) const fn tree_radius() -> i64 {
        MAX_TREE_RADIUS
    }

    pub(crate) const fn tree_height() -> i64 {
        MAX_TREE_HEIGHT
    }

    pub(crate) fn placement_predicates(
        geology_samples: u64,
        river_samples: u64,
        resource_samples: u64,
        resource_accepts: u64,
        exclusion_samples: u64,
        exclusion_rejects: u64,
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
                geology_samples,
                geology_samples,
            ),
            PlacementPredicateReceiptV1::new(
                PlacementPredicateKindV1::RiverChannel,
                vec![
                    D4MaterialRoleV1::Silt,
                    D4MaterialRoleV1::Mud,
                    D4MaterialRoleV1::Ice,
                    D4MaterialRoleV1::TemperateGravel,
                ],
                river_samples,
                river_samples,
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
                resource_samples,
                resource_accepts,
            ),
            PlacementPredicateReceiptV1::new(
                PlacementPredicateKindV1::VegetationExclusion,
                vec![D4MaterialRoleV1::WoodlandLog, D4MaterialRoleV1::BorealLog],
                exclusion_samples,
                exclusion_rejects,
            ),
        ]
    }

    fn is_tree_candidate(&self, x: i64, z: i64, style: TerrainStyleV1) -> bool {
        let threshold = match style {
            TerrainStyleV1::TemperateWoodland => 14,
            TerrainStyleV1::BorealWetland => self.config.pine_threshold_per_1024,
            TerrainStyleV1::AridBadlands => return false,
        };
        Self::sample_threshold(self.tree_seed, x, 0, z, threshold) && !self.in_river_channel(x, z)
    }

    fn tree_rank(&self, x: i64, z: i64) -> u64 {
        sample_hash_2d(self.tree_seed, x, z)
    }

    fn basin_rank(&self, cell_x: i64, cell_z: i64) -> u64 {
        sample_hash_2d(self.basin_seed, cell_x, cell_z)
    }

    fn sample_threshold(seed: u64, x: i64, y: i64, z: i64, threshold: u16) -> bool {
        sample_hash_3d(seed, x, y, z) % 1_024 < u64::from(threshold)
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
    pub(crate) ground_cover_samples: u64,
    pub(crate) ground_cover_accepts: u64,
}

fn natural_sample_seed(domain: &[u8], seed_root: WorldgenSeedRootV2) -> u64 {
    hash_u64(domain, &[seed_root.as_bytes()])
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

fn surface_role(style: TerrainStyleV1) -> D4MaterialRoleV1 {
    match style {
        TerrainStyleV1::TemperateWoodland => D4MaterialRoleV1::TemperateSurface,
        TerrainStyleV1::AridBadlands => D4MaterialRoleV1::AridSand,
        TerrainStyleV1::BorealWetland => D4MaterialRoleV1::Snow,
    }
}

fn shallow_role(style: TerrainStyleV1, roll: u64) -> D4MaterialRoleV1 {
    match style {
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

fn coarse_cell(x: i64, z: i64, edge: i64) -> (i64, i64) {
    (x.div_euclid(edge), z.div_euclid(edge))
}

fn interpolated_river_warp(seed: u64, coordinate: i64, edge: i64) -> i64 {
    let cell = coordinate.div_euclid(edge);
    let local = coordinate.rem_euclid(edge);
    let start = river_warp_anchor(seed, cell, edge);
    let end = river_warp_anchor(seed, cell.saturating_add(1), edge);
    start
        .saturating_mul(edge.saturating_sub(local))
        .saturating_add(end.saturating_mul(local))
        .div_euclid(edge)
}

fn river_warp_anchor(seed: u64, cell: i64, edge: i64) -> i64 {
    let coordinate_bits = u64::from_be_bytes(cell.to_be_bytes());
    let rank = mix_coordinate(seed, coordinate_bits);
    i64::try_from(rank % u64::try_from(edge).unwrap_or(1))
        .unwrap_or_default()
        .saturating_sub(edge.saturating_div(2))
}

fn mix_coordinate(seed: u64, coordinate: u64) -> u64 {
    // The SplitMix64 finalizer is deterministic, allocation-free, and adequate
    // for keyed procedural variation after the plan has derived the seed.
    let mut mixed = seed.wrapping_add(coordinate.wrapping_mul(0x9e37_79b9_7f4a_7c15));
    mixed = (mixed ^ (mixed >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    mixed = (mixed ^ (mixed >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    mixed ^ (mixed >> 31)
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
