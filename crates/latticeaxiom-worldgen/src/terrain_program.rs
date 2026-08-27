use std::collections::{BTreeMap, BTreeSet};

use latticeaxiom_core::{StableId, canonical_json_bytes};
use serde::{Deserialize, Deserializer, Serialize, de};

use crate::{
    ProviderGenerationIdentityV1, TerrainConfigV2, TerrainStyleV1, WorldgenError, WorldgenLimitsV1,
    WorldgenResult, WorldgenSeedRootV2,
    terrain_field::{TerrainColumnSampleV2, TerrainFieldV2},
};

/// Stable identity of one package-owned surface biome.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct SurfaceBiomeIdV1(StableId);

impl SurfaceBiomeIdV1 {
    /// Creates a typed surface-biome identity.
    ///
    /// # Errors
    ///
    /// Returns [`WorldgenError::InvalidTerrainProgram`] when `identity` is not
    /// a `biome` registration.
    pub fn new(identity: StableId) -> WorldgenResult<Self> {
        if identity.kind() != "biome" {
            return Err(WorldgenError::InvalidTerrainProgram {
                field: "biome_id",
                reason: format!("expected a `biome` identity, got `{}`", identity.as_str()),
            });
        }
        Ok(Self(identity))
    }

    /// Returns the underlying stable registration identity.
    #[must_use]
    pub const fn as_stable_id(&self) -> &StableId {
        &self.0
    }
}

impl<'de> Deserialize<'de> for SurfaceBiomeIdV1 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let identity = StableId::deserialize(deserializer)?;
        Self::new(identity).map_err(de::Error::custom)
    }
}

/// Coarse ecological ownership domain selected before terrain generation.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum SurfaceTerrainDomainV1 {
    /// Submerged continental shelf and ocean basin ecology.
    Marine,
    /// Coast, lowland, upland, plateau, and mountain ecology.
    Land,
}

/// Package-authored climate predicate for one ecological terrain program.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "kebab-case", tag = "kind")]
pub enum BiomeSelectionRuleV1 {
    /// Domain fallback selected only when no more-specific predicate matches.
    Fallback,
    /// Inclusive temperature and humidity rectangle in fixed field units.
    ClimateRange {
        /// Inclusive minimum temperature.
        min_temperature: i16,
        /// Inclusive maximum temperature.
        max_temperature: i16,
        /// Inclusive minimum humidity.
        min_humidity: i16,
        /// Inclusive maximum humidity.
        max_humidity: i16,
    },
    /// Matches hot/arid or exceptionally dry cells.
    AridityOrDry {
        /// Exclusive lower bound for `temperature - humidity / 3`.
        min_aridity: i16,
        /// Exclusive upper bound for humidity.
        max_humidity: i16,
    },
}

impl BiomeSelectionRuleV1 {
    fn validate(self) -> WorldgenResult<()> {
        if let Self::ClimateRange {
            min_temperature,
            max_temperature,
            min_humidity,
            max_humidity,
        } = self
            && (min_temperature > max_temperature || min_humidity > max_humidity)
        {
            return Err(WorldgenError::InvalidTerrainProgram {
                field: "selection",
                reason: "climate range minimum must not exceed its maximum".to_owned(),
            });
        }
        Ok(())
    }

    fn matches(self, temperature: i64, humidity: i64) -> bool {
        match self {
            Self::Fallback => false,
            Self::ClimateRange {
                min_temperature,
                max_temperature,
                min_humidity,
                max_humidity,
            } => {
                (i64::from(min_temperature)..=i64::from(max_temperature)).contains(&temperature)
                    && (i64::from(min_humidity)..=i64::from(max_humidity)).contains(&humidity)
            }
            Self::AridityOrDry {
                min_aridity,
                max_humidity,
            } => {
                temperature.saturating_sub(humidity.div_euclid(3)) > i64::from(min_aridity)
                    || humidity < i64::from(max_humidity)
            }
        }
    }
}

/// Generic, platform-executed terrain algorithms a biome may select.
///
/// Algorithm parameters live in the resolved terrain configuration. Product
/// packages choose an algorithm per biome; the platform never assigns a
/// product biome to an algorithm on its own.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum TerrainBaseAlgorithmV1 {
    /// The V2 continental, erosion, ridge, plateau, and local-detail graph.
    ContinentalComposite,
    /// Always-submerged shelf and basin terrain.
    MarineBasin,
    /// Coast-to-mountain land terrain with moderate relief.
    TemperateRelief,
    /// Elevated, erosion-exposed plateau and mountain terrain.
    AridHighlands,
    /// Saturated lowlands with compressed local relief.
    BorealLowlands,
}

