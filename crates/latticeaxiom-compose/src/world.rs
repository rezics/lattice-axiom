//! World catalog, durability, and write-before-open planning models.

use std::collections::BTreeSet;

use latticeaxiom_core::{
    CanonicalHash, CanonicalJsonError, PackageName, SchemaId, StableId, WorldId,
    canonical_json_hash,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;

/// Current bounded world-header schema version.
pub const WORLD_HEADER_SCHEMA_VERSION: u32 = 1;

/// Highest persistence guarantee reached by a world revision.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum DurabilityLevel {
    /// Commit exists only in an in-memory queue.
    Queued,
    /// Storage accepted the write without a synchronous media guarantee.
    Written,
    /// The configured synchronous durability policy completed.
    Durable,
    /// An independently restorable checkpoint contains the revision.
    Checkpointed,
}

/// Health projection shown by the world catalog.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum WorldHealth {
    /// Header and bounded metadata are internally valid.
    Healthy,
    /// Last shutdown was not confirmed clean.
    UncleanShutdown,
    /// Header bytes or checksum are invalid.
    DamagedHeader,
    /// Storage metadata disagrees with the catalog projection.
    MetadataMismatch,
    /// Required content is unavailable locally.
    MissingContent,
}

/// Bounded catalog projection for one world.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct WorldHeader {
    /// Header schema version.
    pub schema_version: u32,
    /// Immutable world identity.
    pub world_id: WorldId,
    /// User-facing name that does not participate in storage identity.
    pub display_name: String,
    /// Concrete dimension registration ID.
    pub dimension: StableId,
    /// Milliseconds since the Unix epoch at creation.
    pub created_at_ms: u64,
    /// Milliseconds since the Unix epoch at last play.
    pub last_played_at_ms: u64,
    /// Frozen game lock hash.
    pub game_lock_hash: CanonicalHash,
    /// Authoritative registration image hash.
    pub registration_hash: CanonicalHash,
    /// Authoritative setting fingerprint.
    pub settings_hash: CanonicalHash,
    /// Required package owners.
    pub required_packages: BTreeSet<PackageName>,
    /// Required persistent schema owners.
    pub required_schemas: BTreeSet<SchemaId>,
    /// Whether normal shutdown reached the clean marker.
    pub clean_shutdown: bool,
    /// Latest world revision confirmed durable.
    pub durable_revision: u64,
    /// Highest durability reached by that revision.
    pub durability: DurabilityLevel,
    /// Bounded health projection.
    pub health: WorldHealth,
    /// Checksum of this header excluding the checksum field.
    pub checksum: CanonicalHash,
}

impl WorldHeader {
    /// Recomputes the bounded header checksum without the claimed checksum field.
    ///
    /// # Errors
    ///
    /// Returns [`CanonicalJsonError`] if the header cannot be encoded.
    pub fn recompute_checksum(&self) -> Result<CanonicalHash, CanonicalJsonError> {
        let mut value = serde_json::to_value(self)?;
        if let Value::Object(fields) = &mut value {
            fields.remove("checksum");
        }
        canonical_json_hash(&value)
    }

    /// Verifies the bounded header checksum.
    ///
    /// # Errors
    ///
    /// Returns [`WorldHeaderChecksumError`] if encoding fails or the checksum
    /// does not match the normalized header.
    pub fn verify_checksum(&self) -> Result<(), WorldHeaderChecksumError> {
        let actual = self.recompute_checksum()?;
        if actual == self.checksum {
            Ok(())
        } else {
            Err(WorldHeaderChecksumError::Mismatch {
                expected: self.checksum,
                actual,
            })
        }
    }
}

