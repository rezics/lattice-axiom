//! World catalog, durability, write-before-open planning, and playable session models.

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

/// Structural host clamps for one playable world session.
///
/// These values bound streaming residency and queues in chunk units. They are
/// not frame-time, latency, or FPS budgets, and they do not replace an accepted
/// performance ADR.
#[allow(
    clippy::struct_field_names,
    reason = "each clamp is independently named in chunk units"
)]
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields, try_from = "PlayableWorldHardLimitsWireV1")]
pub struct PlayableWorldHardLimitsV1 {
    /// Inclusive chunk radius the client may keep in view.
    pub view_distance_chunks: u32,
    /// Inclusive chunk radius the host may generate around the player.
    pub generation_radius_chunks: u32,
    /// Maximum chunks eligible for authoritative simulation at once.
    pub max_active_chunks: u32,
    /// Maximum chunks retained in the resident working set.
    pub max_resident_chunks: u32,
    /// Maximum chunks that may be generating or meshing at once.
    pub max_in_flight_chunks: u32,
    /// Inclusive chunk radius that must remain eligible for durable save.
    pub max_save_radius_chunks: u32,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
#[allow(
    clippy::struct_field_names,
    reason = "wire field names match the public host-clamp contract"
)]
struct PlayableWorldHardLimitsWireV1 {
    view_distance_chunks: u32,
    generation_radius_chunks: u32,
    max_active_chunks: u32,
    max_resident_chunks: u32,
    max_in_flight_chunks: u32,
    max_save_radius_chunks: u32,
}

impl PlayableWorldHardLimitsV1 {
    /// Creates nonzero structural host clamps for a playable world session.
    ///
    /// # Errors
    ///
    /// Returns [`PlayableWorldSessionError::ZeroLimit`] when any clamp is zero.
    pub fn new(
        view_distance_chunks: u32,
        generation_radius_chunks: u32,
        max_active_chunks: u32,
        max_resident_chunks: u32,
        max_in_flight_chunks: u32,
        max_save_radius_chunks: u32,
    ) -> Result<Self, PlayableWorldSessionError> {
        let limits = Self {
            view_distance_chunks,
            generation_radius_chunks,
            max_active_chunks,
            max_resident_chunks,
            max_in_flight_chunks,
            max_save_radius_chunks,
        };
        limits.validate()?;
        Ok(limits)
    }

    /// Rejects a clamp of zero.
    ///
    /// # Errors
    ///
    /// Returns [`PlayableWorldSessionError::ZeroLimit`] when any clamp is zero.
    pub const fn validate(self) -> Result<(), PlayableWorldSessionError> {
        if self.view_distance_chunks == 0 {
            return Err(PlayableWorldSessionError::ZeroLimit {
                field: "view_distance_chunks",
            });
        }
        if self.generation_radius_chunks == 0 {
            return Err(PlayableWorldSessionError::ZeroLimit {
                field: "generation_radius_chunks",
            });
        }
        if self.max_active_chunks == 0 {
            return Err(PlayableWorldSessionError::ZeroLimit {
                field: "max_active_chunks",
            });
        }
        if self.max_resident_chunks == 0 {
            return Err(PlayableWorldSessionError::ZeroLimit {
                field: "max_resident_chunks",
            });
        }
        if self.max_in_flight_chunks == 0 {
            return Err(PlayableWorldSessionError::ZeroLimit {
                field: "max_in_flight_chunks",
            });
        }
        if self.max_save_radius_chunks == 0 {
            return Err(PlayableWorldSessionError::ZeroLimit {
                field: "max_save_radius_chunks",
            });
        }
        Ok(())
    }
}

impl TryFrom<PlayableWorldHardLimitsWireV1> for PlayableWorldHardLimitsV1 {
    type Error = PlayableWorldSessionError;

    fn try_from(value: PlayableWorldHardLimitsWireV1) -> Result<Self, Self::Error> {
        Self::new(
            value.view_distance_chunks,
            value.generation_radius_chunks,
            value.max_active_chunks,
            value.max_resident_chunks,
            value.max_in_flight_chunks,
            value.max_save_radius_chunks,
        )
    }
}