/// Package-owned selection of a terrain provider for one surface biome.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SurfaceBiomeTerrainProgramV1 {
    biome_id: SurfaceBiomeIdV1,
    material_style: TerrainStyleV1,
    domain: SurfaceTerrainDomainV1,
    selection_priority: u16,
    selection: BiomeSelectionRuleV1,
    algorithm: TerrainBaseAlgorithmV1,
    provider: ProviderGenerationIdentityV1,
}

impl SurfaceBiomeTerrainProgramV1 {
    /// Creates a resolved surface-biome terrain program.
    #[must_use]
    pub const fn new(
        biome_id: SurfaceBiomeIdV1,
        material_style: TerrainStyleV1,
        domain: SurfaceTerrainDomainV1,
        selection_priority: u16,
        selection: BiomeSelectionRuleV1,
        algorithm: TerrainBaseAlgorithmV1,
        provider: ProviderGenerationIdentityV1,
    ) -> Self {
        Self {
            biome_id,
            material_style,
            domain,
            selection_priority,
            selection,
            algorithm,
            provider,
        }
    }

    /// Returns the package-owned biome identity.
    #[must_use]
    pub const fn biome_id(&self) -> &SurfaceBiomeIdV1 {
        &self.biome_id
    }

    /// Returns the compatibility material style used by the current materializer.
    #[must_use]
    pub const fn material_style(&self) -> TerrainStyleV1 {
        self.material_style
    }

    /// Returns the coarse ocean/land ownership domain.
    #[must_use]
    pub const fn domain(&self) -> SurfaceTerrainDomainV1 {
        self.domain
    }

    /// Returns the deterministic selection priority; lower values win.
    #[must_use]
    pub const fn selection_priority(&self) -> u16 {
        self.selection_priority
    }

    /// Returns the package-authored climate predicate.
    #[must_use]
    pub const fn selection(&self) -> BiomeSelectionRuleV1 {
        self.selection
    }

    /// Returns the selected terrain algorithm.
    #[must_use]
    pub const fn algorithm(&self) -> TerrainBaseAlgorithmV1 {
        self.algorithm
    }

    /// Returns the output-affecting terrain-provider identity.
    #[must_use]
    pub const fn provider(&self) -> &ProviderGenerationIdentityV1 {
        &self.provider
    }
}

#[derive(Clone, Debug)]
struct CompiledTerrainProgramV1 {
    authored: SurfaceBiomeTerrainProgramV1,
    field: TerrainFieldV2,
}

#[derive(Clone, Debug)]
pub(crate) struct ResolvedTerrainProgramsV1 {
    programs: BTreeMap<TerrainStyleV1, CompiledTerrainProgramV1>,
    ordered: Vec<SurfaceBiomeTerrainProgramV1>,
    domain_field: TerrainFieldV2,
    terrain_config: TerrainConfigV2,
}

impl ResolvedTerrainProgramsV1 {
    pub(crate) fn resolve(
        authored: Vec<SurfaceBiomeTerrainProgramV1>,
        require_boreal: bool,
        limits: WorldgenLimitsV1,
        seed_root: WorldgenSeedRootV2,
        terrain_config: TerrainConfigV2,
    ) -> WorldgenResult<Self> {
        if authored.len() > usize::from(limits.max_provider_offers.get()) {
            return Err(WorldgenError::CollectionLimitExceeded {
                kind: "surface biome terrain programs",
                actual: authored.len(),
                limit: usize::from(limits.max_provider_offers.get()),
            });
        }

        validate_programs(&authored, require_boreal)?;
        let mut ordered = authored;
        ordered.sort_by(|left, right| {
            left.material_style
                .cmp(&right.material_style)
                .then_with(|| left.biome_id.cmp(&right.biome_id))
        });
        let programs = ordered
            .iter()
            .cloned()
            .map(|program| {
                (
                    program.material_style,
                    CompiledTerrainProgramV1 {
                        authored: program,
                        field: TerrainFieldV2::new(seed_root, terrain_config),
                    },
                )
            })
            .collect();
        Ok(Self {
            programs,
            ordered,
            domain_field: TerrainFieldV2::new(seed_root, terrain_config),
            terrain_config,
        })
    }

