//! Canonical child-exit reports consumed atomically with an optional intent.

use latticeaxiom_core::{CanonicalHash, WorldId, canonical_json_bytes, canonical_json_hash};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{
    ChildExitError, DurableWorldRevisionV1, LaunchGeneration, LaunchIntentV1, LaunchModelError,
    LaunchTargetV1, MAX_LAUNCH_INTENT_BYTES, ProcessEpoch, SettingTransactionRevision,
};

/// Stable schema version for [`ChildExitReportV1`].
pub const CHILD_EXIT_SCHEMA_VERSION: u32 = 1;

/// Supervised child role recorded by a child-exit report.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case", tag = "kind")]
pub enum ChildRoleV1 {
    /// Normal package-driven shell.
    Shell,
    /// Exact world process.
    World {
        /// Immutable world identity.
        world_id: WorldId,
    },
    /// Package-minimal recovery shell.
    Recovery,
}

impl ChildRoleV1 {
    /// Returns the world identity when this role is a world process.
    #[must_use]
    pub const fn world_id(self) -> Option<WorldId> {
        match self {
            Self::World { world_id } => Some(world_id),
            Self::Shell | Self::Recovery => None,
        }
    }
}

/// Why a supervised child exited.
///
/// Shell Quit, game Save & Quit, OS close, and crash remain distinct kinds.
/// Written-only and shutdown-timeout paths cannot masquerade as a durable
/// Save & Quit.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ChildExitKindV1 {
    /// Shell or recovery quit without requesting another process.
    ShellQuit,
    /// Shell or recovery published a matching World intent after preflight.
    Handoff,
    /// Game Save & Quit reached a durable barrier and requested the shell.
    SaveAndQuit,
    /// Operating-system window close.
    OsClose,
    /// Child crashed or panicked.
    Crash,
    /// Bounded shutdown wait elapsed.
    ShutdownTimeout,
    /// Writer returned Written without Durable.
    WrittenOnly,
}

impl ChildExitKindV1 {
    /// Reports whether this kind can complete a normal product handoff.
    #[must_use]
    pub const fn is_normal_handoff(self) -> bool {
        matches!(self, Self::Handoff | Self::SaveAndQuit)
    }

    /// Reports whether this kind ends the product without another spawn.
    #[must_use]
    pub const fn is_product_exit(self) -> bool {
        matches!(self, Self::ShellQuit | Self::OsClose)
    }
}

/// Unsealed values used to construct one validated child-exit report.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ChildExitReportDraftV1 {
    /// Generation that booted the exiting child.
    pub child_generation: LaunchGeneration,
    /// Process epoch bound to the child's single-client-App lease.
    pub process_epoch: ProcessEpoch,
    /// Role the child actually ran.
    pub role: ChildRoleV1,
    /// Typed exit kind.
    pub exit_kind: ChildExitKindV1,
    /// Next intent generation when the child published a handoff.
    pub intent_generation: Option<LaunchGeneration>,
    /// Next intent checksum when the child published a handoff.
    pub intent_checksum: Option<CanonicalHash>,
    /// Last settings transaction confirmed before exit.
    pub confirmed_setting_transaction_revision: SettingTransactionRevision,
    /// Latest world revision the writer reported as written.
    pub last_written_world: Option<DurableWorldRevisionV1>,
    /// Latest world revision proven durable.
    pub last_durable_world: Option<DurableWorldRevisionV1>,
    /// Exact shell lock used by the child.
    pub shell_lock_hash: CanonicalHash,
    /// Exact world lock for a world child; absent for shell/recovery.
    pub world_lock_hash: Option<CanonicalHash>,
    /// Read-only preflight plan hash for a world child; absent for shell/recovery.
    pub world_open_plan_hash: Option<CanonicalHash>,
    /// Bounded diagnostic reference. Never world bytes or secrets.
    pub diagnostic_ref: Option<CanonicalHash>,
}