/// Version-one playable world session contract.
///
/// This is distinct from catalog [`WorldHeader`]: it declares the finite
/// vertical range, chunk edge, seed, generation epoch, required package
/// closure, semantic image, and structural host clamps needed to open a
/// playable session. Seed and generation-epoch hashes are opaque
/// [`CanonicalHash`] values here; they bind to `WorldSeedV1` and
/// `GenerationEpochIdV1` when those identities are persisted on this contract.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields, try_from = "PlayableWorldSessionWireV1")]
pub struct PlayableWorldSessionV1 {
    /// Inclusive lowest finite world voxel Y.
    pub vertical_min_y: i32,
    /// Inclusive highest finite world voxel Y. Must be strictly greater than
    /// [`Self::vertical_min_y`].
    pub vertical_max_y: i32,
    /// Edge length of one cubic chunk in voxels.
    pub chunk_edge_voxels: u16,
    /// Canonical hash of the world seed. Bound to `WorldSeedV1` later.
    pub seed_hash: CanonicalHash,
    /// Canonical hash of the generation epoch. Bound to `GenerationEpochIdV1`
    /// later.
    pub generation_epoch_hash: CanonicalHash,
    /// Exact package names required to open this session.
    pub required_package_closure: BTreeSet<PackageName>,
    /// Canonical hash of the compiled semantic image for this session.
    pub semantic_image_hash: CanonicalHash,
    /// Structural host clamps for this session.
    pub hard_limits: PlayableWorldHardLimitsV1,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PlayableWorldSessionWireV1 {
    vertical_min_y: i32,
    vertical_max_y: i32,
    chunk_edge_voxels: u16,
    seed_hash: CanonicalHash,
    generation_epoch_hash: CanonicalHash,
    required_package_closure: BTreeSet<PackageName>,
    semantic_image_hash: CanonicalHash,
    hard_limits: PlayableWorldHardLimitsV1,
}

impl PlayableWorldSessionV1 {
    /// Creates a validated playable world session contract.
    ///
    /// # Errors
    ///
    /// Returns [`PlayableWorldSessionError`] when the vertical range is inverted
    /// or equal, the chunk edge is zero, the package closure is empty, or any
    /// hard limit is zero.
    #[allow(
        clippy::too_many_arguments,
        reason = "the v1 session contract is the complete validated field set"
    )]
    pub fn new(
        vertical_min_y: i32,
        vertical_max_y: i32,
        chunk_edge_voxels: u16,
        seed_hash: CanonicalHash,
        generation_epoch_hash: CanonicalHash,
        required_package_closure: BTreeSet<PackageName>,
        semantic_image_hash: CanonicalHash,
        hard_limits: PlayableWorldHardLimitsV1,
    ) -> Result<Self, PlayableWorldSessionError> {
        let session = Self {
            vertical_min_y,
            vertical_max_y,
            chunk_edge_voxels,
            seed_hash,
            generation_epoch_hash,
            required_package_closure,
            semantic_image_hash,
            hard_limits,
        };
        session.validate()?;
        Ok(session)
    }

    /// Rejects an inverted vertical range, a zero chunk edge, an empty package
    /// closure, or a zero hard limit.
    ///
    /// # Errors
    ///
    /// Returns [`PlayableWorldSessionError`] for any of those contract
    /// violations.
    pub fn validate(&self) -> Result<(), PlayableWorldSessionError> {
        if self.vertical_min_y >= self.vertical_max_y {
            return Err(PlayableWorldSessionError::InvertedVerticalRange {
                min: self.vertical_min_y,
                max: self.vertical_max_y,
            });
        }
        if self.chunk_edge_voxels == 0 {
            return Err(PlayableWorldSessionError::ZeroChunkEdge);
        }
        if self.required_package_closure.is_empty() {
            return Err(PlayableWorldSessionError::EmptyPackageClosure);
        }
        self.hard_limits.validate()
    }

    /// Canonical hash of this session contract, including structural host clamps.
    ///
    /// # Errors
    ///
    /// Returns [`CanonicalJsonError`] if the session cannot be encoded.
    pub fn canonical_hash(&self) -> Result<CanonicalHash, CanonicalJsonError> {
        canonical_json_hash(self)
    }
}

impl TryFrom<PlayableWorldSessionWireV1> for PlayableWorldSessionV1 {
    type Error = PlayableWorldSessionError;