    pub(crate) fn sample(&self, style: TerrainStyleV1, x: i64, z: i64) -> TerrainColumnSampleV2 {
        let program = self
            .programs
            .get(&style)
            .unwrap_or_else(|| missing_validated_program(style));
        let sample = program.field.sample(x, z);
        apply_algorithm(program.authored.algorithm, sample, self.terrain_config)
    }

    pub(crate) fn select_style(
        &self,
        x: i64,
        z: i64,
        temperature: i64,
        humidity: i64,
    ) -> TerrainStyleV1 {
        let domain = self.domain_at(x, z);
        self.ordered
            .iter()
            .filter(|program| {
                program.domain == domain && program.selection.matches(temperature, humidity)
            })
            .min_by_key(|program| (program.selection_priority, &program.biome_id))
            .or_else(|| {
                self.ordered.iter().find(|program| {
                    program.domain == domain && program.selection == BiomeSelectionRuleV1::Fallback
                })
            })
            .unwrap_or_else(|| missing_validated_domain_fallback(domain))
            .material_style
    }

    pub(crate) fn runner_up_style(
        &self,
        winner: TerrainStyleV1,
        x: i64,
        z: i64,
        temperature: i64,
        humidity: i64,
    ) -> TerrainStyleV1 {
        let domain = self.domain_at(x, z);
        self.ordered
            .iter()
            .filter(|program| {
                program.domain == domain
                    && program.material_style != winner
                    && program.selection.matches(temperature, humidity)
            })
            .min_by_key(|program| (program.selection_priority, &program.biome_id))
            .or_else(|| {
                self.ordered.iter().find(|program| {
                    program.domain == domain
                        && program.material_style != winner
                        && program.selection == BiomeSelectionRuleV1::Fallback
                })
            })
            .or_else(|| {
                self.ordered.iter().find(|program| {
                    program.domain != domain && program.selection == BiomeSelectionRuleV1::Fallback
                })
            })
            .unwrap_or_else(|| missing_validated_domain_fallback(domain))
            .material_style
    }

    fn domain_at(&self, x: i64, z: i64) -> SurfaceTerrainDomainV1 {
        if self.domain_field.is_land(x, z) {
            SurfaceTerrainDomainV1::Land
        } else {
            SurfaceTerrainDomainV1::Marine
        }
    }

    pub(crate) fn ordered(&self) -> &[SurfaceBiomeTerrainProgramV1] {
        &self.ordered
    }

    pub(crate) fn canonical_bytes(&self) -> WorldgenResult<Vec<u8>> {
        canonical_json_bytes(&self.ordered).map_err(|error| WorldgenError::CanonicalEncoding {
            kind: "surface biome terrain programs",
            reason: error.to_string(),
        })
    }
}

fn apply_algorithm(
    algorithm: TerrainBaseAlgorithmV1,
    mut sample: TerrainColumnSampleV2,
    config: TerrainConfigV2,
) -> TerrainColumnSampleV2 {
    let sea = config.world.sea_level_y;
    match algorithm {
        TerrainBaseAlgorithmV1::ContinentalComposite => sample,
        TerrainBaseAlgorithmV1::MarineBasin => {
            let maximum_bed = sea.saturating_sub(3);
            sample.height = sample.height.min(maximum_bed);
            let depth = sea.saturating_sub(sample.height);
            sample.family = if depth > i32::from(config.landmass.ocean_depth_voxels) / 2 {
                crate::TerrainFamilyV2::DeepOcean
            } else {
                crate::TerrainFamilyV2::ShallowOcean
            };
            sample.surface_water_y = None;
            sample
        }
        TerrainBaseAlgorithmV1::TemperateRelief => force_land(sample, sea),
        TerrainBaseAlgorithmV1::AridHighlands => {
            sample = force_land(sample, sea);
            let uplift = i32::from(config.relief.plateau_height_voxels).div_euclid(2);
            sample.height = sample.height.saturating_add(uplift).min(
                config
                    .world
                    .ceiling_y
                    .saturating_sub(i32::from(config.underground.lava_depth_voxels).max(8)),
            );
            if matches!(
                sample.family,
                crate::TerrainFamilyV2::Plains
                    | crate::TerrainFamilyV2::RollingHills
                    | crate::TerrainFamilyV2::Wetland
            ) {
                sample.family = crate::TerrainFamilyV2::Plateau;
            }
            sample.surface_water_y = None;
            sample
        }
        TerrainBaseAlgorithmV1::BorealLowlands => {
            sample = force_land(sample, sea);
            let lowland_ceiling = sea
                .saturating_add(i32::from(config.relief.base_height_voxels))
                .saturating_add(i32::from(config.relief.hill_height_voxels).div_euclid(3));
            if !matches!(
                sample.family,
                crate::TerrainFamilyV2::MountainRange | crate::TerrainFamilyV2::Volcanic
            ) {
                sample.height = sample.height.min(lowland_ceiling);
                sample.family = crate::TerrainFamilyV2::Wetland;
            }
            sample
        }
    }
}

