use std::collections::{BTreeMap, BTreeSet};

use latticeaxiom_core::{CanonicalHash, StableId, canonical_json_bytes};
use latticeaxiom_storage::{ChunkCoordinate, ChunkRevisionExpectation, DimensionId};
use serde::{Deserialize, Serialize};

use crate::{
    AdjacentEpochSnapshotV1, AquiferSampleV1, BoundaryAdapterDeclarationV1, BoundaryReceiptV1,
    CaveFaceFieldRequestV1, CaveFaceOccupancyValidationV1, CaveLayerEntranceV1, CaveLayerPortalV1,
    CaveOccupancyArbitrationV1, CaveOwnedDomainV1, CaveTopologyAlgorithmV1,
    CaveTopologyLayerInputV1, CaveVoxelPassabilityReceiptV1, CellEpochStateV1,
    D4BlockCatalogClosureV1, D4MaterialRoleV1, D4RoleVocabularyV1, DrainageSampleV1,
    ExistingSnapshotEvidenceV1, FrozenRoleBindingsV1, GenerationEpochIdV1, GenerationInputHashV1,
    GenerationProvenanceHashV1, GeneratorFingerprintV1, GeologicSampleV1,
    HydrologyFaceContinuityV1, HydrologyFluidBindingsV1, HydrologyOccupancyCandidateV1,
    HydrologyOccupancyHashV1, HydrologyOccupancyInputV1, HydrologyOccupancySampleV1,
    LockedClosureFingerprintV1, NaturalLayerInputV1, PlanActivationIdV1, PlanningCellCoordinateV1,
    ProviderGenerationIdentityV1, ProviderOfferV1, ProviderSlotV1, ResourceFieldSampleV1,
    RiverSampleV1, SnapshotChecksumV1, TerrainConfigHashV2, TerrainConfigV2, TerrainFamilyV2,
    TerrainStyleV1, TerritoryQueryV1, WorldSeedV1, WorldgenConfigHashV1, WorldgenConfigV1,
    WorldgenError, WorldgenLimitsV1, WorldgenResult, WorldgenSeedRootV2,
    cave::{CaveFieldPortalPlanV1, CaveSamplerV1, snapshot_checksum},
    epoch::validate_epoch_boundaries,
    hashes::{concatenated_hash, domain_hash, hash_u64, sample_hash_3d},
    hydrology::{
        HydrologySamplerV1, hydrology_adjacent_chunk, hydrology_face_axis, hydrology_face_hash,
    },
    natural::{NaturalSamplerV1, NaturalWorkCountersV1},
    provider::ResolvedProvidersV1,
    territory::TerritorySamplerV1,
};

const LOCK_FINGERPRINT_DOMAIN: &[u8] = b"latticeaxiom.locked-closure.v1\0";
const GENERATION_INPUT_DOMAIN: &[u8] = b"latticeaxiom.generation-input.v1\0";
const GENERATION_PROVENANCE_DOMAIN: &[u8] = b"latticeaxiom.generation-provenance.v1\0";
const GENERATION_EPOCH_DOMAIN: &[u8] = b"latticeaxiom.generation-epoch.v1\0";
const MATERIAL_DOMAIN: &[u8] = b"latticeaxiom.d4-material.v1\0";
const TREE_DOMAIN: &[u8] = b"latticeaxiom.d4-tree.v1\0";
const GROUND_COVER_DOMAIN: &[u8] = b"latticeaxiom.d4-ground-cover.v1\0";
const SNAPSHOT_SCHEMA: &str = "latticeaxiom:d4-snapshot-candidate@1";
const MAX_TREE_RADIUS: i64 = 2;
const MAX_TREE_HEIGHT: i64 = 6;

/// All immutable inputs required to compile a D4 generation plan.
#[derive(Clone, Debug)]
pub struct GenerationPlanInputV1 {
    dimension: DimensionId,
    world_seed: WorldSeedV1,
    config: WorldgenConfigV1,
    terrain_config: TerrainConfigV2,
    terrain_config_explicit: bool,
    generation_plan_revision: u64,
    plan_activation_id: PlanActivationIdV1,
    provider_offers: Vec<ProviderOfferV1>,
    role_vocabulary: D4RoleVocabularyV1,
    role_bindings: FrozenRoleBindingsV1,
    block_catalog: D4BlockCatalogClosureV1,
    authoritative_semantic_receipt: CanonicalHash,
    locked_receipts: Vec<CanonicalHash>,
    limits: WorldgenLimitsV1,
    natural_layer: Option<NaturalLayerInputV1>,
    cave_topology: Option<CaveTopologyLayerInputV1>,
    hydrology_occupancy: Option<HydrologyOccupancyInputV1>,
}

impl GenerationPlanInputV1 {
    /// Creates a complete plan compilation input.
    #[must_use]
    #[allow(
        clippy::too_many_arguments,
        reason = "plan inputs are explicit hash boundaries"
    )]
    pub fn new(
        dimension: DimensionId,
        world_seed: WorldSeedV1,
        config: WorldgenConfigV1,
        generation_plan_revision: u64,
        plan_activation_id: PlanActivationIdV1,
        provider_offers: Vec<ProviderOfferV1>,
        role_vocabulary: D4RoleVocabularyV1,
        role_bindings: FrozenRoleBindingsV1,
        block_catalog: D4BlockCatalogClosureV1,
        authoritative_semantic_receipt: CanonicalHash,
        locked_receipts: Vec<CanonicalHash>,
        limits: WorldgenLimitsV1,
    ) -> Self {
        let terrain_config = TerrainConfigV2::for_legacy_spine(&config);
        Self {
            dimension,
            world_seed,
            config,
            terrain_config,
            terrain_config_explicit: false,
            generation_plan_revision,
            plan_activation_id,
            provider_offers,
            role_vocabulary,
            role_bindings,
            block_catalog,
            authoritative_semantic_receipt,
            locked_receipts,
            limits,
            natural_layer: None,
            cave_topology: None,
            hydrology_occupancy: None,
        }
    }

    /// Replaces the compatibility terrain profile with a resolved V2 config.
    #[must_use]
    pub const fn with_terrain_config(mut self, terrain_config: TerrainConfigV2) -> Self {
        self.terrain_config = terrain_config;
        self.terrain_config_explicit = true;
        self
    }

    /// Attaches the optional V5 natural layer. D4-only plans omit this.
    #[must_use]
    pub fn with_natural_layer(mut self, natural_layer: NaturalLayerInputV1) -> Self {
        self.natural_layer = Some(natural_layer);
        self
    }

    /// Attaches the optional V6 cave-topology realization layer.
    #[must_use]
    pub fn with_cave_topology_layer(mut self, cave_topology: CaveTopologyLayerInputV1) -> Self {
        self.cave_topology = Some(cave_topology);
        self
    }

    /// Attaches the optional V6 hydrology occupancy layer. D4/D7 snapshots omit this.
    #[must_use]
    pub fn with_hydrology_occupancy(
        mut self,
        hydrology_occupancy: HydrologyOccupancyInputV1,
    ) -> Self {
        self.hydrology_occupancy = Some(hydrology_occupancy);
        self
    }
}

/// Concrete semantic Role binding consumed by the materializer.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RoleBindingReceiptV1 {
    purpose: D4MaterialRoleV1,
    role_id: StableId,
    block_id: StableId,
}

impl RoleBindingReceiptV1 {
    pub(crate) const fn from_parts(
        purpose: D4MaterialRoleV1,
        role_id: StableId,
        block_id: StableId,
    ) -> Self {
        Self {
            purpose,
            role_id,
            block_id,
        }
    }

    /// Returns the functional generator purpose.
    #[must_use]
    pub const fn purpose(&self) -> D4MaterialRoleV1 {
        self.purpose
    }

    /// Returns the package-owned semantic Role ID.
    #[must_use]
    pub const fn role_id(&self) -> &StableId {
        &self.role_id
    }

    /// Returns the frozen concrete block ID used in output.
    #[must_use]
    pub const fn block_id(&self) -> &StableId {
        &self.block_id
    }
}

/// Scaffold-local predicate kinds used for D4 placement diagnostics.
///
/// This enum is not the frozen package-facing Predicate registration schema.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum PlacementPredicateKindV1 {
    /// Minimum-cover plus bounded cave-field occupancy decision.
    CaveOccupancy,
    /// Temperate style and deterministic tree-anchor decision.
    TreeAnchor,
    /// Temperate surface and deterministic ground-cover decision.
    GroundCover,
    /// Deep-rock deterministic copper replacement decision.
    CopperResource,
    /// Queryable geologic stratum or intrusion decision.
    GeologyStratum,
    /// Surface river-channel occupancy decision.
    RiverChannel,
    /// Stable resource-field replacement decision.
    StableResource,
    /// Exclusion-radius vegetation anchor decision.
    VegetationExclusion,
}

/// Deterministic Predicate evaluation counts and the Roles they may place.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PlacementPredicateReceiptV1 {
    predicate: PlacementPredicateKindV1,
    placement_roles: Vec<D4MaterialRoleV1>,
    evaluations: u64,
    accepted: u64,
}

impl PlacementPredicateReceiptV1 {
    pub(crate) fn new(
        predicate: PlacementPredicateKindV1,
        placement_roles: Vec<D4MaterialRoleV1>,
        evaluations: u64,
        accepted: u64,
    ) -> Self {
        Self {
            predicate,
            placement_roles,
            evaluations,
            accepted,
        }
    }

    /// Returns the closed predicate kind.
    #[must_use]
    pub const fn predicate(&self) -> PlacementPredicateKindV1 {
        self.predicate
    }

    /// Returns the functional Roles this predicate is permitted to place.
    #[must_use]
    pub fn placement_roles(&self) -> &[D4MaterialRoleV1] {
        &self.placement_roles
    }

    /// Returns the number of bounded evaluations.
    #[must_use]
    pub const fn evaluations(&self) -> u64 {
        self.evaluations
    }

    /// Returns the number of accepted placements.
    #[must_use]
    pub const fn accepted(&self) -> u64 {
        self.accepted
    }
}
/// Reproducible bounded-work counters for one generated chunk.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct GenerationDiagnosticsV1 {
    /// Exact number of output voxels.
    pub voxel_count: u64,
    /// Exact number of horizontal columns.
    pub column_count: u64,
    /// Territory queries performed for columns and vegetation anchors.
    pub territory_queries: u64,
    /// Deterministic height samples performed.
    pub height_samples: u64,
    /// Coarse cave/void samples performed.
    pub cave_samples: u64,
    /// Cave samples accepted as void.
    pub cave_void_accepts: u64,
    /// Candidate tree anchors inspected.
    pub tree_anchor_samples: u64,
    /// Tree anchors accepted for structure placement.
    pub tree_anchor_accepts: u64,
    /// Ground-cover predicates evaluated.
    pub ground_cover_samples: u64,
    /// Ground-cover placements accepted.
    pub ground_cover_accepts: u64,
    /// Deep-rock resource predicates evaluated.
    pub resource_samples: u64,
    /// Copper placements accepted.
    pub resource_accepts: u64,
    /// Geologic stratum samples performed by the V5 natural layer.
    pub geology_samples: u64,
    /// River/basin samples performed by the V5 natural layer.
    pub river_samples: u64,
    /// Exclusion-radius vegetation comparisons performed by the V5 natural layer.
    pub vegetation_exclusion_samples: u64,
    /// Tree anchors rejected by a nearer exclusive neighbor.
    pub vegetation_exclusion_rejects: u64,
    /// Exact palette UTF-8 bytes plus the allocated `u16` voxel-index buffer.
    pub palette_and_index_bytes: u64,
}

/// Concrete role-resolved D4 voxel draft.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ChunkDraftV1 {
    edge_voxels: u16,
    palette: Vec<StableId>,
    voxel_palette_indices: Vec<u16>,
}

impl ChunkDraftV1 {
    /// Returns the cubic chunk edge in voxels.
    #[must_use]
    pub const fn edge_voxels(&self) -> u16 {
        self.edge_voxels
    }

    /// Returns the canonical concrete block palette.
    #[must_use]
    pub fn palette(&self) -> &[StableId] {
        &self.palette
    }

    /// Returns palette indices in Y layers, then Z rows, with X fastest.
    #[must_use]
    pub fn voxel_palette_indices(&self) -> &[u16] {
        &self.voxel_palette_indices
    }

