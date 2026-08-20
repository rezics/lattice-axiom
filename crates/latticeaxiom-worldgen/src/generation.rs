use std::collections::{BTreeMap, BTreeSet};

use latticeaxiom_core::{CanonicalHash, StableId, canonical_json_bytes};
use latticeaxiom_storage::{ChunkCoordinate, ChunkRevisionExpectation, DimensionId};
use serde::{Deserialize, Serialize};

use crate::{
    AdjacentEpochSnapshotV1, BoundaryAdapterDeclarationV1, CaveFaceFieldRequestV1,
    CaveFaceOccupancyValidationV1, CellEpochStateV1, D4BlockCatalogClosureV1, D4MaterialRoleV1,
    D4RoleVocabularyV1, ExistingSnapshotEvidenceV1, FrozenRoleBindingsV1, GenerationEpochIdV1,
    GenerationInputHashV1, GenerationProvenanceHashV1, GeneratorFingerprintV1,
    LockedClosureFingerprintV1, PlanActivationIdV1, PlanningCellCoordinateV1,
    ProviderGenerationIdentityV1, ProviderOfferV1, ProviderSlotV1, SnapshotChecksumV1,
    TerrainStyleV1, TerritoryQueryV1, WorldSeedV1, WorldgenConfigHashV1, WorldgenConfigV1,
    WorldgenError, WorldgenLimitsV1, WorldgenResult,
    cave::{CaveSamplerV1, snapshot_checksum},
    epoch::validate_epoch_boundaries,
    hashes::{concatenated_hash, domain_hash, hash_u64},
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
    generation_plan_revision: u64,
    plan_activation_id: PlanActivationIdV1,
    provider_offers: Vec<ProviderOfferV1>,
    role_vocabulary: D4RoleVocabularyV1,
    role_bindings: FrozenRoleBindingsV1,
    block_catalog: D4BlockCatalogClosureV1,
    authoritative_semantic_receipt: CanonicalHash,
    locked_receipts: Vec<CanonicalHash>,
    limits: WorldgenLimitsV1,
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
        Self {
            dimension,
            world_seed,
            config,
            generation_plan_revision,
            plan_activation_id,
            provider_offers,
            role_vocabulary,
            role_bindings,
            block_catalog,
            authoritative_semantic_receipt,
            locked_receipts,
            limits,
        }
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
    config: WorldgenConfigV1,
    generation_plan_revision: u64,
    plan_activation_id: PlanActivationIdV1,
    providers: ResolvedProvidersV1,
    roles: Vec<RoleBindingReceiptV1>,
    role_targets: BTreeMap<D4MaterialRoleV1, StableId>,
    config_hash: WorldgenConfigHashV1,
    generator_fingerprint: GeneratorFingerprintV1,
    locked_closure_fingerprint: LockedClosureFingerprintV1,
    generation_input_hash: GenerationInputHashV1,
    generation_provenance_hash: GenerationProvenanceHashV1,
    generation_epoch: GenerationEpochIdV1,
    limits: WorldgenLimitsV1,
    territory: TerritorySamplerV1,
    cave: CaveSamplerV1,
}