/// Canonical, checksummed child-exit envelope.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ChildExitReportV1 {
    schema_version: u32,
    child_generation: LaunchGeneration,
    process_epoch: ProcessEpoch,
    role: ChildRoleV1,
    exit_kind: ChildExitKindV1,
    intent_generation: Option<LaunchGeneration>,
    intent_checksum: Option<CanonicalHash>,
    confirmed_setting_transaction_revision: SettingTransactionRevision,
    last_written_world: Option<DurableWorldRevisionV1>,
    last_durable_world: Option<DurableWorldRevisionV1>,
    shell_lock_hash: CanonicalHash,
    world_lock_hash: Option<CanonicalHash>,
    world_open_plan_hash: Option<CanonicalHash>,
    diagnostic_ref: Option<CanonicalHash>,
    checksum: CanonicalHash,
}

impl ChildExitReportV1 {
    /// Validates, checksums, and seals a V1 child-exit report.
    ///
    /// # Errors
    ///
    /// Returns [`LaunchModelError`] for an invalid role/kind shape or canonical
    /// encoding failure.
    pub fn seal(draft: ChildExitReportDraftV1) -> Result<Self, LaunchModelError> {
        validate_draft(&draft)?;
        let mut report = Self {
            schema_version: CHILD_EXIT_SCHEMA_VERSION,
            child_generation: draft.child_generation,
            process_epoch: draft.process_epoch,
            role: draft.role,
            exit_kind: draft.exit_kind,
            intent_generation: draft.intent_generation,
            intent_checksum: draft.intent_checksum,
            confirmed_setting_transaction_revision: draft.confirmed_setting_transaction_revision,
            last_written_world: draft.last_written_world,
            last_durable_world: draft.last_durable_world,
            shell_lock_hash: draft.shell_lock_hash,
            world_lock_hash: draft.world_lock_hash,
            world_open_plan_hash: draft.world_open_plan_hash,
            diagnostic_ref: draft.diagnostic_ref,
            checksum: CanonicalHash::digest(b"unsealed child-exit report"),
        };
        report.checksum =
            report
                .recompute_checksum()
                .map_err(|error| LaunchModelError::CanonicalEncoding {
                    reason: error.to_string(),
                })?;
        Ok(report)
    }

    /// Authenticates bounded canonical child-exit bytes at rest.
    ///
    /// # Errors
    ///
    /// Returns [`ChildExitError`] for oversized, malformed, non-canonical,
    /// unsupported, checksum-invalid, or ill-shaped bytes.
    pub fn authenticate_at_rest(bytes: &[u8]) -> Result<Self, ChildExitError> {
        if bytes.len() > MAX_LAUNCH_INTENT_BYTES {
            return Err(ChildExitError::InputTooLarge {
                actual_bytes: bytes.len(),
                maximum_bytes: MAX_LAUNCH_INTENT_BYTES,
            });
        }
        let report =
            serde_json::from_slice::<Self>(bytes).map_err(|error| ChildExitError::InvalidJson {
                reason: bounded_parser_reason(&error.to_string()),
            })?;
        let canonical =
            canonical_json_bytes(&report).map_err(|error| ChildExitError::InvalidJson {
                reason: bounded_parser_reason(&error.to_string()),
            })?;
        if canonical != bytes {
            return Err(ChildExitError::NonCanonicalBytes);
        }
        if report.schema_version != CHILD_EXIT_SCHEMA_VERSION {
            return Err(ChildExitError::UnsupportedSchema {
                actual: report.schema_version,
                expected: CHILD_EXIT_SCHEMA_VERSION,
            });
        }
        if validate_draft(&ChildExitReportDraftV1 {
            child_generation: report.child_generation,
            process_epoch: report.process_epoch,
            role: report.role,
            exit_kind: report.exit_kind,
            intent_generation: report.intent_generation,
            intent_checksum: report.intent_checksum,
            confirmed_setting_transaction_revision: report.confirmed_setting_transaction_revision,
            last_written_world: report.last_written_world,
            last_durable_world: report.last_durable_world,
            shell_lock_hash: report.shell_lock_hash,
            world_lock_hash: report.world_lock_hash,
            world_open_plan_hash: report.world_open_plan_hash,
            diagnostic_ref: report.diagnostic_ref,
        })
        .is_err()
        {
            return Err(ChildExitError::InvalidShape);
        }
        let actual = report
            .recompute_checksum()
            .map_err(|error| ChildExitError::InvalidJson {
                reason: bounded_parser_reason(&error.to_string()),
            })?;
        if actual != report.checksum {
            return Err(ChildExitError::ChecksumMismatch {
                expected: report.checksum,
                actual,
            });
        }
        Ok(report)
    }