    fn try_from(value: PlayableWorldSessionWireV1) -> Result<Self, Self::Error> {
        Self::new(
            value.vertical_min_y,
            value.vertical_max_y,
            value.chunk_edge_voxels,
            value.seed_hash,
            value.generation_epoch_hash,
            value.required_package_closure,
            value.semantic_image_hash,
            value.hard_limits,
        )
    }
}

/// Playable world session contract validation failure.
#[derive(Clone, Debug, Eq, Error, PartialEq)]
pub enum PlayableWorldSessionError {
    /// `vertical_min_y` is not strictly less than `vertical_max_y`.
    #[error("playable world session vertical range is inverted: {min} is not less than {max}")]
    InvertedVerticalRange {
        /// Inclusive minimum Y that failed validation.
        min: i32,
        /// Inclusive maximum Y that failed validation.
        max: i32,
    },
    /// `chunk_edge_voxels` was zero.
    #[error("playable world session chunk edge must be nonzero")]
    ZeroChunkEdge,
    /// `required_package_closure` contained no package names.
    #[error("playable world session required package closure must not be empty")]
    EmptyPackageClosure,
    /// A structural host clamp was zero.
    #[error("playable world session hard limit `{field}` must be nonzero")]
    ZeroLimit {
        /// Invalid limit field.
        field: &'static str,
    },
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

    #[test]
    fn playable_world_session_accepts_finite_range_and_round_trips() {
        let session = fixture_session();
        assert_eq!(session.vertical_min_y, -64);
        assert_eq!(session.vertical_max_y, 127);
        assert_eq!(session.chunk_edge_voxels, 16);
        assert_eq!(
            session.required_package_closure,
            BTreeSet::from([package_name("terrenia")])
        );
        session
            .validate()
            .unwrap_or_else(|error| panic!("valid session was rejected: {error}"));

        let encoded = serde_json::to_string(&session)
            .unwrap_or_else(|error| panic!("session JSON encode failed: {error}"));
        let decoded = serde_json::from_str::<PlayableWorldSessionV1>(&encoded)
            .unwrap_or_else(|error| panic!("session JSON decode failed: {error}"));
        assert_eq!(session, decoded);
        assert!(
            serde_json::from_str::<PlayableWorldSessionV1>(&encoded.replace(
                "\"semantic_image_hash\"",
                "\"mystery\":0,\"semantic_image_hash\""
            ))
            .is_err()
        );
    }

    #[test]
    fn playable_world_session_rejects_inverted_vertical_range() {
        assert_eq!(
            PlayableWorldSessionV1::new(
                8,
                -8,
                16,
                CanonicalHash::digest(b"seed"),
                CanonicalHash::digest(b"epoch"),
                BTreeSet::from([package_name("terrenia")]),
                CanonicalHash::digest(b"semantic"),
                fixture_limits(),
            ),
            Err(PlayableWorldSessionError::InvertedVerticalRange { min: 8, max: -8 })
        );

        assert_eq!(
            PlayableWorldSessionV1::new(
                4,
                4,
                16,
                CanonicalHash::digest(b"seed"),
                CanonicalHash::digest(b"epoch"),
                BTreeSet::from([package_name("terrenia")]),
                CanonicalHash::digest(b"semantic"),
                fixture_limits(),
            ),
            Err(PlayableWorldSessionError::InvertedVerticalRange { min: 4, max: 4 })
        );
    }

    #[test]
    fn playable_world_session_rejects_zero_chunk_edge_and_empty_closure() {
        assert_eq!(
            PlayableWorldSessionV1::new(
                -64,
                127,
                0,
                CanonicalHash::digest(b"seed"),
                CanonicalHash::digest(b"epoch"),
                BTreeSet::from([package_name("terrenia")]),
                CanonicalHash::digest(b"semantic"),
                fixture_limits(),
            ),
            Err(PlayableWorldSessionError::ZeroChunkEdge)
        );

        assert_eq!(
            PlayableWorldSessionV1::new(
                -64,
                127,
                16,
                CanonicalHash::digest(b"seed"),
                CanonicalHash::digest(b"epoch"),
                BTreeSet::new(),
                CanonicalHash::digest(b"semantic"),
                fixture_limits(),
            ),
            Err(PlayableWorldSessionError::EmptyPackageClosure)
        );
    }

