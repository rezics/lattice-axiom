//! Canonical local-settings envelope and old-or-complete-new file protocol.

use std::collections::BTreeMap;
#[cfg(windows)]
use std::fs::OpenOptions;
use std::fs::{self, File};
use std::io::{self, Write};
#[cfg(windows)]
use std::os::windows::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use latticeaxiom_compose::SettingScope;
use latticeaxiom_core::{CanonicalHash, CanonicalJsonError, StableId, canonical_json_bytes};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;

use super::binding::BindingProfileV1;
use super::overlay::{ScopeOverlay, SettingTransactionRevision, SettingWriter, StoreRevision};

/// Canonical schema identity for [`LatticeLocalSettingsV1`].
pub const LOCAL_SETTINGS_SCHEMA: &str = "latticeaxiom:schema/local-settings@1";

/// Supported local-settings schema version.
pub const LOCAL_SETTINGS_SCHEMA_VERSION: u32 = 1;

/// Visible local-settings file name.
pub const LOCAL_SETTINGS_FILE_NAME: &str = "local-settings.v1.json";

/// Sibling temporary file used by the publish protocol.
pub const LOCAL_SETTINGS_TEMPORARY_FILE_NAME: &str = ".local-settings.v1.json.tmp";

/// Maximum accepted local-settings file size.
pub const MAX_LOCAL_SETTINGS_BYTES: usize = 1024 * 1024;

/// One stored device or user value.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct StoredSettingEntryV1 {
    schema_version: u32,
    value: Value,
}

impl StoredSettingEntryV1 {
    /// Creates one stored entry.
    #[must_use]
    pub const fn new(schema_version: u32, value: Value) -> Self {
        Self {
            schema_version,
            value,
        }
    }

    /// Returns the owner-controlled value schema version.
    #[must_use]
    pub const fn schema_version(&self) -> u32 {
        self.schema_version
    }

    /// Returns the stored tagged JSON value.
    #[must_use]
    pub const fn value(&self) -> &Value {
        &self.value
    }
}

/// Process-restart journal retained until the next boot confirms it.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PendingRestartJournalV1 {
    generation: u64,
    intended_transaction_revision: SettingTransactionRevision,
    intended_user: BTreeMap<StableId, StoredSettingEntryV1>,
    intended_device: BTreeMap<StableId, StoredSettingEntryV1>,
    intended_binding_profile: BindingProfileV1,
}

impl PendingRestartJournalV1 {
    /// Creates a pending journal for one `ProcessRestart` transaction.
    #[must_use]
    pub fn new(
        generation: u64,
        intended_transaction_revision: SettingTransactionRevision,
        intended_user: BTreeMap<StableId, StoredSettingEntryV1>,
        intended_device: BTreeMap<StableId, StoredSettingEntryV1>,
        intended_binding_profile: BindingProfileV1,
    ) -> Self {
        Self {
            generation,
            intended_transaction_revision,
            intended_user,
            intended_device,
            intended_binding_profile,
        }
    }

    /// Returns the journal generation.
    #[must_use]
    pub const fn generation(&self) -> u64 {
        self.generation
    }

    /// Returns the transaction revision that becomes confirmed after boot.
    #[must_use]
    pub const fn intended_transaction_revision(&self) -> SettingTransactionRevision {
        self.intended_transaction_revision
    }
}

/// Versioned device/user envelope persisted by the local-settings protocol.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct LatticeLocalSettingsV1 {
    schema: String,
    schema_version: u32,
    store_revision: StoreRevision,
    transaction_revision: SettingTransactionRevision,
    writer: SettingWriter,
    device: BTreeMap<StableId, StoredSettingEntryV1>,
    user: BTreeMap<StableId, StoredSettingEntryV1>,
    binding_profile: BindingProfileV1,
    pending_restart: Option<PendingRestartJournalV1>,
}

impl LatticeLocalSettingsV1 {
    /// Creates an empty confirmed envelope at revision zero.
    #[must_use]
    pub fn empty() -> Self {
        Self {
            schema: LOCAL_SETTINGS_SCHEMA.to_owned(),
            schema_version: LOCAL_SETTINGS_SCHEMA_VERSION,
            store_revision: StoreRevision::new(0),
            transaction_revision: SettingTransactionRevision::new(0),
            writer: SettingWriter::LocalUser,
            device: BTreeMap::new(),
            user: BTreeMap::new(),
            binding_profile: BindingProfileV1::empty(),
            pending_restart: None,
        }
    }