    /// Encodes the sealed envelope as exact canonical JSON bytes.
    ///
    /// # Errors
    ///
    /// Returns [`LaunchModelError::CanonicalEncoding`] if serialization fails.
    pub fn canonical_bytes(&self) -> Result<Vec<u8>, LaunchModelError> {
        canonical_json_bytes(self).map_err(|error| LaunchModelError::CanonicalEncoding {
            reason: error.to_string(),
        })
    }

    /// Validates this report against the child the supervisor actually launched.
    ///
    /// # Errors
    ///
    /// Returns [`ChildExitError`] when generation, epoch, role, or shell lock
    /// differs from the trusted supervisor context.
    pub fn validate_supervised_child(
        &self,
        child_generation: LaunchGeneration,
        process_epoch: ProcessEpoch,
        role: ChildRoleV1,
        shell_lock_hash: CanonicalHash,
    ) -> Result<(), ChildExitError> {
        if self.child_generation != child_generation
            || self.process_epoch != process_epoch
            || self.role != role
        {
            return Err(ChildExitError::SupervisedChildMismatch);
        }
        if self.shell_lock_hash != shell_lock_hash {
            return Err(ChildExitError::ShellLockMismatch);
        }
        Ok(())
    }

    /// Reports whether a pending intent is compatible with this exit.
    #[must_use]
    pub fn matches_intent(&self, intent: &LaunchIntentV1) -> bool {
        let Some(intent_generation) = self.intent_generation else {
            return false;
        };
        let Some(intent_checksum) = self.intent_checksum else {
            return false;
        };
        if intent.generation() != intent_generation || intent.checksum() != intent_checksum {
            return false;
        }
        if intent.shell_lock_hash() != self.shell_lock_hash
            || intent.confirmed_setting_transaction_revision()
                != self.confirmed_setting_transaction_revision
        {
            return false;
        }
        match (self.role, self.exit_kind, intent.target()) {
            (
                ChildRoleV1::Shell | ChildRoleV1::Recovery,
                ChildExitKindV1::Handoff,
                LaunchTargetV1::World { .. },
            ) => intent.world_lock_hash().is_some() && intent.world_open_plan_hash().is_some(),
            (
                ChildRoleV1::World { world_id },
                ChildExitKindV1::SaveAndQuit,
                LaunchTargetV1::Shell,
            ) => {
                intent.world_lock_hash().is_none()
                    && intent.world_open_plan_hash().is_none()
                    && self.world_lock_hash.is_some()
                    && self
                        .last_durable_world
                        .is_some_and(|durable| durable.world_id() == world_id)
            }
            _ => false,
        }
    }

    /// Returns the schema version.
    #[must_use]
    pub const fn schema_version(&self) -> u32 {
        self.schema_version
    }

    /// Returns the generation that booted the exiting child.
    #[must_use]
    pub const fn child_generation(&self) -> LaunchGeneration {
        self.child_generation
    }

    /// Returns the child's process epoch.
    #[must_use]
    pub const fn process_epoch(&self) -> ProcessEpoch {
        self.process_epoch
    }

    /// Returns the child's role.
    #[must_use]
    pub const fn role(&self) -> ChildRoleV1 {
        self.role
    }

    /// Returns the typed exit kind.
    #[must_use]
    pub const fn exit_kind(&self) -> ChildExitKindV1 {
        self.exit_kind
    }

    /// Returns the published next-intent generation, if any.
    #[must_use]
    pub const fn intent_generation(&self) -> Option<LaunchGeneration> {
        self.intent_generation
    }

    /// Returns the published next-intent checksum, if any.
    #[must_use]
    pub const fn intent_checksum(&self) -> Option<CanonicalHash> {
        self.intent_checksum
    }

    /// Returns the last confirmed settings revision.
    #[must_use]
    pub const fn confirmed_setting_transaction_revision(&self) -> SettingTransactionRevision {
        self.confirmed_setting_transaction_revision
    }

    /// Returns the latest written world revision, if reported.
    #[must_use]
    pub const fn last_written_world(&self) -> Option<DurableWorldRevisionV1> {
        self.last_written_world
    }