    /// Returns the concrete block at a local `(x, y, z)` coordinate.
    #[must_use]
    pub fn block_at(&self, x: u16, y: u16, z: u16) -> Option<&StableId> {
        if x >= self.edge_voxels || y >= self.edge_voxels || z >= self.edge_voxels {
            return None;
        }
        let edge = usize::from(self.edge_voxels);
        let index = (usize::from(y) * edge + usize::from(z)) * edge + usize::from(x);
        let palette_index = usize::from(*self.voxel_palette_indices.get(index)?);
        self.palette.get(palette_index)
    }

    /// Returns canonical compact JSON for golden and conformance tests.
    ///
    /// # Errors
    ///
    /// Returns a canonical encoding error if serialization fails.
    pub fn canonical_bytes(&self) -> WorldgenResult<Vec<u8>> {
        encode_canonical("ChunkDraftV1", self)
    }
}

/// Deterministic algorithm-scaffold evidence paired with a chunk draft.
///
/// Runtime activation and storage CAS guards intentionally live only on
/// [`D4SnapshotCandidateV1`], so they cannot perturb canonical receipt bytes.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct GenerationReceiptV1 {
    dimension: DimensionId,
    chunk: ChunkCoordinate,
    planning_cell: PlanningCellCoordinateV1,
    generation_plan_revision: u64,
    generation_epoch: GenerationEpochIdV1,
    config_hash: WorldgenConfigHashV1,
    terrain_config_hash: TerrainConfigHashV2,
    generator_fingerprint: GeneratorFingerprintV1,
    locked_closure_fingerprint: LockedClosureFingerprintV1,
    generation_input_hash: GenerationInputHashV1,
    generation_provenance_hash: GenerationProvenanceHashV1,
    providers: Vec<(ProviderSlotV1, ProviderGenerationIdentityV1)>,
    role_bindings: Vec<RoleBindingReceiptV1>,
    placement_predicates: Vec<PlacementPredicateReceiptV1>,
    cave_field_requests: Vec<CaveFaceFieldRequestV1>,
    cave_occupancy_validations: Vec<CaveFaceOccupancyValidationV1>,
    styles_present: Vec<TerrainStyleV1>,
    draft_hash: CanonicalHash,
    diagnostics: GenerationDiagnosticsV1,
}

impl GenerationReceiptV1 {
    /// Returns the generated dimension.
    #[must_use]
    pub const fn dimension(&self) -> &DimensionId {
        &self.dimension
    }

    /// Returns the generated chunk coordinate.
    #[must_use]
    pub const fn chunk(&self) -> ChunkCoordinate {
        self.chunk
    }

    /// Returns the locally frozen generation epoch.
    #[must_use]
    pub const fn generation_epoch(&self) -> GenerationEpochIdV1 {
        self.generation_epoch
    }

    /// Returns the output-affecting generation input hash.
    #[must_use]
    pub const fn generation_input_hash(&self) -> GenerationInputHashV1 {
        self.generation_input_hash
    }

    /// Returns deterministic Predicate evaluation and placement Role receipts.
    #[must_use]
    pub fn placement_predicates(&self) -> &[PlacementPredicateReceiptV1] {
        &self.placement_predicates
    }

    /// Returns shared-face requests applied to the raw cave field.
    #[must_use]
    pub fn cave_field_requests(&self) -> &[CaveFaceFieldRequestV1] {
        &self.cave_field_requests
    }

    /// Returns raw-field versus final-occupancy validation for all six faces.
    #[must_use]
    pub fn cave_occupancy_validations(&self) -> &[CaveFaceOccupancyValidationV1] {
        &self.cave_occupancy_validations
    }

    /// Returns the canonical functional Role-to-block receipts.
    #[must_use]
    pub fn role_bindings(&self) -> &[RoleBindingReceiptV1] {
        &self.role_bindings
    }

    /// Returns the styles that actually materialized in the draft.
    #[must_use]
    pub fn styles_present(&self) -> &[TerrainStyleV1] {
        &self.styles_present
    }

    /// Returns bounded-work diagnostic counters.
    #[must_use]
    pub const fn diagnostics(&self) -> GenerationDiagnosticsV1 {
        self.diagnostics
    }

    /// Returns canonical compact JSON for the atomic provenance sidecar.
    ///
    /// # Errors
    ///
    /// Returns a canonical encoding error if serialization fails.
    pub fn canonical_bytes(&self) -> WorldgenResult<Vec<u8>> {
        encode_canonical("GenerationReceiptV1", self)
    }

    /// Returns the canonical receipt hash.
    ///
    /// # Errors
    ///
    /// Returns a canonical encoding error if serialization fails.
    pub fn canonical_hash(&self) -> WorldgenResult<CanonicalHash> {
        self.canonical_bytes().map(CanonicalHash::digest)
    }
}

/// Fully validated provisional snapshot bytes and their generation evidence.
///
/// This value is snapshot-first but intentionally does not implement the final
/// D3 world-wire envelope or a writer. Storage must atomically persist these
/// exact bytes and the provenance sidecar, and must still match the activation
/// token and chunk-revision CAS condition before publication.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct D4SnapshotCandidateV1 {
    draft: ChunkDraftV1,
    receipt: GenerationReceiptV1,
    snapshot_bytes: Vec<u8>,
    checksum: SnapshotChecksumV1,
    plan_activation_id: PlanActivationIdV1,
    expected_chunk_revision: ChunkRevisionExpectation,
}

impl D4SnapshotCandidateV1 {
    /// Returns the role-resolved chunk draft.
    #[must_use]
    pub const fn draft(&self) -> &ChunkDraftV1 {
        &self.draft
    }

    /// Returns provenance evidence that must be persisted atomically as a sidecar.
    #[must_use]
    pub const fn receipt(&self) -> &GenerationReceiptV1 {
        &self.receipt
    }

    /// Returns the runtime plan activation that storage must still consider current.
    #[must_use]
    pub const fn plan_activation_id(&self) -> PlanActivationIdV1 {
        self.plan_activation_id
    }

    /// Returns the optimistic storage CAS condition captured before generation.
    #[must_use]
    pub const fn expected_chunk_revision(&self) -> ChunkRevisionExpectation {
        self.expected_chunk_revision
    }

    /// Returns exact provisional snapshot bytes.
    #[must_use]
    pub fn snapshot_bytes(&self) -> &[u8] {
        &self.snapshot_bytes
    }

    /// Returns the checksum of [`Self::snapshot_bytes`].
    #[must_use]
    pub const fn checksum(&self) -> SnapshotChecksumV1 {
        self.checksum
    }
}

/// Request for reusing storage evidence or preparing a new snapshot candidate.
#[derive(Clone, Debug)]
pub struct ChunkGenerationRequestV1 {
    coordinate: ChunkCoordinate,
    existing_snapshot: Option<ExistingSnapshotEvidenceV1>,
    cell_epoch: CellEpochStateV1,
    adjacent_epochs: AdjacentEpochSnapshotV1,
    boundary_declarations: Vec<BoundaryAdapterDeclarationV1>,
    expected_chunk_revision: ChunkRevisionExpectation,
    boundary_receipts: Vec<BoundaryReceiptV1>,
}

impl ChunkGenerationRequestV1 {
    /// Creates a bounded chunk generation request with a complete four-neighbor view.
    #[must_use]
    pub fn new(
        coordinate: ChunkCoordinate,
        existing_snapshot: Option<ExistingSnapshotEvidenceV1>,
        cell_epoch: CellEpochStateV1,
        adjacent_epochs: AdjacentEpochSnapshotV1,
        boundary_declarations: Vec<BoundaryAdapterDeclarationV1>,
    ) -> Self {
        Self {
            coordinate,
            existing_snapshot,
            cell_epoch,
            adjacent_epochs,
            boundary_declarations,
            expected_chunk_revision: ChunkRevisionExpectation::Absent,
            boundary_receipts: Vec::new(),
        }
    }

    /// Replaces the default `Absent` storage CAS condition.
    #[must_use]
    pub const fn with_expected_chunk_revision(
        mut self,
        expected_chunk_revision: ChunkRevisionExpectation,
    ) -> Self {
        self.expected_chunk_revision = expected_chunk_revision;
        self
    }

    /// Attaches verified epoch-boundary receipts. Declarations alone still fail closed.
    #[must_use]
    pub fn with_boundary_receipts(mut self, boundary_receipts: Vec<BoundaryReceiptV1>) -> Self {
        self.boundary_receipts = boundary_receipts;
        self
    }
}

/// Result of snapshot-first generation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ChunkGenerationOutcomeV1 {
    /// Caller-trusted storage evidence was returned without recomputation.
    Existing(ExistingSnapshotEvidenceV1),
    /// New bytes are validated but still require atomic storage publication.
    Prepared(Box<D4SnapshotCandidateV1>),
}

/// Immutable, validated D4 generation plan.
#[derive(Clone, Debug)]
pub struct GenerationPlanV1 {
    dimension: DimensionId,
    world_seed: WorldSeedV1,
    seed_root: WorldgenSeedRootV2,
    config: WorldgenConfigV1,
    terrain_config: TerrainConfigV2,
    generation_plan_revision: u64,
    plan_activation_id: PlanActivationIdV1,
    providers: ResolvedProvidersV1,
    roles: Vec<RoleBindingReceiptV1>,
    role_targets: BTreeMap<D4MaterialRoleV1, StableId>,
    config_hash: WorldgenConfigHashV1,
    terrain_config_hash: TerrainConfigHashV2,
    generator_fingerprint: GeneratorFingerprintV1,
    locked_closure_fingerprint: LockedClosureFingerprintV1,
    generation_input_hash: GenerationInputHashV1,
    generation_provenance_hash: GenerationProvenanceHashV1,
    generation_epoch: GenerationEpochIdV1,
    material_seed: u64,
    tree_seed: u64,
    ground_cover_seed: u64,
    limits: WorldgenLimitsV1,
    territory: TerritorySamplerV1,
    cave: CaveSamplerV1,
    natural: Option<NaturalSamplerV1>,
    hydrology: Option<HydrologySamplerV1>,
}

