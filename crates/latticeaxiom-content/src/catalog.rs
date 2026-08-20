use std::collections::{BTreeMap, BTreeSet};

use latticeaxiom_core::{CanonicalHash, StableId, canonical_json_bytes, canonical_json_hash};
use serde::{Deserialize, Serialize};

use crate::{
    BiomeDefinitionV1, BlockDefinitionV1, BlockRuleReferencesV1, BlockStatePropertyV1,
    BlockStateSemanticsV1, BlockStateV1, ContentError, ContentResult, FluidDefinitionV1,
    ValidatedBlockDefinitionV1, biome::normalize_biome_definition,
    block::validate_block_definition, fluid::validate_fluid_definition, header::validate_exact_id,
};

/// Content catalog schema major compiled by this crate.
pub const CONTENT_CATALOG_SCHEMA_MAJOR: u32 = 1;

/// Caller-supplied in-memory compilation safety limits.
///
/// These are not snapshot palette or persistence wire limits. Callers may
/// choose tighter profile limits. These limits apply after a DTO has been
/// decoded and before compiler-owned cloning/allocation or publication; the
/// byte-stream owner must independently cap input bytes and decode depth.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ContentCatalogLimitsV1 {
    /// Maximum block definitions.
    pub max_blocks: usize,
    /// Maximum fluid definitions.
    pub max_fluids: usize,
    /// Maximum biome definitions.
    pub max_biomes: usize,
    /// Maximum material Role bindings.
    pub max_material_role_bindings: usize,
    /// Maximum state properties on one block.
    pub max_state_properties_per_block: usize,
    /// Maximum allowed values on one state property.
    pub max_values_per_state_property: usize,
    /// Maximum explicit states on one block.
    pub max_states_per_block: usize,
    /// Maximum biome influence radius in cells.
    pub max_biome_spatial_cells: u16,
    /// Maximum channel offers on one biome.
    pub max_biome_channel_offers: usize,
    /// Maximum emitted intent kinds on one biome.
    pub max_biome_emitted_intents: usize,
    /// Maximum channel values or intents emitted per chunk.
    pub max_biome_emissions_per_chunk: u32,
    /// Maximum fixed fluid tick period.
    pub max_fluid_tick_period: u16,
    /// Maximum fluid cells processed per tick.
    pub max_fluid_cells_per_tick: u32,
    /// Maximum queued fluid frontier entries.
    pub max_fluid_queue_depth: u32,
    /// Maximum fluid input plus output bytes in flight.
    pub max_fluid_in_flight_bytes: u32,
}

impl Default for ContentCatalogLimitsV1 {
    fn default() -> Self {
        Self {
            max_blocks: 4_096,
            max_fluids: 256,
            max_biomes: 256,
            max_material_role_bindings: 4_096,
            max_state_properties_per_block: 16,
            max_values_per_state_property: 64,
            max_states_per_block: 4_096,
            max_biome_spatial_cells: 4_096,
            max_biome_channel_offers: 64,
            max_biome_emitted_intents: 64,
            max_biome_emissions_per_chunk: 1_048_576,
            max_fluid_tick_period: 4_096,
            max_fluid_cells_per_tick: 1_048_576,
            max_fluid_queue_depth: 4_194_304,
            max_fluid_in_flight_bytes: 67_108_864,
        }
    }
}

/// Authored rows accepted by [`ContentCatalogV1::compile`].
///
/// Deserializing this representation is not itself a limit-aware streaming
/// decoder. An owner reading untrusted bytes must bound the byte stream and
/// nesting before constructing this DTO, then apply [`ContentCatalogLimitsV1`]
/// during compilation.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ContentCatalogInputV1 {
    /// Catalog schema major.
    pub schema_major: u32,
    /// Intrinsic block definitions in arbitrary discovery order.
    pub blocks: Vec<BlockDefinitionV1>,
    /// Fluid definitions in arbitrary discovery order.
    pub fluids: Vec<FluidDefinitionV1>,
    /// Biome definitions in arbitrary discovery order.
    pub biomes: Vec<BiomeDefinitionV1>,
    /// Material purpose, Role, and concrete block bindings.
    pub material_role_bindings: Vec<MaterialRoleBindingV1>,
}