    /// Returns the latest durable world revision, if proven.
    #[must_use]
    pub const fn last_durable_world(&self) -> Option<DurableWorldRevisionV1> {
        self.last_durable_world
    }

    /// Returns the exact shell lock hash.
    #[must_use]
    pub const fn shell_lock_hash(&self) -> CanonicalHash {
        self.shell_lock_hash
    }

    /// Returns the optional frozen world lock hash.
    #[must_use]
    pub const fn world_lock_hash(&self) -> Option<CanonicalHash> {
        self.world_lock_hash
    }

    /// Returns the optional world-open plan hash.
    #[must_use]
    pub const fn world_open_plan_hash(&self) -> Option<CanonicalHash> {
        self.world_open_plan_hash
    }

    /// Returns the bounded diagnostic reference.
    #[must_use]
    pub const fn diagnostic_ref(&self) -> Option<CanonicalHash> {
        self.diagnostic_ref
    }

    /// Returns the body checksum.
    #[must_use]
    pub const fn checksum(&self) -> CanonicalHash {
        self.checksum
    }

    fn recompute_checksum(&self) -> Result<CanonicalHash, latticeaxiom_core::CanonicalJsonError> {
        let mut value = serde_json::to_value(self)?;
        if let Value::Object(fields) = &mut value {
            fields.remove("checksum");
        }
        canonical_json_hash(&value)
    }
}

fn validate_draft(draft: &ChildExitReportDraftV1) -> Result<(), LaunchModelError> {
    if draft.intent_generation.is_some() != draft.intent_checksum.is_some() {
        return Err(LaunchModelError::InvalidChildExitShape);
    }
    if let Some(intent_generation) = draft.intent_generation {
        let expected = draft.child_generation.next()?;
        if intent_generation != expected {
            return Err(LaunchModelError::InvalidChildExitShape);
        }
    }
    match draft.role {
        ChildRoleV1::Shell | ChildRoleV1::Recovery => validate_shell_like_draft(draft),
        ChildRoleV1::World { world_id } => validate_world_draft(draft, world_id),
    }
}

fn validate_shell_like_draft(draft: &ChildExitReportDraftV1) -> Result<(), LaunchModelError> {
    if draft.world_lock_hash.is_some()
        || draft.world_open_plan_hash.is_some()
        || draft.last_written_world.is_some()
        || draft.last_durable_world.is_some()
    {
        return Err(LaunchModelError::InvalidChildExitShape);
    }
    match draft.exit_kind {
        ChildExitKindV1::SaveAndQuit | ChildExitKindV1::WrittenOnly => {
            Err(LaunchModelError::InvalidChildExitShape)
        }
        ChildExitKindV1::Handoff if draft.intent_generation.is_none() => {
            Err(LaunchModelError::InvalidChildExitShape)
        }
        ChildExitKindV1::ShellQuit | ChildExitKindV1::OsClose
            if draft.intent_generation.is_some() =>
        {
            Err(LaunchModelError::InvalidChildExitShape)
        }
        ChildExitKindV1::Handoff
        | ChildExitKindV1::ShellQuit
        | ChildExitKindV1::OsClose
        | ChildExitKindV1::Crash
        | ChildExitKindV1::ShutdownTimeout => Ok(()),
    }
}