impl GenerationPlanV1 {
    /// Validates and compiles an immutable D4 generation plan.
    ///
    /// # Errors
    ///
    /// Fails before generation for malformed config, missing or conflicting
    /// exclusive providers, incomplete Role/content closure, inconsistent
    /// provider fingerprints, or any preflight budget violation.
    #[allow(
        clippy::too_many_lines,
        reason = "plan compilation keeps D4 hashing and the optional V5 layer in one transaction"
    )]
    pub fn compile(input: GenerationPlanInputV1) -> WorldgenResult<Self> {
        preflight_plan_limits(&input)?;
        input.config.validate()?;
        if input.terrain_config_explicit {
            input.terrain_config.validate_against(&input.config)?;
        } else {
            input
                .terrain_config
                .validate_legacy_against(&input.config)?;
        }
        preflight_plan_input_bytes(&input)?;
        let roles = resolve_roles(
            &input.role_vocabulary,
            &input.role_bindings,
            &input.block_catalog,
        )?;
        let snapshot_bound = preflight_snapshot_bound(&input, &roles)?;
        preflight_live_generation_bound(&input, &roles, snapshot_bound)?;
        let providers = ResolvedProvidersV1::resolve(input.provider_offers, input.limits)?;
        let mut role_targets = roles
            .iter()
            .map(|receipt| (receipt.purpose, receipt.block_id.clone()))
            .collect::<BTreeMap<_, _>>();
        let config_bytes = input.config.canonical_bytes()?;
        let config_hash = input.config.canonical_hash()?;
        let terrain_config_bytes = input.terrain_config.canonical_bytes()?;
        let terrain_config_hash = input.terrain_config.canonical_hash()?;
        let provider_bytes = encode_canonical("resolved providers", &providers.ordered())?;
        let role_bytes = encode_canonical("frozen D4 role receipts", &roles)?;
        let generator_fingerprint = providers.fingerprint();

        let mut locked_receipts = input.locked_receipts;
        locked_receipts.sort();
        locked_receipts.dedup();
        let locked_bytes = encode_canonical("locked closure receipts", &locked_receipts)?;
        let locked_closure_fingerprint = LockedClosureFingerprintV1::from_hash(domain_hash(
            LOCK_FINGERPRINT_DOMAIN,
            &[locked_bytes.as_slice()],
        ));
        let revision_bytes = input.generation_plan_revision.to_be_bytes();
        let seed_root = WorldgenSeedRootV2::from_world_seed(input.world_seed);
        let d4_input_hash = GenerationInputHashV1::from_hash(concatenated_hash(
            GENERATION_INPUT_DOMAIN,
            &[
                input.world_seed.as_bytes(),
                config_bytes.as_slice(),
                terrain_config_bytes.as_slice(),
                &revision_bytes,
                provider_bytes.as_slice(),
                input.authoritative_semantic_receipt.as_bytes(),
                role_bytes.as_slice(),
            ],
        ));
        let mut generation_input_hash = d4_input_hash;
        let mut generation_provenance_hash =
            GenerationProvenanceHashV1::from_hash(concatenated_hash(
                GENERATION_PROVENANCE_DOMAIN,
                &[generation_input_hash.as_bytes(), locked_bytes.as_slice()],
            ));
        let mut generation_epoch = GenerationEpochIdV1::from_hash(concatenated_hash(
            GENERATION_EPOCH_DOMAIN,
            &[
                &revision_bytes,
                input.authoritative_semantic_receipt.as_bytes(),
                config_hash.as_bytes(),
                terrain_config_hash.as_bytes(),
                provider_bytes.as_slice(),
            ],
        ));

        let mut territory = TerritorySamplerV1::new(
            seed_root,
            d4_input_hash,
            input.config.clone(),
            input.terrain_config,
            providers
                .identity(ProviderSlotV1::TerrainTransition)
                .clone(),
        );
        let mut cave = CaveSamplerV1::new(seed_root, input.config.clone());
        let mut roles = roles;
        let natural = if let Some(layer) = input.natural_layer {
            let sampler = NaturalSamplerV1::compile(
                seed_root,
                &input.config,
                input.limits,
                layer,
                &providers,
                &input.role_bindings,
                &input.block_catalog,
                &roles,
            )?;
            territory = territory.with_boreal(sampler.boreal_params());
            for receipt in sampler.receipts() {
                role_targets.insert(receipt.purpose(), receipt.block_id().clone());
                roles.push(receipt.clone());
            }
            generation_input_hash = GenerationInputHashV1::from_hash(concatenated_hash(
                GENERATION_INPUT_DOMAIN,
                &[d4_input_hash.as_bytes(), sampler.layer_hash().as_bytes()],
            ));
            generation_provenance_hash = GenerationProvenanceHashV1::from_hash(concatenated_hash(
                GENERATION_PROVENANCE_DOMAIN,
                &[generation_input_hash.as_bytes(), locked_bytes.as_slice()],
            ));
            generation_epoch = GenerationEpochIdV1::from_hash(concatenated_hash(
                GENERATION_EPOCH_DOMAIN,
                &[generation_epoch.as_bytes(), sampler.layer_hash().as_bytes()],
            ));
            Some(sampler)
        } else {
            None
        };
        if let Some(layer) = input.cave_topology {
            let topology_hash = layer.canonical_hash()?;
            cave = cave.with_topology(layer);
            generation_input_hash = GenerationInputHashV1::from_hash(concatenated_hash(
                GENERATION_INPUT_DOMAIN,
                &[generation_input_hash.as_bytes(), topology_hash.as_bytes()],
            ));
            generation_provenance_hash = GenerationProvenanceHashV1::from_hash(concatenated_hash(
                GENERATION_PROVENANCE_DOMAIN,
                &[generation_input_hash.as_bytes(), locked_bytes.as_slice()],
            ));
            generation_epoch = GenerationEpochIdV1::from_hash(concatenated_hash(
                GENERATION_EPOCH_DOMAIN,
                &[generation_epoch.as_bytes(), topology_hash.as_bytes()],
            ));
        }
        let hydrology = match (input.hydrology_occupancy, natural.as_ref()) {
            (Some(layer), Some(natural_sampler)) => Some(HydrologySamplerV1::compile(
                seed_root,
                &input.config,
                &input.terrain_config,
                natural_sampler,
                layer,
            )?),
            (Some(_), None) => {
                return Err(WorldgenError::MissingNaturalLayerForHydrology);
            }
            (None, _) => None,
        };
        let material_seed = hash_u64(MATERIAL_DOMAIN, &[seed_root.as_bytes()]);
        let tree_seed = hash_u64(TREE_DOMAIN, &[seed_root.as_bytes()]);
        let ground_cover_seed = hash_u64(GROUND_COVER_DOMAIN, &[seed_root.as_bytes()]);

        Ok(Self {
            dimension: input.dimension,
            world_seed: input.world_seed,
            seed_root,
            config: input.config,
            terrain_config: input.terrain_config,
            generation_plan_revision: input.generation_plan_revision,
            plan_activation_id: input.plan_activation_id,
            providers,
            roles,
            role_targets,
            config_hash,
            terrain_config_hash,
            generator_fingerprint,
            locked_closure_fingerprint,
            generation_input_hash,
            generation_provenance_hash,
            generation_epoch,
            material_seed,
            tree_seed,
            ground_cover_seed,
            limits: input.limits,
            territory,
            cave,
            natural,
            hydrology,
        })
    }

    /// Returns the runtime activation token required for safe publication.
    #[must_use]
    pub const fn plan_activation_id(&self) -> PlanActivationIdV1 {
        self.plan_activation_id
    }

    /// Returns the dimension controlled by this plan.
    #[must_use]
    pub const fn dimension(&self) -> &DimensionId {
        &self.dimension
    }

    /// Returns the exact persisted world seed.
    #[must_use]
    pub const fn world_seed(&self) -> WorldSeedV1 {
        self.world_seed
    }

    /// Returns the version-two field root isolated from provenance revisions.
    #[must_use]
    pub const fn seed_root(&self) -> WorldgenSeedRootV2 {
        self.seed_root
    }

    /// Returns the closed integer configuration compiled into this plan.
    #[must_use]
    pub const fn config(&self) -> &WorldgenConfigV1 {
        &self.config
    }

    /// Returns the fully resolved Worldgen V2 terrain configuration.
    #[must_use]
    pub const fn terrain_config(&self) -> &TerrainConfigV2 {
        &self.terrain_config
    }

    /// Returns the concrete block bound to a compiled D4 material purpose.
    ///
    /// Compiled plans contain every required purpose. This accessor is for
    /// spawn classification and diagnostics, not a second materializer.
    #[must_use]
    pub fn role_target(&self, purpose: D4MaterialRoleV1) -> &StableId {
        self.role_targets
            .get(&purpose)
            .unwrap_or_else(|| missing_role_target(purpose))
    }

    /// Returns the canonical config hash.
    #[must_use]
    pub const fn config_hash(&self) -> WorldgenConfigHashV1 {
        self.config_hash
    }

    /// Returns the canonical resolved terrain-configuration hash.
    #[must_use]
    pub const fn terrain_config_hash(&self) -> TerrainConfigHashV2 {
        self.terrain_config_hash
    }

    /// Returns the aggregate generator fingerprint.
    #[must_use]
    pub const fn generator_fingerprint(&self) -> GeneratorFingerprintV1 {
        self.generator_fingerprint
    }

    /// Returns the provenance-only locked closure fingerprint.
    #[must_use]
    pub const fn locked_closure_fingerprint(&self) -> LockedClosureFingerprintV1 {
        self.locked_closure_fingerprint
    }

    /// Returns the output-affecting generation input hash.
    #[must_use]
    pub const fn generation_input_hash(&self) -> GenerationInputHashV1 {
        self.generation_input_hash
    }

    /// Returns the package/artifact-aware generation provenance hash.
    #[must_use]
    pub const fn generation_provenance_hash(&self) -> GenerationProvenanceHashV1 {
        self.generation_provenance_hash
    }

    /// Returns the epoch offered to never-materialized planning cells.
    #[must_use]
    pub const fn generation_epoch(&self) -> GenerationEpochIdV1 {
        self.generation_epoch
    }

    /// Queries the D7-compatible coarse territory result at world `(x, z)`.
    #[must_use]
    pub fn territory_query(&self, x: i64, z: i64) -> TerritoryQueryV1 {
        self.territory.query(x, z)
    }

    /// Returns the concrete terrain style selected for materialization at `(x, z)`.
    ///
    /// Unlike [`Self::territory_query`], this includes the deterministic style
    /// choice inside an authored transition band.
    #[must_use]
    pub fn material_style(&self, x: i64, z: i64) -> TerrainStyleV1 {
        let sample = self.territory.sample(x, z);
        self.territory.choose_material_style(x, z, sample)
    }

    /// Returns whether the V5 natural layer is compiled into this plan.
    #[must_use]
    pub const fn has_natural_layer(&self) -> bool {
        self.natural.is_some()
    }

    /// Returns whether the V6 cave-topology layer is compiled into this plan.
    #[must_use]
    pub const fn has_cave_topology_layer(&self) -> bool {
        self.cave.has_topology()
    }

    /// Returns whether the V6 hydrology occupancy layer is compiled into this plan.
    #[must_use]
    pub const fn has_hydrology_occupancy(&self) -> bool {
        self.hydrology.is_some()
    }

    /// Returns frozen water/lava identities compiled into hydrology occupancy.
    #[must_use]
    pub fn hydrology_fluids(&self) -> Option<&HydrologyFluidBindingsV1> {
        self.hydrology.as_ref().map(HydrologySamplerV1::fluids)
    }

    /// Returns the topology ownership identity at world `(x, y, z)`.
    #[must_use]
    pub fn cave_topology_domain(&self, x: i64, y: i64, z: i64) -> Option<&StableId> {
        self.cave.topology_domain(x, y, z)
    }

    /// Returns the dimension-default cave topology domain.
    #[must_use]
    pub fn cave_topology_default_domain(&self) -> Option<&StableId> {
        self.cave
            .topology()
            .map(crate::cave_topology::TopologyFieldV1::default_domain)
    }

    /// Returns underground-owned topology domains in canonical order.
    #[must_use]
    pub fn cave_topology_owned_domains(&self) -> Option<&[CaveOwnedDomainV1]> {
        self.cave
            .topology()
            .map(crate::cave_topology::TopologyFieldV1::owned_domains)
    }

    /// Returns compiled topology portals.
    #[must_use]
    pub fn cave_topology_portals(&self) -> Option<&[CaveLayerPortalV1]> {
        self.cave
            .topology()
            .map(crate::cave_topology::TopologyFieldV1::portals)
    }

    /// Returns compiled surface-to-destination topology entrances.
    #[must_use]
    pub fn cave_topology_entrances(&self) -> Option<&[CaveLayerEntranceV1]> {
        self.cave
            .topology()
            .map(crate::cave_topology::TopologyFieldV1::entrances)
    }

    /// Returns the resolved topology cell edge in world voxels.
    #[must_use]
    pub fn cave_topology_cell_edge_voxels(&self) -> Option<u32> {
        self.cave
            .topology()
            .map(crate::cave_topology::TopologyFieldV1::cell_edge_voxels)
    }

    /// Returns the bounded branch contributor compiled into topology.
    #[must_use]
    pub fn cave_topology_branch(&self) -> Option<&crate::CaveBranchContributorV1> {
        self.cave
            .topology()
            .map(crate::cave_topology::TopologyFieldV1::branch)
    }

    /// Returns the occupancy-layer hash, independent of snapshot bytes.
    #[must_use]
    pub fn hydrology_occupancy_hash(&self) -> Option<HydrologyOccupancyHashV1> {
        self.hydrology
            .as_ref()
            .map(HydrologySamplerV1::occupancy_hash)
    }

    /// Returns the exclusive provider identity compiled into a slot.
    #[must_use]
    pub fn provider_identity(&self, slot: ProviderSlotV1) -> Option<&ProviderGenerationIdentityV1> {
        self.providers.try_identity(slot)
    }

    /// Returns deterministic terrain height intent at world `(x, z)`.
    #[must_use]
    pub fn terrain_height(&self, x: i64, z: i64) -> i32 {
        let sample = self.territory.sample(x, z);
        let height = self.territory.height(x, z, sample);
        self.natural
            .as_ref()
            .map_or(height, |natural| natural.adjust_height(x, z, height))
    }

    /// Returns the inclusive standing-water level of an inland lake basin.
    #[must_use]
    pub fn surface_water_level(&self, x: i64, z: i64) -> Option<i32> {
        self.territory.surface_water_y(x, z)
    }

    /// Returns the macro shape family independently from climate materials.
    #[must_use]
    pub fn terrain_family(&self, x: i64, z: i64) -> TerrainFamilyV2 {
        self.territory.family(x, z)
    }

    /// Returns the locally queryable surface river sample at world `(x, z)`.
    #[must_use]
    pub fn river_sample(&self, x: i64, z: i64) -> Option<RiverSampleV1> {
        self.natural
            .as_ref()
            .map(|natural| natural.river_sample(x, z))
    }

    /// Returns the queryable geologic sample at world `(x, y, z)`.
    #[must_use]
    pub fn geologic_sample(&self, x: i64, y: i64, z: i64) -> Option<GeologicSampleV1> {
        let sample = self.territory.sample(x, z);
        let height = self.terrain_height(x, z);
        let style = self.territory.choose_material_style(x, z, sample);
        self.natural
            .as_ref()
            .map(|natural| natural.geologic_sample(x, y, z, height, style))
    }

    /// Returns the stable resource-field sample at world `(x, y, z)`.
    #[must_use]
    pub fn resource_field_sample(&self, x: i64, y: i64, z: i64) -> Option<ResourceFieldSampleV1> {
        let sample = self.territory.sample(x, z);
        let height = self.terrain_height(x, z);
        let style = self.territory.choose_material_style(x, z, sample);
        self.natural
            .as_ref()
            .map(|natural| natural.resource_sample(x, y, z, height, style))
    }

    /// Returns the queryable aquifer table at world `(x, z)`.
    #[must_use]
    pub fn aquifer_sample(&self, x: i64, z: i64) -> Option<AquiferSampleV1> {
        let hydrology = self.hydrology.as_ref()?;
        Some(hydrology.aquifer_sample(x, z, self.terrain_height(x, z)))
    }

    /// Returns the queryable vertical drainage decision at world `(x, z)`.
    #[must_use]
    pub fn drainage_sample(&self, x: i64, z: i64) -> Option<DrainageSampleV1> {
        let hydrology = self.hydrology.as_ref()?;
        Some(hydrology.drainage_sample(x, z, self.river_sample(x, z)))
    }

    /// Returns initial water/lava occupancy at world `(x, y, z)`.
    ///
    /// Occupancy is a coordinate query. It does not read neighbor chunks or
    /// mutate the solid snapshot candidate.
    #[must_use]
    pub fn hydrology_occupancy_sample(
        &self,
        x: i64,
        y: i64,
        z: i64,
    ) -> Option<HydrologyOccupancySampleV1> {
        let hydrology = self.hydrology.as_ref()?;
        let height = self.terrain_height(x, z);
        let occupancy = self.cave.occupancy(x, y, z, height);
        let sample = self.territory.sample(x, z);
        let style = self.territory.choose_material_style(x, z, sample);
        Some(hydrology.occupy(
            x,
            y,
            z,
            height,
            occupancy.allows_fluid_occupancy(),
            self.river_sample(x, z),
            style,
            self.surface_water_level(x, z),
        ))
    }

    /// Builds a versioned hydrology occupancy candidate for one chunk.
    ///
    /// The candidate is not a storage snapshot and does not change D4/D7 chunk
    /// bytes. Only the authority may accept it as a later revision.
    ///
    /// # Errors
    ///
    /// Returns a missing-layer, arithmetic, or accounting-budget error.
    pub fn hydrology_occupancy_candidate(
        &self,
        coordinate: ChunkCoordinate,
    ) -> WorldgenResult<HydrologyOccupancyCandidateV1> {
        self.hydrology_occupancy_candidate_with_draft(coordinate, None)
    }

    /// Builds hydrology occupancy by reusing cave decisions already frozen in
    /// a snapshot candidate from this plan.
    ///
    /// Omitted hydrology is represented by `None`. A candidate from another
    /// activation, dimension, epoch, or generation input fails closed.
    ///
    /// # Errors
    ///
    /// Returns an identity, draft-shape, arithmetic, or accounting-budget error.
    pub fn hydrology_occupancy_candidate_for_snapshot(
        &self,
        snapshot: &D4SnapshotCandidateV1,
    ) -> WorldgenResult<Option<HydrologyOccupancyCandidateV1>> {
        if self.hydrology.is_none() {
            return Ok(None);
        }
        let receipt = snapshot.receipt();
        if snapshot.plan_activation_id() != self.plan_activation_id
            || receipt.dimension() != &self.dimension
            || receipt.generation_epoch() != self.generation_epoch
            || receipt.generation_input_hash() != self.generation_input_hash
        {
            return Err(WorldgenError::InvalidHydrologyOccupancy {
                field: "snapshot",
                reason: "snapshot candidate does not belong to this generation plan".to_owned(),
            });
        }
        self.hydrology_occupancy_candidate_with_draft(receipt.chunk(), Some(snapshot.draft()))
            .map(Some)
    }

    fn hydrology_occupancy_candidate_with_draft(
        &self,
        coordinate: ChunkCoordinate,
        draft: Option<&ChunkDraftV1>,
    ) -> WorldgenResult<HydrologyOccupancyCandidateV1> {
        let Some(hydrology) = self.hydrology.as_ref() else {
            return Err(WorldgenError::InvalidHydrologyOccupancy {
                field: "layer",
                reason: "hydrology occupancy is not compiled into this plan".to_owned(),
            });
        };
        let edge = usize::from(self.config.chunk_edge_voxels);
        let origin = chunk_origin(coordinate, self.config.chunk_edge_voxels)?;
        let draft_cave = draft
            .map(|draft| DraftCaveOccupancyV1::new(self, draft, edge))
            .transpose()?;
        let mut accounting = hydrology.start_accounting();
        let mut cells = Vec::new();
        let mut columns = Vec::with_capacity(edge.saturating_mul(edge));
        for local_z in 0..edge {
            for local_x in 0..edge {
                let world_x = local_world_axis(origin.0, local_x);
                let world_z = local_world_axis(origin.2, local_z);
                let height = self.terrain_height(world_x, world_z);
                let territory = self.territory.sample(world_x, world_z);
                let style = self
                    .territory
                    .choose_material_style(world_x, world_z, territory);
                columns.push(hydrology.column(
                    world_x,
                    world_z,
                    height,
                    self.river_sample(world_x, world_z),
                    style,
                    self.surface_water_level(world_x, world_z),
                ));
            }
        }
        for local_y in 0..edge {
            for local_z in 0..edge {
                for local_x in 0..edge {
                    HydrologySamplerV1::examine(&mut accounting, 1);
                    let world_x = local_world_axis(origin.0, local_x);
                    let world_y = local_world_axis(origin.1, local_y);
                    let world_z = local_world_axis(origin.2, local_z);
                    let column_index = local_z.saturating_mul(edge).saturating_add(local_x);
                    let column = columns.get(column_index).copied().ok_or(
                        WorldgenError::ArithmeticOverflow {
                            operation: "hydrology occupancy column lookup",
                        },
                    )?;
                    let voxel_index = local_y
                        .saturating_mul(edge)
                        .saturating_add(local_z)
                        .saturating_mul(edge)
                        .saturating_add(local_x);
                    let cave_allows_fluid = draft_cave.map_or_else(
                        || {
                            self.cave
                                .occupancy(world_x, world_y, world_z, column.surface_y())
                                .allows_fluid_occupancy()
                        },
                        |mask| {
                            world_y <= i64::from(column.surface_y())
                                && mask.allows_fluid_occupancy(voxel_index)
                        },
                    );
                    let sample = hydrology.occupy_column(
                        world_x,
                        world_y,
                        world_z,
                        cave_allows_fluid,
                        column,
                    );
                    let Some(cell) = HydrologySamplerV1::occupancy_cell(
                        u16::try_from(local_x).unwrap_or_default(),
                        u16::try_from(local_y).unwrap_or_default(),
                        u16::try_from(local_z).unwrap_or_default(),
                        &sample,
                    ) else {
                        continue;
                    };
                    HydrologySamplerV1::occupy_cell(
                        &mut accounting,
                        sample.kind() == crate::HydrologyOccupancyKindV1::Drainage,
                    )?;
                    cells.push(cell);
                }
            }
        }
        hydrology.candidate(
            self.dimension.clone(),
            coordinate,
            self.generation_epoch,
            self.generation_input_hash,
            cells,
            accounting,
        )
    }

    /// Returns a direction-independent occupancy continuity receipt for one face.
    ///
    /// Opposite faces of the same boundary hash the same world occupancy. Chunk
    /// approach direction cannot change the receipt.
    ///
    /// # Errors
    ///
    /// Returns a missing-layer or arithmetic error at the coordinate boundary.
    pub fn hydrology_face_continuity(
        &self,
        coordinate: ChunkCoordinate,
        face: crate::ChunkFaceV1,
    ) -> WorldgenResult<HydrologyFaceContinuityV1> {
        if self.hydrology.is_none() {
            return Err(WorldgenError::InvalidHydrologyOccupancy {
                field: "layer",
                reason: "hydrology occupancy is not compiled into this plan".to_owned(),
            });
        }
        let neighbor = hydrology_adjacent_chunk(coordinate, face)?;
        let (first, second) = if coordinate < neighbor {
            (coordinate, neighbor)
        } else {
            (neighbor, coordinate)
        };
        let outward = canonical_outward_face(first, second);
        let edge = i64::from(self.config.chunk_edge_voxels);
        let origin = chunk_origin(first, self.config.chunk_edge_voxels)?;
        let mut packed = Vec::new();
        let mut occupied = 0_u32;
        for v in 0..edge {
            for u in 0..edge {
                let (local_x, local_y, local_z) = face_local_sample(outward, u, v, edge);
                let world_x = origin.0.saturating_add(local_x);
                let world_y = origin.1.saturating_add(local_y);
                let world_z = origin.2.saturating_add(local_z);
                let Some(sample) = self.hydrology_occupancy_sample(world_x, world_y, world_z)
                else {
                    return Err(WorldgenError::InvalidHydrologyOccupancy {
                        field: "layer",
                        reason: "hydrology occupancy sampler was lost after compilation".to_owned(),
                    });
                };
                if sample.is_occupied() {
                    occupied = occupied.saturating_add(1);
                }
                packed.push((
                    u16::try_from(u).unwrap_or_default(),
                    u16::try_from(v).unwrap_or_default(),
                    sample.kind(),
                    sample.level(),
                    sample.flow() as u8,
                ));
            }
        }
        Ok(HydrologySamplerV1::face_continuity(
            coordinate,
            neighbor,
            hydrology_face_axis(face),
            hydrology_face_hash(&packed),
            occupied,
        ))
    }

    /// Returns deterministic signed density intent at world `(x, y, z)`.
    ///
    /// Positive values are solid, zero is the surface, and negative values are
    /// void. Minimal caves turn otherwise-solid samples negative.
    #[must_use]
    pub fn terrain_density(&self, x: i64, y: i64, z: i64) -> i64 {
        let height = self.terrain_height(x, z);
        if self.cave.is_void(x, y, z, height) {
            -1
        } else {
            i64::from(height).saturating_sub(y)
        }
    }

    /// Returns the bounded integer cave field; non-positive values are void.
    #[must_use]
    pub fn cave_signed_distance_fixed(&self, x: i64, y: i64, z: i64) -> i32 {
        self.cave.signed_distance_fixed(x, y, z)
    }

    /// Returns local, branch, and portal field samples plus final occupancy.
    ///
    /// Material, ore, and fluid placement must use the final occupancy flag.
    /// The raw field may be void under minimum cover without becoming empty.
    #[must_use]
    pub fn cave_occupancy_arbitration(&self, x: i64, y: i64, z: i64) -> CaveOccupancyArbitrationV1 {
        self.cave.occupancy(x, y, z, self.terrain_height(x, z))
    }

    /// Returns the domain-owned topology algorithm at world `(x, y, z)`.
    #[must_use]
    pub fn cave_topology_algorithm(
        &self,
        x: i64,
        y: i64,
        z: i64,
    ) -> Option<CaveTopologyAlgorithmV1> {
        self.cave.topology_algorithm(x, y, z)
    }

    /// Returns whether world `(x, y, z)` lies inside declared cave influence.
    #[must_use]
    pub fn cave_in_declared_influence(&self, x: i64, y: i64, z: i64) -> bool {
        self.cave.in_declared_influence(x, y, z)
    }

    /// Returns voxel passability receipts for compiled surface entrances.
    #[must_use]
    pub fn cave_passability_receipts(&self) -> Vec<CaveVoxelPassabilityReceiptV1> {
        self.cave
            .passability_receipts(|x, z| self.terrain_height(x, z))
    }

    /// Returns direction-independent raw cave-field requests for one chunk.
    ///
    /// # Errors
    ///
    /// Returns an arithmetic error at the persistent chunk-coordinate boundary.
    pub fn cave_face_field_requests(
        &self,
        coordinate: ChunkCoordinate,
    ) -> WorldgenResult<Vec<CaveFaceFieldRequestV1>> {
        self.cave.face_requests(coordinate)
    }

    /// Collects unique raw-field portal requests for `chunks`.
    ///
    /// Chunk order cannot change the compiled plan. Opposite faces of one
    /// shared boundary collapse to a single request.
    ///
    /// # Errors
    ///
    /// Returns an arithmetic error at the persistent chunk-coordinate boundary.
    pub fn cave_field_portal_plan(
        &self,
        chunks: impl IntoIterator<Item = ChunkCoordinate>,
    ) -> WorldgenResult<CaveFieldPortalPlanV1> {
        self.cave.field_portal_plan(chunks)
    }

    /// Returns a direction-independent cave key for one chunk face.
    ///
    /// # Errors
    ///
    /// Returns an arithmetic error when the adjacent `i32` chunk coordinate is
    /// outside the persistent coordinate domain.
    pub fn shared_face_key(
        &self,
        coordinate: ChunkCoordinate,
        face: crate::ChunkFaceV1,
    ) -> WorldgenResult<crate::SharedFaceKeyV1> {
        self.cave.shared_face_key(coordinate, face)
    }

    /// Builds a request for a chunk that has never been materialized.
    ///
    /// The request carries an unassigned local epoch, a complete unassigned
    /// four-neighbor view, and the default `Absent` storage CAS condition. It
    /// does not read or open storage.
    ///
    /// # Errors
    ///
    /// Returns an arithmetic error when a cardinal planning-cell neighbor is
    /// not representable.
    pub fn vacant_generation_request(
        &self,
        coordinate: ChunkCoordinate,
    ) -> WorldgenResult<ChunkGenerationRequestV1> {
        let cell = self.planning_cell(coordinate);
        Ok(ChunkGenerationRequestV1::new(
            coordinate,
            None,
            CellEpochStateV1::Unassigned,
            AdjacentEpochSnapshotV1::all_unassigned(cell)?,
            Vec::new(),
        ))
    }

    /// Reuses consistent storage evidence or creates a new snapshot-first candidate.
    ///
    /// # Errors
    ///
    /// Fails before materialization for mismatched existing evidence, an
    /// unavailable frozen epoch, an incomplete adjacent-state snapshot, any
    /// unverified cross-epoch boundary, coordinate overflow, or exceeded work/
    /// output budgets.
    #[allow(
        clippy::too_many_lines,
        reason = "the snapshot-first gate remains one auditable transaction"
    )]
    pub fn generate(
        &self,
        request: ChunkGenerationRequestV1,
    ) -> WorldgenResult<ChunkGenerationOutcomeV1> {
        let expected_cell = self.planning_cell(request.coordinate);
        if let Some(existing) = request.existing_snapshot {
            if existing.coordinate() != request.coordinate {
                return Err(WorldgenError::SnapshotCoordinateMismatch {
                    request: request.coordinate,
                    snapshot: existing.coordinate(),
                });
            }
            if existing.dimension() != &self.dimension {
                return Err(WorldgenError::SnapshotDimensionMismatch {
                    expected: self.dimension.clone(),
                    actual: Box::new(existing.dimension().clone()),
                });
            }
            if existing.planning_cell() != expected_cell {
                return Err(WorldgenError::SnapshotPlanningCellMismatch {
                    expected: expected_cell,
                    actual: existing.planning_cell(),
                });
            }
            if request.cell_epoch != CellEpochStateV1::Frozen(existing.epoch()) {
                return Err(WorldgenError::SnapshotEpochStateMismatch {
                    snapshot_epoch: existing.epoch(),
                    reported: request.cell_epoch,
                });
            }
            return Ok(ChunkGenerationOutcomeV1::Existing(existing));
        }

        match request.cell_epoch {
            CellEpochStateV1::Unassigned => {}
            CellEpochStateV1::Frozen(epoch) if epoch == self.generation_epoch => {}
            CellEpochStateV1::Frozen(frozen) => {
                return Err(WorldgenError::FrozenEpochUnavailable {
                    cell: expected_cell,
                    frozen,
                    active: self.generation_epoch,
                });
            }
        }

        validate_epoch_boundaries(
            expected_cell,
            self.generation_epoch,
            &request.adjacent_epochs,
            &request.boundary_declarations,
            &request.boundary_receipts,
            self.limits,
        )?;
        let cave_field_requests = self.cave.face_requests(request.coordinate)?;
        let (draft, diagnostics, styles_present) = self.materialize(request.coordinate)?;
        let cave_occupancy_validations =
            self.validate_cave_face_occupancy(request.coordinate, &draft, &cave_field_requests)?;
        let mut placement_predicates = placement_predicate_receipts(diagnostics);
        if self.natural.is_some() {
            placement_predicates.extend(NaturalSamplerV1::placement_predicates(
                diagnostics.geology_samples,
                diagnostics.river_samples,
                diagnostics.resource_samples,
                diagnostics.resource_accepts,
                diagnostics.vegetation_exclusion_samples,
                diagnostics.vegetation_exclusion_rejects,
            ));
        }
        let draft_bytes = draft.canonical_bytes()?;
        let draft_hash = CanonicalHash::digest(&draft_bytes);
        let receipt = GenerationReceiptV1 {
            dimension: self.dimension.clone(),
            chunk: request.coordinate,
            planning_cell: expected_cell,
            generation_plan_revision: self.generation_plan_revision,
            generation_epoch: self.generation_epoch,
            config_hash: self.config_hash,
            terrain_config_hash: self.terrain_config_hash,
            generator_fingerprint: self.generator_fingerprint,
            locked_closure_fingerprint: self.locked_closure_fingerprint,
            generation_input_hash: self.generation_input_hash,
            generation_provenance_hash: self.generation_provenance_hash,
            providers: self.providers.ordered(),
            role_bindings: self.roles.clone(),
            placement_predicates,
            cave_field_requests,
            cave_occupancy_validations,
            styles_present,
            draft_hash,
            diagnostics,
        };
        let envelope = SnapshotEnvelopeV1 {
            schema: SNAPSHOT_SCHEMA,
            dimension: &self.dimension,
            coordinate: request.coordinate,
            planning_cell: expected_cell,
            generation_epoch: self.generation_epoch,
            generation_input_hash: self.generation_input_hash,
            draft: &draft,
        };
        let snapshot_bytes = encode_canonical("D4 snapshot candidate", &envelope)?;
        let snapshot_length =
            u64::try_from(snapshot_bytes.len()).map_err(|_| WorldgenError::ArithmeticOverflow {
                operation: "snapshot byte length",
            })?;
        if snapshot_length > self.limits.max_snapshot_bytes.get() {
            return Err(WorldgenError::BudgetExceeded {
                budget: "snapshot bytes",
                required: snapshot_length,
                limit: self.limits.max_snapshot_bytes.get(),
            });
        }
        let checksum = snapshot_checksum(&snapshot_bytes);
        Ok(ChunkGenerationOutcomeV1::Prepared(Box::new(
            D4SnapshotCandidateV1 {
                draft,
                receipt,
                snapshot_bytes,
                checksum,
                plan_activation_id: self.plan_activation_id,
                expected_chunk_revision: request.expected_chunk_revision,
            },
        )))
    }

    fn planning_cell(&self, coordinate: ChunkCoordinate) -> PlanningCellCoordinateV1 {
        let edge = i64::from(self.config.planning_cell_edge_chunks);
        PlanningCellCoordinateV1::new(
            i64::from(coordinate.x).div_euclid(edge),
            i64::from(coordinate.z).div_euclid(edge),
        )
    }

    #[allow(
        clippy::too_many_lines,
        reason = "the bounded hot path keeps allocation and diagnostic counters in one auditable flow"
    )]
    fn materialize(
        &self,
        coordinate: ChunkCoordinate,
    ) -> WorldgenResult<(ChunkDraftV1, GenerationDiagnosticsV1, Vec<TerrainStyleV1>)> {
        let edge = usize::from(self.config.chunk_edge_voxels);
        let voxel_count = checked_cube_u64(u64::from(self.config.chunk_edge_voxels))?;
        let column_count = u64::from(self.config.chunk_edge_voxels)
            .checked_mul(u64::from(self.config.chunk_edge_voxels))
            .ok_or(WorldgenError::ArithmeticOverflow {
                operation: "chunk column count",
            })?;
        let origin = chunk_origin(coordinate, self.config.chunk_edge_voxels)?;
        let mut columns = Vec::with_capacity(usize::try_from(column_count).map_err(|_| {
            WorldgenError::ArithmeticOverflow {
                operation: "column allocation length",
            }
        })?);
        let mut styles = BTreeSet::new();
        for local_z in 0..edge {
            for local_x in 0..edge {
                let world_x = origin
                    .0
                    .saturating_add(i64::try_from(local_x).unwrap_or_default());
                let world_z = origin
                    .2
                    .saturating_add(i64::try_from(local_z).unwrap_or_default());
                let sample = self.territory.sample(world_x, world_z);
                let base_height = self.territory.height(world_x, world_z, sample);
                let in_river_channel = self
                    .natural
                    .as_ref()
                    .is_some_and(|natural| natural.in_river_channel(world_x, world_z));
                let height = self.natural.as_ref().map_or(base_height, |natural| {
                    natural.adjust_height_for_channel(base_height, in_river_channel)
                });
                let material_style = self
                    .territory
                    .choose_material_style(world_x, world_z, sample);
                styles.insert(material_style);
                columns.push(ColumnSampleV1 {
                    height,
                    material_style,
                    in_river_channel,
                });
            }
        }

        let mut palette = self
            .role_targets
            .values()
            .cloned()
            .collect::<Vec<StableId>>();
        palette.sort();
        palette.dedup();
        let palette_lookup = palette
            .iter()
            .enumerate()
            .map(|(index, block)| (block.clone(), u16::try_from(index).unwrap_or(u16::MAX)))
            .collect::<BTreeMap<_, _>>();
        let mut indices = Vec::with_capacity(usize::try_from(voxel_count).map_err(|_| {
            WorldgenError::ArithmeticOverflow {
                operation: "voxel allocation length",
            }
        })?);
        let mut counters = WorkCountersV1 {
            territory_queries: column_count,
            height_samples: column_count,
            cave_samples: 0,
            cave_void_accepts: 0,
            tree_anchor_samples: 0,
            tree_anchor_accepts: 0,
            ground_cover_samples: 0,
            ground_cover_accepts: 0,
            resource_samples: 0,
            resource_accepts: 0,
        };
        let mut natural_counters = NaturalWorkCountersV1::default();
        let vegetation = if self.natural.is_some() {
            self.natural_vegetation_overlay(origin, edge, &columns, &mut natural_counters)?
        } else {
            self.vegetation_overlay(origin, edge, &columns, &mut counters)?
        };

        for local_y in 0..edge {
            let world_y = origin
                .1
                .saturating_add(i64::try_from(local_y).unwrap_or_default());
            for local_z in 0..edge {
                for local_x in 0..edge {
                    let world_x = origin
                        .0
                        .saturating_add(i64::try_from(local_x).unwrap_or_default());
                    let world_z = origin
                        .2
                        .saturating_add(i64::try_from(local_z).unwrap_or_default());
                    let column_index = local_z * edge + local_x;
                    let column = &columns[column_index];
                    let voxel_index = (local_y * edge + local_z) * edge + local_x;
                    let purpose = self.material_role(
                        world_x,
                        world_y,
                        world_z,
                        *column,
                        vegetation[voxel_index],
                        &mut counters,
                        &mut natural_counters,
                    );
                    let block = self.role_target(purpose);
                    let palette_index = palette_lookup.get(block).copied().ok_or(
                        WorldgenError::ArithmeticOverflow {
                            operation: "role target palette lookup",
                        },
                    )?;
                    indices.push(palette_index);
                }
            }
        }

        let work_units = counters
            .territory_queries
            .saturating_add(counters.height_samples)
            .saturating_add(counters.cave_samples)
            .saturating_add(counters.tree_anchor_samples)
            .saturating_add(counters.ground_cover_samples)
            .saturating_add(counters.resource_samples)
            .saturating_add(natural_counters.geology_samples)
            .saturating_add(natural_counters.river_samples)
            .saturating_add(natural_counters.resource_samples)
            .saturating_add(natural_counters.tree_anchor_samples)
            .saturating_add(natural_counters.exclusion_samples)
            .saturating_add(natural_counters.ground_cover_samples);
        if work_units > self.limits.max_work_units.get() {
            return Err(WorldgenError::BudgetExceeded {
                budget: "deterministic samples",
                required: work_units,
                limit: self.limits.max_work_units.get(),
            });
        }
        let palette_and_index_bytes = voxel_count.saturating_mul(2).saturating_add(
            palette
                .iter()
                .map(|id| u64::try_from(id.as_str().len()).unwrap_or(u64::MAX))
                .sum::<u64>(),
        );
        let diagnostics = GenerationDiagnosticsV1 {
            voxel_count,
            column_count,
            territory_queries: counters.territory_queries,
            height_samples: counters.height_samples,
            cave_samples: counters.cave_samples,
            cave_void_accepts: counters.cave_void_accepts,
            tree_anchor_samples: counters
                .tree_anchor_samples
                .saturating_add(natural_counters.tree_anchor_samples),
            tree_anchor_accepts: counters
                .tree_anchor_accepts
                .saturating_add(natural_counters.tree_anchor_accepts),
            ground_cover_samples: counters
                .ground_cover_samples
                .saturating_add(natural_counters.ground_cover_samples),
            ground_cover_accepts: counters
                .ground_cover_accepts
                .saturating_add(natural_counters.ground_cover_accepts),
            resource_samples: counters
                .resource_samples
                .saturating_add(natural_counters.resource_samples),
            resource_accepts: counters
                .resource_accepts
                .saturating_add(natural_counters.resource_accepts),
            geology_samples: natural_counters.geology_samples,
            river_samples: natural_counters.river_samples,
            vegetation_exclusion_samples: natural_counters.exclusion_samples,
            vegetation_exclusion_rejects: natural_counters.exclusion_rejects,
            palette_and_index_bytes,
        };
        Ok((
            ChunkDraftV1 {
                edge_voxels: self.config.chunk_edge_voxels,
                palette,
                voxel_palette_indices: indices,
            },
            diagnostics,
            styles.into_iter().collect(),
        ))
    }

    fn validate_cave_face_occupancy(
        &self,
        coordinate: ChunkCoordinate,
        draft: &ChunkDraftV1,
        requests: &[CaveFaceFieldRequestV1],
    ) -> WorldgenResult<Vec<CaveFaceOccupancyValidationV1>> {
        let edge = i64::from(self.config.chunk_edge_voxels);
        let origin = chunk_origin(coordinate, self.config.chunk_edge_voxels)?;
        let empty = self.role_target(D4MaterialRoleV1::Empty);
        let mut validations = Vec::with_capacity(requests.len());
        for request in requests {
            if !request.portal_requested() {
                validations.push(CaveFaceOccupancyValidationV1::new(*request, 0, 0, 0));
                continue;
            }
            let center_u = i64::from(request.portal_u_voxel());
            let center_v = i64::from(request.portal_v_voxel());
            let radius = i64::from(request.clearance_radius_voxels());
            let mut aperture_samples = 0_u32;
            let mut field_void_samples = 0_u32;
            let mut final_empty_samples = 0_u32;
            for offset_v in -radius..=radius {
                for offset_u in -radius..=radius {
                    let u = center_u.saturating_add(offset_u);
                    let v = center_v.saturating_add(offset_v);
                    if !(0..edge).contains(&u) || !(0..edge).contains(&v) {
                        continue;
                    }
                    let (local_x, local_y, local_z) = face_local_sample(request.face(), u, v, edge);
                    let world_x = origin.0.saturating_add(local_x);
                    let world_y = origin.1.saturating_add(local_y);
                    let world_z = origin.2.saturating_add(local_z);
                    aperture_samples = aperture_samples.saturating_add(1);
                    if self.cave.signed_distance_fixed(world_x, world_y, world_z) <= 0 {
                        field_void_samples = field_void_samples.saturating_add(1);
                    }
                    let local_x =
                        u16::try_from(local_x).map_err(|_| WorldgenError::ArithmeticOverflow {
                            operation: "cave validation local X",
                        })?;
                    let local_y =
                        u16::try_from(local_y).map_err(|_| WorldgenError::ArithmeticOverflow {
                            operation: "cave validation local Y",
                        })?;
                    let local_z =
                        u16::try_from(local_z).map_err(|_| WorldgenError::ArithmeticOverflow {
                            operation: "cave validation local Z",
                        })?;
                    let block = draft.block_at(local_x, local_y, local_z).ok_or(
                        WorldgenError::ArithmeticOverflow {
                            operation: "cave validation draft lookup",
                        },
                    )?;
                    if block == empty {
                        final_empty_samples = final_empty_samples.saturating_add(1);
                    }
                }
            }
            validations.push(CaveFaceOccupancyValidationV1::new(
                *request,
                aperture_samples,
                field_void_samples,
                final_empty_samples,
            ));
        }
        Ok(validations)
    }

    #[allow(
        clippy::too_many_arguments,
        reason = "column, vegetation, and diagnostic counters stay explicit on the voxel path"
    )]
    fn material_role(
        &self,
        x: i64,
        y: i64,
        z: i64,
        column: ColumnSampleV1,
        vegetation: Option<D4MaterialRoleV1>,
        counters: &mut WorkCountersV1,
        natural_counters: &mut NaturalWorkCountersV1,
    ) -> D4MaterialRoleV1 {
        if y < i64::from(self.config.world_floor_y) || y > i64::from(self.config.world_ceiling_y) {
            return D4MaterialRoleV1::Empty;
        }
        if y > i64::from(column.height) {
            return vegetation.unwrap_or(D4MaterialRoleV1::Empty);
        }
        counters.cave_samples = counters.cave_samples.saturating_add(1);
        let occupancy = self.cave.occupancy(x, y, z, column.height);
        if occupancy.is_finally_void() {
            counters.cave_void_accepts = counters.cave_void_accepts.saturating_add(1);
            return D4MaterialRoleV1::Empty;
        }

        let depth = i64::from(column.height).saturating_sub(y);
        if let Some(natural) = &self.natural {
            natural_counters.river_samples = natural_counters.river_samples.saturating_add(1);
            if column.in_river_channel && y == i64::from(column.height) {
                return natural.channel_bed_role(x, z, column.material_style);
            }
            natural_counters.geology_samples = natural_counters.geology_samples.saturating_add(1);
            natural_counters.resource_samples = natural_counters.resource_samples.saturating_add(1);
            let resource = natural.resource_sample(x, y, z, column.height, column.material_style);
            if let Some(role) = resource.role() {
                natural_counters.resource_accepts =
                    natural_counters.resource_accepts.saturating_add(1);
                return role;
            }
            return natural
                .geologic_sample(x, y, z, column.height, column.material_style)
                .role();
        }
        match column.material_style {
            TerrainStyleV1::TemperateWoodland | TerrainStyleV1::BorealWetland => {
                self.temperate_material(x, y, z, depth, counters)
            }
            TerrainStyleV1::AridBadlands => self.arid_material(x, y, z, depth, counters),
        }
    }

    #[allow(
        clippy::too_many_lines,
        reason = "bounded ground cover and tree rasterization share one overlay allocation"
    )]
    fn vegetation_overlay(
        &self,
        origin: (i64, i64, i64),
        edge: usize,
        columns: &[ColumnSampleV1],
        counters: &mut WorkCountersV1,
    ) -> WorldgenResult<Vec<Option<D4MaterialRoleV1>>> {
        let voxel_count = edge
            .checked_mul(edge)
            .and_then(|square| square.checked_mul(edge))
            .ok_or(WorldgenError::ArithmeticOverflow {
                operation: "vegetation overlay voxel count",
            })?;
        let mut overlay = vec![None; voxel_count];

        for local_z in 0..edge {
            for local_x in 0..edge {
                let column = &columns[local_z * edge + local_x];
                if column.material_style != TerrainStyleV1::TemperateWoodland {
                    continue;
                }
                let world_x = origin
                    .0
                    .saturating_add(i64::try_from(local_x).unwrap_or_default());
                let world_z = origin
                    .2
                    .saturating_add(i64::try_from(local_z).unwrap_or_default());
                let cover_y = i64::from(column.height).saturating_add(1);
                counters.ground_cover_samples = counters.ground_cover_samples.saturating_add(1);
                if Self::sample_threshold(
                    self.ground_cover_seed,
                    world_x,
                    cover_y,
                    world_z,
                    self.config.ground_cover_threshold_per_1024,
                ) {
                    counters.ground_cover_accepts = counters.ground_cover_accepts.saturating_add(1);
                    set_vegetation_role(
                        &mut overlay,
                        origin,
                        edge,
                        world_x,
                        cover_y,
                        world_z,
                        D4MaterialRoleV1::WoodlandGroundCover,
                    );
                }
            }
        }

        let edge_i64 = i64::try_from(edge).map_err(|_| WorldgenError::ArithmeticOverflow {
            operation: "vegetation chunk edge",
        })?;
        let minimum_x = origin.0.saturating_sub(MAX_TREE_RADIUS);
        let maximum_x = origin
            .0
            .saturating_add(edge_i64.saturating_sub(1))
            .saturating_add(MAX_TREE_RADIUS);
        let minimum_z = origin.2.saturating_sub(MAX_TREE_RADIUS);
        let maximum_z = origin
            .2
            .saturating_add(edge_i64.saturating_sub(1))
            .saturating_add(MAX_TREE_RADIUS);
        for anchor_z in minimum_z..=maximum_z {
            for anchor_x in minimum_x..=maximum_x {
                counters.tree_anchor_samples = counters.tree_anchor_samples.saturating_add(1);
                counters.territory_queries = counters.territory_queries.saturating_add(1);
                let sample = self.territory.sample(anchor_x, anchor_z);
                let style = self
                    .territory
                    .choose_material_style(anchor_x, anchor_z, sample);
                if style != TerrainStyleV1::TemperateWoodland
                    || !self.is_tree_anchor(anchor_x, anchor_z)
                {
                    continue;
                }
                counters.tree_anchor_accepts = counters.tree_anchor_accepts.saturating_add(1);
                counters.height_samples = counters.height_samples.saturating_add(1);
                let anchor_height = i64::from(self.territory.height(anchor_x, anchor_z, sample));
                for relative_y in 4..=MAX_TREE_HEIGHT {
                    for offset_z in -MAX_TREE_RADIUS..=MAX_TREE_RADIUS {
                        for offset_x in -MAX_TREE_RADIUS..=MAX_TREE_RADIUS {
                            if offset_x.abs().saturating_add(offset_z.abs())
                                > MAX_TREE_RADIUS.saturating_add(1)
                            {
                                continue;
                            }
                            set_vegetation_role(
                                &mut overlay,
                                origin,
                                edge,
                                anchor_x.saturating_add(offset_x),
                                anchor_height.saturating_add(relative_y),
                                anchor_z.saturating_add(offset_z),
                                D4MaterialRoleV1::WoodlandLeaves,
                            );
                        }
                    }
                }
                for relative_y in 1..=4 {
                    set_vegetation_role(
                        &mut overlay,
                        origin,
                        edge,
                        anchor_x,
                        anchor_height.saturating_add(relative_y),
                        anchor_z,
                        D4MaterialRoleV1::WoodlandLog,
                    );
                }
            }
        }
        Ok(overlay)
    }

    #[allow(
        clippy::too_many_lines,
        reason = "exclusion-radius rasterization keeps halo and canopy writes in one overlay"
    )]
    fn natural_vegetation_overlay(
        &self,
        origin: (i64, i64, i64),
        edge: usize,
        columns: &[ColumnSampleV1],
        counters: &mut NaturalWorkCountersV1,
    ) -> WorldgenResult<Vec<Option<D4MaterialRoleV1>>> {
        let Some(natural) = &self.natural else {
            return Ok(vec![None; edge.saturating_mul(edge).saturating_mul(edge)]);
        };
        let voxel_count = edge
            .checked_mul(edge)
            .and_then(|square| square.checked_mul(edge))
            .ok_or(WorldgenError::ArithmeticOverflow {
                operation: "natural vegetation overlay voxel count",
            })?;
        let mut overlay = vec![None; voxel_count];
        for local_z in 0..edge {
            for local_x in 0..edge {
                let column = &columns[local_z * edge + local_x];
                let world_x = origin
                    .0
                    .saturating_add(i64::try_from(local_x).unwrap_or_default());
                let world_z = origin
                    .2
                    .saturating_add(i64::try_from(local_z).unwrap_or_default());
                counters.ground_cover_samples = counters.ground_cover_samples.saturating_add(1);
                counters.river_samples = counters.river_samples.saturating_add(1);
                if let Some(role) = natural.ground_cover_role(
                    world_x,
                    world_z,
                    column.material_style,
                    column.in_river_channel,
                ) {
                    counters.ground_cover_accepts = counters.ground_cover_accepts.saturating_add(1);
                    set_vegetation_role(
                        &mut overlay,
                        origin,
                        edge,
                        world_x,
                        i64::from(column.height).saturating_add(1),
                        world_z,
                        role,
                    );
                }
            }
        }

        let edge_i64 = i64::try_from(edge).map_err(|_| WorldgenError::ArithmeticOverflow {
            operation: "natural vegetation chunk edge",
        })?;
        let radius = NaturalSamplerV1::tree_radius()
            .saturating_add(i64::from(natural.config().tree_exclusion_radius_voxels));
        let minimum_x = origin.0.saturating_sub(radius);
        let maximum_x = origin
            .0
            .saturating_add(edge_i64.saturating_sub(1))
            .saturating_add(radius);
        let minimum_z = origin.2.saturating_sub(radius);
        let maximum_z = origin
            .2
            .saturating_add(edge_i64.saturating_sub(1))
            .saturating_add(radius);
        for anchor_z in minimum_z..=maximum_z {
            for anchor_x in minimum_x..=maximum_x {
                if !natural.may_have_tree_anchor(anchor_x, anchor_z) {
                    continue;
                }
                let sample = self.territory.sample(anchor_x, anchor_z);
                let style = self
                    .territory
                    .choose_material_style(anchor_x, anchor_z, sample);
                let Some((log, leaves)) = NaturalSamplerV1::tree_roles(style) else {
                    continue;
                };
                if !natural.is_exclusive_tree_anchor(anchor_x, anchor_z, style, counters) {
                    continue;
                }
                let anchor_height = i64::from(self.terrain_height(anchor_x, anchor_z));
                for relative_y in 4..=NaturalSamplerV1::tree_height() {
                    for offset_z in
                        -NaturalSamplerV1::tree_radius()..=NaturalSamplerV1::tree_radius()
                    {
                        for offset_x in
                            -NaturalSamplerV1::tree_radius()..=NaturalSamplerV1::tree_radius()
                        {
                            if offset_x.abs().saturating_add(offset_z.abs())
                                > NaturalSamplerV1::tree_radius().saturating_add(1)
                            {
                                continue;
                            }
                            set_vegetation_role(
                                &mut overlay,
                                origin,
                                edge,
                                anchor_x.saturating_add(offset_x),
                                anchor_height.saturating_add(relative_y),
                                anchor_z.saturating_add(offset_z),
                                leaves,
                            );
                        }
                    }
                }
                for relative_y in 1..=4 {
                    set_vegetation_role(
                        &mut overlay,
                        origin,
                        edge,
                        anchor_x,
                        anchor_height.saturating_add(relative_y),
                        anchor_z,
                        log,
                    );
                }
            }
        }
        Ok(overlay)
    }

    fn temperate_material(
        &self,
        x: i64,
        y: i64,
        z: i64,
        depth: i64,
        counters: &mut WorkCountersV1,
    ) -> D4MaterialRoleV1 {
        if depth == 0 {
            return D4MaterialRoleV1::TemperateSurface;
        }
        if depth <= 3 {
            return match self.material_roll(x, y, z) % 16 {
                0 => D4MaterialRoleV1::TemperateClay,
                1 => D4MaterialRoleV1::TemperateGravel,
                _ => D4MaterialRoleV1::TemperateSubsurface,
            };
        }
        counters.resource_samples = counters.resource_samples.saturating_add(1);
        if Self::sample_threshold(
            self.material_seed,
            x,
            y,
            z,
            self.config.copper_threshold_per_1024,
        ) {
            counters.resource_accepts = counters.resource_accepts.saturating_add(1);
            D4MaterialRoleV1::CopperResource
        } else if self.material_roll(x, y, z).is_multiple_of(7) {
            D4MaterialRoleV1::TemperateSecondaryRock
        } else {
            D4MaterialRoleV1::TemperateBaseRock
        }
    }

    fn arid_material(
        &self,
        x: i64,
        y: i64,
        z: i64,
        depth: i64,
        counters: &mut WorkCountersV1,
    ) -> D4MaterialRoleV1 {
        let red = self.material_roll(x, i64::from(self.config.arid_base_height), z) & 1 == 1;
        if depth == 0 {
            return if red {
                D4MaterialRoleV1::AridRedSand
            } else {
                D4MaterialRoleV1::AridSand
            };
        }
        if depth <= 4 {
            return if red {
                D4MaterialRoleV1::AridRedSandstone
            } else {
                D4MaterialRoleV1::AridSandstone
            };
        }
        counters.resource_samples = counters.resource_samples.saturating_add(1);
        if Self::sample_threshold(
            self.material_seed,
            x,
            y,
            z,
            self.config.copper_threshold_per_1024,
        ) {
            counters.resource_accepts = counters.resource_accepts.saturating_add(1);
            D4MaterialRoleV1::CopperResource
        } else {
            D4MaterialRoleV1::AridBaseRock
        }
    }
    fn is_tree_anchor(&self, x: i64, z: i64) -> bool {
        Self::sample_threshold(self.tree_seed, x, 0, z, self.config.tree_threshold_per_1024)
    }

    fn material_roll(&self, x: i64, y: i64, z: i64) -> u64 {
        sample_hash_3d(self.material_seed, x, y, z)
    }

    fn sample_threshold(seed: u64, x: i64, y: i64, z: i64, threshold: u16) -> bool {
        sample_hash_3d(seed, x, y, z) % 1_024 < u64::from(threshold)
    }
}

