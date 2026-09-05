//! Deterministic model of atomic publication for the bounded world header.

use std::sync::Mutex;

pub use latticeaxiom_world_catalog::{HeaderProjectionV1 as WorldHeaderBodyV1, WorldHeaderV1};
use thiserror::Error;

use crate::DigestV1;

/// A validated header together with its exact publication bytes.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PreparedWorldHeaderV1 {
    header: WorldHeaderV1,
    canonical_bytes: Vec<u8>,
    projection_hash: DigestV1,
}

impl PreparedWorldHeaderV1 {
    /// Seals and canonically encodes a header projection.
    ///
    /// # Errors
    ///
    /// Returns an error when the catalog codec cannot seal or encode the header.
    pub fn new(body: WorldHeaderBodyV1) -> Result<Self, HeaderPublishErrorV1> {
        let header = WorldHeaderV1::seal(body)
            .map_err(|error| HeaderPublishErrorV1::Codec(error.to_string()))?;
        Self::prepare(header)
    }

    /// Validates exact canonical bytes and prepares them for publication.
    ///
    /// # Errors
    ///
    /// Returns an error when the catalog codec rejects the bytes.
    pub fn from_canonical_bytes(bytes: &[u8]) -> Result<Self, HeaderPublishErrorV1> {
        let header = WorldHeaderV1::decode_canonical(bytes)
            .map_err(|error| HeaderPublishErrorV1::Codec(error.to_string()))?;
        Self::prepare(header)
    }

    fn prepare(header: WorldHeaderV1) -> Result<Self, HeaderPublishErrorV1> {
        let canonical_bytes = header
            .encode_canonical()
            .map_err(|error| HeaderPublishErrorV1::Codec(error.to_string()))?;
        let projection_hash = DigestV1::from_bytes(header.checksum.into_bytes());
        Ok(Self {
            header,
            canonical_bytes,
            projection_hash,
        })
    }

    /// Returns the validated catalog header.
    #[must_use]
    pub const fn header(&self) -> &WorldHeaderV1 {
        &self.header
    }

    /// Returns the exact canonical bytes.
    #[must_use]
    pub fn canonical_bytes(&self) -> &[u8] {
        &self.canonical_bytes
    }

    /// Returns the checksum of the authoritative header projection.
    #[must_use]
    pub const fn projection_hash(&self) -> DigestV1 {
        self.projection_hash
    }
}

/// Atomic header publication contract.
pub trait HeaderPublisher: Send + Sync {
    /// Reads the currently visible header, bounded by `max_bytes`.
    ///
    /// # Errors
    ///
    /// Returns an error for poisoned state or an oversized visible header.
    fn read_visible(&self, max_bytes: usize) -> Result<Option<Vec<u8>>, HeaderPublishErrorV1>;

    /// Publishes a prepared header atomically.
    ///
    /// # Errors
    ///
    /// Returns an error at an injected publication fault point.
    fn publish(
        &self,
        prepared: &PreparedWorldHeaderV1,
    ) -> Result<HeaderPublishReceiptV1, HeaderPublishErrorV1>;
}

/// One-shot failure locations in the atomic replacement protocol.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HeaderFaultPointV1 {
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
pub enum HeaderPublishStageV1 {
    /// Temporary bytes were written.
    TempWritten,
    /// Temporary-file contents were synchronized.
    FileSynced,
    /// The visible name was atomically replaced.
    Replaced,
    /// The containing directory was synchronized.
    DirectorySynced,
}

/// Evidence returned after a complete publication.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HeaderPublishReceiptV1 {
    /// Projection checksum committed by the header.
    pub projection_hash: DigestV1,
    /// Number of canonical bytes published.
    pub published_bytes: usize,
}

/// Failure to prepare, read, or publish a world header.
#[derive(Clone, Debug, Eq, Error, PartialEq)]
pub enum HeaderPublishErrorV1 {
    /// The catalog header codec rejected the value.
    #[error("world header codec failed: {0}")]
    Codec(String),
    /// The visible header exceeds the caller's read bound.
    #[error("visible world header has {actual} bytes; read limit is {max}")]
    ReadLimitExceeded {
        /// Visible byte count rejected by the caller's bound.
        actual: usize,
        /// Maximum visible byte count accepted by the caller.
        max: usize,
    },
    /// A deterministic one-shot fault was reached.
    #[error("injected world-header publication fault at {0:?}")]
    Injected(HeaderFaultPointV1),
    /// Internal deterministic state was poisoned by a panic.
    #[error("world-header publisher state is poisoned")]
    StatePoisoned,
}

