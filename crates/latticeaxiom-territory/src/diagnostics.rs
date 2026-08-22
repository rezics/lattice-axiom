//! Stable, load-order-independent ownership-conflict diagnostics.

use latticeaxiom_core::canonical_json_hash;
use serde::{Deserialize, Serialize};

use crate::{TerritoryConflictDiagnosticHashV1, TerritoryError, TerritoryResult};

/// Machine-readable ownership conflict that failed plan compilation.
///
/// Diagnostics are derived from [`TerritoryError`] and never mutate a compiled
/// plan. Provider lists are sorted, so registration order cannot change bytes.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum TerritoryConflictDiagnosticV1 {
    /// The dimension registered no generation coordinator.
    MissingCoordinator,
    /// Two or more generation coordinators were registered.
    ConflictingCoordinators {
        /// Sorted provider identities.
        providers: Vec<String>,
    },
    /// Inheritance produced no exclusive owner for a channel domain.
    MissingPrimaryOwner {
        /// Exclusive channel name.
        channel: String,
        /// Ownership domain identity.
        domain: String,
    },
    /// Two or more exclusive owners claimed the same channel domain.
    ConflictingPrimaryOwners {
        /// Exclusive channel name.
        channel: String,
        /// Ownership domain identity.
        domain: String,
        /// Sorted provider identities.
        providers: Vec<String>,
    },
    /// One provider revision declared inconsistent implementation hashes.
    ProviderFingerprintMismatch {
        /// Provider stable identity.
        provider: String,
    },
    /// Same-level terrain ownership or parent graph was invalid.
    InvalidTerritoryDomain {
        /// Territory identity.
        domain: String,
        /// Validation diagnostic.
        reason: String,
    },
    /// Same-level underground ownership or nesting was invalid.
    InvalidUndergroundTerritory {
        /// Territory identity.
        territory: String,
        /// Validation diagnostic.
        reason: String,
    },
}

impl TerritoryConflictDiagnosticV1 {
    /// Returns the canonical diagnostic hash.
    ///
    /// # Errors
    ///
    /// Returns an error if canonical JSON encoding fails.
    pub fn canonical_hash(&self) -> TerritoryResult<TerritoryConflictDiagnosticHashV1> {
        canonical_json_hash(self)
            .map(TerritoryConflictDiagnosticHashV1::from_hash)
            .map_err(|error| TerritoryError::CanonicalEncoding {
                kind: "territory conflict diagnostic",
                reason: error.to_string(),
            })
    }

    /// Returns whether this diagnostic is an exclusive-owner cardinality failure.
    #[must_use]
    pub const fn is_exclusive_owner_conflict(&self) -> bool {
        matches!(
            self,
            Self::MissingCoordinator
                | Self::ConflictingCoordinators { .. }
                | Self::MissingPrimaryOwner { .. }
                | Self::ConflictingPrimaryOwners { .. }
                | Self::ProviderFingerprintMismatch { .. }
        )
    }
}

impl TerritoryError {
    /// Returns a stable conflict diagnostic when compilation failed closed on
    /// ownership, same-level collision, or fingerprint mismatch.
    #[must_use]
    pub fn conflict_diagnostic(&self) -> Option<TerritoryConflictDiagnosticV1> {
        match self {
            Self::MissingCoordinator => Some(TerritoryConflictDiagnosticV1::MissingCoordinator),
            Self::ConflictingCoordinators { providers } => {
                Some(TerritoryConflictDiagnosticV1::ConflictingCoordinators {
                    providers: providers.clone(),
                })
            }
            Self::MissingPrimaryOwner { channel, domain } => {
                Some(TerritoryConflictDiagnosticV1::MissingPrimaryOwner {
                    channel: (*channel).to_owned(),
                    domain: domain.clone(),
                })
            }
            Self::ConflictingPrimaryOwners {
                channel,
                domain,
                providers,
            } => Some(TerritoryConflictDiagnosticV1::ConflictingPrimaryOwners {
                channel: (*channel).to_owned(),
                domain: domain.clone(),
                providers: providers.clone(),
            }),
            Self::ProviderFingerprintMismatch { provider } => {
                Some(TerritoryConflictDiagnosticV1::ProviderFingerprintMismatch {
                    provider: provider.clone(),
                })
            }
            Self::InvalidTerritoryDomain { domain, reason } => {
                Some(TerritoryConflictDiagnosticV1::InvalidTerritoryDomain {
                    domain: domain.clone(),
                    reason: reason.clone(),
                })
            }
            Self::InvalidUndergroundTerritory { territory, reason } => {
                Some(TerritoryConflictDiagnosticV1::InvalidUndergroundTerritory {
                    territory: territory.clone(),
                    reason: reason.clone(),
                })
            }
            _ => None,
        }
    }
}