#[derive(Clone, Copy, Debug)]
struct ColumnSampleV1 {
    height: i32,
    material_style: TerrainStyleV1,
    in_river_channel: bool,
}

fn local_world_axis(origin: i64, local: usize) -> i64 {
    origin.saturating_add(i64::try_from(local).unwrap_or_default())
}

#[derive(Clone, Copy, Debug)]
struct DraftCaveOccupancyV1<'a> {
    voxel_palette_indices: &'a [u16],
    empty_palette_index: u16,
}

impl<'a> DraftCaveOccupancyV1<'a> {
    fn new(plan: &GenerationPlanV1, draft: &'a ChunkDraftV1, edge: usize) -> WorldgenResult<Self> {
        if draft.edge_voxels() != plan.config.chunk_edge_voxels {
            return Err(WorldgenError::InvalidHydrologyOccupancy {
                field: "snapshot",
                reason: "snapshot draft edge does not match the generation plan".to_owned(),
            });
        }
        let expected_voxels = edge
            .checked_mul(edge)
            .and_then(|area| area.checked_mul(edge))
            .ok_or(WorldgenError::ArithmeticOverflow {
                operation: "snapshot draft voxel count",
            })?;
        if draft.voxel_palette_indices().len() != expected_voxels {
            return Err(WorldgenError::InvalidHydrologyOccupancy {
                field: "snapshot",
                reason: "snapshot draft voxel count does not match its edge".to_owned(),
            });
        }
        let empty = plan.role_target(D4MaterialRoleV1::Empty);
        let empty_palette_index = draft
            .palette()
            .iter()
            .position(|block| block == empty)
            .and_then(|index| u16::try_from(index).ok())
            .ok_or_else(|| WorldgenError::InvalidHydrologyOccupancy {
                field: "snapshot",
                reason: "snapshot draft does not contain the plan's empty material".to_owned(),
            })?;
        Ok(Self {
            voxel_palette_indices: draft.voxel_palette_indices(),
            empty_palette_index,
        })
    }

