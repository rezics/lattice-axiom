//! Confined same-directory filesystem implementation of the intent slot.

use std::{
    fs::{self, File, OpenOptions},
    io::{self, Read, Write},
    path::{Component, Path, PathBuf},
};

#[cfg(windows)]
use std::os::windows::fs::OpenOptionsExt;

#[cfg(test)]
use std::cell::Cell;

use latticeaxiom_core::CanonicalHash;

use crate::{
    AtomicChildExitStore, AtomicLaunchIntentStore, ChildExitDisposition, ChildExitSlot, IntentSlot,
    IntentStoreError, MAX_LAUNCH_INTENT_BYTES, MutationDurability, PublishDisposition,
    RecoveryClaimOutcome, SlotDisposition, StoreOperation, TerminalPredecessor,
};

const TEMPORARY_FILE: &str = ".launch-intent.tmp";
const PENDING_FILE: &str = "launch-intent.pending";
const CLAIMED_FILE: &str = "launch-intent.claimed";
const CONSUMED_FILE: &str = "launch-intent.consumed";
const QUARANTINED_FILE: &str = "launch-intent.quarantined";
const RECOVERY_CLAIMED_FILE: &str = "launch-intent.recovery-claimed";
const PREDECESSOR_CONSUMED_FILE: &str = "launch-intent.predecessor.consumed";
const PREDECESSOR_QUARANTINED_FILE: &str = "launch-intent.predecessor.quarantined";
const PREDECESSOR_RECOVERY_FILE: &str = "launch-intent.predecessor.recovery-claimed";
const CHILD_EXIT_TEMPORARY_FILE: &str = ".child-exit.tmp";
const CHILD_EXIT_PENDING_FILE: &str = "child-exit.pending";
const CHILD_EXIT_CONSUMED_FILE: &str = "child-exit.consumed";
const CHILD_EXIT_QUARANTINED_FILE: &str = "child-exit.quarantined";

