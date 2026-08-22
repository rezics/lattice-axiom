use std::collections::{BTreeMap, BTreeSet};

use latticeaxiom_core::WorldId;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{
    DisplayName, HeaderCodecError, LiveWorldLocation, MAX_WORLD_HEADER_BYTES, ReadOnlyWorldSource,
    SourceReadError, WorldDiagnostic, WorldHeaderV1, WorldOpenAction, WorldOpenPlan,
    WorldOpenStatus,
};

/// Bootstrap upper bound on concurrent sidecar reads.
pub const MAX_CONCURRENT_HEADER_READS: usize = 8;

/// Healthy bounded projection retained by the catalog index.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CatalogProjection {
    /// Immutable identity read from both directory and sidecar.
    pub world_id: WorldId,
    /// User-facing name indexed independently of location.
    pub display_name: DisplayName,
    /// Metadata epoch visible during the bounded scan.
    pub metadata_epoch: u64,
    /// Whether the projection claims a clean shutdown.
    pub clean_shutdown: bool,
    /// Latest durable frontier visible in the projection.
    pub durable_frontier: u64,
}

impl From<&WorldHeaderV1> for CatalogProjection {
    fn from(header: &WorldHeaderV1) -> Self {
        Self {
            world_id: header.projection.world_id,
            display_name: header.projection.display_name.clone(),
            metadata_epoch: header.projection.metadata_epoch,
            clean_shutdown: header.projection.clean_shutdown,
            durable_frontier: header.projection.durable_frontier,
        }
    }
}

/// Typed per-entry catalog failure.
#[derive(Clone, Debug, Eq, Error, PartialEq)]
pub enum CatalogEntryFailure {
    /// Sidecar or candidate could not be read.
    #[error("world entry is unreadable: {0}")]
    Unreadable(String),
    /// Sidecar access was denied.
    #[error("permission denied while reading the world header")]
    PermissionDenied,
    /// Sidecar exceeded the fixed 64 KiB limit.
    #[error("world header has {actual} bytes; the limit is 65536")]
    HeaderTooLarge {
        /// Observed source size.
        actual: usize,
    },
    /// Sidecar checksum is invalid.
    #[error("world header checksum is invalid")]
    BadChecksum,
    /// Sidecar schema is unsupported.
    #[error("world header schema {observed} is unsupported")]
    UnsupportedHeader {
        /// Observed schema version.
        observed: u32,
    },
    /// Sidecar JSON is malformed or not canonical.
    #[error("world header is malformed or noncanonical: {0}")]
    MalformedHeader(String),
    /// Directory UUID and sidecar UUID disagree.
    #[error("world directory identity {directory} does not match header identity {header}")]
    DirectoryIdentityMismatch {
        /// `UUIDv4` encoded by the direct child directory.
        directory: WorldId,
        /// `UUIDv4` read from the sidecar.
        header: WorldId,
    },
    /// The same live world identity occurs in multiple locations.
    #[error("duplicate live world identity {world_id}")]
    DuplicateWorldId {
        /// Duplicated world identity.
        world_id: WorldId,
    },
    /// A temporary publication artifact was observed instead of a sidecar.
    #[error("incomplete temporary world-header publication")]
    IncompleteTemporary,
    /// Candidate resolution would traverse a symlink, junction, or reparse point.
    #[error("world candidate escapes its allowlisted root")]
    SymlinkEscape,
    /// Managed-trash tombstone is unavailable or invalid.
    #[error("managed-trash tombstone is invalid: {0}")]
    TrashTombstoneError(String),
}

/// Catalog-visible state for one structural location.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CatalogEntryState {
    /// A checksum-verified bounded header was decoded.
    Projected(CatalogProjection),
    /// This entry remains visible with a typed local error.
    Failed(CatalogEntryFailure),
}

/// One world-list entry keyed only by an allowlisted root and UUID directory.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CatalogEntry {
    /// Structural, path-independent source location.
    pub location: LiveWorldLocation,
    /// Bounded projection or typed failure.
    pub state: CatalogEntryState,
}