    fn allows_fluid_occupancy(self, voxel_index: usize) -> bool {
        self.voxel_palette_indices.get(voxel_index).copied() == Some(self.empty_palette_index)
    }
}

fn set_vegetation_role(
    overlay: &mut [Option<D4MaterialRoleV1>],
    origin: (i64, i64, i64),
    edge: usize,
    x: i64,
    y: i64,
    z: i64,
    role: D4MaterialRoleV1,
) {
    let local_x = x.saturating_sub(origin.0);
    let local_y = y.saturating_sub(origin.1);
    let local_z = z.saturating_sub(origin.2);
    let Ok(local_x) = usize::try_from(local_x) else {
        return;
    };
    let Ok(local_y) = usize::try_from(local_y) else {
        return;
    };
    let Ok(local_z) = usize::try_from(local_z) else {
        return;
    };
    if local_x >= edge || local_y >= edge || local_z >= edge {
        return;
    }
    let index = (local_y * edge + local_z) * edge + local_x;
    let slot = &mut overlay[index];
    if role == D4MaterialRoleV1::WoodlandLog
        || role == D4MaterialRoleV1::BorealLog
        || slot.is_none()
        || *slot == Some(D4MaterialRoleV1::WoodlandGroundCover)
        || *slot == Some(D4MaterialRoleV1::Moss)
        || *slot == Some(D4MaterialRoleV1::Peat)
    {
        *slot = Some(role);
    }
}
#[derive(Clone, Copy, Debug)]
struct WorkCountersV1 {
    territory_queries: u64,
    height_samples: u64,
    cave_samples: u64,
    cave_void_accepts: u64,
    tree_anchor_samples: u64,
    tree_anchor_accepts: u64,
    ground_cover_samples: u64,
    ground_cover_accepts: u64,
    resource_samples: u64,
    resource_accepts: u64,
}