    /// Returns the store revision.
    #[must_use]
    pub const fn store_revision(&self) -> StoreRevision {
        self.store_revision
    }

    /// Returns the last confirmed transaction revision.
    #[must_use]
    pub const fn transaction_revision(&self) -> SettingTransactionRevision {
        self.transaction_revision
    }

    /// Returns device-scope stored values.
    #[must_use]
    pub const fn device(&self) -> &BTreeMap<StableId, StoredSettingEntryV1> {
        &self.device
    }

    /// Returns user-scope stored values.
    #[must_use]
    pub const fn user(&self) -> &BTreeMap<StableId, StoredSettingEntryV1> {
        &self.user
    }

    /// Returns the persisted user binding profile.
    #[must_use]
    pub const fn binding_profile(&self) -> &BindingProfileV1 {
        &self.binding_profile
    }

    /// Returns the pending restart journal, if any.
    #[must_use]
    pub const fn pending_restart(&self) -> Option<&PendingRestartJournalV1> {
        self.pending_restart.as_ref()
    }

    /// Returns the device overlay for effective-value resolution.
    #[must_use]
    pub fn device_overlay(&self) -> ScopeOverlay {
        scope_overlay(SettingScope::Device, self, &self.device)
    }

    /// Returns the user overlay for effective-value resolution.
    #[must_use]
    pub fn user_overlay(&self) -> ScopeOverlay {
        scope_overlay(SettingScope::User, self, &self.user)
    }

    /// Replaces user-scope values and the binding profile in one revision bump.
    #[must_use]
    pub fn with_user_commit(
        &self,
        user: BTreeMap<StableId, StoredSettingEntryV1>,
        binding_profile: BindingProfileV1,
    ) -> Self {
        Self {
            schema: LOCAL_SETTINGS_SCHEMA.to_owned(),
            schema_version: LOCAL_SETTINGS_SCHEMA_VERSION,
            store_revision: self.store_revision.saturating_next(),
            transaction_revision: self.transaction_revision.saturating_next(),
            writer: SettingWriter::LocalUser,
            device: self.device.clone(),
            user,
            binding_profile,
            pending_restart: None,
        }
    }

    /// Stages a `ProcessRestart` journal without replacing the confirmed snapshot.
    #[must_use]
    pub fn with_pending_restart(&self, journal: PendingRestartJournalV1) -> Self {
        Self {
            pending_restart: Some(journal),
            store_revision: self.store_revision.saturating_next(),
            ..self.clone()
        }
    }

    /// Confirms a pending journal, replacing the durable snapshot.
    #[must_use]
    pub fn confirm_pending_restart(&self) -> Self {
        let Some(journal) = &self.pending_restart else {
            return self.clone();
        };
        Self {
            schema: LOCAL_SETTINGS_SCHEMA.to_owned(),
            schema_version: LOCAL_SETTINGS_SCHEMA_VERSION,
            store_revision: self.store_revision.saturating_next(),
            transaction_revision: journal.intended_transaction_revision,
            writer: SettingWriter::LocalUser,
            device: journal.intended_device.clone(),
            user: journal.intended_user.clone(),
            binding_profile: journal.intended_binding_profile.clone(),
            pending_restart: None,
        }
    }

    /// Drops a pending journal and keeps the confirmed snapshot.
    #[must_use]
    pub fn reject_pending_restart(&self) -> Self {
        Self {
            pending_restart: None,
            ..self.clone()
        }
    }

    /// Encodes the envelope as recursively key-sorted compact JSON.
    ///
    /// # Errors
    ///
    /// Returns [`CanonicalJsonError`] if serialization fails.
    pub fn canonical_bytes(&self) -> Result<Vec<u8>, CanonicalJsonError> {
        canonical_json_bytes(self)
    }
}

fn scope_overlay(
    scope: SettingScope,
    envelope: &LatticeLocalSettingsV1,
    values: &BTreeMap<StableId, StoredSettingEntryV1>,
) -> ScopeOverlay {
    ScopeOverlay::new(
        scope,
        envelope.store_revision,
        envelope.transaction_revision,
        envelope.writer,
        None,
        values
            .iter()
            .map(|(id, entry)| (id.clone(), entry.value.clone()))
            .collect(),
    )
}