    #[test]
    fn playable_world_session_rejects_zero_hard_limit() {
        assert_eq!(
            PlayableWorldHardLimitsV1::new(0, 12, 32, 64, 4, 16),
            Err(PlayableWorldSessionError::ZeroLimit {
                field: "view_distance_chunks",
            })
        );
        assert_eq!(
            PlayableWorldHardLimitsV1::new(8, 0, 32, 64, 4, 16),
            Err(PlayableWorldSessionError::ZeroLimit {
                field: "generation_radius_chunks",
            })
        );
        assert_eq!(
            PlayableWorldHardLimitsV1::new(8, 12, 0, 64, 4, 16),
            Err(PlayableWorldSessionError::ZeroLimit {
                field: "max_active_chunks",
            })
        );
        assert_eq!(
            PlayableWorldHardLimitsV1::new(8, 12, 32, 0, 4, 16),
            Err(PlayableWorldSessionError::ZeroLimit {
                field: "max_resident_chunks",
            })
        );
        assert_eq!(
            PlayableWorldHardLimitsV1::new(8, 12, 32, 64, 0, 16),
            Err(PlayableWorldSessionError::ZeroLimit {
                field: "max_in_flight_chunks",
            })
        );
        assert_eq!(
            PlayableWorldHardLimitsV1::new(8, 12, 32, 64, 4, 0),
            Err(PlayableWorldSessionError::ZeroLimit {
                field: "max_save_radius_chunks",
            })
        );

        let zeroed = PlayableWorldHardLimitsV1 {
            view_distance_chunks: 0,
            generation_radius_chunks: 12,
            max_active_chunks: 32,
            max_resident_chunks: 64,
            max_in_flight_chunks: 4,
            max_save_radius_chunks: 16,
        };
        let session_error = PlayableWorldSessionV1::new(
            -64,
            127,
            16,
            CanonicalHash::digest(b"seed"),
            CanonicalHash::digest(b"epoch"),
            BTreeSet::from([package_name("terrenia")]),
            CanonicalHash::digest(b"semantic"),
            zeroed,
        );
        assert_eq!(
            session_error,
            Err(PlayableWorldSessionError::ZeroLimit {
                field: "view_distance_chunks",
            })
        );
    }

    #[test]
    fn playable_world_session_canonical_hash_is_stable() {
        let session = fixture_session();
        let hash = session
            .canonical_hash()
            .unwrap_or_else(|error| panic!("session hash failed: {error}"));
        assert_eq!(
            hash,
            session
                .canonical_hash()
                .unwrap_or_else(|error| panic!("session hash failed: {error}"))
        );

        let encoded = serde_json::to_value(&session)
            .unwrap_or_else(|error| panic!("session JSON value encode failed: {error}"));
        let decoded = serde_json::from_value::<PlayableWorldSessionV1>(encoded)
            .unwrap_or_else(|error| panic!("session JSON value decode failed: {error}"));
        assert_eq!(
            hash,
            decoded
                .canonical_hash()
                .unwrap_or_else(|error| panic!("decoded session hash failed: {error}"))
        );

        let mut mutated = session.clone();
        mutated.seed_hash = CanonicalHash::digest(b"other-seed");
        let mutated_hash = mutated
            .canonical_hash()
            .unwrap_or_else(|error| panic!("mutated session hash failed: {error}"));
        assert_ne!(hash, mutated_hash);
    }

    fn fixture_session() -> PlayableWorldSessionV1 {
        PlayableWorldSessionV1::new(
            -64,
            127,
            16,
            CanonicalHash::digest(b"seed"),
            CanonicalHash::digest(b"epoch"),
            BTreeSet::from([package_name("terrenia")]),
            CanonicalHash::digest(b"semantic"),
            fixture_limits(),
        )
        .unwrap_or_else(|error| panic!("valid fixture session was rejected: {error}"))
    }

    fn fixture_limits() -> PlayableWorldHardLimitsV1 {
        PlayableWorldHardLimitsV1::new(8, 12, 32, 64, 4, 16)
            .unwrap_or_else(|error| panic!("valid fixture limits were rejected: {error}"))
    }
}