fn placement_predicate_receipts(
    diagnostics: GenerationDiagnosticsV1,
) -> Vec<PlacementPredicateReceiptV1> {
    vec![
        PlacementPredicateReceiptV1 {
            predicate: PlacementPredicateKindV1::CaveOccupancy,
            placement_roles: vec![D4MaterialRoleV1::Empty],
            evaluations: diagnostics.cave_samples,
            accepted: diagnostics.cave_void_accepts,
        },
        PlacementPredicateReceiptV1 {
            predicate: PlacementPredicateKindV1::TreeAnchor,
            placement_roles: vec![
                D4MaterialRoleV1::WoodlandLog,
                D4MaterialRoleV1::WoodlandLeaves,
            ],
            evaluations: diagnostics.tree_anchor_samples,
            accepted: diagnostics.tree_anchor_accepts,
        },
        PlacementPredicateReceiptV1 {
            predicate: PlacementPredicateKindV1::GroundCover,
            placement_roles: vec![D4MaterialRoleV1::WoodlandGroundCover],
            evaluations: diagnostics.ground_cover_samples,
            accepted: diagnostics.ground_cover_accepts,
        },
        PlacementPredicateReceiptV1 {
            predicate: PlacementPredicateKindV1::CopperResource,
            placement_roles: vec![D4MaterialRoleV1::CopperResource],
            evaluations: diagnostics.resource_samples,
            accepted: diagnostics.resource_accepts,
        },
    ]
}
#[derive(Serialize)]
struct SnapshotEnvelopeV1<'a> {
    schema: &'static str,
    dimension: &'a DimensionId,
    coordinate: ChunkCoordinate,
    planning_cell: PlanningCellCoordinateV1,
    generation_epoch: GenerationEpochIdV1,
    generation_input_hash: GenerationInputHashV1,
    draft: &'a ChunkDraftV1,
}