/// Why a visible file was isolated instead of being overwritten.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum IsolationReason {
    /// The file was not valid canonical JSON for this schema.
    Corrupt {
        /// Decoder diagnostic.
        detail: String,
    },
    /// The file required a newer schema than this implementation supports.
    NewerRequired {
        /// Version found on disk.
        found: u32,
        /// Highest version this implementation can read.
        supported: u32,
    },
}

/// Origin of an in-memory envelope after load.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum LocalSettingsOrigin {
    /// No complete file existed.
    Missing,
    /// An old-complete or new-complete file was accepted.
    Complete,
    /// The original file was isolated and defaults were used in memory only.
    Isolated {
        /// Why the original file was preserved aside.
        reason: IsolationReason,
        /// Stable isolation identity.
        isolated_id: String,
    },
}

/// Result of reading the local-settings store.
#[derive(Clone, Debug, PartialEq)]
pub struct LocalSettingsLoad {
    envelope: LatticeLocalSettingsV1,
    origin: LocalSettingsOrigin,
}

impl LocalSettingsLoad {
    /// Returns the confirmed envelope. Isolated loads are in-memory defaults.
    #[must_use]
    pub const fn envelope(&self) -> &LatticeLocalSettingsV1 {
        &self.envelope
    }

    /// Returns how the envelope was obtained.
    #[must_use]
    pub const fn origin(&self) -> &LocalSettingsOrigin {
        &self.origin
    }
}

/// Evidence returned after a complete publication.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LocalSettingsPublishReceipt {
    /// Canonical hash of the published bytes.
    pub content_hash: CanonicalHash,
    /// Number of canonical bytes published.
    pub published_bytes: usize,
}

/// One-shot failure locations in the atomic replacement protocol.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LocalSettingsFaultPoint {
    /// Fail after writing the temporary file.
    TempWrite,
    /// Fail while synchronizing the temporary file.
    FileSync,
    /// Fail while replacing the visible file.
    Replace,
    /// Fail while synchronizing the containing directory.
    DirectorySync,
}

/// Successfully reached publication stages.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LocalSettingsPublishStage {
    /// Temporary bytes were written.
    TempWritten,
    /// Temporary-file contents were synchronized.
    FileSynced,
    /// The visible name was atomically replaced.
    Replaced,
    /// The containing directory was synchronized.
    DirectorySynced,
}

/// Failure to decode, isolate, or publish local settings.
#[derive(Clone, Debug, Eq, Error, PartialEq)]
pub enum LocalSettingsPersistError {
    /// Canonical encoding failed.
    #[error("local settings canonical encoding failed: {0}")]
    Canonical(String),
    /// The visible file exceeded the caller's read bound.
    #[error("local settings file has {actual} bytes; read limit is {max}")]
    ReadLimitExceeded {
        /// Visible byte count.
        actual: usize,
        /// Accepted maximum.
        max: usize,
    },
    /// A deterministic one-shot fault was reached.
    #[error("injected local-settings publication fault at {0:?}")]
    Injected(LocalSettingsFaultPoint),
    /// Internal deterministic state was poisoned by a panic.
    #[error("local-settings publisher state is poisoned")]
    StatePoisoned,
    /// Filesystem mutation failed.
    #[error("local settings I/O failed for `{path}`: {reason}")]
    Io {
        /// Path that failed.
        path: String,
        /// Operating-system diagnostic.
        reason: String,
    },
}

/// Canonical local-settings persistence protocol.
pub trait LocalSettingsStore {
    /// Reads the currently visible envelope, isolating corrupt/newer files.
    ///
    /// # Errors
    ///
    /// Returns [`LocalSettingsPersistError`] for I/O, poisoned state, or an
    /// oversized visible file.
    fn load(&self) -> Result<LocalSettingsLoad, LocalSettingsPersistError>;

    /// Publishes a prepared envelope atomically.
    ///
    /// # Errors
    ///
    /// Returns [`LocalSettingsPersistError`] at an injected fault or I/O error.
    fn persist(
        &self,
        envelope: &LatticeLocalSettingsV1,
    ) -> Result<LocalSettingsPublishReceipt, LocalSettingsPersistError>;
}

/// Prepared canonical bytes for one envelope.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PreparedLocalSettingsV1 {
    canonical_bytes: Vec<u8>,
    content_hash: CanonicalHash,
}