/// Package-owned material purpose resolved through a Role to a concrete block.
///
/// This explicit binding row is not a generic material ontology. Worldgen
/// consumes `block`, never a Tag scan or namespace-order fallback.
#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(deny_unknown_fields)]
pub struct MaterialRoleBindingV1 {
    /// Versioned package-owned functional purpose.
    pub purpose: StableId,
    /// Versioned `block-role` identity.
    pub role: StableId,
    /// Exact concrete `block` output.
    pub block: StableId,
}

/// Validated material Role binding.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct ValidatedMaterialRoleBindingV1(MaterialRoleBindingV1);

impl ValidatedMaterialRoleBindingV1 {
    /// Returns the package-owned purpose.
    #[must_use]
    pub const fn purpose(&self) -> &StableId {
        &self.0.purpose
    }

    /// Returns the semantic Role identity.
    #[must_use]
    pub const fn role(&self) -> &StableId {
        &self.0.role
    }

    /// Returns the exact concrete block output.
    #[must_use]
    pub const fn block(&self) -> &StableId {
        &self.0.block
    }
}

/// Validated, canonically ordered D9 content catalog.
///
/// Fields are private so deserialization cannot bypass validation. Compile is
/// the sole public constructor.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ContentCatalogV1 {
    schema_major: u32,
    blocks: Vec<ValidatedBlockDefinitionV1>,
    fluids: Vec<FluidDefinitionV1>,
    biomes: Vec<BiomeDefinitionV1>,
    material_role_bindings: Vec<ValidatedMaterialRoleBindingV1>,
}

impl ContentCatalogV1 {
    /// Validates, resolves, and canonically sorts untrusted catalog rows.
    ///
    /// # Errors
    ///
    /// Returns a typed error for unsupported schema majors, invalid definition
    /// contracts, exceeded limits, duplicate IDs, unresolved Role/fallback
    /// references, fallback cycles, or canonical encoding failures.
    pub fn compile(
        input: ContentCatalogInputV1,
        limits: ContentCatalogLimitsV1,
    ) -> ContentResult<Self> {
        if input.schema_major != CONTENT_CATALOG_SCHEMA_MAJOR {
            return Err(ContentError::UnsupportedCatalogSchema {
                expected: CONTENT_CATALOG_SCHEMA_MAJOR,
                actual: input.schema_major,
            });
        }
        enforce_count("block_definitions", input.blocks.len(), limits.max_blocks)?;
        enforce_count("fluid_definitions", input.fluids.len(), limits.max_fluids)?;
        enforce_count("biome_definitions", input.biomes.len(), limits.max_biomes)?;
        enforce_count(
            "material_role_bindings",
            input.material_role_bindings.len(),
            limits.max_material_role_bindings,
        )?;

        let mut blocks = input
            .blocks
            .into_iter()
            .map(|definition| {
                validate_block_definition(
                    definition,
                    limits.max_state_properties_per_block,
                    limits.max_values_per_state_property,
                    limits.max_states_per_block,
                )
            })
            .collect::<ContentResult<Vec<_>>>()?;
        blocks.sort_by(|left, right| {
            left.definition()
                .header
                .stable_id
                .cmp(&right.definition().header.stable_id)
        });
        reject_duplicate_ids(
            "block definition",
            blocks
                .iter()
                .map(|definition| &definition.definition().header.stable_id),
        )?;

        let mut fluids = input.fluids;
        for definition in &fluids {
            validate_fluid_definition(
                definition,
                limits.max_fluid_tick_period,
                limits.max_fluid_cells_per_tick,
                limits.max_fluid_queue_depth,
                limits.max_fluid_in_flight_bytes,
            )?;
        }
        fluids.sort_by(|left, right| left.header.stable_id.cmp(&right.header.stable_id));
        reject_duplicate_ids(
            "fluid definition",
            fluids.iter().map(|definition| &definition.header.stable_id),
        )?;

        let mut biomes = input
            .biomes
            .into_iter()
            .map(|definition| {
                normalize_biome_definition(
                    definition,
                    limits.max_biome_spatial_cells,
                    limits.max_biome_channel_offers,
                    limits.max_biome_emitted_intents,
                    limits.max_biome_emissions_per_chunk,
                )
            })
            .collect::<ContentResult<Vec<_>>>()?;
        biomes.sort_by(|left, right| left.header.stable_id.cmp(&right.header.stable_id));
        reject_duplicate_ids(
            "biome definition",
            biomes.iter().map(|definition| &definition.header.stable_id),
        )?;
        validate_biome_fallbacks(&biomes)?;

        let block_ids = blocks
            .iter()
            .map(|definition| definition.definition().header.stable_id.clone())
            .collect::<BTreeSet<_>>();
        let material_role_bindings =
            validate_material_bindings(input.material_role_bindings, &block_ids)?;

        Ok(Self {
            schema_major: CONTENT_CATALOG_SCHEMA_MAJOR,
            blocks,
            fluids,
            biomes,
            material_role_bindings,
        })
    }