/// In-memory catalog projection with separate identity and display-name indices.
#[derive(Clone, Debug, Default)]
pub struct WorldCatalogIndex {
    entries: BTreeMap<LiveWorldLocation, CatalogEntry>,
    by_world_id: BTreeMap<WorldId, BTreeSet<LiveWorldLocation>>,
    by_display_name: BTreeMap<DisplayName, BTreeSet<LiveWorldLocation>>,
}

impl WorldCatalogIndex {
    /// Inserts or replaces one entry without deriving its location from a name.
    pub fn insert(&mut self, entry: CatalogEntry) {
        if let Some(previous) = self.entries.remove(&entry.location) {
            self.remove_indices(&previous);
        }
        self.add_indices(&entry);
        self.entries.insert(entry.location, entry);
    }

    /// Returns an entry by structural location.
    #[must_use]
    pub fn entry(&self, location: LiveWorldLocation) -> Option<&CatalogEntry> {
        self.entries.get(&location)
    }

    /// Returns every location with the exact canonical display name.
    #[must_use]
    pub fn by_display_name(&self, name: &DisplayName) -> Vec<&CatalogEntry> {
        self.by_display_name
            .get(name)
            .into_iter()
            .flatten()
            .filter_map(|location| self.entries.get(location))
            .collect()
    }

    /// Returns all locations carrying one immutable world identity.
    #[must_use]
    pub fn by_world_id(&self, world_id: WorldId) -> Vec<&CatalogEntry> {
        self.by_world_id
            .get(&world_id)
            .into_iter()
            .flatten()
            .filter_map(|location| self.entries.get(location))
            .collect()
    }

    /// Returns duplicate live identities without hiding any entry.
    #[must_use]
    pub fn duplicate_world_ids(&self) -> Vec<WorldId> {
        self.by_world_id
            .iter()
            .filter_map(|(world_id, locations)| (locations.len() > 1).then_some(*world_id))
            .collect()
    }

    /// Replaces only the display-name projection for a refreshed metadata epoch.
    ///
    /// The structural location and world ID remain unchanged. This method does
    /// not write authoritative metadata; it models a later validated catalog
    /// refresh after an external rename transaction.
    ///
    /// # Errors
    ///
    /// Returns [`CatalogIndexError`] when the location is absent or currently
    /// represents a failed entry.
    pub fn refresh_display_name(
        &mut self,
        location: LiveWorldLocation,
        display_name: DisplayName,
    ) -> Result<(), CatalogIndexError> {
        let mut entry = self
            .entries
            .remove(&location)
            .ok_or(CatalogIndexError::MissingEntry)?;
        self.remove_indices(&entry);
        let CatalogEntryState::Projected(projection) = &mut entry.state else {
            self.add_indices(&entry);
            self.entries.insert(location, entry);
            return Err(CatalogIndexError::FailedEntry);
        };
        projection.display_name = display_name;
        self.add_indices(&entry);
        self.entries.insert(location, entry);
        Ok(())
    }

    /// Number of visible entries, including failures and duplicates.
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Returns whether the index contains no visible entries.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    fn add_indices(&mut self, entry: &CatalogEntry) {
        self.by_world_id
            .entry(entry.location.world_id)
            .or_default()
            .insert(entry.location);
        if let CatalogEntryState::Projected(projection) = &entry.state {
            self.by_display_name
                .entry(projection.display_name.clone())
                .or_default()
                .insert(entry.location);
        }
    }

    fn remove_indices(&mut self, entry: &CatalogEntry) {
        remove_location(
            &mut self.by_world_id,
            &entry.location.world_id,
            entry.location,
        );
        if let CatalogEntryState::Projected(projection) = &entry.state {
            remove_location(
                &mut self.by_display_name,
                &projection.display_name,
                entry.location,
            );
        }
    }
}

fn remove_location<K: Ord>(
    index: &mut BTreeMap<K, BTreeSet<LiveWorldLocation>>,
    key: &K,
    location: LiveWorldLocation,
) {
    let remove_key = index.get_mut(key).is_some_and(|locations| {
        locations.remove(&location);
        locations.is_empty()
    });
    if remove_key {
        index.remove(key);
    }
}