impl PreparedLocalSettingsV1 {
    /// Canonically encodes an envelope.
    ///
    /// # Errors
    ///
    /// Returns [`LocalSettingsPersistError::Canonical`] if serialization fails.
    pub fn new(envelope: &LatticeLocalSettingsV1) -> Result<Self, LocalSettingsPersistError> {
        let canonical_bytes = envelope
            .canonical_bytes()
            .map_err(|source| LocalSettingsPersistError::Canonical(source.to_string()))?;
        Ok(Self {
            content_hash: CanonicalHash::digest(&canonical_bytes),
            canonical_bytes,
        })
    }

    /// Returns the exact canonical bytes.
    #[must_use]
    pub fn canonical_bytes(&self) -> &[u8] {
        &self.canonical_bytes
    }
}

/// Decodes complete local-settings bytes without mutating storage.
///
/// # Errors
///
/// Returns [`IsolationReason`] for corrupt or newer-required files.
pub fn decode_local_settings(bytes: &[u8]) -> Result<LatticeLocalSettingsV1, IsolationReason> {
    let envelope = serde_json::from_slice::<LatticeLocalSettingsV1>(bytes).map_err(|source| {
        IsolationReason::Corrupt {
            detail: source.to_string(),
        }
    })?;
    if envelope.schema != LOCAL_SETTINGS_SCHEMA {
        return Err(IsolationReason::Corrupt {
            detail: format!("unsupported local-settings schema `{}`", envelope.schema),
        });
    }
    if envelope.schema_version == 0 {
        return Err(IsolationReason::Corrupt {
            detail: "local-settings schema version zero is not a contract".to_owned(),
        });
    }
    if envelope.schema_version > LOCAL_SETTINGS_SCHEMA_VERSION {
        return Err(IsolationReason::NewerRequired {
            found: envelope.schema_version,
            supported: LOCAL_SETTINGS_SCHEMA_VERSION,
        });
    }
    if envelope.binding_profile.schema_version() != 1 {
        return Err(IsolationReason::Corrupt {
            detail: format!(
                "unsupported binding-profile schema version {}",
                envelope.binding_profile.schema_version()
            ),
        });
    }
    Ok(envelope)
}

#[derive(Debug, Default)]
struct PublisherState {
    visible: Option<Vec<u8>>,
    temporary: Option<Vec<u8>>,
    isolated: BTreeMap<String, Vec<u8>>,
    fault: Option<LocalSettingsFaultPoint>,
    trace: Vec<LocalSettingsPublishStage>,
}

/// In-memory, deterministic oracle for the four-stage publication protocol.
#[derive(Debug, Default)]
pub struct DeterministicLocalSettingsStore {
    state: Mutex<PublisherState>,
}

impl DeterministicLocalSettingsStore {
    /// Creates an empty store.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Arms a fault consumed by the next publication that reaches it.
    ///
    /// # Errors
    ///
    /// Returns an error if the publisher state lock is poisoned.
    pub fn inject_fault(
        &self,
        fault: LocalSettingsFaultPoint,
    ) -> Result<(), LocalSettingsPersistError> {
        self.state
            .lock()
            .map_err(|_| LocalSettingsPersistError::StatePoisoned)?
            .fault = Some(fault);
        Ok(())
    }

    /// Returns an owned copy of the most recent publication trace.
    ///
    /// # Errors
    ///
    /// Returns an error if the publisher state lock is poisoned.
    pub fn trace(&self) -> Result<Vec<LocalSettingsPublishStage>, LocalSettingsPersistError> {
        Ok(self
            .state
            .lock()
            .map_err(|_| LocalSettingsPersistError::StatePoisoned)?
            .trace
            .clone())
    }

    /// Installs visible bytes without going through persist, for load fixtures.
    ///
    /// # Errors
    ///
    /// Returns an error if the publisher state lock is poisoned.
    pub fn install_visible(&self, bytes: Vec<u8>) -> Result<(), LocalSettingsPersistError> {
        self.state
            .lock()
            .map_err(|_| LocalSettingsPersistError::StatePoisoned)?
            .visible = Some(bytes);
        Ok(())
    }