type LocatedSlot = (SlotDisposition, &'static str, PathBuf);

#[derive(Debug)]
struct SlotLayout {
    current: Option<LocatedSlot>,
    predecessor: Option<LocatedSlot>,
}

/// Atomic single-slot store rooted at one literal, already-created directory.
///
/// All names are compile-time literals, moves stay in one canonical root, and
/// symlink or non-regular slot paths are rejected. The root must be private to
/// one trusted launcher. Hostile concurrent path replacement requires a
/// platform directory-handle adapter beyond this portable D0 implementation.
///
/// Windows opens the directory with the minimum write-data access needed by
/// `FlushFileBuffers`; Rust's safe [`File::sync_all`] wrapper performs the
/// flush. A platform open or flush failure still takes the exact-hash re-read
/// path and is reported as [`MutationDurability::Indeterminate`].
#[derive(Debug)]
pub struct FileLaunchIntentStore {
    root: PathBuf,
    #[cfg(test)]
    directory_syncs_until_fault: Cell<Option<usize>>,
}

impl FileLaunchIntentStore {
    /// Opens a store in a literal absolute directory without creating it.
    ///
    /// # Errors
    ///
    /// Returns [`IntentStoreError::UnsafeRoot`] for relative, dot-segment,
    /// symlinked, non-directory, or non-canonical roots; returns an I/O error
    /// if metadata cannot be inspected.
    pub fn open(root: impl AsRef<Path>) -> Result<Self, IntentStoreError> {
        Ok(Self {
            root: open_confined_root(root)?,
            #[cfg(test)]
            directory_syncs_until_fault: Cell::new(None),
        })
    }

    /// Returns the canonical confined root.
    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    fn path(&self, name: &'static str) -> PathBuf {
        self.root.join(name)
    }

    fn present_slots(
        &self,
        files: &[(SlotDisposition, &'static str)],
    ) -> Result<Vec<LocatedSlot>, IntentStoreError> {
        let mut present = Vec::with_capacity(files.len());
        for &(disposition, name) in files {
            let path = self.path(name);
            if self.inspect_regular_slot(&path, name)? {
                present.push((disposition, name, path));
            }
        }
        Ok(present)
    }

    fn slot_layout(&self) -> Result<SlotLayout, IntentStoreError> {
        let mut current = self.present_slots(&slot_files())?;
        let mut predecessors = self.present_slots(&predecessor_files())?;
        if predecessors.len() > 1 {
            return Err(IntentStoreError::ConflictingSlots);
        }

        let active_count = current
            .iter()
            .filter(|(state, ..)| {
                matches!(state, SlotDisposition::Pending | SlotDisposition::Claimed)
            })
            .count();
        if active_count > 1 {
            return Err(IntentStoreError::ConflictingSlots);
        }
        if active_count == 1 {
            let index = current
                .iter()
                .position(|(state, ..)| {
                    matches!(state, SlotDisposition::Pending | SlotDisposition::Claimed)
                })
                .unwrap_or(0);
            let active = current.remove(index);
            let direct_predecessor = match current.len() {
                0 => None,
                1 => {
                    let candidate = current.pop();
                    if matches!(
                        candidate.as_ref().map(|(state, ..)| *state),
                        Some(SlotDisposition::Quarantined)
                    ) && !predecessors.is_empty()
                    {
                        // A quarantined rejected intent is not an authenticated
                        // watermark; its older typed sidecar is authoritative.
                        None
                    } else {
                        candidate
                    }
                }
                2 => {
                    let recovery_index = current
                        .iter()
                        .position(|(state, ..)| *state == SlotDisposition::RecoveryClaimed);
                    let valid_source = current.iter().any(|(state, ..)| {
                        matches!(
                            state,
                            SlotDisposition::Consumed | SlotDisposition::Quarantined
                        )
                    });
                    if recovery_index.is_none() || !valid_source {
                        return Err(IntentStoreError::ConflictingSlots);
                    }
                    Some(current.remove(recovery_index.unwrap_or(0)))
                }
                _ => return Err(IntentStoreError::ConflictingSlots),
            };
            if direct_predecessor
                .as_ref()
                .is_some_and(|(state, ..)| *state != SlotDisposition::RecoveryClaimed)
                && !predecessors.is_empty()
            {
                return Err(IntentStoreError::ConflictingSlots);
            }
            return Ok(SlotLayout {
                current: Some(active),
                predecessor: direct_predecessor.or_else(|| predecessors.pop()),
            });
        }

        match current.len() {
            0 => Ok(SlotLayout {
                current: predecessors.pop(),
                predecessor: None,
            }),
            1 => Ok(SlotLayout {
                current: current.pop(),
                predecessor: predecessors.pop(),
            }),
            2 => {
                let recovery_index = current
                    .iter()
                    .position(|(state, ..)| *state == SlotDisposition::RecoveryClaimed);
                if recovery_index.is_none()
                    || !current.iter().any(|(state, ..)| {
                        matches!(
                            state,
                            SlotDisposition::Consumed | SlotDisposition::Quarantined
                        )
                    })
                {
                    return Err(IntentStoreError::ConflictingSlots);
                }
                let recovery = current.remove(recovery_index.unwrap_or(0));
                let source = current.pop();
                Ok(SlotLayout {
                    current: Some(recovery),
                    predecessor: source.or_else(|| predecessors.pop()),
                })
            }
            _ => Err(IntentStoreError::ConflictingSlots),
        }
    }

    fn exact_terminal(
        &self,
        expected_blob_hash: CanonicalHash,
    ) -> Result<LocatedSlot, IntentStoreError> {
        let Some((state, label, path)) = self.slot_layout()?.current else {
            return Err(IntentStoreError::UnexpectedState {
                expected: "consumed, quarantined, or recovery-claimed",
                actual: "empty",
            });
        };
        if !state.is_terminal() {
            return Err(IntentStoreError::UnexpectedState {
                expected: "consumed, quarantined, or recovery-claimed",
                actual: state.as_str(),
            });
        }
        if CanonicalHash::digest(self.read_path(&path, label)?) != expected_blob_hash {
            return Err(IntentStoreError::BlobMismatch);
        }
        Ok((state, label, path))
    }

    fn inspect_regular_slot(
        &self,
        path: &Path,
        label: &'static str,
    ) -> Result<bool, IntentStoreError> {
        let metadata = match fs::symlink_metadata(path) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(false),
            Err(source) => return Err(IntentStoreError::io(StoreOperation::Read, source)),
        };
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            return Err(IntentStoreError::UnsafeSlot { slot: label });
        }
        let canonical = fs::canonicalize(path)
            .map_err(|source| IntentStoreError::io(StoreOperation::Read, source))?;
        if canonical != path || canonical.parent() != Some(self.root.as_path()) {
            return Err(IntentStoreError::UnsafeSlot { slot: label });
        }
        Ok(true)
    }

    fn read_path(&self, path: &Path, label: &'static str) -> Result<Vec<u8>, IntentStoreError> {
        if !self.inspect_regular_slot(path, label)? {
            return Err(IntentStoreError::UnexpectedState {
                expected: label,
                actual: "empty",
            });
        }
        let file = File::open(path)
            .map_err(|source| IntentStoreError::io(StoreOperation::Read, source))?;
        let metadata = file
            .metadata()
            .map_err(|source| IntentStoreError::io(StoreOperation::Read, source))?;
        let maximum = MAX_LAUNCH_INTENT_BYTES as u64;
        if metadata.len() > maximum {
            return Err(IntentStoreError::SlotTooLarge {
                actual_bytes: metadata.len(),
                maximum_bytes: maximum,
            });
        }
        let mut bytes =
            Vec::with_capacity(usize::try_from(metadata.len()).unwrap_or(MAX_LAUNCH_INTENT_BYTES));
        file.take(maximum.saturating_add(1))
            .read_to_end(&mut bytes)
            .map_err(|source| IntentStoreError::io(StoreOperation::Read, source))?;
        if bytes.len() > MAX_LAUNCH_INTENT_BYTES {
            return Err(IntentStoreError::SlotTooLarge {
                actual_bytes: u64::try_from(bytes.len()).unwrap_or(u64::MAX),
                maximum_bytes: maximum,
            });
        }
        Ok(bytes)
    }

    fn write_temporary(&self, bytes: &[u8]) -> Result<PathBuf, IntentStoreError> {
        if bytes.len() > MAX_LAUNCH_INTENT_BYTES {
            return Err(IntentStoreError::SlotTooLarge {
                actual_bytes: u64::try_from(bytes.len()).unwrap_or(u64::MAX),
                maximum_bytes: MAX_LAUNCH_INTENT_BYTES as u64,
            });
        }
        let temporary = self.path(TEMPORARY_FILE);
        match fs::symlink_metadata(&temporary) {
            Ok(metadata) => {
                if metadata.file_type().is_symlink() || !metadata.is_file() {
                    return Err(IntentStoreError::UnsafeSlot {
                        slot: TEMPORARY_FILE,
                    });
                }
                fs::remove_file(&temporary).map_err(|source| {
                    IntentStoreError::io(StoreOperation::WriteTemporary, source)
                })?;
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(source) => {
                return Err(IntentStoreError::io(StoreOperation::WriteTemporary, source));
            }
        }
        let mut file = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&temporary)
            .map_err(|source| IntentStoreError::io(StoreOperation::WriteTemporary, source))?;
        file.write_all(bytes)
            .map_err(|source| IntentStoreError::io(StoreOperation::WriteTemporary, source))?;
        file.sync_all()
            .map_err(|source| IntentStoreError::io(StoreOperation::SyncTemporary, source))?;
        Ok(temporary)
    }

    fn direct_file_is_exact(
        &self,
        state: SlotDisposition,
        expected_hash: CanonicalHash,
    ) -> Result<bool, IntentStoreError> {
        let label = file_for(state);
        let path = self.path(label);
        if !self.inspect_regular_slot(&path, label)? {
            return Ok(false);
        }
        Ok(CanonicalHash::digest(self.read_path(&path, label)?) == expected_hash)
    }

    fn observes_exact(
        &self,
        state: SlotDisposition,
        expected_hash: CanonicalHash,
    ) -> Result<bool, IntentStoreError> {
        let Some((actual, label, path)) = self.slot_layout()?.current else {
            return Ok(false);
        };
        Ok(
            actual == state
                && CanonicalHash::digest(self.read_path(&path, label)?) == expected_hash,
        )
    }

    fn sync_visible(
        &self,
        state: SlotDisposition,
        expected_hash: CanonicalHash,
    ) -> Result<MutationDurability, IntentStoreError> {
        match self.sync_directory() {
            Ok(()) => Ok(MutationDurability::Durable),
            Err(_error) if self.direct_file_is_exact(state, expected_hash)? => {
                Ok(MutationDurability::Indeterminate)
            }
            Err(error) => Err(error),
        }
    }

    fn residue_paths(&self, current: &Path) -> Result<Vec<PathBuf>, IntentStoreError> {
        let mut paths = Vec::with_capacity(3);
        for (_, name) in predecessor_files().into_iter().chain(slot_files()) {
            let path = self.path(name);
            if path != current && self.inspect_regular_slot(&path, name)? {
                paths.push(path);
            }
        }
        Ok(paths)
    }

    fn remove_residues(
        &self,
        state: SlotDisposition,
        hash: CanonicalHash,
    ) -> Result<MutationDurability, IntentStoreError> {
        let Some((actual, _, current)) = self.slot_layout()?.current else {
            return Err(IntentStoreError::UnexpectedState {
                expected: state.as_str(),
                actual: "empty",
            });
        };
        if actual != state || !self.observes_exact(state, hash)? {
            return Err(IntentStoreError::BlobMismatch);
        }
        let residues = self.residue_paths(&current)?;
        if residues.is_empty() {
            return Ok(MutationDurability::Durable);
        }
        for path in residues {
            fs::remove_file(path)
                .map_err(|source| IntentStoreError::io(StoreOperation::Retire, source))?;
        }
        match self.sync_directory() {
            Ok(()) => Ok(MutationDurability::Durable),
            Err(_error)
                if self.direct_file_is_exact(state, hash)?
                    && self.residue_paths(&current)?.is_empty() =>
            {
                Ok(MutationDurability::Indeterminate)
            }
            Err(error) => Err(error),
        }
    }
    fn ensure_predecessor_sidecar(
        &self,
        current_state: SlotDisposition,
        current_hash: CanonicalHash,
    ) -> Result<MutationDurability, IntentStoreError> {
        let layout = self.slot_layout()?;
        let Some((predecessor_state, predecessor_label, predecessor_path)) = layout.predecessor
        else {
            return Ok(MutationDurability::Durable);
        };
        if predecessor_files()
            .iter()
            .any(|(_, label)| *label == predecessor_label)
        {
            return Ok(MutationDurability::Durable);
        }
        let predecessor_bytes = self.read_path(&predecessor_path, predecessor_label)?;
        let predecessor_hash = CanonicalHash::digest(&predecessor_bytes);
        let sidecar_label = predecessor_file_for(predecessor_state);
        let sidecar = self.path(sidecar_label);
        if self.inspect_regular_slot(&sidecar, sidecar_label)? {
            return Err(IntentStoreError::ConflictingSlots);
        }
        fs::rename(predecessor_path, &sidecar)
            .map_err(|source| IntentStoreError::io(StoreOperation::Replace, source))?;
        match self.sync_directory() {
            Ok(()) => Ok(MutationDurability::Durable),
            Err(error) => {
                let sidecar_visible = self.inspect_regular_slot(&sidecar, sidecar_label)?
                    && CanonicalHash::digest(self.read_path(&sidecar, sidecar_label)?)
                        == predecessor_hash;
                if self.observes_exact(current_state, current_hash)? && sidecar_visible {
                    Ok(MutationDurability::Indeterminate)
                } else {
                    Err(error)
                }
            }
        }
    }

    fn move_exact(
        &self,
        from_state: SlotDisposition,
        to_state: SlotDisposition,
        expected_hash: CanonicalHash,
        preserve_predecessor: bool,
        cleanup_after: bool,
    ) -> Result<MutationDurability, IntentStoreError> {
        let layout = self.slot_layout()?;
        let Some((actual, from_label, from)) = layout.current else {
            return Err(IntentStoreError::UnexpectedState {
                expected: from_state.as_str(),
                actual: "empty",
            });
        };
        if actual != from_state {
            return Err(IntentStoreError::UnexpectedState {
                expected: from_state.as_str(),
                actual: actual.as_str(),
            });
        }
        if CanonicalHash::digest(self.read_path(&from, from_label)?) != expected_hash {
            return Err(IntentStoreError::BlobMismatch);
        }

        let mut durability = MutationDurability::Durable;
        if preserve_predecessor {
            durability = self.ensure_predecessor_sidecar(from_state, expected_hash)?;
        } else if !cleanup_after {
            durability = self.remove_residues(from_state, expected_hash)?;
        }

        let to_label = file_for(to_state);
        let to = self.path(to_label);
        if self.inspect_regular_slot(&to, to_label)? {
            return Err(IntentStoreError::ConflictingSlots);
        }
        fs::rename(from, &to)
            .map_err(|source| IntentStoreError::io(StoreOperation::Replace, source))?;
        durability = least_durable(durability, self.sync_visible(to_state, expected_hash)?);
        if cleanup_after && durability == MutationDurability::Durable {
            durability = match self.remove_residues(to_state, expected_hash) {
                Ok(cleanup) => cleanup,
                Err(_error) if self.direct_file_is_exact(to_state, expected_hash)? => {
                    MutationDurability::Indeterminate
                }
                Err(error) => return Err(error),
            };
        }
        Ok(durability)
    }

    fn recovery_already_visible(
        &self,
        request_hash: CanonicalHash,
    ) -> Result<bool, IntentStoreError> {
        self.observes_exact(SlotDisposition::RecoveryClaimed, request_hash)
    }

    fn sync_directory(&self) -> Result<(), IntentStoreError> {
        #[cfg(test)]
        if let Some(remaining) = self.directory_syncs_until_fault.get() {
            if remaining == 0 {
                self.directory_syncs_until_fault.set(None);
                return Err(IntentStoreError::io(
                    StoreOperation::SyncDirectory,
                    io::Error::other("injected post-commit directory sync failure"),
                ));
            }
            self.directory_syncs_until_fault
                .set(Some(remaining.saturating_sub(1)));
        }
        self.platform_sync_directory()
    }

    #[cfg(unix)]
    fn platform_sync_directory(&self) -> Result<(), IntentStoreError> {
        File::open(&self.root)
            .and_then(|directory| directory.sync_all())
            .map_err(|source| IntentStoreError::io(StoreOperation::SyncDirectory, source))
    }

    #[cfg(windows)]
    fn platform_sync_directory(&self) -> Result<(), IntentStoreError> {
        // FILE_WRITE_DATA is the narrow write right accepted by
        // FlushFileBuffers. On a directory it maps to FILE_ADD_FILE.
        const FILE_WRITE_DATA: u32 = 0x0000_0002;
        const FILE_SHARE_READ: u32 = 0x0000_0001;
        const FILE_SHARE_WRITE: u32 = 0x0000_0002;
        const FILE_SHARE_DELETE: u32 = 0x0000_0004;
        const FILE_FLAG_BACKUP_SEMANTICS: u32 = 0x0200_0000;

        OpenOptions::new()
            .access_mode(FILE_WRITE_DATA)
            .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE)
            .custom_flags(FILE_FLAG_BACKUP_SEMANTICS)
            .open(&self.root)
            .and_then(|directory| directory.sync_all())
            .map_err(|source| IntentStoreError::io(StoreOperation::SyncDirectory, source))
    }

    #[cfg(not(any(unix, windows)))]
    fn platform_sync_directory(&self) -> Result<(), IntentStoreError> {
        Err(IntentStoreError::io(
            StoreOperation::SyncDirectory,
            io::Error::new(
                io::ErrorKind::Unsupported,
                "directory flush semantics are unsupported on this platform",
            ),
        ))
    }

    #[cfg(test)]
    fn fail_directory_sync_after(&self, successful_calls: usize) {
        self.directory_syncs_until_fault.set(Some(successful_calls));
    }
}