/// Failure to update a catalog projection.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum CatalogIndexError {
    /// The structural location is absent.
    #[error("catalog entry does not exist")]
    MissingEntry,
    /// The entry has no valid header projection to rename.
    #[error("catalog entry has no valid display-name projection")]
    FailedEntry,
}

/// Synchronous reference scanner for already-resolved direct children.
///
/// A production host may schedule up to [`MAX_CONCURRENT_HEADER_READS`]
/// equivalent reads. This reference remains serial, which is within the bound,
/// and never reads authoritative metadata during catalog scan.
#[derive(Debug, Default)]
pub struct CatalogScanner;

impl CatalogScanner {
    /// Scans every supplied direct-child candidate and retains per-entry errors.
    pub fn scan<S>(
        source: &mut S,
        candidates: impl IntoIterator<Item = LiveWorldLocation>,
    ) -> WorldCatalogIndex
    where
        S: ReadOnlyWorldSource,
    {
        let mut index = WorldCatalogIndex::default();
        for location in candidates {
            let state = match source.read_header_bounded(location, MAX_WORLD_HEADER_BYTES) {
                Ok(Some(bytes)) => match WorldHeaderV1::decode_canonical(&bytes) {
                    Ok(header) if header.projection.world_id == location.world_id => {
                        CatalogEntryState::Projected(CatalogProjection::from(&header))
                    }
                    Ok(header) => {
                        CatalogEntryState::Failed(CatalogEntryFailure::DirectoryIdentityMismatch {
                            directory: location.world_id,
                            header: header.projection.world_id,
                        })
                    }
                    Err(error) => CatalogEntryState::Failed(map_header_error(error)),
                },
                Ok(None) => CatalogEntryState::Failed(CatalogEntryFailure::IncompleteTemporary),
                Err(error) => CatalogEntryState::Failed(map_source_error(error)),
            };
            index.insert(CatalogEntry { location, state });
        }
        index
    }
}

fn map_source_error(error: SourceReadError) -> CatalogEntryFailure {
    match error {
        SourceReadError::PermissionDenied { .. } => CatalogEntryFailure::PermissionDenied,
        SourceReadError::LimitExceeded { available, .. } => {
            CatalogEntryFailure::HeaderTooLarge { actual: available }
        }
        other => CatalogEntryFailure::Unreadable(other.to_string()),
    }
}

fn map_header_error(error: HeaderCodecError) -> CatalogEntryFailure {
    match error {
        HeaderCodecError::TooLarge { actual } => CatalogEntryFailure::HeaderTooLarge { actual },
        HeaderCodecError::BadChecksum { .. } => CatalogEntryFailure::BadChecksum,
        HeaderCodecError::UnsupportedSchema { observed } => {
            CatalogEntryFailure::UnsupportedHeader { observed }
        }
        other => CatalogEntryFailure::MalformedHeader(other.to_string()),
    }
}

/// Player-facing catalog card state from `@latticeaxiom/world-library`.
///
/// Preflight [`WorldOpenStatus`] remains the next-safe-step machine. Cards must
/// not collapse every failure into a generic “cannot open” label.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum CatalogCardState {
    /// Frozen lock matches; Continue/Play may prepare a launch intent.
    ReadyExact,
    /// Compatible closure exists; the player must accept an explicit diff.
    ReadyCompatible,
    /// Required packages or artifacts are missing locally.
    MissingPackage,
    /// A checkpointed staged migration is the next safe step.
    MigrationRequired,
    /// Crash, unclean shutdown, header repair, or restore remains.
    Recoverable,
    /// Bytes may be inspected or exported; a writer must not open.
    ReadOnly,
    /// Identity, checksum, or preservation is unsafe.
    Corrupt,
}