    /// Returns the canonical schema major.
    #[must_use]
    pub const fn schema_major(&self) -> u32 {
        self.schema_major
    }

    /// Returns canonically sorted block definitions.
    #[must_use]
    pub fn blocks(&self) -> &[ValidatedBlockDefinitionV1] {
        &self.blocks
    }

    /// Returns canonically sorted fluid definitions.
    #[must_use]
    pub fn fluids(&self) -> &[FluidDefinitionV1] {
        &self.fluids
    }

    /// Returns canonically sorted biome definitions.
    #[must_use]
    pub fn biomes(&self) -> &[BiomeDefinitionV1] {
        &self.biomes
    }

    /// Returns canonically sorted material Role bindings.
    #[must_use]
    pub fn material_role_bindings(&self) -> &[ValidatedMaterialRoleBindingV1] {
        &self.material_role_bindings
    }

    /// Finds an exact block definition.
    #[must_use]
    pub fn block(&self, id: &StableId) -> Option<&ValidatedBlockDefinitionV1> {
        self.blocks
            .binary_search_by(|candidate| candidate.definition().header.stable_id.cmp(id))
            .ok()
            .map(|index| &self.blocks[index])
    }

    /// Finds an exact fluid definition.
    #[must_use]
    pub fn fluid(&self, id: &StableId) -> Option<&FluidDefinitionV1> {
        self.fluids
            .binary_search_by(|candidate| candidate.header.stable_id.cmp(id))
            .ok()
            .map(|index| &self.fluids[index])
    }

    /// Finds an exact biome definition.
    #[must_use]
    pub fn biome(&self, id: &StableId) -> Option<&BiomeDefinitionV1> {
        self.biomes
            .binary_search_by(|candidate| candidate.header.stable_id.cmp(id))
            .ok()
            .map(|index| &self.biomes[index])
    }

    /// Encodes the canonical authoritative projection used for the hash.
    ///
    /// # Errors
    ///
    /// Returns an error if canonical JSON encoding fails.
    pub fn canonical_authoritative_bytes(&self) -> ContentResult<Vec<u8>> {
        canonical_json_bytes(&self.authoritative_projection()).map_err(ContentError::from)
    }

    /// Computes SHA-256 over [`Self::canonical_authoritative_bytes`].
    ///
    /// This is a deterministic verification helper, not a new named hash
    /// domain or a replacement for the registration owner's semantic hash.
    ///
    /// # Errors
    ///
    /// Returns an error if canonical JSON encoding fails.
    pub fn canonical_authoritative_hash(&self) -> ContentResult<CanonicalHash> {
        canonical_json_hash(&self.authoritative_projection()).map_err(ContentError::from)
    }

    fn authoritative_projection(&self) -> AuthoritativeCatalogProjection<'_> {
        AuthoritativeCatalogProjection {
            schema_major: self.schema_major,
            blocks: self
                .blocks
                .iter()
                .map(|definition| {
                    let definition = definition.definition();
                    AuthoritativeBlockProjection {
                        header: &definition.header,
                        state_schema: &definition.state_schema,
                        states: &definition.states,
                        default_state: &definition.default_state,
                        rules: &definition.rules,
                    }
                })
                .collect(),
            fluids: self
                .fluids
                .iter()
                .map(|definition| AuthoritativeFluidProjection {
                    header: &definition.header,
                    state_schema: &definition.state_schema,
                    replace_or_displace_predicate: &definition.replace_or_displace_predicate,
                    collision_policy: &definition.collision_policy,
                    selection_policy: &definition.selection_policy,
                    fixed_tick_period: definition.fixed_tick_period,
                    update_policy: definition.update_policy,
                })
                .collect(),
            biomes: &self.biomes,
            material_role_bindings: &self.material_role_bindings,
        }
    }
}

#[derive(Serialize)]
struct AuthoritativeCatalogProjection<'a> {
    schema_major: u32,
    blocks: Vec<AuthoritativeBlockProjection<'a>>,
    fluids: Vec<AuthoritativeFluidProjection<'a>>,
    biomes: &'a [BiomeDefinitionV1],
    material_role_bindings: &'a [ValidatedMaterialRoleBindingV1],
}