impl AtomicLaunchIntentStore for FileLaunchIntentStore {
    fn read(&mut self) -> Result<IntentSlot, IntentStoreError> {
        let layout = self.slot_layout()?;
        let Some((state, label, path)) = layout.current else {
            return Ok(IntentSlot::Empty);
        };
        let predecessor = match layout.predecessor {
            Some((predecessor_state, predecessor_label, predecessor_path)) => {
                Some(TerminalPredecessor::occupied(
                    predecessor_state,
                    self.read_path(&predecessor_path, predecessor_label)?,
                )?)
            }
            None => None,
        };
        IntentSlot::occupied_with_predecessor(state, self.read_path(&path, label)?, predecessor)
    }

    fn publish(&mut self, canonical_bytes: &[u8]) -> Result<PublishDisposition, IntentStoreError> {
        match self.read()? {
            IntentSlot::Empty => {}
            IntentSlot::Occupied {
                disposition, bytes, ..
            } if bytes == canonical_bytes => {
                return Ok(match disposition {
                    SlotDisposition::Pending => PublishDisposition::AlreadyPublished,
                    handled => PublishDisposition::AlreadyHandled(handled),
                });
            }
            IntentSlot::Occupied { .. } => return Err(IntentStoreError::Occupied),
        }
        let temporary = self.write_temporary(canonical_bytes)?;
        let pending = self.path(PENDING_FILE);
        if self.inspect_regular_slot(&pending, PENDING_FILE)? {
            return Err(IntentStoreError::ConflictingSlots);
        }
        fs::rename(temporary, &pending)
            .map_err(|source| IntentStoreError::io(StoreOperation::Replace, source))?;
        Ok(publish_result(self.sync_visible(
            SlotDisposition::Pending,
            CanonicalHash::digest(canonical_bytes),
        )?))
    }