    /// Installs leftover temporary bytes without replacing the visible file.
    ///
    /// # Errors
    ///
    /// Returns an error if the publisher state lock is poisoned.
    pub fn install_temporary(&self, bytes: Vec<u8>) -> Result<(), LocalSettingsPersistError> {
        self.state
            .lock()
            .map_err(|_| LocalSettingsPersistError::StatePoisoned)?
            .temporary = Some(bytes);
        Ok(())
    }

    /// Returns isolated files keyed by isolation identity.
    ///
    /// # Errors
    ///
    /// Returns an error if the publisher state lock is poisoned.
    pub fn isolated(&self) -> Result<BTreeMap<String, Vec<u8>>, LocalSettingsPersistError> {
        Ok(self
            .state
            .lock()
            .map_err(|_| LocalSettingsPersistError::StatePoisoned)?
            .isolated
            .clone())
    }
}

impl LocalSettingsStore for DeterministicLocalSettingsStore {
    fn load(&self) -> Result<LocalSettingsLoad, LocalSettingsPersistError> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| LocalSettingsPersistError::StatePoisoned)?;
        if let Some(bytes) = &state.visible
            && bytes.len() > MAX_LOCAL_SETTINGS_BYTES
        {
            return Err(LocalSettingsPersistError::ReadLimitExceeded {
                actual: bytes.len(),
                max: MAX_LOCAL_SETTINGS_BYTES,
            });
        }
        if let Some(bytes) = state.visible.clone() {
            match decode_local_settings(&bytes) {
                Ok(envelope) => {
                    state.temporary = None;
                    Ok(LocalSettingsLoad {
                        envelope,
                        origin: LocalSettingsOrigin::Complete,
                    })
                }
                Err(reason) => {
                    let isolated_id = isolation_id(&reason, &bytes);
                    state.isolated.insert(isolated_id.clone(), bytes);
                    state.visible = None;
                    state.temporary = None;
                    Ok(LocalSettingsLoad {
                        envelope: LatticeLocalSettingsV1::empty(),
                        origin: LocalSettingsOrigin::Isolated {
                            reason,
                            isolated_id,
                        },
                    })
                }
            }
        } else {
            if let Some(temporary) = state.temporary.take() {
                let reason = IsolationReason::Corrupt {
                    detail: "incomplete temporary local-settings file is not new-complete"
                        .to_owned(),
                };
                let isolated_id = isolation_id(&reason, &temporary);
                state.isolated.insert(isolated_id, temporary);
            }
            Ok(LocalSettingsLoad {
                envelope: LatticeLocalSettingsV1::empty(),
                origin: LocalSettingsOrigin::Missing,
            })
        }
    }

    fn persist(
        &self,
        envelope: &LatticeLocalSettingsV1,
    ) -> Result<LocalSettingsPublishReceipt, LocalSettingsPersistError> {
        let prepared = PreparedLocalSettingsV1::new(envelope)?;
        let mut state = self
            .state
            .lock()
            .map_err(|_| LocalSettingsPersistError::StatePoisoned)?;
        state.trace.clear();
        state.temporary = Some(prepared.canonical_bytes.clone());

        for (stage, fault) in [
            (
                LocalSettingsPublishStage::TempWritten,
                LocalSettingsFaultPoint::TempWrite,
            ),
            (
                LocalSettingsPublishStage::FileSynced,
                LocalSettingsFaultPoint::FileSync,
            ),
            (
                LocalSettingsPublishStage::Replaced,
                LocalSettingsFaultPoint::Replace,
            ),
            (
                LocalSettingsPublishStage::DirectorySynced,
                LocalSettingsFaultPoint::DirectorySync,
            ),
        ] {
            state.trace.push(stage);
            if state.fault == Some(fault) {
                state.fault = None;
                return Err(LocalSettingsPersistError::Injected(fault));
            }
            if stage == LocalSettingsPublishStage::Replaced {
                let PublisherState {
                    visible, temporary, ..
                } = &mut *state;
                visible.clone_from(temporary);
            }
        }
        state.temporary = None;
        Ok(LocalSettingsPublishReceipt {
            content_hash: prepared.content_hash,
            published_bytes: prepared.canonical_bytes.len(),
        })
    }
}

/// Filesystem adapter for the canonical local-settings protocol.
#[derive(Debug)]
pub struct FilesystemLocalSettingsStore {
    root: PathBuf,
}

