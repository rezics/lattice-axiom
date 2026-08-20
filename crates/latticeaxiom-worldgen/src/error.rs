use latticeaxiom_core::StableId;
use latticeaxiom_storage::ChunkCoordinate;
use thiserror::Error;

use crate::{CellEpochStateV1, GenerationEpochIdV1, PlanningCellCoordinateV1, ProviderSlotV1};

/// Result returned by world-generation contract operations.
pub type WorldgenResult<T> = Result<T, WorldgenError>;

/// Fail-closed validation or bounded-generation error.
#[derive(Debug, Error)]
pub enum WorldgenError {
    /// A closed configuration field violated its documented bounds.
    #[error("invalid worldgen config field `{field}`: {reason}")]
    InvalidConfig {
        /// Stable field name.
        field: &'static str,
        /// Actionable reason.
        reason: String,
    },
    /// A closed JSON configuration could not be decoded.
    #[error("invalid closed WorldgenConfigV1 JSON: {reason}")]
    InvalidConfigEncoding {
        /// Serde diagnostic.
        reason: String,
    },
    /// Canonical DTO encoding failed.
    #[error("failed to encode canonical {kind}: {reason}")]
    CanonicalEncoding {
        /// Kind being encoded.
        kind: &'static str,
        /// Underlying serialization diagnostic.
        reason: String,
    },
    /// A bounded collection exceeded its preflight limit.
    #[error("{kind} count {actual} exceeds hard limit {limit}")]
    CollectionLimitExceeded {
        /// Bounded collection kind.
        kind: &'static str,
        /// Observed count.
        actual: usize,
        /// Configured hard maximum.
        limit: usize,
    },
    /// Required coordinator/provider slot had no offer.
    #[error("required provider slot `{slot}` has no offer")]
    MissingProvider {
        /// Missing slot.
        slot: ProviderSlotV1,
    },
    /// An exclusive coordinator/provider slot had multiple offers.
    #[error("exclusive provider slot `{slot}` has conflicting offers: {providers:?}")]
    ConflictingProviders {
        /// Conflicting slot.
        slot: ProviderSlotV1,
        /// Canonically sorted provider identities.
        providers: Vec<StableId>,
    },
    /// One provider revision claimed two implementation fingerprints.
    #[error(
        "provider `{provider}` contract {contract_major} revision {algorithm_revision} has inconsistent implementation fingerprints"
    )]
    ProviderFingerprintConflict {
        /// Provider registration identity.
        provider: StableId,
        /// Contract major.
        contract_major: u32,
        /// Owner-controlled algorithm revision.
        algorithm_revision: u32,
    },
    /// Role vocabulary did not define one required material purpose.
    #[error("D4 role vocabulary is missing `{purpose}`")]
    MissingRolePurpose {
        /// Stable purpose name.
        purpose: &'static str,
    },
    /// One material purpose appeared more than once in authored input.
    #[error("D4 role vocabulary defines `{purpose}` more than once")]
    DuplicateRolePurpose {
        /// Stable purpose name.
        purpose: &'static str,
    },
    /// Two material purposes used the same role identity.
    #[error("role identity `{role}` is assigned to multiple D4 purposes")]
    DuplicateRoleIdentity {
        /// Reused role identity.
        role: StableId,
    },
    /// A material purpose did not point at a block-role registration.
    #[error("D4 purpose `{purpose}` expects a `block-role` identity, got `{role}`")]
    InvalidRoleIdentityKind {
        /// Stable purpose name.
        purpose: &'static str,
        /// Rejected identity.
        role: StableId,
    },
    /// A required frozen role had no concrete target.
    #[error("frozen role `{role}` has no concrete binding")]
    MissingRoleBinding {
        /// Missing role identity.
        role: StableId,
    },
    /// One role identity appeared more than once in frozen binding input.
    #[error("frozen role `{role}` has multiple concrete bindings")]
    DuplicateRoleBinding {
        /// Duplicate role identity.
        role: StableId,
    },
    /// A role target was not a block registration.
    #[error("frozen role `{role}` targets non-block identity `{target}`")]
    InvalidRoleTargetKind {
        /// Role identity.
        role: StableId,
        /// Rejected target.
        target: Box<StableId>,
    },
    /// Two D4 material roles resolved to the same concrete block.
    #[error(
        "D4 material purposes `{first_purpose}` and `{second_purpose}` both resolve to `{target}`"
    )]
    DuplicateRequiredRoleTarget {
        /// First stable purpose name.
        first_purpose: &'static str,
        /// Second stable purpose name.
        second_purpose: &'static str,
        /// Reused concrete target.
        target: Box<StableId>,
    },
    /// The D4 catalog closure reused a content identity.
    #[error("D4 block catalog contains duplicate identity `{block}`")]
    DuplicateCatalogBlock {
        /// Duplicate block identity.
        block: StableId,
    },
    /// A D4 catalog member was not a block registration.
    #[error("D4 block catalog contains non-block identity `{content}`")]
    InvalidCatalogBlockKind {
        /// Rejected content identity.
        content: StableId,
    },
    /// The D4 content closure was smaller than the normative minimum.
    #[error("D4 catalog closure requires at least {minimum} blocks, got {actual}")]
    IncompleteCatalogClosure {
        /// Minimum required unique block identities.
        minimum: usize,
        /// Observed unique identities.
        actual: usize,
    },
    /// A generated role target was absent from the supplied content closure.
    #[error("role `{role}` targets `{target}`, which is absent from the D4 catalog closure")]
    RoleTargetOutsideCatalog {
        /// Role identity.
        role: StableId,
        /// Missing target identity.
        target: Box<StableId>,
    },
    /// A checked coordinate or size computation overflowed.
    #[error("worldgen arithmetic overflow while computing {operation}")]
    ArithmeticOverflow {
        /// Stable operation name.
        operation: &'static str,
    },
    /// Preflight work or output size exceeded a hard budget.
    #[error("generation budget `{budget}` requires {required}, exceeding limit {limit}")]
    BudgetExceeded {
        /// Budget dimension.
        budget: &'static str,
        /// Required units.
        required: u64,
        /// Allowed units.
        limit: u64,
    },
    /// Existing snapshot coordinates did not match the request.
    #[error("existing snapshot coordinate {snapshot:?} does not match request {request:?}")]
    SnapshotCoordinateMismatch {
        /// Requested chunk coordinate.
        request: ChunkCoordinate,
        /// Existing snapshot coordinate.
        snapshot: ChunkCoordinate,
    },
    /// Existing snapshot dimension did not match the active plan.
    #[error("existing snapshot dimension `{actual}` does not match active dimension `{expected}")]
    SnapshotDimensionMismatch {
        /// Dimension compiled into the active plan.
        expected: latticeaxiom_storage::DimensionId,
        /// Dimension recorded by the existing snapshot.
        actual: Box<latticeaxiom_storage::DimensionId>,
    },
    /// Existing snapshot planning cell did not match its chunk coordinate.
    #[error("existing snapshot planning cell {actual:?} does not match expected {expected:?}")]
    SnapshotPlanningCellMismatch {
        /// Planning cell derived from the requested chunk.
        expected: PlanningCellCoordinateV1,
        /// Planning cell recorded by the existing snapshot.
        actual: PlanningCellCoordinateV1,
    },
    /// Existing snapshot epoch did not match the epoch state from the same read.
    #[error(
        "existing snapshot epoch {snapshot_epoch} conflicts with reported cell state {reported:?}"
    )]
    SnapshotEpochStateMismatch {
        /// Epoch recorded by the existing snapshot evidence.
        snapshot_epoch: GenerationEpochIdV1,
        /// Cell state reported alongside the evidence.
        reported: CellEpochStateV1,
    },
    /// Existing snapshot bytes did not match the supplied checksum.
    #[error("existing snapshot checksum does not match its exact bytes")]
    SnapshotChecksumMismatch,
    /// A frozen cell requires a generation epoch unavailable to this plan.
    #[error(
        "planning cell {cell:?} is frozen to epoch {frozen}, but active plan provides {active}"
    )]
    FrozenEpochUnavailable {
        /// Planning cell.
        cell: PlanningCellCoordinateV1,
        /// Previously frozen epoch.
        frozen: GenerationEpochIdV1,
        /// Active plan epoch.
        active: GenerationEpochIdV1,
    },
    /// A reported neighbor was not cardinally adjacent to the target cell.
    #[error("planning cell {neighbor:?} is not cardinally adjacent to {cell:?}")]
    NonAdjacentEpochCell {
        /// Target cell.
        cell: PlanningCellCoordinateV1,
        /// Invalid neighbor.
        neighbor: PlanningCellCoordinateV1,
    },
    /// A complete adjacent-state snapshot repeated one cardinal cell.
    #[error("complete adjacent epoch snapshot repeats planning cell {cell:?}")]
    DuplicateAdjacentEpochCell {
        /// Repeated neighbor cell.
        cell: PlanningCellCoordinateV1,
    },
    /// A four-entry snapshot did not contain every cardinal neighbor exactly once.
    #[error("adjacent epoch snapshot for {cell:?} is not a complete cardinal view")]
    IncompleteAdjacentEpochSnapshot {
        /// Target planning cell.
        cell: PlanningCellCoordinateV1,
    },
    /// The adjacent-state snapshot described a different target cell.
    #[error("adjacent epoch snapshot targets {actual:?}, but generation requires {expected:?}")]
    AdjacentEpochSnapshotCellMismatch {
        /// Planning cell derived from the chunk coordinate.
        expected: PlanningCellCoordinateV1,
        /// Planning cell declared by the snapshot.
        actual: PlanningCellCoordinateV1,
    },
    /// No adapter connected an old and new planning-cell epoch.
    #[error("no boundary adapter connects epochs {epoch_a} and {epoch_b}")]
    MissingBoundaryAdapter {
        /// First canonical epoch.
        epoch_a: GenerationEpochIdV1,
        /// Second canonical epoch.
        epoch_b: GenerationEpochIdV1,
    },
    /// More than one adapter claimed an epoch pair.
    #[error("multiple boundary adapters connect epochs {epoch_a} and {epoch_b}")]
    ConflictingBoundaryAdapters {
        /// First canonical epoch.
        epoch_a: GenerationEpochIdV1,
        /// Second canonical epoch.
        epoch_b: GenerationEpochIdV1,
    },
    /// A matching adapter declaration lacked a verifier-minted application receipt.
    #[error(
        "boundary adapter declaration `{adapter}` for epochs {epoch_a} and {epoch_b} is not verified"
    )]
    BoundaryAdapterNotVerified {
        /// First canonical epoch.
        epoch_a: GenerationEpochIdV1,
        /// Second canonical epoch.
        epoch_b: GenerationEpochIdV1,
        /// Unverified declaration identity.
        adapter: Box<StableId>,
    },
    /// A boundary adapter declaration was malformed or unbounded.
    #[error("invalid boundary adapter declaration `{adapter}`: {reason}")]
    InvalidBoundaryAdapterDeclaration {
        /// Adapter stable ID.
        adapter: StableId,
        /// Validation reason.
        reason: String,
    },
}
