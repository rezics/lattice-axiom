//! Territory compilation and validation errors.

use latticeaxiom_core::IdentifierError;
use latticeaxiom_worldgen::{GenerationEpochIdV1, PlanningCellCoordinateV1};
use thiserror::Error;

/// Result type returned by territory planning operations.
pub type TerritoryResult<T> = Result<T, TerritoryError>;

/// A deterministic territory plan failed validation or compilation.
#[derive(Debug, Error)]
pub enum TerritoryError {
    /// Canonical JSON encoding failed.
    #[error("failed to encode {kind} canonically: {reason}")]
    CanonicalEncoding {
        /// Kind of payload being encoded.
        kind: &'static str,
        /// Underlying encoder diagnostic.
        reason: String,
    },
    /// A stable identifier was malformed.
    #[error(transparent)]
    Identifier(#[from] IdentifierError),
    /// A stable identifier used the wrong registration kind.
    #[error("stable ID `{value}` must use registration kind `{expected}`")]
    InvalidStableKind {
        /// Rejected identifier.
        value: String,
        /// Required registration kind.
        expected: &'static str,
    },
    /// A bounded rectangle or height range was empty or inverted.
    #[error("invalid {kind}: minimum {minimum} must be less than maximum {maximum}")]
    InvalidBounds {
        /// Bound kind.
        kind: &'static str,
        /// Inclusive lower bound.
        minimum: i64,
        /// Exclusive upper bound.
        maximum: i64,
    },
    /// A collection exceeded a hard compilation limit.
    #[error("{kind} count {actual} exceeds hard limit {limit}")]
    CollectionLimitExceeded {
        /// Collection kind.
        kind: &'static str,
        /// Observed item count.
        actual: usize,
        /// Maximum accepted count.
        limit: usize,
    },
    /// Work or memory exceeded a contribution budget.
    #[error("{kind} budget {actual} exceeds hard limit {limit}")]
    BudgetExceeded {
        /// Budget kind.
        kind: &'static str,
        /// Requested amount.
        actual: u64,
        /// Maximum accepted amount.
        limit: u64,
    },
    /// Atlas scales did not form a valid coarse-to-fine hierarchy.
    #[error("invalid atlas scale hierarchy: {reason}")]
    InvalidAtlasScale {
        /// Validation diagnostic.
        reason: String,
    },
    /// An atlas territory had an invalid parent or duplicate identity.
    #[error("invalid territory domain `{domain}`: {reason}")]
    InvalidTerritoryDomain {
        /// Territory identity.
        domain: String,
        /// Validation diagnostic.
        reason: String,
    },
    /// No dimension coordinator was registered.
    #[error("dimension requires exactly one generation coordinator")]
    MissingCoordinator,
    /// Multiple dimension coordinators were registered.
    #[error("dimension has conflicting generation coordinators: {providers:?}")]
    ConflictingCoordinators {
        /// Sorted provider identities.
        providers: Vec<String>,
    },
    /// An ownership domain had no primary owner after inheritance.
    #[error("missing primary owner for {channel} domain `{domain}`")]
    MissingPrimaryOwner {
        /// Exclusive channel.
        channel: &'static str,
        /// Ownership domain.
        domain: String,
    },
    /// An ownership domain had multiple primary owners.
    #[error("conflicting primary owners for {channel} domain `{domain}`: {providers:?}")]
    ConflictingPrimaryOwners {
        /// Exclusive channel.
        channel: &'static str,
        /// Ownership domain.
        domain: String,
        /// Sorted provider identities.
        providers: Vec<String>,
    },
    /// The same provider revision declared inconsistent implementation hashes.
    #[error("provider identity `{provider}` declared inconsistent fingerprints")]
    ProviderFingerprintMismatch {
        /// Provider stable ID.
        provider: String,
    },
    /// A layered spatial contribution violated its compositor contract.
    #[error("invalid spatial contribution `{contribution}`: {reason}")]
    InvalidContribution {
        /// Contribution identity.
        contribution: String,
        /// Validation diagnostic.
        reason: String,
    },
    /// An underground territory violated Atlas nesting rules.
    #[error("invalid underground territory `{territory}`: {reason}")]
    InvalidUndergroundTerritory {
        /// Territory identity.
        territory: String,
        /// Validation diagnostic.
        reason: String,
    },
    /// Cave adjacency or portal evidence was invalid.
    #[error("invalid cave adjacency: {reason}")]
    InvalidCaveAdjacency {
        /// Validation diagnostic.
        reason: String,
    },
    /// A declared required cave portal was absent.
    #[error("required cave portal {portal} is missing")]
    MissingRequiredPortal {
        /// Canonical portal identity.
        portal: String,
    },
    /// An abstract hydrology plan was invalid.
    #[error("invalid abstract hydrology plan: {reason}")]
    InvalidHydrologyPlan {
        /// Validation diagnostic.
        reason: String,
    },
    /// Serialized planning-cell epoch ledger evidence was not canonical.
    #[error("invalid planning-cell epoch ledger: {reason}")]
    InvalidEpochLedger {
        /// Validation diagnostic.
        reason: String,
    },
    /// A cell was already frozen to another generation epoch.
    #[error("planning cell {cell:?} is frozen to {frozen}, not requested epoch {requested}")]
    EpochAlreadyFrozen {
        /// Planning cell.
        cell: PlanningCellCoordinateV1,
        /// Existing frozen epoch.
        frozen: GenerationEpochIdV1,
        /// Rejected requested epoch.
        requested: GenerationEpochIdV1,
    },
    /// Transition construction observed a cell without a durable epoch.
    #[error("planning cell {cell:?} has no frozen generation epoch")]
    CellEpochUnassigned {
        /// Planning cell.
        cell: PlanningCellCoordinateV1,
    },
    /// Transition cells were not cardinally adjacent.
    #[error("planning cells {first:?} and {second:?} are not cardinal neighbors")]
    NonAdjacentPlanningCells {
        /// First cell.
        first: PlanningCellCoordinateV1,
        /// Second cell.
        second: PlanningCellCoordinateV1,
    },
    /// A transition adapter did not match the adjacent frozen epochs.
    #[error("transition adapter epochs do not match frozen cell epochs")]
    TransitionEpochMismatch,
    /// Adapter portal requirements did not match the validated cave boundary.
    #[error("transition portal requirements do not match the cave adjacency")]
    TransitionPortalMismatch,
    /// A statistics query would exceed its bounded sample limit.
    #[error("statistics region contains {actual} cells, exceeding limit {limit}")]
    StatisticsRegionTooLarge {
        /// Requested cell count.
        actual: u64,
        /// Maximum accepted cell count.
        limit: u64,
    },
    /// Integer arithmetic overflowed while validating a bounded plan.
    #[error("integer overflow while computing {kind}")]
    ArithmeticOverflow {
        /// Failed computation.
        kind: &'static str,
    },
}

impl TerritoryError {
    pub(crate) const fn non_adjacent(
        first: PlanningCellCoordinateV1,
        second: PlanningCellCoordinateV1,
    ) -> Self {
        Self::NonAdjacentPlanningCells { first, second }
    }
}