    fn publish_replacing_terminal(
        &mut self,
        expected_terminal_blob_hash: CanonicalHash,
        canonical_bytes: &[u8],
    ) -> Result<PublishDisposition, IntentStoreError> {
        if let IntentSlot::Occupied {
            disposition: SlotDisposition::Pending,
            bytes,
            predecessor,
            ..
        } = self.read()?
        {
            if bytes != canonical_bytes {
                return Err(IntentStoreError::Occupied);
            }
            return if predecessor.as_ref().map(TerminalPredecessor::blob_hash)
                == Some(expected_terminal_blob_hash)
            {
                Ok(PublishDisposition::AlreadyPublished)
            } else {
                Err(IntentStoreError::BlobMismatch)
            };
        }
        self.exact_terminal(expected_terminal_blob_hash)?;
        let temporary = self.write_temporary(canonical_bytes)?;
        let pending = self.path(PENDING_FILE);
        if self.inspect_regular_slot(&pending, PENDING_FILE)? {
            return Err(IntentStoreError::ConflictingSlots);
        }
        fs::rename(temporary, &pending)
            .map_err(|source| IntentStoreError::io(StoreOperation::Replace, source))?;
        Ok(publish_result(self.sync_visible(
            SlotDisposition::Pending,
            CanonicalHash::digest(canonical_bytes),
        )?))
    }

    fn claim(
        &mut self,
        expected_blob_hash: CanonicalHash,
    ) -> Result<MutationDurability, IntentStoreError> {
        self.move_exact(
            SlotDisposition::Pending,
            SlotDisposition::Claimed,
            expected_blob_hash,
            false,
            true,
        )
    }

    fn consume(
        &mut self,
        expected_blob_hash: CanonicalHash,
    ) -> Result<MutationDurability, IntentStoreError> {
        self.move_exact(
            SlotDisposition::Claimed,
            SlotDisposition::Consumed,
            expected_blob_hash,
            false,
            false,
        )
    }

    fn quarantine(
        &mut self,
        expected_blob_hash: CanonicalHash,
    ) -> Result<MutationDurability, IntentStoreError> {
        let state = self
            .read()?
            .disposition()
            .ok_or(IntentStoreError::UnexpectedState {
                expected: "pending, claimed, or consumed",
                actual: "empty",
            })?;
        if !matches!(
            state,
            SlotDisposition::Pending | SlotDisposition::Claimed | SlotDisposition::Consumed
        ) {
            return Err(IntentStoreError::UnexpectedState {
                expected: "pending, claimed, or consumed",
                actual: state.as_str(),
            });
        }
        self.move_exact(
            state,
            SlotDisposition::Quarantined,
            expected_blob_hash,
            matches!(state, SlotDisposition::Pending | SlotDisposition::Claimed),
            false,
        )
    }

    fn claim_recovery(
        &mut self,
        expected_terminal_blob_hash: Option<CanonicalHash>,
        recovery_request_bytes: &[u8],
    ) -> Result<RecoveryClaimOutcome, IntentStoreError> {
        if recovery_request_bytes.len() > MAX_LAUNCH_INTENT_BYTES {
            return Err(IntentStoreError::SlotTooLarge {
                actual_bytes: u64::try_from(recovery_request_bytes.len()).unwrap_or(u64::MAX),
                maximum_bytes: MAX_LAUNCH_INTENT_BYTES as u64,
            });
        }
        let request_hash = CanonicalHash::digest(recovery_request_bytes);
        if self.recovery_already_visible(request_hash)? {
            return Ok(RecoveryClaimOutcome::new(
                request_hash,
                MutationDurability::Indeterminate,
            ));
        }

        match expected_terminal_blob_hash {
            Some(expected_hash) => {
                let (state, _, _) = self.exact_terminal(expected_hash)?;
                if state == SlotDisposition::RecoveryClaimed {
                    return Err(IntentStoreError::BlobMismatch);
                }
            }
            None => {
                if let IntentSlot::Occupied { disposition, .. } = self.read()? {
                    return Err(IntentStoreError::UnexpectedState {
                        expected: "empty",
                        actual: disposition.as_str(),
                    });
                }
            }
        }

        let temporary = self.write_temporary(recovery_request_bytes)?;
        let recovery = self.path(RECOVERY_CLAIMED_FILE);
        if self.inspect_regular_slot(&recovery, RECOVERY_CLAIMED_FILE)? {
            return Err(IntentStoreError::ConflictingSlots);
        }
        fs::rename(temporary, &recovery)
            .map_err(|source| IntentStoreError::io(StoreOperation::Replace, source))?;
        let mut durability = self.sync_visible(SlotDisposition::RecoveryClaimed, request_hash)?;
        if durability == MutationDurability::Durable {
            durability = match self.remove_residues(SlotDisposition::RecoveryClaimed, request_hash)
            {
                Ok(cleanup) => cleanup,
                Err(_error)
                    if self
                        .direct_file_is_exact(SlotDisposition::RecoveryClaimed, request_hash)? =>
                {
                    MutationDurability::Indeterminate
                }
                Err(error) => return Err(error),
            };
        }
        Ok(RecoveryClaimOutcome::new(request_hash, durability))
    }
}

/// Atomic child-exit store rooted at one literal, already-created directory.
///
/// Slot names are compile-time literals distinct from the launch-intent files,
/// so both stores may share one launcher-owned private root.
#[derive(Debug)]
pub struct FileChildExitStore {
    root: PathBuf,
}

impl FileChildExitStore {
    /// Opens a store in a literal absolute directory without creating it.
    ///
    /// # Errors
    ///
    /// Returns [`IntentStoreError::UnsafeRoot`] for relative, dot-segment,
    /// symlinked, non-directory, or non-canonical roots.
    pub fn open(root: impl AsRef<Path>) -> Result<Self, IntentStoreError> {
        Ok(Self {
            root: open_confined_root(root)?,
        })
    }

    /// Returns the canonical confined root.
    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    fn path(&self, name: &'static str) -> PathBuf {
        self.root.join(name)
    }