/// World-header checksum verification failure.
#[derive(Debug, Error)]
pub enum WorldHeaderChecksumError {
    /// Canonical JSON encoding failed.
    #[error(transparent)]
    Canonical(#[from] CanonicalJsonError),
    /// The claimed checksum differs from the normalized header.
    #[error("world header checksum mismatch: expected {expected}, recomputed {actual}")]
    Mismatch {
        /// Claimed checksum.
        expected: CanonicalHash,
        /// Recomputed checksum.
        actual: CanonicalHash,
    },
}

/// Preflight result before a writer or package business code is opened.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum WorldOpenStatus {
    /// Exact frozen lock and all authoritative receipts match.
    ReadyExact,
    /// Compatible artifacts exist but an explicit diff remains.
    ReadyCompatible,
    /// Required artifacts must be acquired or built.
    NeedsDownloadOrBuild,
    /// Schema or content migration is required.
    NeedsMigration,
    /// World may be opened without authoritative mutation.
    RecoverableReadOnly,
    /// Writable and read-only opening are both unsafe.
    Blocked,
}

/// Risk level attached to an open plan.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum WorldOpenRisk {
    /// No mutation or compatibility decision is required.
    None,
    /// Reversible acquisition or compatible artifact selection is required.
    Low,
    /// Checkpoint, clone, or migration is required.
    Elevated,
    /// Opening could lose authoritative data and is therefore blocked.
    Critical,
}

/// Explicit recovery or continuation action offered by preflight.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case", tag = "action")]
pub enum WorldOpenAction {
    /// Activate the exact frozen closure.
    UseFrozenLock,
    /// Build or acquire a package artifact.
    PreparePackage {
        /// Package to prepare.
        package: PackageName,
    },
    /// Open without authoritative mutation.
    OpenReadOnly,
    /// Restore a named checkpoint.
    RestoreCheckpoint {
        /// Stable checkpoint ID.
        checkpoint: StableId,
    },
    /// Clone and stage a migration.
    CloneAndMigrate,
}

/// Immutable result of metadata-only world preflight.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct WorldOpenPlan {
    /// World being opened.
    pub world_id: WorldId,
    /// Normative readiness state.
    pub status: WorldOpenStatus,
    /// Highest identified risk.
    pub risk: WorldOpenRisk,
    /// Human-readable diagnostic codes with structured details elsewhere.
    pub diagnostics: Vec<String>,
    /// Explicit available actions.
    pub actions: Vec<WorldOpenAction>,
    /// Whether any writer may be opened after accepting this plan.
    pub writable: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn world_header_checksum_detects_mutation() {
        let mut header = WorldHeader {
            schema_version: WORLD_HEADER_SCHEMA_VERSION,
            world_id: WorldId::new_v4(),
            display_name: "Conformance World".to_owned(),
            dimension: stable_id("terrenia:dimension/terrenia"),
            created_at_ms: 1,
            last_played_at_ms: 2,
            game_lock_hash: CanonicalHash::digest(b"lock"),
            registration_hash: CanonicalHash::digest(b"registration"),
            settings_hash: CanonicalHash::digest(b"settings"),
            required_packages: BTreeSet::from([package_name("terrenia")]),
            required_schemas: BTreeSet::new(),
            clean_shutdown: true,
            durable_revision: 7,
            durability: DurabilityLevel::Durable,
            health: WorldHealth::Healthy,
            checksum: CanonicalHash::digest(b"pending"),
        };
        header.checksum = header
            .recompute_checksum()
            .unwrap_or_else(|error| panic!("header checksum failed: {error}"));
        assert!(header.verify_checksum().is_ok());

        header.display_name = "Changed".to_owned();
        assert!(header.verify_checksum().is_err());
    }

    #[test]
    fn runtime_setting_impact_rejects_graph_recomposition() {
        assert!(serde_json::from_str::<crate::RuntimeApplyImpact>(r#""graph-recompose""#).is_err());
        assert_eq!(
            serde_json::from_str::<crate::CompositionApplyImpact>(r#""graph-recompose""#).ok(),
            Some(crate::CompositionApplyImpact::GraphRecompose)
        );
    }

    fn stable_id(value: &str) -> StableId {
        value
            .parse()
            .unwrap_or_else(|error| panic!("fixture stable ID `{value}` is invalid: {error}"))
    }

    fn package_name(value: &str) -> PackageName {
        value
            .parse()
            .unwrap_or_else(|error| panic!("fixture package `{value}` is invalid: {error}"))
    }
}