fn force_land(mut sample: TerrainColumnSampleV2, sea: i32) -> TerrainColumnSampleV2 {
    if sample.family != crate::TerrainFamilyV2::LakeBasin && sample.height <= sea {
        sample.height = sample.height.max(sea.saturating_add(1));
        sample.family = crate::TerrainFamilyV2::Coast;
        sample.surface_water_y = None;
    }
    sample
}

fn validate_programs(
    authored: &[SurfaceBiomeTerrainProgramV1],
    require_boreal: bool,
) -> WorldgenResult<()> {
    let mut styles = BTreeMap::<TerrainStyleV1, Vec<SurfaceBiomeIdV1>>::new();
    let mut biomes = BTreeSet::new();
    let mut fingerprints = BTreeMap::new();
    let mut selection_priorities = BTreeSet::new();
    let mut fallback_counts = BTreeMap::<SurfaceTerrainDomainV1, usize>::new();
    for program in authored {
        program.selection.validate()?;
        validate_program_domain(program)?;
        register_selection_contract(program, &mut selection_priorities, &mut fallback_counts)?;
        styles
            .entry(program.material_style)
            .or_default()
            .push(program.biome_id.clone());
        if !biomes.insert(program.biome_id.clone()) {
            return Err(WorldgenError::DuplicateTerrainBiome {
                biome: program.biome_id.as_stable_id().clone(),
            });
        }
        let provider = &program.provider;
        let key = (
            provider.provider_stable_id().clone(),
            provider.contract_major(),
            provider.algorithm_revision(),
        );
        if let Some(previous) =
            fingerprints.insert(key.clone(), *provider.implementation_fingerprint())
            && previous != *provider.implementation_fingerprint()
        {
            return Err(WorldgenError::ProviderFingerprintConflict {
                provider: key.0,
                contract_major: key.1.get(),
                algorithm_revision: key.2,
            });
        }
    }

    for domain in [SurfaceTerrainDomainV1::Marine, SurfaceTerrainDomainV1::Land] {
        if fallback_counts.get(&domain).copied().unwrap_or_default() != 1 {
            return Err(WorldgenError::InvalidTerrainProgram {
                field: "selection",
                reason: format!("domain `{domain:?}` must define exactly one fallback"),
            });
        }
    }

    let required = if require_boreal {
        &[
            TerrainStyleV1::Marine,
            TerrainStyleV1::TemperateWoodland,
            TerrainStyleV1::AridBadlands,
            TerrainStyleV1::BorealWetland,
        ][..]
    } else {
        &[
            TerrainStyleV1::Marine,
            TerrainStyleV1::TemperateWoodland,
            TerrainStyleV1::AridBadlands,
        ][..]
    };
    for style in required {
        match styles.remove(style).unwrap_or_default().as_slice() {
            [] => return Err(WorldgenError::MissingTerrainProgram { style: *style }),
            [_] => {}
            conflicts => {
                return Err(WorldgenError::ConflictingTerrainPrograms {
                    style: *style,
                    biomes: conflicts
                        .iter()
                        .map(|biome| biome.as_stable_id().clone())
                        .collect(),
                });
            }
        }
    }
    if let Some((style, conflicts)) = styles
        .into_iter()
        .find(|(_, conflicts)| conflicts.len() > 1)
    {
        return Err(WorldgenError::ConflictingTerrainPrograms {
            style,
            biomes: conflicts
                .into_iter()
                .map(|biome| biome.as_stable_id().clone())
                .collect(),
        });
    }
    Ok(())
}