#[derive(Debug, Default)]
struct PublisherState {
    visible: Option<Vec<u8>>,
    temporary: Option<Vec<u8>>,
    fault: Option<HeaderFaultPointV1>,
    trace: Vec<HeaderPublishStageV1>,
}

/// In-memory, deterministic oracle for the four-stage publication protocol.
#[derive(Debug, Default)]
pub struct DeterministicHeaderPublisher {
    state: Mutex<PublisherState>,
}

impl DeterministicHeaderPublisher {
    /// Creates an empty publisher.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Arms a fault consumed by the next publication that reaches it.
    ///
    /// # Errors
    ///
    /// Returns an error if the publisher state lock is poisoned.
    pub fn inject_fault(&self, fault: HeaderFaultPointV1) -> Result<(), HeaderPublishErrorV1> {
        self.state
            .lock()
            .map_err(|_| HeaderPublishErrorV1::StatePoisoned)?
            .fault = Some(fault);
        Ok(())
    }

    /// Returns a clone of the last temporary-file contents.
    ///
    /// # Errors
    ///
    /// Returns an error if the publisher state lock is poisoned.
    pub fn temporary_bytes(&self) -> Result<Option<Vec<u8>>, HeaderPublishErrorV1> {
        Ok(self
            .state
            .lock()
            .map_err(|_| HeaderPublishErrorV1::StatePoisoned)?
            .temporary
            .clone())
    }

    /// Returns an owned copy of the most recent publication trace.
    ///
    /// # Errors
    ///
    /// Returns an error if the publisher state lock is poisoned.
    pub fn trace(&self) -> Result<Vec<HeaderPublishStageV1>, HeaderPublishErrorV1> {
        Ok(self
            .state
            .lock()
            .map_err(|_| HeaderPublishErrorV1::StatePoisoned)?
            .trace
            .clone())
    }
}

impl HeaderPublisher for DeterministicHeaderPublisher {
    fn read_visible(&self, max_bytes: usize) -> Result<Option<Vec<u8>>, HeaderPublishErrorV1> {
        let state = self
            .state
            .lock()
            .map_err(|_| HeaderPublishErrorV1::StatePoisoned)?;
        if let Some(bytes) = &state.visible
            && bytes.len() > max_bytes
        {
            return Err(HeaderPublishErrorV1::ReadLimitExceeded {
                actual: bytes.len(),
                max: max_bytes,
            });
        }
        Ok(state.visible.clone())
    }

    fn publish(
        &self,
        prepared: &PreparedWorldHeaderV1,
    ) -> Result<HeaderPublishReceiptV1, HeaderPublishErrorV1> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| HeaderPublishErrorV1::StatePoisoned)?;
        state.trace.clear();
        state.temporary = Some(prepared.canonical_bytes.clone());

        for (stage, fault) in [
            (
                HeaderPublishStageV1::TempWritten,
                HeaderFaultPointV1::TempWrite,
            ),
            (
                HeaderPublishStageV1::FileSynced,
                HeaderFaultPointV1::FileSync,
            ),
            (HeaderPublishStageV1::Replaced, HeaderFaultPointV1::Replace),
            (
                HeaderPublishStageV1::DirectorySynced,
                HeaderFaultPointV1::DirectorySync,
            ),
        ] {
            state.trace.push(stage);
            if state.fault == Some(fault) {
                state.fault = None;
                return Err(HeaderPublishErrorV1::Injected(fault));
            }
            if stage == HeaderPublishStageV1::Replaced {
                let PublisherState {
                    visible, temporary, ..
                } = &mut *state;
                visible.clone_from(temporary);
            }
        }

        Ok(HeaderPublishReceiptV1 {
            projection_hash: prepared.projection_hash,
            published_bytes: prepared.canonical_bytes.len(),
        })
    }
}