    fn inspect_regular_slot(
        &self,
        path: &Path,
        label: &'static str,
    ) -> Result<bool, IntentStoreError> {
        let metadata = match fs::symlink_metadata(path) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(false),
            Err(source) => return Err(IntentStoreError::io(StoreOperation::Read, source)),
        };
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            return Err(IntentStoreError::UnsafeSlot { slot: label });
        }
        let canonical = fs::canonicalize(path)
            .map_err(|source| IntentStoreError::io(StoreOperation::Read, source))?;
        if canonical != path || canonical.parent() != Some(self.root.as_path()) {
            return Err(IntentStoreError::UnsafeSlot { slot: label });
        }
        Ok(true)
    }

    fn read_path(&self, path: &Path, label: &'static str) -> Result<Vec<u8>, IntentStoreError> {
        if !self.inspect_regular_slot(path, label)? {
            return Err(IntentStoreError::UnexpectedState {
                expected: label,
                actual: "empty",
            });
        }
        let file = File::open(path)
            .map_err(|source| IntentStoreError::io(StoreOperation::Read, source))?;
        let metadata = file
            .metadata()
            .map_err(|source| IntentStoreError::io(StoreOperation::Read, source))?;
        let maximum = MAX_LAUNCH_INTENT_BYTES as u64;
        if metadata.len() > maximum {
            return Err(IntentStoreError::SlotTooLarge {
                actual_bytes: metadata.len(),
                maximum_bytes: maximum,
            });
        }
        let mut bytes =
            Vec::with_capacity(usize::try_from(metadata.len()).unwrap_or(MAX_LAUNCH_INTENT_BYTES));
        file.take(maximum.saturating_add(1))
            .read_to_end(&mut bytes)
            .map_err(|source| IntentStoreError::io(StoreOperation::Read, source))?;
        if bytes.len() > MAX_LAUNCH_INTENT_BYTES {
            return Err(IntentStoreError::SlotTooLarge {
                actual_bytes: u64::try_from(bytes.len()).unwrap_or(u64::MAX),
                maximum_bytes: maximum,
            });
        }
        Ok(bytes)
    }

    fn present(
        &self,
    ) -> Result<Option<(ChildExitDisposition, &'static str, PathBuf)>, IntentStoreError> {
        let mut present = Vec::with_capacity(3);
        for (disposition, name) in child_exit_files() {
            let path = self.path(name);
            if self.inspect_regular_slot(&path, name)? {
                present.push((disposition, name, path));
            }
        }
        match present.len() {
            0 => Ok(None),
            1 => Ok(present.pop()),
            _ => Err(IntentStoreError::ConflictingSlots),
        }
    }

    fn write_temporary(&self, bytes: &[u8]) -> Result<PathBuf, IntentStoreError> {
        if bytes.len() > MAX_LAUNCH_INTENT_BYTES {
            return Err(IntentStoreError::SlotTooLarge {
                actual_bytes: u64::try_from(bytes.len()).unwrap_or(u64::MAX),
                maximum_bytes: MAX_LAUNCH_INTENT_BYTES as u64,
            });
        }
        let temporary = self.path(CHILD_EXIT_TEMPORARY_FILE);
        match fs::symlink_metadata(&temporary) {
            Ok(metadata) => {
                if metadata.file_type().is_symlink() || !metadata.is_file() {
                    return Err(IntentStoreError::UnsafeSlot {
                        slot: CHILD_EXIT_TEMPORARY_FILE,
                    });
                }
                fs::remove_file(&temporary).map_err(|source| {
                    IntentStoreError::io(StoreOperation::WriteTemporary, source)
                })?;
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(source) => {
                return Err(IntentStoreError::io(StoreOperation::WriteTemporary, source));
            }
        }
        let mut file = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&temporary)
            .map_err(|source| IntentStoreError::io(StoreOperation::WriteTemporary, source))?;
        file.write_all(bytes)
            .map_err(|source| IntentStoreError::io(StoreOperation::WriteTemporary, source))?;
        file.sync_all()
            .map_err(|source| IntentStoreError::io(StoreOperation::SyncTemporary, source))?;
        Ok(temporary)
    }

    fn observes_exact(
        &self,
        state: ChildExitDisposition,
        expected_hash: CanonicalHash,
    ) -> Result<bool, IntentStoreError> {
        let Some((actual, label, path)) = self.present()? else {
            return Ok(false);
        };
        Ok(
            actual == state
                && CanonicalHash::digest(self.read_path(&path, label)?) == expected_hash,
        )
    }

    fn sync_directory(&self) -> Result<(), IntentStoreError> {
        sync_confined_directory(&self.root)
    }

    fn sync_visible(
        &self,
        state: ChildExitDisposition,
        expected_hash: CanonicalHash,
    ) -> Result<MutationDurability, IntentStoreError> {
        match self.sync_directory() {
            Ok(()) => Ok(MutationDurability::Durable),
            Err(_error) if self.observes_exact(state, expected_hash)? => {
                Ok(MutationDurability::Indeterminate)
            }
            Err(error) => Err(error),
        }
    }

    fn move_exact(
        &self,
        from_state: ChildExitDisposition,
        to_state: ChildExitDisposition,
        expected_hash: CanonicalHash,
    ) -> Result<MutationDurability, IntentStoreError> {
        let Some((actual, from_label, from)) = self.present()? else {
            return Err(IntentStoreError::UnexpectedState {
                expected: from_state.as_str(),
                actual: "empty",
            });
        };
        if actual != from_state {
            return Err(IntentStoreError::UnexpectedState {
                expected: from_state.as_str(),
                actual: actual.as_str(),
            });
        }
        if CanonicalHash::digest(self.read_path(&from, from_label)?) != expected_hash {
            return Err(IntentStoreError::BlobMismatch);
        }
        let to_label = child_exit_file_for(to_state);
        let to = self.path(to_label);
        if self.inspect_regular_slot(&to, to_label)? {
            return Err(IntentStoreError::ConflictingSlots);
        }
        fs::rename(from, &to)
            .map_err(|source| IntentStoreError::io(StoreOperation::Replace, source))?;
        self.sync_visible(to_state, expected_hash)
    }
}

impl AtomicChildExitStore for FileChildExitStore {
    fn read(&mut self) -> Result<ChildExitSlot, IntentStoreError> {
        let Some((disposition, label, path)) = self.present()? else {
            return Ok(ChildExitSlot::Empty);
        };
        ChildExitSlot::occupied(disposition, self.read_path(&path, label)?)
    }

    fn publish(&mut self, canonical_bytes: &[u8]) -> Result<PublishDisposition, IntentStoreError> {
        match self.read()? {
            ChildExitSlot::Empty => {}
            ChildExitSlot::Occupied {
                disposition: ChildExitDisposition::Pending,
                bytes,
                ..
            } if bytes == canonical_bytes => {
                return Ok(PublishDisposition::AlreadyPublished);
            }
            ChildExitSlot::Occupied {
                disposition: ChildExitDisposition::Pending,
                ..
            } => return Err(IntentStoreError::Occupied),
            ChildExitSlot::Occupied {
                disposition: ChildExitDisposition::Consumed,
                bytes,
                ..
            } if bytes == canonical_bytes => {
                return Ok(PublishDisposition::AlreadyHandled(
                    SlotDisposition::Consumed,
                ));
            }
            ChildExitSlot::Occupied {
                disposition: ChildExitDisposition::Quarantined,
                bytes,
                ..
            } if bytes == canonical_bytes => {
                return Ok(PublishDisposition::AlreadyHandled(
                    SlotDisposition::Quarantined,
                ));
            }
            ChildExitSlot::Occupied { .. } => {
                let Some((_, _label, path)) = self.present()? else {
                    return Err(IntentStoreError::UnexpectedState {
                        expected: "consumed or quarantined",
                        actual: "empty",
                    });
                };
                fs::remove_file(path)
                    .map_err(|source| IntentStoreError::io(StoreOperation::Retire, source))?;
            }
        }
        let temporary = self.write_temporary(canonical_bytes)?;
        let pending = self.path(CHILD_EXIT_PENDING_FILE);
        if self.inspect_regular_slot(&pending, CHILD_EXIT_PENDING_FILE)? {
            return Err(IntentStoreError::ConflictingSlots);
        }
        fs::rename(temporary, &pending)
            .map_err(|source| IntentStoreError::io(StoreOperation::Replace, source))?;
        Ok(publish_result(self.sync_visible(
            ChildExitDisposition::Pending,
            CanonicalHash::digest(canonical_bytes),
        )?))
    }