impl FilesystemLocalSettingsStore {
    /// Opens a store in an existing directory.
    ///
    /// # Errors
    ///
    /// Returns [`LocalSettingsPersistError::Io`] when the directory cannot be
    /// created or inspected.
    pub fn open(root: impl AsRef<Path>) -> Result<Self, LocalSettingsPersistError> {
        let root = root.as_ref();
        fs::create_dir_all(root).map_err(|source| io_error(root, &source))?;
        Ok(Self {
            root: root.to_path_buf(),
        })
    }

    fn visible_path(&self) -> PathBuf {
        self.root.join(LOCAL_SETTINGS_FILE_NAME)
    }

    fn temporary_path(&self) -> PathBuf {
        self.root.join(LOCAL_SETTINGS_TEMPORARY_FILE_NAME)
    }

    fn isolated_path(&self, isolated_id: &str) -> PathBuf {
        self.root
            .join(format!("local-settings.v1.isolated.{isolated_id}.json"))
    }

    fn isolate_bytes(
        &self,
        path: &Path,
        bytes: &[u8],
        reason: &IsolationReason,
    ) -> Result<String, LocalSettingsPersistError> {
        let isolated_id = isolation_id(reason, bytes);
        let dest = self.isolated_path(&isolated_id);
        match fs::rename(path, &dest) {
            Ok(()) => Ok(isolated_id),
            Err(source) => Err(io_error(&dest, &source)),
        }
    }

    fn read_regular_file(path: &Path) -> Result<Option<Vec<u8>>, LocalSettingsPersistError> {
        match fs::read(path) {
            Ok(bytes) => {
                if bytes.len() > MAX_LOCAL_SETTINGS_BYTES {
                    return Err(LocalSettingsPersistError::ReadLimitExceeded {
                        actual: bytes.len(),
                        max: MAX_LOCAL_SETTINGS_BYTES,
                    });
                }
                Ok(Some(bytes))
            }
            Err(source) if source.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(source) => Err(io_error(path, &source)),
        }
    }

    fn write_temporary(&self, bytes: &[u8]) -> Result<(), LocalSettingsPersistError> {
        let temporary = self.temporary_path();
        match fs::remove_file(&temporary) {
            Ok(()) | Err(_) => {}
        }
        let mut file = File::create(&temporary).map_err(|source| io_error(&temporary, &source))?;
        file.write_all(bytes)
            .map_err(|source| io_error(&temporary, &source))?;
        file.sync_all()
            .map_err(|source| io_error(&temporary, &source))?;
        Ok(())
    }

    fn replace_visible(&self) -> Result<(), LocalSettingsPersistError> {
        let temporary = self.temporary_path();
        let dest = self.visible_path();
        match fs::rename(&temporary, &dest) {
            Ok(()) => Ok(()),
            Err(_) if dest.exists() => replace_existing(&temporary, &dest),
            Err(source) => Err(io_error(&dest, &source)),
        }
    }

    fn sync_directory(&self) -> Result<(), LocalSettingsPersistError> {
        platform_sync_directory(&self.root)
    }
}

impl LocalSettingsStore for FilesystemLocalSettingsStore {
    fn load(&self) -> Result<LocalSettingsLoad, LocalSettingsPersistError> {
        let visible = self.visible_path();
        let temporary = self.temporary_path();
        if let Some(bytes) = Self::read_regular_file(&visible)? {
            match decode_local_settings(&bytes) {
                Ok(envelope) => {
                    let _ = fs::remove_file(&temporary);
                    Ok(LocalSettingsLoad {
                        envelope,
                        origin: LocalSettingsOrigin::Complete,
                    })
                }
                Err(reason) => {
                    let isolated_id = self.isolate_bytes(&visible, &bytes, &reason)?;
                    let _ = fs::remove_file(&temporary);
                    Ok(LocalSettingsLoad {
                        envelope: LatticeLocalSettingsV1::empty(),
                        origin: LocalSettingsOrigin::Isolated {
                            reason,
                            isolated_id,
                        },
                    })
                }
            }
        } else {
            if let Some(bytes) = Self::read_regular_file(&temporary)? {
                let reason = IsolationReason::Corrupt {
                    detail: "incomplete temporary local-settings file is not new-complete"
                        .to_owned(),
                };
                let _ = self.isolate_bytes(&temporary, &bytes, &reason);
            }
            Ok(LocalSettingsLoad {
                envelope: LatticeLocalSettingsV1::empty(),
                origin: LocalSettingsOrigin::Missing,
            })
        }
    }