fn validate_program_domain(program: &SurfaceBiomeTerrainProgramV1) -> WorldgenResult<()> {
    let marine_style = program.material_style == TerrainStyleV1::Marine;
    let marine_domain = program.domain == SurfaceTerrainDomainV1::Marine;
    if marine_style == marine_domain {
        return Ok(());
    }
    Err(WorldgenError::InvalidTerrainProgram {
        field: "domain",
        reason: format!(
            "style `{:?}` is incompatible with domain `{:?}`",
            program.material_style, program.domain
        ),
    })
}

fn register_selection_contract(
    program: &SurfaceBiomeTerrainProgramV1,
    priorities: &mut BTreeSet<(SurfaceTerrainDomainV1, u16)>,
    fallback_counts: &mut BTreeMap<SurfaceTerrainDomainV1, usize>,
) -> WorldgenResult<()> {
    if program.selection == BiomeSelectionRuleV1::Fallback {
        fallback_counts
            .entry(program.domain)
            .and_modify(|count| *count = count.saturating_add(1))
            .or_insert(1);
        return Ok(());
    }
    if priorities.insert((program.domain, program.selection_priority)) {
        return Ok(());
    }
    Err(WorldgenError::InvalidTerrainProgram {
        field: "selection_priority",
        reason: format!(
            "domain `{:?}` defines priority {} more than once",
            program.domain, program.selection_priority
        ),
    })
}

fn missing_validated_program(style: TerrainStyleV1) -> &'static CompiledTerrainProgramV1 {
    panic!("validated terrain programs lost material style `{style:?}`")
}

fn missing_validated_domain_fallback(
    domain: SurfaceTerrainDomainV1,
) -> &'static SurfaceBiomeTerrainProgramV1 {
    panic!("validated terrain programs lost domain fallback `{domain:?}`")
}

#[cfg(test)]
mod tests {
    use std::num::NonZeroU32;

    use latticeaxiom_core::CanonicalHash;

    use super::*;
    use crate::{TerrainConfigV2, WorldSeedV1};

    fn program(style: TerrainStyleV1, path: &str) -> SurfaceBiomeTerrainProgramV1 {
        let biome = SurfaceBiomeIdV1::new(
            format!("test:biome/{path}")
                .parse()
                .expect("test biome identity is valid"),
        )
        .expect("test identity has biome kind");
        let provider = format!("test:worldgen-provider/{path}@1")
            .parse()
            .expect("test provider identity is valid");
        SurfaceBiomeTerrainProgramV1::new(
            biome,
            style,
            if style == TerrainStyleV1::Marine {
                SurfaceTerrainDomainV1::Marine
            } else {
                SurfaceTerrainDomainV1::Land
            },
            if matches!(
                style,
                TerrainStyleV1::Marine | TerrainStyleV1::TemperateWoodland
            ) {
                u16::MAX
            } else {
                10
            },
            if matches!(
                style,
                TerrainStyleV1::Marine | TerrainStyleV1::TemperateWoodland
            ) {
                BiomeSelectionRuleV1::Fallback
            } else {
                BiomeSelectionRuleV1::ClimateRange {
                    min_temperature: -1_024,
                    max_temperature: 1_024,
                    min_humidity: -1_024,
                    max_humidity: 1_024,
                }
            },
            TerrainBaseAlgorithmV1::ContinentalComposite,
            ProviderGenerationIdentityV1::new(
                provider,
                NonZeroU32::MIN,
                1,
                CanonicalHash::digest(path),
            ),
        )
    }

    #[test]
    fn resolution_requires_every_enabled_surface_style() {
        let result = ResolvedTerrainProgramsV1::resolve(
            vec![
                program(TerrainStyleV1::Marine, "marine"),
                program(TerrainStyleV1::TemperateWoodland, "temperate"),
            ],
            false,
            WorldgenLimitsV1::default(),
            WorldgenSeedRootV2::from_world_seed(WorldSeedV1::from_csprng_bytes([7; 32])),
            TerrainConfigV2::representative_test_baseline(),
        );
        assert!(matches!(
            result,
            Err(WorldgenError::MissingTerrainProgram {
                style: TerrainStyleV1::AridBadlands
            })
        ));
    }

    #[test]
    fn biome_identity_rejects_non_biome_kinds() {
        let identity = "test:block/not-a-biome"
            .parse()
            .expect("test stable identity is valid");
        assert!(matches!(
            SurfaceBiomeIdV1::new(identity),
            Err(WorldgenError::InvalidTerrainProgram {
                field: "biome_id",
                ..
            })
        ));
    }
}