    fn consume(
        &mut self,
        expected_blob_hash: CanonicalHash,
    ) -> Result<MutationDurability, IntentStoreError> {
        self.move_exact(
            ChildExitDisposition::Pending,
            ChildExitDisposition::Consumed,
            expected_blob_hash,
        )
    }

    fn quarantine(
        &mut self,
        expected_blob_hash: CanonicalHash,
    ) -> Result<MutationDurability, IntentStoreError> {
        self.move_exact(
            ChildExitDisposition::Pending,
            ChildExitDisposition::Quarantined,
            expected_blob_hash,
        )
    }
}

fn child_exit_files() -> [(ChildExitDisposition, &'static str); 3] {
    [
        (ChildExitDisposition::Pending, CHILD_EXIT_PENDING_FILE),
        (ChildExitDisposition::Consumed, CHILD_EXIT_CONSUMED_FILE),
        (
            ChildExitDisposition::Quarantined,
            CHILD_EXIT_QUARANTINED_FILE,
        ),
    ]
}

fn child_exit_file_for(state: ChildExitDisposition) -> &'static str {
    match state {
        ChildExitDisposition::Pending => CHILD_EXIT_PENDING_FILE,
        ChildExitDisposition::Consumed => CHILD_EXIT_CONSUMED_FILE,
        ChildExitDisposition::Quarantined => CHILD_EXIT_QUARANTINED_FILE,
    }
}

fn open_confined_root(root: impl AsRef<Path>) -> Result<PathBuf, IntentStoreError> {
    let root = root.as_ref();
    if !root.is_absolute()
        || contains_literal_dot_segment(root)
        || root.components().any(|component| {
            matches!(component, Component::CurDir | Component::ParentDir)
                || matches!(component.as_os_str().to_str(), Some("." | ".."))
        })
    {
        return Err(IntentStoreError::UnsafeRoot);
    }
    reject_symlinked_components(root)?;
    let metadata = fs::symlink_metadata(root)
        .map_err(|source| IntentStoreError::io(StoreOperation::Read, source))?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(IntentStoreError::UnsafeRoot);
    }
    let canonical = fs::canonicalize(root)
        .map_err(|source| IntentStoreError::io(StoreOperation::Read, source))?;
    if canonical != root {
        return Err(IntentStoreError::UnsafeRoot);
    }
    Ok(canonical)
}

fn sync_confined_directory(root: &Path) -> Result<(), IntentStoreError> {
    #[cfg(unix)]
    {
        File::open(root)
            .and_then(|directory| directory.sync_all())
            .map_err(|source| IntentStoreError::io(StoreOperation::SyncDirectory, source))
    }
    #[cfg(windows)]
    {
        const FILE_WRITE_DATA: u32 = 0x0000_0002;
        const FILE_SHARE_READ: u32 = 0x0000_0001;
        const FILE_SHARE_WRITE: u32 = 0x0000_0002;
        const FILE_SHARE_DELETE: u32 = 0x0000_0004;
        const FILE_FLAG_BACKUP_SEMANTICS: u32 = 0x0200_0000;

        OpenOptions::new()
            .access_mode(FILE_WRITE_DATA)
            .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE)
            .custom_flags(FILE_FLAG_BACKUP_SEMANTICS)
            .open(root)
            .and_then(|directory| directory.sync_all())
            .map_err(|source| IntentStoreError::io(StoreOperation::SyncDirectory, source))
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = root;
        Err(IntentStoreError::io(
            StoreOperation::SyncDirectory,
            io::Error::new(
                io::ErrorKind::Unsupported,
                "directory flush semantics are unsupported on this platform",
            ),
        ))
    }
}

fn publish_result(durability: MutationDurability) -> PublishDisposition {
    match durability {
        MutationDurability::Durable => PublishDisposition::Published,
        MutationDurability::Indeterminate => PublishDisposition::PublishedIndeterminate,
    }
}

fn least_durable(left: MutationDurability, right: MutationDurability) -> MutationDurability {
    if left == MutationDurability::Indeterminate || right == MutationDurability::Indeterminate {
        MutationDurability::Indeterminate
    } else {
        MutationDurability::Durable
    }
}

fn slot_files() -> [(SlotDisposition, &'static str); 5] {
    [
        (SlotDisposition::Pending, PENDING_FILE),
        (SlotDisposition::Claimed, CLAIMED_FILE),
        (SlotDisposition::Consumed, CONSUMED_FILE),
        (SlotDisposition::Quarantined, QUARANTINED_FILE),
        (SlotDisposition::RecoveryClaimed, RECOVERY_CLAIMED_FILE),
    ]
}

fn predecessor_files() -> [(SlotDisposition, &'static str); 3] {
    [
        (SlotDisposition::Consumed, PREDECESSOR_CONSUMED_FILE),
        (SlotDisposition::Quarantined, PREDECESSOR_QUARANTINED_FILE),
        (SlotDisposition::RecoveryClaimed, PREDECESSOR_RECOVERY_FILE),
    ]
}

fn file_for(state: SlotDisposition) -> &'static str {
    match state {
        SlotDisposition::Pending => PENDING_FILE,
        SlotDisposition::Claimed => CLAIMED_FILE,
        SlotDisposition::Consumed => CONSUMED_FILE,
        SlotDisposition::Quarantined => QUARANTINED_FILE,
        SlotDisposition::RecoveryClaimed => RECOVERY_CLAIMED_FILE,
    }
}

fn predecessor_file_for(state: SlotDisposition) -> &'static str {
    match state {
        SlotDisposition::Consumed => PREDECESSOR_CONSUMED_FILE,
        SlotDisposition::Pending | SlotDisposition::Claimed | SlotDisposition::Quarantined => {
            PREDECESSOR_QUARANTINED_FILE
        }
        SlotDisposition::RecoveryClaimed => PREDECESSOR_RECOVERY_FILE,
    }
}

fn contains_literal_dot_segment(path: &Path) -> bool {
    path.as_os_str()
        .to_string_lossy()
        .split(['/', '\\'])
        .any(|segment| matches!(segment, "." | ".."))
}