    fn persist(
        &self,
        envelope: &LatticeLocalSettingsV1,
    ) -> Result<LocalSettingsPublishReceipt, LocalSettingsPersistError> {
        let prepared = PreparedLocalSettingsV1::new(envelope)?;
        self.write_temporary(&prepared.canonical_bytes)?;
        self.replace_visible()?;
        self.sync_directory()?;
        let _ = fs::remove_file(self.root.join(backup_file_name()));
        Ok(LocalSettingsPublishReceipt {
            content_hash: prepared.content_hash,
            published_bytes: prepared.canonical_bytes.len(),
        })
    }
}

fn replace_existing(temp: &Path, dest: &Path) -> Result<(), LocalSettingsPersistError> {
    let backup = dest.with_file_name(backup_file_name());
    let _ = fs::remove_file(&backup);
    fs::rename(dest, &backup).map_err(|source| io_error(dest, &source))?;
    match fs::rename(temp, dest) {
        Ok(()) => {
            let _ = fs::remove_file(&backup);
            Ok(())
        }
        Err(source) => {
            let _ = fs::rename(&backup, dest);
            Err(io_error(dest, &source))
        }
    }
}

fn backup_file_name() -> &'static str {
    ".local-settings.v1.json.bak"
}

fn isolation_id(reason: &IsolationReason, bytes: &[u8]) -> String {
    let kind = match reason {
        IsolationReason::Corrupt { .. } => "corrupt",
        IsolationReason::NewerRequired { .. } => "newer-required",
    };
    let digest = CanonicalHash::digest(bytes);
    let hex = digest.to_string();
    format!("{kind}-{}", &hex[..8.min(hex.len())])
}

fn io_error(path: &Path, source: &io::Error) -> LocalSettingsPersistError {
    LocalSettingsPersistError::Io {
        path: path.display().to_string(),
        reason: source.to_string(),
    }
}

#[cfg(unix)]
fn platform_sync_directory(root: &Path) -> Result<(), LocalSettingsPersistError> {
    File::open(root)
        .and_then(|directory| directory.sync_all())
        .map_err(|source| io_error(root, &source))
}

#[cfg(windows)]
fn platform_sync_directory(root: &Path) -> Result<(), LocalSettingsPersistError> {
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
        .map_err(|source| io_error(root, &source))
}