#[derive(Serialize)]
struct AuthoritativeBlockProjection<'a> {
    header: &'a crate::ContentHeaderV1,
    state_schema: &'a [BlockStatePropertyV1],
    states: &'a [BlockStateSemanticsV1],
    default_state: &'a BlockStateV1,
    rules: &'a BlockRuleReferencesV1,
}

#[derive(Serialize)]
struct AuthoritativeFluidProjection<'a> {
    header: &'a crate::ContentHeaderV1,
    state_schema: &'a latticeaxiom_core::SchemaId,
    replace_or_displace_predicate: &'a StableId,
    collision_policy: &'a crate::FluidCollisionPolicyV1,
    selection_policy: &'a crate::FluidSelectionPolicyV1,
    fixed_tick_period: std::num::NonZeroU16,
    update_policy: crate::BoundedFluidUpdatePolicyV1,
}

fn enforce_count(resource: &'static str, actual: usize, limit: usize) -> ContentResult<()> {
    if actual > limit {
        return Err(ContentError::LimitExceeded {
            resource,
            actual,
            limit,
        });
    }
    Ok(())
}

fn reject_duplicate_ids<'a>(
    resource: &'static str,
    ids: impl IntoIterator<Item = &'a StableId>,
) -> ContentResult<()> {
    let mut previous: Option<&StableId> = None;
    for id in ids {
        if previous == Some(id) {
            return Err(ContentError::Duplicate {
                resource,
                id: id.to_string(),
            });
        }
        previous = Some(id);
    }
    Ok(())
}

fn validate_material_bindings(
    mut bindings: Vec<MaterialRoleBindingV1>,
    block_ids: &BTreeSet<StableId>,
) -> ContentResult<Vec<ValidatedMaterialRoleBindingV1>> {
    bindings.sort();
    let mut purposes = BTreeSet::new();
    let mut roles = BTreeSet::new();
    let mut validated = Vec::with_capacity(bindings.len());
    for binding in bindings {
        if binding.purpose.major().is_none() {
            return Err(ContentError::InvalidMaterialBinding {
                purpose: binding.purpose,
                reason: "material purpose must be versioned",
            });
        }
        if binding.role.kind() != "block-role" || binding.role.major().is_none() {
            return Err(ContentError::InvalidMaterialBinding {
                purpose: binding.purpose,
                reason: "Role must be a versioned `block-role` identity",
            });
        }
        validate_exact_id(&binding.block, "block", "material Role target")?;
        if !block_ids.contains(&binding.block) {
            return Err(ContentError::UnknownReference {
                owner: binding.role.to_string(),
                resource: "block",
                target: binding.block,
            });
        }
        if !purposes.insert(binding.purpose.clone()) {
            return Err(ContentError::Duplicate {
                resource: "material purpose binding",
                id: binding.purpose.to_string(),
            });
        }
        if !roles.insert(binding.role.clone()) {
            return Err(ContentError::Duplicate {
                resource: "material Role binding",
                id: binding.role.to_string(),
            });
        }
        validated.push(ValidatedMaterialRoleBindingV1(binding));
    }
    validated.sort_by(|left, right| left.role().cmp(right.role()));
    Ok(validated)
}

fn validate_biome_fallbacks(biomes: &[BiomeDefinitionV1]) -> ContentResult<()> {
    let fallbacks = biomes
        .iter()
        .map(|biome| (biome.header.stable_id.clone(), biome.fallback.clone()))
        .collect::<BTreeMap<_, _>>();
    for (biome, fallback) in &fallbacks {
        if let Some(fallback) = fallback
            && !fallbacks.contains_key(fallback)
        {
            return Err(ContentError::UnknownReference {
                owner: biome.to_string(),
                resource: "biome fallback",
                target: fallback.clone(),
            });
        }
    }
    for start in fallbacks.keys() {
        let mut path = Vec::new();
        let mut positions = BTreeMap::new();
        let mut current = Some(start);
        while let Some(id) = current {
            if let Some(position) = positions.insert(id.clone(), path.len()) {
                let mut cycle = path[position..].to_vec();
                cycle.sort();
                cycle.dedup();
                return Err(ContentError::BiomeFallbackCycle { cycle });
            }
            path.push(id.clone());
            current = fallbacks.get(id).and_then(Option::as_ref);
        }
    }
    Ok(())
}