fn preflight_plan_input_bytes(input: &GenerationPlanInputV1) -> WorldgenResult<()> {
    let mut total = 4_096_u64;
    add_plan_bytes(
        &mut total,
        input.terrain_config.canonical_bytes()?.len(),
        "terrain config bytes",
    )?;
    add_plan_bytes(
        &mut total,
        input.dimension.as_str().len(),
        "dimension identity bytes",
    )?;
    for offer in &input.provider_offers {
        add_identity_bytes(
            &mut total,
            offer.identity().provider_stable_id(),
            input.limits,
        )?;
    }
    if let Some(layer) = &input.natural_layer {
        for offer in layer.provider_offers() {
            add_identity_bytes(
                &mut total,
                offer.identity().provider_stable_id(),
                input.limits,
            )?;
        }
        for (_, role) in layer.vocabulary().iter() {
            add_identity_bytes(&mut total, role, input.limits)?;
        }
    }
    if let Some(layer) = &input.hydrology_occupancy {
        add_identity_bytes(&mut total, layer.fluids().water(), input.limits)?;
        add_identity_bytes(&mut total, layer.fluids().lava(), input.limits)?;
        add_identity_bytes(&mut total, layer.fluids().water_predicate(), input.limits)?;
        add_identity_bytes(&mut total, layer.fluids().lava_predicate(), input.limits)?;
    }
    for (_, role) in input.role_vocabulary.iter() {
        add_identity_bytes(&mut total, role, input.limits)?;
    }
    for (role, target) in input.role_bindings.iter() {
        add_identity_bytes(&mut total, role, input.limits)?;
        add_identity_bytes(&mut total, target, input.limits)?;
    }
    for block in input.block_catalog.blocks() {
        add_identity_bytes(&mut total, block, input.limits)?;
    }
    let locked_bytes = u64::try_from(input.locked_receipts.len())
        .map_err(|_| WorldgenError::ArithmeticOverflow {
            operation: "locked receipt count",
        })?
        .checked_mul(32)
        .ok_or(WorldgenError::ArithmeticOverflow {
            operation: "locked receipt bytes",
        })?;
    total = total
        .checked_add(locked_bytes)
        .ok_or(WorldgenError::ArithmeticOverflow {
            operation: "plan input byte total",
        })?;
    if total > input.limits.max_plan_input_bytes.get() {
        return Err(WorldgenError::BudgetExceeded {
            budget: "plan input bytes",
            required: total,
            limit: input.limits.max_plan_input_bytes.get(),
        });
    }
    Ok(())
}

