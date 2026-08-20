use latticeaxiom_core::{CanonicalJsonError, StableId};
use thiserror::Error;

/// Result type returned by content validation and compilation.
pub type ContentResult<T> = Result<T, ContentError>;

/// Stable content-contract validation failure.
#[derive(Debug, Error)]
pub enum ContentError {
    /// Canonical JSON encoding failed.
    #[error(transparent)]
    CanonicalEncoding(#[from] CanonicalJsonError),
    /// A contract schema major is unsupported.
    #[error("unsupported content catalog schema major {actual}; expected {expected}")]
    UnsupportedCatalogSchema {
        /// Supported major.
        expected: u32,
        /// Rejected major.
        actual: u32,
    },
    /// A definition used the wrong exact registration kind.
    #[error("{context} `{id}` must use registration kind `{expected}`, got `{actual}`")]
    WrongIdentityKind {
        /// Field or definition being validated.
        context: &'static str,
        /// Rejected identity.
        id: StableId,
        /// Required kind.
        expected: &'static str,
        /// Actual kind.
        actual: String,
    },
    /// An exact content identity incorrectly carried a contract major.
    #[error("{context} exact identity `{id}` must not carry a major suffix")]
    ExactIdentityHasMajor {
        /// Field or definition being validated.
        context: &'static str,
        /// Rejected identity.
        id: StableId,
    },
    /// A versioned contract identity omitted its major suffix.
    #[error("{context} contract identity `{id}` requires a major suffix")]
    ContractIdentityMissingMajor {
        /// Field or definition being validated.
        context: &'static str,
        /// Rejected identity.
        id: StableId,
    },
    /// A definition declared an unexpected schema identity.
    #[error("{kind} definition `{id}` uses schema `{actual}`, expected `{expected}`")]
    WrongDefinitionSchema {
        /// Definition kind.
        kind: &'static str,
        /// Definition identity.
        id: StableId,
        /// Required schema text.
        expected: &'static str,
        /// Actual schema text.
        actual: String,
    },
    /// A bounded collection exceeded its configured limit.
    #[error("{resource} count {actual} exceeds limit {limit}")]
    LimitExceeded {
        /// Bounded resource name.
        resource: &'static str,
        /// Observed count or value.
        actual: usize,
        /// Inclusive limit.
        limit: usize,
    },
    /// Two rows used the same stable identity or canonical key.
    #[error("duplicate {resource} `{id}`")]
    Duplicate {
        /// Duplicated resource class.
        resource: &'static str,
        /// Stable diagnostic identity or key.
        id: String,
    },
    /// A definition-scoped finite state token violated its bounded grammar.
    #[error("invalid {context} token `{value}`: {reason}")]
    InvalidDefinitionToken {
        /// Token family being validated.
        context: &'static str,
        /// Rejected token.
        value: String,
        /// Stable reason.
        reason: &'static str,
    },
    /// A block-state property declaration is malformed.
    #[error("block `{block}` state property `{key}` is invalid: {reason}")]
    InvalidStateProperty {
        /// Owning block.
        block: StableId,
        /// State key.
        key: StableId,
        /// Stable reason.
        reason: &'static str,
    },
    /// A block-state row is malformed or not declared by its schema.
    #[error("block `{block}` has invalid state: {reason}")]
    InvalidBlockState {
        /// Owning block.
        block: StableId,
        /// Stable reason.
        reason: &'static str,
    },
    /// Intrinsic shape and occupancy semantics contradict each other.
    #[error("block `{block}` has inconsistent intrinsic semantics: {reason}")]
    InvalidBlockSemantics {
        /// Owning block.
        block: StableId,
        /// Stable reason.
        reason: &'static str,
    },
    /// A definition references a missing catalog row.
    #[error("{owner} references unknown {resource} `{target}`")]
    UnknownReference {
        /// Stable owner description.
        owner: String,
        /// Referenced resource class.
        resource: &'static str,
        /// Missing identity.
        target: StableId,
    },
    /// A biome declaration violates a closed v1 rule.
    #[error("biome `{biome}` is invalid: {reason}")]
    InvalidBiome {
        /// Biome identity.
        biome: StableId,
        /// Stable reason.
        reason: &'static str,
    },
    /// Biome fallback edges contain a cycle.
    #[error("biome fallback graph contains a cycle through {cycle:?}")]
    BiomeFallbackCycle {
        /// Stable-sorted identities observed in the cycle.
        cycle: Vec<StableId>,
    },
    /// A material purpose or Role binding is malformed.
    #[error("material binding `{purpose}` is invalid: {reason}")]
    InvalidMaterialBinding {
        /// Domain-private material purpose.
        purpose: StableId,
        /// Stable reason.
        reason: &'static str,
    },
    /// A solid palette references an unknown or invalid block state.
    #[error("solid palette entry for `{block}` is invalid: {reason}")]
    InvalidSolidPaletteEntry {
        /// Referenced block.
        block: StableId,
        /// Stable reason.
        reason: &'static str,
    },
    /// A fluid palette references an unknown fluid definition.
    #[error("fluid palette references unknown fluid `{fluid}`")]
    UnknownFluidPaletteEntry {
        /// Missing fluid.
        fluid: StableId,
    },
}

/// Error returned when a fluid level is outside the frozen `0..=7` range.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
#[error("fluid level {actual} is outside 0..=7")]
pub struct FluidLevelError {
    /// Rejected level.
    pub actual: u8,
}