fn validate_world_draft(
    draft: &ChildExitReportDraftV1,
    world_id: WorldId,
) -> Result<(), LaunchModelError> {
    if draft.world_lock_hash.is_none() || draft.world_open_plan_hash.is_none() {
        return Err(LaunchModelError::InvalidChildExitShape);
    }
    if draft
        .last_written_world
        .is_some_and(|written| written.world_id() != world_id)
        || draft
            .last_durable_world
            .is_some_and(|durable| durable.world_id() != world_id)
    {
        return Err(LaunchModelError::InvalidChildExitShape);
    }
    if let (Some(written), Some(durable)) = (draft.last_written_world, draft.last_durable_world)
        && written.revision().get() < durable.revision().get()
    {
        return Err(LaunchModelError::InvalidChildExitShape);
    }
    match draft.exit_kind {
        ChildExitKindV1::ShellQuit | ChildExitKindV1::Handoff => {
            Err(LaunchModelError::InvalidChildExitShape)
        }
        ChildExitKindV1::SaveAndQuit => {
            let Some(durable) = draft.last_durable_world else {
                return Err(LaunchModelError::InvalidChildExitShape);
            };
            if draft.intent_generation.is_none() {
                return Err(LaunchModelError::InvalidChildExitShape);
            }
            if draft
                .last_written_world
                .is_some_and(|written| written.revision() != durable.revision())
            {
                return Err(LaunchModelError::InvalidChildExitShape);
            }
            Ok(())
        }
        ChildExitKindV1::WrittenOnly if draft.last_written_world.is_none() => {
            Err(LaunchModelError::InvalidChildExitShape)
        }
        ChildExitKindV1::WrittenOnly
        | ChildExitKindV1::OsClose
        | ChildExitKindV1::Crash
        | ChildExitKindV1::ShutdownTimeout => Ok(()),
    }
}

fn bounded_parser_reason(reason: &str) -> String {
    const MAX_CHARS: usize = 256;
    reason.chars().take(MAX_CHARS).collect()
}

#[cfg(test)]
mod tests {
    use latticeaxiom_core::WorldId;

    use super::*;
    use crate::WorldRevision;

    fn generation(value: u64) -> LaunchGeneration {
        LaunchGeneration::new(value)
            .unwrap_or_else(|error| panic!("test generation must be non-zero: {error}"))
    }

    fn epoch(value: u64) -> ProcessEpoch {
        ProcessEpoch::new(value)
            .unwrap_or_else(|error| panic!("test process epoch must be non-zero: {error}"))
    }

    fn shell_quit() -> ChildExitReportV1 {
        ChildExitReportV1::seal(ChildExitReportDraftV1 {
            child_generation: generation(1),
            process_epoch: epoch(1),
            role: ChildRoleV1::Shell,
            exit_kind: ChildExitKindV1::ShellQuit,
            intent_generation: None,
            intent_checksum: None,
            confirmed_setting_transaction_revision: SettingTransactionRevision::new(0),
            last_written_world: None,
            last_durable_world: None,
            shell_lock_hash: CanonicalHash::digest(b"shell"),
            world_lock_hash: None,
            world_open_plan_hash: None,
            diagnostic_ref: None,
        })
        .unwrap_or_else(|error| panic!("valid shell-quit report was rejected: {error}"))
    }

    #[test]
    fn shell_quit_round_trips_exact_canonical_bytes() {
        let report = shell_quit();
        let bytes = report
            .canonical_bytes()
            .unwrap_or_else(|error| panic!("report did not encode: {error}"));
        assert_eq!(
            ChildExitReportV1::authenticate_at_rest(&bytes).ok(),
            Some(report)
        );
    }

    #[test]
    fn world_save_and_quit_requires_durable_revision_and_next_intent() {
        let world_id = WorldId::new_v4();
        let invalid = ChildExitReportV1::seal(ChildExitReportDraftV1 {
            child_generation: generation(2),
            process_epoch: epoch(2),
            role: ChildRoleV1::World { world_id },
            exit_kind: ChildExitKindV1::SaveAndQuit,
            intent_generation: None,
            intent_checksum: None,
            confirmed_setting_transaction_revision: SettingTransactionRevision::new(3),
            last_written_world: None,
            last_durable_world: Some(DurableWorldRevisionV1::new(world_id, WorldRevision::new(4))),
            shell_lock_hash: CanonicalHash::digest(b"shell"),
            world_lock_hash: Some(CanonicalHash::digest(b"world")),
            world_open_plan_hash: Some(CanonicalHash::digest(b"plan")),
            diagnostic_ref: None,
        });
        assert_eq!(invalid, Err(LaunchModelError::InvalidChildExitShape));
    }

    #[test]
    fn whitespace_is_rejected_as_non_canonical() {
        let mut bytes = shell_quit()
            .canonical_bytes()
            .unwrap_or_else(|error| panic!("report did not encode: {error}"));
        bytes.push(b'\n');
        assert_eq!(
            ChildExitReportV1::authenticate_at_rest(&bytes),
            Err(ChildExitError::NonCanonicalBytes)
        );
    }
}