/// Classifies one visible card from a bounded scan entry and optional preflight.
#[must_use]
pub fn classify_catalog_card(
    entry: &CatalogEntryState,
    plan: Option<&WorldOpenPlan>,
) -> CatalogCardState {
    match (entry, plan) {
        (CatalogEntryState::Failed(_), _) => CatalogCardState::Corrupt,
        (_, Some(plan)) => classify_open_plan(plan),
        (CatalogEntryState::Projected(projection), None) if !projection.clean_shutdown => {
            CatalogCardState::Recoverable
        }
        (CatalogEntryState::Projected(_), None) => CatalogCardState::Recoverable,
    }
}

fn classify_open_plan(plan: &WorldOpenPlan) -> CatalogCardState {
    match plan.status {
        WorldOpenStatus::ReadyExact => CatalogCardState::ReadyExact,
        WorldOpenStatus::ReadyCompatible => CatalogCardState::ReadyCompatible,
        WorldOpenStatus::NeedsDownloadOrBuild => CatalogCardState::MissingPackage,
        WorldOpenStatus::NeedsMigration => CatalogCardState::MigrationRequired,
        WorldOpenStatus::RecoverableReadOnly if recoverable_card(plan) => {
            CatalogCardState::Recoverable
        }
        WorldOpenStatus::RecoverableReadOnly => CatalogCardState::ReadOnly,
        WorldOpenStatus::Blocked => CatalogCardState::Corrupt,
    }
}

fn recoverable_card(plan: &WorldOpenPlan) -> bool {
    plan.actions.iter().any(|action| {
        matches!(
            action,
            WorldOpenAction::RestoreCheckpoint { .. } | WorldOpenAction::RepairHeader { .. }
        )
    }) || plan.diagnostics.iter().any(|diagnostic| {
        matches!(
            diagnostic,
            WorldDiagnostic::UncleanShutdown
                | WorldDiagnostic::NonDurableFrontier { .. }
                | WorldDiagnostic::HeaderRepairRequired { .. }
        )
    })
}