fn add_identity_bytes(
    total: &mut u64,
    identity: &StableId,
    limits: WorldgenLimitsV1,
) -> WorldgenResult<()> {
    let bytes =
        u64::try_from(identity.as_str().len()).map_err(|_| WorldgenError::ArithmeticOverflow {
            operation: "stable identity byte length",
        })?;
    if bytes > u64::from(limits.max_identity_bytes.get()) {
        return Err(WorldgenError::BudgetExceeded {
            budget: "stable identity bytes",
            required: bytes,
            limit: u64::from(limits.max_identity_bytes.get()),
        });
    }
    *total = total
        .checked_add(bytes)
        .ok_or(WorldgenError::ArithmeticOverflow {
            operation: "plan identity byte total",
        })?;
    Ok(())
}

fn add_plan_bytes(total: &mut u64, bytes: usize, operation: &'static str) -> WorldgenResult<()> {
    let bytes =
        u64::try_from(bytes).map_err(|_| WorldgenError::ArithmeticOverflow { operation })?;
    *total = total
        .checked_add(bytes)
        .ok_or(WorldgenError::ArithmeticOverflow {
            operation: "plan input byte total",
        })?;
    Ok(())
}

fn preflight_plan_limits(input: &GenerationPlanInputV1) -> WorldgenResult<()> {
    if input.provider_offers.len() > usize::from(input.limits.max_provider_offers.get()) {
        return Err(WorldgenError::CollectionLimitExceeded {
            kind: "provider offers",
            actual: input.provider_offers.len(),
            limit: usize::from(input.limits.max_provider_offers.get()),
        });
    }
    if input.role_bindings.len() > usize::from(input.limits.max_role_bindings.get()) {
        return Err(WorldgenError::CollectionLimitExceeded {
            kind: "frozen role bindings",
            actual: input.role_bindings.len(),
            limit: usize::from(input.limits.max_role_bindings.get()),
        });
    }
    if input.block_catalog.blocks().len() > usize::from(input.limits.max_catalog_blocks.get()) {
        return Err(WorldgenError::CollectionLimitExceeded {
            kind: "D4 catalog blocks",
            actual: input.block_catalog.blocks().len(),
            limit: usize::from(input.limits.max_catalog_blocks.get()),
        });
    }
    if input.config.chunk_edge_voxels > input.limits.max_chunk_edge_voxels.get() {
        return Err(WorldgenError::BudgetExceeded {
            budget: "chunk edge voxels",
            required: u64::from(input.config.chunk_edge_voxels),
            limit: u64::from(input.limits.max_chunk_edge_voxels.get()),
        });
    }
    let voxel_count = checked_cube_u64(u64::from(input.config.chunk_edge_voxels))?;
    if voxel_count > u64::from(input.limits.max_voxels_per_chunk.get()) {
        return Err(WorldgenError::BudgetExceeded {
            budget: "voxels per chunk",
            required: voxel_count,
            limit: u64::from(input.limits.max_voxels_per_chunk.get()),
        });
    }
    let conservative_work =
        voxel_count
            .checked_mul(80)
            .ok_or(WorldgenError::ArithmeticOverflow {
                operation: "conservative generation work bound",
            })?;
    if conservative_work > input.limits.max_work_units.get() {
        return Err(WorldgenError::BudgetExceeded {
            budget: "deterministic samples",
            required: conservative_work,
            limit: input.limits.max_work_units.get(),
        });
    }
    Ok(())
}

fn preflight_snapshot_bound(
    input: &GenerationPlanInputV1,
    roles: &[RoleBindingReceiptV1],
) -> WorldgenResult<u64> {
    let voxel_count = checked_cube_u64(u64::from(input.config.chunk_edge_voxels))?;
    let palette_bytes = roles.iter().try_fold(0_u64, |total, role| {
        let identity_bytes = u64::try_from(role.block_id.as_str().len()).map_err(|_| {
            WorldgenError::ArithmeticOverflow {
                operation: "block identity byte length",
            }
        })?;
        total.checked_add(identity_bytes.saturating_add(3)).ok_or(
            WorldgenError::ArithmeticOverflow {
                operation: "snapshot palette byte bound",
            },
        )
    })?;
    let index_bytes = voxel_count
        .checked_mul(3)
        .ok_or(WorldgenError::ArithmeticOverflow {
            operation: "snapshot voxel-index byte bound",
        })?;
    let required = index_bytes
        .checked_add(palette_bytes)
        .and_then(|bytes| bytes.checked_add(4_096))
        .ok_or(WorldgenError::ArithmeticOverflow {
            operation: "snapshot byte bound",
        })?;
    if required > input.limits.max_snapshot_bytes.get() {
        return Err(WorldgenError::BudgetExceeded {
            budget: "snapshot bytes",
            required,
            limit: input.limits.max_snapshot_bytes.get(),
        });
    }
    Ok(required)
}

fn preflight_live_generation_bound(
    input: &GenerationPlanInputV1,
    roles: &[RoleBindingReceiptV1],
    snapshot_bound: u64,
) -> WorldgenResult<()> {
    let voxel_count = checked_cube_u64(u64::from(input.config.chunk_edge_voxels))?;
    let column_count = u64::from(input.config.chunk_edge_voxels)
        .checked_mul(u64::from(input.config.chunk_edge_voxels))
        .ok_or(WorldgenError::ArithmeticOverflow {
            operation: "live generation column count",
        })?;
    let palette_bytes = roles.iter().try_fold(0_u64, |total, role| {
        let bytes = u64::try_from(role.block_id.as_str().len()).map_err(|_| {
            WorldgenError::ArithmeticOverflow {
                operation: "live palette identity bytes",
            }
        })?;
        total
            .checked_add(bytes)
            .ok_or(WorldgenError::ArithmeticOverflow {
                operation: "live palette identity total",
            })
    })?;
    let required = snapshot_bound
        .checked_mul(2)
        .and_then(|bytes| bytes.checked_add(voxel_count.saturating_mul(8)))
        .and_then(|bytes| bytes.checked_add(column_count.saturating_mul(64)))
        .and_then(|bytes| bytes.checked_add(palette_bytes.saturating_mul(4)))
        .and_then(|bytes| bytes.checked_add(65_536))
        .ok_or(WorldgenError::ArithmeticOverflow {
            operation: "live generation byte bound",
        })?;
    if required > input.limits.max_live_generation_bytes.get() {
        return Err(WorldgenError::BudgetExceeded {
            budget: "live generation bytes",
            required,
            limit: input.limits.max_live_generation_bytes.get(),
        });
    }
    Ok(())
}

fn resolve_roles(
    vocabulary: &D4RoleVocabularyV1,
    bindings: &FrozenRoleBindingsV1,
    catalog: &D4BlockCatalogClosureV1,
) -> WorldgenResult<Vec<RoleBindingReceiptV1>> {
    let mut receipts = Vec::with_capacity(D4MaterialRoleV1::ALL.len());
    let mut targets = BTreeMap::<StableId, D4MaterialRoleV1>::new();
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
        if let Some(first) = targets.insert(target.clone(), purpose) {
            return Err(WorldgenError::DuplicateRequiredRoleTarget {
                first_purpose: first.as_str(),
                second_purpose: purpose.as_str(),
                target: Box::new(target.clone()),
            });
        }
        receipts.push(RoleBindingReceiptV1 {
            purpose,
            role_id: role.clone(),
            block_id: target.clone(),
        });
    }
    Ok(receipts)
}

fn checked_cube_u64(edge: u64) -> WorldgenResult<u64> {
    edge.checked_mul(edge)
        .and_then(|square| square.checked_mul(edge))
        .ok_or(WorldgenError::ArithmeticOverflow {
            operation: "cubic chunk voxel count",
        })
}

pub(crate) fn face_local_sample(
    face: crate::ChunkFaceV1,
    u: i64,
    v: i64,
    edge: i64,
) -> (i64, i64, i64) {
    match face {
        crate::ChunkFaceV1::NegativeX => (0, u, v),
        crate::ChunkFaceV1::PositiveX => (edge.saturating_sub(1), u, v),
        crate::ChunkFaceV1::NegativeY => (u, 0, v),
        crate::ChunkFaceV1::PositiveY => (u, edge.saturating_sub(1), v),
        crate::ChunkFaceV1::NegativeZ => (u, v, 0),
        crate::ChunkFaceV1::PositiveZ => (u, v, edge.saturating_sub(1)),
    }
}

fn canonical_outward_face(first: ChunkCoordinate, second: ChunkCoordinate) -> crate::ChunkFaceV1 {
    if second.x != first.x {
        if second.x > first.x {
            crate::ChunkFaceV1::PositiveX
        } else {
            crate::ChunkFaceV1::NegativeX
        }
    } else if second.y != first.y {
        if second.y > first.y {
            crate::ChunkFaceV1::PositiveY
        } else {
            crate::ChunkFaceV1::NegativeY
        }
    } else if second.z > first.z {
        crate::ChunkFaceV1::PositiveZ
    } else {
        crate::ChunkFaceV1::NegativeZ
    }
}

pub(crate) fn chunk_origin(
    coordinate: ChunkCoordinate,
    edge: u16,
) -> WorldgenResult<(i64, i64, i64)> {
    let edge = i64::from(edge);
    let x = i64::from(coordinate.x)
        .checked_mul(edge)
        .ok_or(WorldgenError::ArithmeticOverflow {
            operation: "chunk origin X",
        })?;
    let y = i64::from(coordinate.y)
        .checked_mul(edge)
        .ok_or(WorldgenError::ArithmeticOverflow {
            operation: "chunk origin Y",
        })?;
    let z = i64::from(coordinate.z)
        .checked_mul(edge)
        .ok_or(WorldgenError::ArithmeticOverflow {
            operation: "chunk origin Z",
        })?;
    Ok((x, y, z))
}

fn encode_canonical<T>(kind: &'static str, value: &T) -> WorldgenResult<Vec<u8>>
where
    T: Serialize + ?Sized,
{
    canonical_json_bytes(value).map_err(|error| WorldgenError::CanonicalEncoding {
        kind,
        reason: error.to_string(),
    })
}

fn missing_role_target(purpose: D4MaterialRoleV1) -> &'static StableId {
    panic!("validated generation plan lost role target `{purpose}`")
}