fn reject_symlinked_components(path: &Path) -> Result<(), IntentStoreError> {
    let mut current = PathBuf::new();
    for component in path.components() {
        current.push(component.as_os_str());
        if matches!(component, Component::Prefix(_) | Component::RootDir) {
            continue;
        }
        match fs::symlink_metadata(&current) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return Err(IntentStoreError::UnsafeRoot);
            }
            Ok(_) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                return Err(IntentStoreError::UnsafeRoot);
            }
            Err(source) => {
                return Err(IntentStoreError::io(StoreOperation::Read, source));
            }
        }
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicU64, Ordering};

    use super::*;

    static NEXT_TEMP: AtomicU64 = AtomicU64::new(1);
    const INTENT: &[u8] = br#"{"schema_version":1}"#;
    const NEXT_INTENT: &[u8] = br#"{"schema_version":1,"generation":2}"#;
    const RECOVERY: &[u8] = br#"{"schema_version":1,"kind":"recovery","checksum":"test"}"#;

    #[derive(Debug)]
    struct TestDirectory(PathBuf);

    impl TestDirectory {
        fn create() -> Self {
            let serial = NEXT_TEMP.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!(
                "latticeaxiom-launcher-{}-{serial}",
                std::process::id()
            ));
            fs::create_dir(&path)
                .unwrap_or_else(|error| panic!("test directory was not created: {error}"));
            Self(
                fs::canonicalize(path)
                    .unwrap_or_else(|error| panic!("test directory did not canonicalize: {error}")),
            )
        }
    }

    impl Drop for TestDirectory {
        fn drop(&mut self) {
            let temporary_root = fs::canonicalize(std::env::temp_dir())
                .unwrap_or_else(|error| panic!("temporary root did not canonicalize: {error}"));
            assert!(self.0.starts_with(&temporary_root));
            if let Err(error) = fs::remove_dir_all(&self.0)
                && error.kind() != io::ErrorKind::NotFound
            {
                panic!("test directory cleanup failed: {error}");
            }
        }
    }

    fn open(directory: &TestDirectory) -> FileLaunchIntentStore {
        FileLaunchIntentStore::open(&directory.0)
            .unwrap_or_else(|error| panic!("store did not open: {error}"))
    }

    fn assert_published(result: &Result<PublishDisposition, IntentStoreError>) {
        assert!(matches!(
            result,
            Ok(PublishDisposition::Published | PublishDisposition::PublishedIndeterminate)
        ));
    }

    fn restart(directory: &TestDirectory, state: SlotDisposition, bytes: &[u8]) -> IntentSlot {
        let mut store = open(directory);
        let slot = store
            .read()
            .unwrap_or_else(|error| panic!("restarted store did not read: {error}"));
        assert_eq!(slot.disposition(), Some(state));
        assert_eq!(slot.bytes(), Some(bytes));
        slot
    }

    fn consumed(directory: &TestDirectory) -> (FileLaunchIntentStore, CanonicalHash) {
        let mut store = open(directory);
        let hash = CanonicalHash::digest(INTENT);
        assert_published(&store.publish(INTENT));
        assert!(store.claim(hash).is_ok());
        assert!(store.consume(hash).is_ok());
        (store, hash)
    }

    #[cfg(windows)]
    #[test]
    fn windows_directory_flush_makes_claim_and_recovery_durable() {
        let transition_directory = TestDirectory::create();
        let mut transition = open(&transition_directory);
        let intent_hash = CanonicalHash::digest(INTENT);
        assert_eq!(
            transition.publish(INTENT).ok(),
            Some(PublishDisposition::Published)
        );
        assert_eq!(
            transition.claim(intent_hash).ok(),
            Some(MutationDurability::Durable)
        );
        assert_eq!(
            transition.consume(intent_hash).ok(),
            Some(MutationDurability::Durable)
        );

        let recovery_directory = TestDirectory::create();
        let (mut recovery, source_hash) = consumed(&recovery_directory);
        let outcome = recovery
            .claim_recovery(Some(source_hash), RECOVERY)
            .unwrap_or_else(|error| panic!("recovery was not durably claimed: {error}"));
        assert_eq!(outcome.durability(), MutationDurability::Durable);
        let recovered = restart(
            &recovery_directory,
            SlotDisposition::RecoveryClaimed,
            RECOVERY,
        );
        assert!(recovered.predecessor().is_none());
    }

    #[test]
    fn publish_claim_consume_and_quarantine_are_bounded() {
        let directory = TestDirectory::create();
        let mut store = open(&directory);
        let hash = CanonicalHash::digest(INTENT);
        assert_published(&store.publish(INTENT));
        assert_eq!(
            store.publish(INTENT).ok(),
            Some(PublishDisposition::AlreadyPublished)
        );
        assert!(store.claim(hash).is_ok());
        assert!(store.consume(hash).is_ok());
        assert!(store.quarantine(hash).is_ok());
        assert_eq!(
            store.publish(INTENT).ok(),
            Some(PublishDisposition::AlreadyHandled(
                SlotDisposition::Quarantined
            ))
        );
    }

    #[test]
    fn replacement_retains_exact_terminal_until_claim() {
        let directory = TestDirectory::create();
        let (mut store, old_hash) = consumed(&directory);
        assert_published(&store.publish_replacing_terminal(old_hash, NEXT_INTENT));
        assert_eq!(
            store.publish_replacing_terminal(old_hash, NEXT_INTENT).ok(),
            Some(PublishDisposition::AlreadyPublished)
        );
        assert!(matches!(
            store.publish_replacing_terminal(CanonicalHash::digest(b"wrong"), NEXT_INTENT),
            Err(IntentStoreError::BlobMismatch)
        ));
        let pending = store
            .read()
            .unwrap_or_else(|error| panic!("pending replacement did not read: {error}"));
        assert_eq!(pending.disposition(), Some(SlotDisposition::Pending));
        assert_eq!(
            pending.predecessor().map(TerminalPredecessor::blob_hash),
            Some(old_hash)
        );

        let next_hash = CanonicalHash::digest(NEXT_INTENT);
        assert!(store.claim(next_hash).is_ok());
        let claimed = restart(&directory, SlotDisposition::Claimed, NEXT_INTENT);
        #[cfg(any(unix, windows))]
        assert!(claimed.predecessor().is_none());
    }

    #[test]
    fn quarantine_preserves_authenticated_predecessor_watermark() {
        let directory = TestDirectory::create();
        let (mut store, old_hash) = consumed(&directory);
        assert_published(&store.publish_replacing_terminal(old_hash, NEXT_INTENT));
        let next_hash = CanonicalHash::digest(NEXT_INTENT);
        assert!(store.quarantine(next_hash).is_ok());
        let quarantined = restart(&directory, SlotDisposition::Quarantined, NEXT_INTENT);
        assert_eq!(
            quarantined
                .predecessor()
                .map(TerminalPredecessor::blob_hash),
            Some(old_hash)
        );
    }

    #[test]
    fn recovery_claim_persists_exact_versioned_request_bytes() {
        let directory = TestDirectory::create();
        let mut store = open(&directory);
        let outcome = store
            .claim_recovery(None, RECOVERY)
            .unwrap_or_else(|error| panic!("recovery was not claimed: {error}"));
        assert_eq!(outcome.blob_hash(), CanonicalHash::digest(RECOVERY));
        let slot = restart(&directory, SlotDisposition::RecoveryClaimed, RECOVERY);
        assert!(slot.predecessor().is_none());
    }

    #[test]
    fn recovery_claim_accepts_exact_consumed_or_quarantined_source() {
        for quarantine_source in [false, true] {
            let directory = TestDirectory::create();
            let (mut store, source_hash) = consumed(&directory);
            if quarantine_source {
                assert!(store.quarantine(source_hash).is_ok());
            }
            let outcome = store
                .claim_recovery(Some(source_hash), RECOVERY)
                .unwrap_or_else(|error| panic!("terminal recovery was not claimed: {error}"));
            assert_eq!(outcome.blob_hash(), CanonicalHash::digest(RECOVERY));
            let _ = restart(&directory, SlotDisposition::RecoveryClaimed, RECOVERY);
        }
    }

    #[test]
    fn recovery_residue_does_not_poison_a_newer_pending_handoff() {
        let directory = TestDirectory::create();
        let (mut store, source_hash) = consumed(&directory);
        let recovery = store
            .claim_recovery(Some(source_hash), RECOVERY)
            .unwrap_or_else(|error| panic!("recovery was not claimed: {error}"));
        assert_published(&store.publish_replacing_terminal(recovery.blob_hash(), NEXT_INTENT));
        let pending = restart(&directory, SlotDisposition::Pending, NEXT_INTENT);
        assert_eq!(
            pending.predecessor().map(TerminalPredecessor::blob_hash),
            Some(recovery.blob_hash())
        );
    }

    #[test]
    fn post_rename_sync_faults_reconcile_all_mutations() {
        let publish_dir = TestDirectory::create();
        let mut publish = open(&publish_dir);
        publish.fail_directory_sync_after(0);
        assert_eq!(
            publish.publish(INTENT).ok(),
            Some(PublishDisposition::PublishedIndeterminate)
        );
        let _ = restart(&publish_dir, SlotDisposition::Pending, INTENT);

        let claim_dir = TestDirectory::create();
        let mut claim = open(&claim_dir);
        let hash = CanonicalHash::digest(INTENT);
        assert_published(&claim.publish(INTENT));
        claim.fail_directory_sync_after(0);
        assert_eq!(
            claim.claim(hash).ok(),
            Some(MutationDurability::Indeterminate)
        );
        let _ = restart(&claim_dir, SlotDisposition::Claimed, INTENT);

        let consume_dir = TestDirectory::create();
        let mut consume = open(&consume_dir);
        assert_published(&consume.publish(INTENT));
        assert!(consume.claim(hash).is_ok());
        consume.fail_directory_sync_after(0);
        assert_eq!(
            consume.consume(hash).ok(),
            Some(MutationDurability::Indeterminate)
        );
        let _ = restart(&consume_dir, SlotDisposition::Consumed, INTENT);

        let quarantine_dir = TestDirectory::create();
        let mut quarantine = open(&quarantine_dir);
        assert_published(&quarantine.publish(INTENT));
        quarantine.fail_directory_sync_after(0);
        assert_eq!(
            quarantine.quarantine(hash).ok(),
            Some(MutationDurability::Indeterminate)
        );
        let _ = restart(&quarantine_dir, SlotDisposition::Quarantined, INTENT);

        let replace_dir = TestDirectory::create();
        let (mut replace, predecessor_hash) = consumed(&replace_dir);
        replace.fail_directory_sync_after(0);
        assert_eq!(
            replace
                .publish_replacing_terminal(predecessor_hash, NEXT_INTENT)
                .ok(),
            Some(PublishDisposition::PublishedIndeterminate)
        );
        let replaced = restart(&replace_dir, SlotDisposition::Pending, NEXT_INTENT);
        assert_eq!(
            replaced.predecessor().map(TerminalPredecessor::blob_hash),
            Some(predecessor_hash)
        );

        let recovery_dir = TestDirectory::create();
        let mut recovery = open(&recovery_dir);
        recovery.fail_directory_sync_after(0);
        let outcome = recovery
            .claim_recovery(None, RECOVERY)
            .unwrap_or_else(|error| panic!("recovery was not reconciled: {error}"));
        assert_eq!(outcome.durability(), MutationDurability::Indeterminate);
        let _ = restart(&recovery_dir, SlotDisposition::RecoveryClaimed, RECOVERY);
    }

    #[cfg(any(unix, windows))]
    #[test]
    fn post_delete_sync_fault_restarts_at_authoritative_destination() {
        let directory = TestDirectory::create();
        let (mut store, old_hash) = consumed(&directory);
        assert_published(&store.publish_replacing_terminal(old_hash, NEXT_INTENT));
        store.fail_directory_sync_after(1);
        let next_hash = CanonicalHash::digest(NEXT_INTENT);
        assert_eq!(
            store.claim(next_hash).ok(),
            Some(MutationDurability::Indeterminate)
        );
        let slot = restart(&directory, SlotDisposition::Claimed, NEXT_INTENT);
        assert!(slot.predecessor().is_none());

        let recovery_directory = TestDirectory::create();
        let (mut recovery, source_hash) = consumed(&recovery_directory);
        recovery.fail_directory_sync_after(1);
        let outcome = recovery
            .claim_recovery(Some(source_hash), RECOVERY)
            .unwrap_or_else(|error| panic!("recovery cleanup was not reconciled: {error}"));
        assert_eq!(outcome.durability(), MutationDurability::Indeterminate);
        let recovered = restart(
            &recovery_directory,
            SlotDisposition::RecoveryClaimed,
            RECOVERY,
        );
        assert!(recovered.predecessor().is_none());
    }

    #[test]
    fn child_exit_publish_consume_and_quarantine_are_one_shot() {
        let directory = TestDirectory::create();
        let mut store = FileChildExitStore::open(&directory.0)
            .unwrap_or_else(|error| panic!("child-exit store did not open: {error}"));
        let hash = CanonicalHash::digest(INTENT);
        assert_published(&store.publish(INTENT));
        assert_eq!(
            store.publish(INTENT).ok(),
            Some(PublishDisposition::AlreadyPublished)
        );
        assert!(store.consume(hash).is_ok());
        assert_eq!(
            store.read().ok().and_then(|slot| slot.disposition()),
            Some(ChildExitDisposition::Consumed)
        );
        assert_published(&store.publish(NEXT_INTENT));
        let next_hash = CanonicalHash::digest(NEXT_INTENT);
        assert!(store.quarantine(next_hash).is_ok());
        assert_eq!(
            store.read().ok().and_then(|slot| slot.disposition()),
            Some(ChildExitDisposition::Quarantined)
        );
    }

    #[test]
    fn relative_and_dot_segment_roots_are_rejected() {
        assert!(matches!(
            FileLaunchIntentStore::open("relative"),
            Err(IntentStoreError::UnsafeRoot)
        ));
        let directory = TestDirectory::create();
        let leaf = directory
            .0
            .file_name()
            .unwrap_or_else(|| panic!("test directory has no final component"));
        let with_dot = PathBuf::from(format!(
            r"{}\..\{}",
            directory.0.display(),
            leaf.to_string_lossy()
        ));
        assert!(matches!(
            FileLaunchIntentStore::open(with_dot),
            Err(IntentStoreError::UnsafeRoot)
        ));
    }

    #[cfg(unix)]
    #[test]
    fn symlink_slot_cannot_escape_the_root() {
        use std::os::unix::fs::symlink;

        let directory = TestDirectory::create();
        let outside = directory.0.with_extension("outside");
        fs::write(&outside, b"secret")
            .unwrap_or_else(|error| panic!("outside fixture was not written: {error}"));
        symlink(&outside, directory.0.join(PENDING_FILE))
            .unwrap_or_else(|error| panic!("symlink fixture was not created: {error}"));
        let mut store = open(&directory);
        assert!(matches!(
            store.read(),
            Err(IntentStoreError::UnsafeSlot { .. })
        ));
        fs::remove_file(outside)
            .unwrap_or_else(|error| panic!("outside fixture cleanup failed: {error}"));
    }
}