impl GenerationPlanV1 {
    /// Validates and compiles an immutable D4 generation plan.
    ///
    /// # Errors
    ///
    /// Fails before generation for malformed config, missing or conflicting
    /// exclusive providers, incomplete Role/content closure, inconsistent
    /// provider fingerprints, or any preflight budget violation.
    pub fn compile(input: GenerationPlanInputV1) -> WorldgenResult<Self> {
        preflight_plan_limits(&input)?;
        input.config.validate()?;
        preflight_plan_input_bytes(&input)?;
        let roles = resolve_roles(
            &input.role_vocabulary,
            &input.role_bindings,
            &input.block_catalog,
        )?;
        let snapshot_bound = preflight_snapshot_bound(&input, &roles)?;
        preflight_live_generation_bound(&input, &roles, snapshot_bound)?;
        let providers = ResolvedProvidersV1::resolve(input.provider_offers, input.limits)?;
        let role_targets = roles
            .iter()
            .map(|receipt| (receipt.purpose, receipt.block_id.clone()))
            .collect::<BTreeMap<_, _>>();
        let config_bytes = input.config.canonical_bytes()?;
        let config_hash = input.config.canonical_hash()?;
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
        let generation_input_hash = GenerationInputHashV1::from_hash(concatenated_hash(
            GENERATION_INPUT_DOMAIN,
            &[
                input.world_seed.as_bytes(),
                config_bytes.as_slice(),
                &revision_bytes,
                provider_bytes.as_slice(),
                input.authoritative_semantic_receipt.as_bytes(),
                role_bytes.as_slice(),
            ],
        ));
        let generation_provenance_hash = GenerationProvenanceHashV1::from_hash(concatenated_hash(
            GENERATION_PROVENANCE_DOMAIN,
            &[generation_input_hash.as_bytes(), locked_bytes.as_slice()],
        ));
        let generation_epoch = GenerationEpochIdV1::from_hash(concatenated_hash(
            GENERATION_EPOCH_DOMAIN,
            &[
                &revision_bytes,
                input.authoritative_semantic_receipt.as_bytes(),
                config_hash.as_bytes(),
                provider_bytes.as_slice(),
            ],
        ));

        let territory = TerritorySamplerV1::new(
            input.world_seed,
            generation_input_hash,
            input.config.clone(),
            providers.identity(ProviderSlotV1::StyleSelector).clone(),
            providers
                .identity(ProviderSlotV1::TerrainTransition)
                .clone(),
            providers.identity(ProviderSlotV1::TemperateTerrain).clone(),
            providers.identity(ProviderSlotV1::AridTerrain).clone(),
        );
        let cave = CaveSamplerV1::new(
            input.world_seed,
            generation_input_hash,
            input.config.clone(),
            providers.identity(ProviderSlotV1::CaveTopology).clone(),
        );

        Ok(Self {
            dimension: input.dimension,
            world_seed: input.world_seed,
            config: input.config,
            generation_plan_revision: input.generation_plan_revision,
            plan_activation_id: input.plan_activation_id,
            providers,
            roles,
            role_targets,
            config_hash,
            generator_fingerprint,
            locked_closure_fingerprint,
            generation_input_hash,
            generation_provenance_hash,
            generation_epoch,
            limits: input.limits,
            territory,
            cave,
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

    /// Returns the canonical config hash.
    #[must_use]
    pub const fn config_hash(&self) -> WorldgenConfigHashV1 {
        self.config_hash
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

    /// Returns deterministic terrain height intent at world `(x, z)`.
    #[must_use]
    pub fn terrain_height(&self, x: i64, z: i64) -> i32 {
        let sample = self.territory.sample(x, z);
        self.territory.height(x, z, sample)
    }

    /// Returns deterministic signed density intent at world `(x, y, z)`.
    ///
    /// Positive values are solid, zero is the surface, and negative values are
    /// void. Minimal caves turn otherwise-solid samples negative.
    #[must_use]
    pub fn terrain_density(&self, x: i64, y: i64, z: i64) -> i64 {
        let sample = self.territory.sample(x, z);
        let height = self.territory.height(x, z, sample);
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
            self.limits,
        )?;
        let cave_field_requests = self.cave.face_requests(request.coordinate)?;
        let (draft, diagnostics, styles_present) = self.materialize(request.coordinate)?;
        let cave_occupancy_validations =
            self.validate_cave_face_occupancy(request.coordinate, &draft, &cave_field_requests)?;
        let placement_predicates = placement_predicate_receipts(diagnostics);
        let draft_bytes = draft.canonical_bytes()?;
        let draft_hash = CanonicalHash::digest(&draft_bytes);
        let receipt = GenerationReceiptV1 {
            dimension: self.dimension.clone(),
            chunk: request.coordinate,
            planning_cell: expected_cell,
            generation_plan_revision: self.generation_plan_revision,
            generation_epoch: self.generation_epoch,
            config_hash: self.config_hash,
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
                let height = self.territory.height(world_x, world_z, sample);
                let material_style = self
                    .territory
                    .choose_material_style(world_x, world_z, sample);
                styles.insert(material_style);
                columns.push(ColumnSampleV1 {
                    height,
                    material_style,
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
        let vegetation = self.vegetation_overlay(origin, edge, &columns, &mut counters)?;

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
            .saturating_add(counters.resource_samples);
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
            tree_anchor_samples: counters.tree_anchor_samples,
            tree_anchor_accepts: counters.tree_anchor_accepts,
            ground_cover_samples: counters.ground_cover_samples,
            ground_cover_accepts: counters.ground_cover_accepts,
            resource_samples: counters.resource_samples,
            resource_accepts: counters.resource_accepts,
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

    fn material_role(
        &self,
        x: i64,
        y: i64,
        z: i64,
        column: ColumnSampleV1,
        vegetation: Option<D4MaterialRoleV1>,
        counters: &mut WorkCountersV1,
    ) -> D4MaterialRoleV1 {
        if y < i64::from(self.config.world_floor_y) || y > i64::from(self.config.world_ceiling_y) {
            return D4MaterialRoleV1::Empty;
        }
        if y > i64::from(column.height) {
            return vegetation.unwrap_or(D4MaterialRoleV1::Empty);
        }
        counters.cave_samples = counters.cave_samples.saturating_add(1);
        if self.cave.is_void(x, y, z, column.height) {
            counters.cave_void_accepts = counters.cave_void_accepts.saturating_add(1);
            return D4MaterialRoleV1::Empty;
        }

        let depth = i64::from(column.height).saturating_sub(y);
        match column.material_style {
            TerrainStyleV1::TemperateWoodland => self.temperate_material(x, y, z, depth, counters),
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
                if self.hash_threshold(
                    GROUND_COVER_DOMAIN,
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
        if self.hash_threshold(
            MATERIAL_DOMAIN,
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
        if self.hash_threshold(
            MATERIAL_DOMAIN,
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
        self.hash_threshold(TREE_DOMAIN, x, 0, z, self.config.tree_threshold_per_1024)
    }

    fn material_roll(&self, x: i64, y: i64, z: i64) -> u64 {
        hash_u64(
            MATERIAL_DOMAIN,
            &[
                self.world_seed.as_bytes(),
                self.generation_input_hash.as_bytes(),
                &x.to_be_bytes(),
                &y.to_be_bytes(),
                &z.to_be_bytes(),
            ],
        )
    }

    fn hash_threshold(&self, domain: &[u8], x: i64, y: i64, z: i64, threshold: u16) -> bool {
        hash_u64(
            domain,
            &[
                self.world_seed.as_bytes(),
                self.generation_input_hash.as_bytes(),
                &x.to_be_bytes(),
                &y.to_be_bytes(),
                &z.to_be_bytes(),
            ],
        ) % 1_024
            < u64::from(threshold)
    }

    fn role_target(&self, purpose: D4MaterialRoleV1) -> &StableId {
        self.role_targets
            .get(&purpose)
            .unwrap_or_else(|| missing_role_target(purpose))
    }
}

#[derive(Clone, Copy, Debug)]
struct ColumnSampleV1 {
    height: i32,
    material_style: TerrainStyleV1,
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
        || slot.is_none()
        || *slot == Some(D4MaterialRoleV1::WoodlandGroundCover)
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

fn face_local_sample(face: crate::ChunkFaceV1, u: i64, v: i64, edge: i64) -> (i64, i64, i64) {
    match face {
        crate::ChunkFaceV1::NegativeX => (0, u, v),
        crate::ChunkFaceV1::PositiveX => (edge.saturating_sub(1), u, v),
        crate::ChunkFaceV1::NegativeY => (u, 0, v),
        crate::ChunkFaceV1::PositiveY => (u, edge.saturating_sub(1), v),
        crate::ChunkFaceV1::NegativeZ => (u, v, 0),
        crate::ChunkFaceV1::PositiveZ => (u, v, edge.saturating_sub(1)),
    }
}

fn chunk_origin(coordinate: ChunkCoordinate, edge: u16) -> WorldgenResult<(i64, i64, i64)> {
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