#[cfg(not(any(unix, windows)))]
fn platform_sync_directory(root: &Path) -> Result<(), LocalSettingsPersistError> {
    Err(LocalSettingsPersistError::Io {
        path: root.display().to_string(),
        reason: "directory flush semantics are unsupported on this platform".to_owned(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT_TEST_DIRECTORY: AtomicU64 = AtomicU64::new(0);

    fn user_envelope(scale: f64) -> LatticeLocalSettingsV1 {
        let mut envelope = LatticeLocalSettingsV1::empty();
        envelope = envelope.with_user_commit(
            BTreeMap::from([(
                match "latticeaxiom:setting/ui-scale".parse() {
                    Ok(id) => id,
                    Err(error) => panic!("ui-scale id: {error}"),
                },
                StoredSettingEntryV1::new(1, serde_json::json!(scale)),
            )]),
            BindingProfileV1::empty(),
        );
        envelope
    }

    #[test]
    fn persist_faults_leave_old_or_new_complete() {
        let store = DeterministicLocalSettingsStore::new();
        let first = user_envelope(1.0);
        match store.persist(&first) {
            Ok(_) => {}
            Err(error) => panic!("first persist failed: {error}"),
        }
        for fault in [
            LocalSettingsFaultPoint::TempWrite,
            LocalSettingsFaultPoint::FileSync,
            LocalSettingsFaultPoint::Replace,
        ] {
            match store.inject_fault(fault) {
                Ok(()) => {}
                Err(error) => panic!("inject failed: {error}"),
            }
            let second = user_envelope(2.0);
            assert!(store.persist(&second).is_err());
            let loaded = match store.load() {
                Ok(value) => value,
                Err(error) => panic!("load after {fault:?} failed: {error}"),
            };
            assert_eq!(loaded.origin(), &LocalSettingsOrigin::Complete);
            assert_eq!(loaded.envelope(), &first);
        }

        match store.inject_fault(LocalSettingsFaultPoint::DirectorySync) {
            Ok(()) => {}
            Err(error) => panic!("inject failed: {error}"),
        }
        let second = user_envelope(2.0);
        assert!(store.persist(&second).is_err());
        let loaded = match store.load() {
            Ok(value) => value,
            Err(error) => panic!("load after directory-sync fault failed: {error}"),
        };
        assert_eq!(loaded.envelope(), &second);
    }

    #[test]
    fn leftover_temporary_does_not_replace_old_complete() {
        let store = DeterministicLocalSettingsStore::new();
        let first = user_envelope(1.0);
        match store.persist(&first) {
            Ok(_) => {}
            Err(error) => panic!("persist failed: {error}"),
        }
        match store.install_temporary(b"{incomplete".to_vec()) {
            Ok(()) => {}
            Err(error) => panic!("install temporary failed: {error}"),
        }
        let loaded = match store.load() {
            Ok(value) => value,
            Err(error) => panic!("load failed: {error}"),
        };
        assert_eq!(loaded.envelope(), &first);
        assert_eq!(loaded.origin(), &LocalSettingsOrigin::Complete);
    }

    #[test]
    fn corrupt_and_newer_files_are_isolated_without_default_overwrite() {
        let store = DeterministicLocalSettingsStore::new();
        match store.install_visible(b"{not-json".to_vec()) {
            Ok(()) => {}
            Err(error) => panic!("install visible failed: {error}"),
        }
        let loaded = match store.load() {
            Ok(value) => value,
            Err(error) => panic!("corrupt load failed: {error}"),
        };
        assert!(matches!(
            loaded.origin(),
            LocalSettingsOrigin::Isolated {
                reason: IsolationReason::Corrupt { .. },
                ..
            }
        ));
        assert_eq!(loaded.envelope(), &LatticeLocalSettingsV1::empty());
        let isolated = match store.isolated() {
            Ok(value) => value,
            Err(error) => panic!("isolated map failed: {error}"),
        };
        assert_eq!(isolated.len(), 1);
        assert_eq!(isolated.values().next(), Some(&b"{not-json".to_vec()));

        let mut newer = serde_json::to_value(LatticeLocalSettingsV1::empty())
            .unwrap_or_else(|error| panic!("encode empty: {error}"));
        newer["schema_version"] = serde_json::json!(2);
        let newer_bytes =
            serde_json::to_vec(&newer).unwrap_or_else(|error| panic!("encode newer: {error}"));
        let store = DeterministicLocalSettingsStore::new();
        match store.install_visible(newer_bytes.clone()) {
            Ok(()) => {}
            Err(error) => panic!("install newer failed: {error}"),
        }
        let loaded = match store.load() {
            Ok(value) => value,
            Err(error) => panic!("newer load failed: {error}"),
        };
        assert!(matches!(
            loaded.origin(),
            LocalSettingsOrigin::Isolated {
                reason: IsolationReason::NewerRequired {
                    found: 2,
                    supported: 1
                },
                ..
            }
        ));
        let isolated = match store.isolated() {
            Ok(value) => value,
            Err(error) => panic!("isolated newer failed: {error}"),
        };
        assert_eq!(isolated.values().next(), Some(&newer_bytes));
    }

    #[test]
    fn filesystem_round_trip_survives_replacement_process_restart() {
        let serial = NEXT_TEST_DIRECTORY.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "latticeaxiom-local-settings-{}-{serial}",
            std::process::id()
        ));
        let store = match FilesystemLocalSettingsStore::open(&path) {
            Ok(value) => value,
            Err(error) => panic!("open failed: {error}"),
        };
        let envelope = user_envelope(2.0);
        match store.persist(&envelope) {
            Ok(_) => {}
            Err(error) => panic!("filesystem persist failed: {error}"),
        }
        let reopened = match FilesystemLocalSettingsStore::open(&path) {
            Ok(value) => value,
            Err(error) => panic!("reopen failed: {error}"),
        };
        let loaded = match reopened.load() {
            Ok(value) => value,
            Err(error) => panic!("filesystem load failed: {error}"),
        };
        assert_eq!(loaded.origin(), &LocalSettingsOrigin::Complete);
        assert_eq!(loaded.envelope(), &envelope);
        let _ = fs::remove_dir_all(&path);
    }
}