/// Serializable diagnostic code for catalog failures.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum CatalogDiagnosticCode {
    /// Generic unreadable entry.
    Unreadable,
    /// Permission denied.
    PermissionDenied,
    /// Header exceeded its bound.
    HeaderTooLarge,
    /// Header checksum failed.
    BadChecksum,
    /// Header schema is unsupported.
    UnsupportedHeader,
    /// Duplicate live identity.
    DuplicateWorldId,
    /// Header and authoritative metadata differ.
    MetadataMismatch,
    /// Interrupted temporary publication.
    IncompleteTemporary,
    /// Symlink, junction, or reparse escape.
    SymlinkEscape,
    /// Invalid trash tombstone.
    TrashTombstoneError,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        AuthoritativeMetadataV1, MemoryWorldRecord, MemoryWorldSource, WorldRootId,
        header::tests::fixture_projection,
    };

    #[test]
    fn display_index_is_independent_from_location_and_allows_duplicates() {
        let projection = fixture_projection();
        let header =
            WorldHeaderV1::seal(projection.clone()).unwrap_or_else(|error| panic!("{error}"));
        let bytes = header
            .encode_canonical()
            .unwrap_or_else(|error| panic!("{error}"));
        let metadata = AuthoritativeMetadataV1::seal(projection.clone())
            .unwrap_or_else(|error| panic!("{error}"));
        let first = LiveWorldLocation::new(WorldRootId(1), projection.world_id);
        let second = LiveWorldLocation::new(WorldRootId(2), projection.world_id);
        let mut source = MemoryWorldSource::default();
        for location in [first, second] {
            source.insert(
                location,
                MemoryWorldRecord {
                    header_bytes: Some(bytes.clone()),
                    metadata: Some(metadata.clone()),
                },
            );
        }

        let mut index = CatalogScanner::scan(&mut source, [first, second]);
        assert_eq!(index.len(), 2);
        assert_eq!(index.duplicate_world_ids(), vec![projection.world_id]);
        assert_eq!(index.by_display_name(&projection.display_name).len(), 2);

        let renamed = DisplayName::new("Renamed").unwrap_or_else(|error| panic!("{error}"));
        assert!(index.refresh_display_name(first, renamed.clone()).is_ok());
        assert_eq!(index.entry(first).map(|entry| entry.location), Some(first));
        assert_eq!(index.by_display_name(&renamed).len(), 1);
    }

    #[test]
    fn a_bad_header_does_not_hide_other_entries_or_read_metadata() {
        let projection = fixture_projection();
        let good_location = LiveWorldLocation::new(WorldRootId(1), projection.world_id);
        let bad_world = "223e4567-e89b-42d3-a456-426614174000"
            .parse()
            .unwrap_or_else(|error| panic!("fixture world ID: {error}"));
        let bad_location = LiveWorldLocation::new(WorldRootId(1), bad_world);
        let good_header = WorldHeaderV1::seal(projection.clone())
            .unwrap_or_else(|error| panic!("{error}"))
            .encode_canonical()
            .unwrap_or_else(|error| panic!("{error}"));
        let mut source = MemoryWorldSource::default();
        source.insert(
            good_location,
            MemoryWorldRecord {
                header_bytes: Some(good_header),
                metadata: None,
            },
        );
        source.insert(
            bad_location,
            MemoryWorldRecord {
                header_bytes: Some(b"not json".to_vec()),
                metadata: None,
            },
        );

        let index = CatalogScanner::scan(&mut source, [bad_location, good_location]);
        assert_eq!(index.len(), 2);
        assert!(matches!(
            index.entry(bad_location).map(|entry| &entry.state),
            Some(CatalogEntryState::Failed(_))
        ));
        assert_eq!(source.audit().metadata_reads, 0);
        assert_eq!(
            classify_catalog_card(
                &index
                    .entry(bad_location)
                    .expect("failed entry remains visible")
                    .state,
                None
            ),
            CatalogCardState::Corrupt
        );
        assert_eq!(
            classify_catalog_card(
                &index
                    .entry(good_location)
                    .expect("healthy entry remains visible")
                    .state,
                None
            ),
            CatalogCardState::Recoverable
        );
    }

    #[test]
    fn catalog_cards_keep_preflight_states_distinct() {
        let projection = fixture_projection();
        let entry = CatalogEntryState::Projected(CatalogProjection::from(
            &WorldHeaderV1::seal(projection.clone()).unwrap_or_else(|error| panic!("{error}")),
        ));
        let world_id = projection.world_id;
        let exact = WorldOpenPlan {
            world_id,
            status: WorldOpenStatus::ReadyExact,
            risk: crate::WorldOpenRisk::None,
            reconciliation: crate::ReconciliationState::InSync { metadata_epoch: 1 },
            next_safe_step: Some(WorldOpenAction::UseFrozenLock),
            actions: vec![WorldOpenAction::UseFrozenLock],
            diagnostics: Vec::new(),
            activation_binding: None,
        };
        assert_eq!(
            classify_catalog_card(&entry, Some(&exact)),
            CatalogCardState::ReadyExact
        );
        let mut missing = exact.clone();
        missing.status = WorldOpenStatus::NeedsDownloadOrBuild;
        missing.actions = vec![WorldOpenAction::PreparePackage {
            package: "@example/game"
                .parse()
                .unwrap_or_else(|error| panic!("{error}")),
        }];
        assert_eq!(
            classify_catalog_card(&entry, Some(&missing)),
            CatalogCardState::MissingPackage
        );
        let mut read_only = exact.clone();
        read_only.status = WorldOpenStatus::RecoverableReadOnly;
        read_only.actions = vec![WorldOpenAction::OpenReadOnly, WorldOpenAction::Export];
        assert_eq!(
            classify_catalog_card(&entry, Some(&read_only)),
            CatalogCardState::ReadOnly
        );
        let mut recoverable = read_only;
        recoverable.diagnostics = vec![WorldDiagnostic::UncleanShutdown];
        recoverable.actions.insert(
            0,
            WorldOpenAction::RestoreCheckpoint {
                checkpoint: crate::CheckpointId::new("checkpoint-1")
                    .unwrap_or_else(|error| panic!("{error}")),
            },
        );
        assert_eq!(
            classify_catalog_card(&entry, Some(&recoverable)),
            CatalogCardState::Recoverable
        );
    }
}
