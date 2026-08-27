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
}

/// Package-owned selection of a terrain provider for one surface biome.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SurfaceBiomeTerrainProgramV1 {
    biome_id: SurfaceBiomeIdV1,
    material_style: TerrainStyleV1,
    algorithm: TerrainBaseAlgorithmV1,
    provider: ProviderGenerationIdentityV1,
}

impl SurfaceBiomeTerrainProgramV1 {
    /// Creates a resolved surface-biome terrain program.
    #[must_use]
    pub const fn new(
        biome_id: SurfaceBiomeIdV1,
        material_style: TerrainStyleV1,
        algorithm: TerrainBaseAlgorithmV1,
        provider: ProviderGenerationIdentityV1,
    ) -> Self {
        Self {
            biome_id,
            material_style,
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
        Ok(Self { programs, ordered })
    }

    pub(crate) fn sample(&self, style: TerrainStyleV1, x: i64, z: i64) -> TerrainColumnSampleV2 {
        let program = self
            .programs
            .get(&style)
            .unwrap_or_else(|| missing_validated_program(style));
        match program.authored.algorithm {
            TerrainBaseAlgorithmV1::ContinentalComposite => program.field.sample(x, z),
        }
    }

    pub(crate) fn contains(&self, style: TerrainStyleV1) -> bool {
        self.programs.contains_key(&style)
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

fn validate_programs(
    authored: &[SurfaceBiomeTerrainProgramV1],
    require_boreal: bool,
) -> WorldgenResult<()> {
    let mut styles = BTreeMap::<TerrainStyleV1, Vec<SurfaceBiomeIdV1>>::new();
    let mut biomes = BTreeSet::new();
    let mut fingerprints = BTreeMap::new();
    for program in authored {
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

    let required = if require_boreal {
        &[
            TerrainStyleV1::TemperateWoodland,
            TerrainStyleV1::AridBadlands,
            TerrainStyleV1::BorealWetland,
        ][..]
    } else {
        &[
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

fn missing_validated_program(style: TerrainStyleV1) -> &'static CompiledTerrainProgramV1 {
    panic!("validated terrain programs lost material style `{style:?}`")
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
            vec![program(TerrainStyleV1::TemperateWoodland, "temperate")],
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
