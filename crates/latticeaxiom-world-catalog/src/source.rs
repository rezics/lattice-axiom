use std::collections::{BTreeMap, VecDeque};

use thiserror::Error;

use crate::{AuthoritativeMetadataV1, LiveWorldLocation};

/// The only operations exposed to pure world preflight.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum ReadOperation {
    /// Read the bounded `world-header.json` sidecar.
    Header,
    /// Read authoritative metadata without opening a writer.
    Metadata,
}

/// Read-only data source used by catalog preflight.
///
/// This boundary intentionally has no directory creation, writer, package
/// loading, module callback, decoder callback, or migration method.
pub trait ReadOnlyWorldSource {
    /// Reads at most `max_bytes` from the optional sidecar.
    ///
    /// # Errors
    ///
    /// Returns [`SourceReadError`] when the entry cannot be read safely. An
    /// absent sidecar is represented by `Ok(None)` so metadata reconciliation
    /// may propose an explicit repair.
    fn read_header_bounded(
        &mut self,
        location: LiveWorldLocation,
        max_bytes: usize,
    ) -> Result<Option<Vec<u8>>, SourceReadError>;

    /// Reads the metadata projection through a read-only storage handle.
    ///
    /// # Errors
    ///
    /// Returns [`SourceReadError`] when authoritative metadata cannot be read.
    fn read_metadata_read_only(
        &mut self,
        location: LiveWorldLocation,
    ) -> Result<AuthoritativeMetadataV1, SourceReadError>;
}

/// Typed source read failure retained in a world entry or open plan.
#[derive(Clone, Debug, Eq, Error, PartialEq)]
pub enum SourceReadError {
    /// The requested world or metadata object is absent.
    #[error("{operation:?} data was not found")]
    NotFound {
        /// Failed read operation.
        operation: ReadOperation,
    },
    /// The host denied access to the requested object.
    #[error("permission denied while reading {operation:?} data")]
    PermissionDenied {
        /// Failed read operation.
        operation: ReadOperation,
    },
    /// A sidecar exceeded the caller-provided bounded read.
    #[error("bounded {operation:?} read rejected {available} available bytes; limit is {limit}")]
    LimitExceeded {
        /// Failed read operation.
        operation: ReadOperation,
        /// Bytes available in the source object.
        available: usize,
        /// Maximum bytes requested by the caller.
        limit: usize,
    },
    /// Deterministic adapter or storage failure.
    #[error("{operation:?} read failed: {code}")]
    Fault {
        /// Failed read operation.
        operation: ReadOperation,
        /// Stable, machine-readable fault code.
        code: String,
    },
}

/// Number of operations performed through a [`MemoryWorldSource`].
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ReadAudit {
    /// Bounded sidecar reads.
    pub header_reads: u64,
    /// Read-only metadata reads.
    pub metadata_reads: u64,
}

/// One memory-backed world used by headless and fault conformance tests.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MemoryWorldRecord {
    /// Optional encoded sidecar; absence models an interrupted DB-first publish.
    pub header_bytes: Option<Vec<u8>>,
    /// Optional authoritative read-only metadata.
    pub metadata: Option<AuthoritativeMetadataV1>,
}

/// Deterministic in-memory implementation of [`ReadOnlyWorldSource`].
///
/// Mutating setup methods only change this adapter's memory. They do not model
/// or expose a world writer or a filesystem.
#[derive(Debug, Default)]
pub struct MemoryWorldSource {
    records: BTreeMap<LiveWorldLocation, MemoryWorldRecord>,
    faults: BTreeMap<(LiveWorldLocation, ReadOperation), VecDeque<SourceReadError>>,
    audit: ReadAudit,
}

impl MemoryWorldSource {
    /// Inserts or replaces a memory fixture.
    pub fn insert(&mut self, location: LiveWorldLocation, record: MemoryWorldRecord) {
        self.records.insert(location, record);
    }

    /// Queues a one-shot fault for a specific read operation.
    pub fn inject_fault(
        &mut self,
        location: LiveWorldLocation,
        operation: ReadOperation,
        fault: SourceReadError,
    ) {
        self.faults
            .entry((location, operation))
            .or_default()
            .push_back(fault);
    }

    /// Returns read instrumentation for purity assertions.
    #[must_use]
    pub const fn audit(&self) -> ReadAudit {
        self.audit
    }

    fn take_fault(
        &mut self,
        location: LiveWorldLocation,
        operation: ReadOperation,
    ) -> Option<SourceReadError> {
        self.faults
            .get_mut(&(location, operation))
            .and_then(VecDeque::pop_front)
    }
}

impl ReadOnlyWorldSource for MemoryWorldSource {
    fn read_header_bounded(
        &mut self,
        location: LiveWorldLocation,
        max_bytes: usize,
    ) -> Result<Option<Vec<u8>>, SourceReadError> {
        self.audit.header_reads = self.audit.header_reads.saturating_add(1);
        if let Some(fault) = self.take_fault(location, ReadOperation::Header) {
            return Err(fault);
        }
        let record = self
            .records
            .get(&location)
            .ok_or(SourceReadError::NotFound {
                operation: ReadOperation::Header,
            })?;
        let Some(bytes) = &record.header_bytes else {
            return Ok(None);
        };
        if bytes.len() > max_bytes {
            return Err(SourceReadError::LimitExceeded {
                operation: ReadOperation::Header,
                available: bytes.len(),
                limit: max_bytes,
            });
        }
        Ok(Some(bytes.clone()))
    }

    fn read_metadata_read_only(
        &mut self,
        location: LiveWorldLocation,
    ) -> Result<AuthoritativeMetadataV1, SourceReadError> {
        self.audit.metadata_reads = self.audit.metadata_reads.saturating_add(1);
        if let Some(fault) = self.take_fault(location, ReadOperation::Metadata) {
            return Err(fault);
        }
        self.records
            .get(&location)
            .and_then(|record| record.metadata.clone())
            .ok_or(SourceReadError::NotFound {
                operation: ReadOperation::Metadata,
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{WorldRootId, header::tests::fixture_projection};

    #[test]
    fn memory_source_is_bounded_and_faults_are_one_shot() {
        let projection = fixture_projection();
        let location = LiveWorldLocation::new(WorldRootId(1), projection.world_id);
        let metadata =
            AuthoritativeMetadataV1::seal(projection).unwrap_or_else(|error| panic!("{error}"));
        let mut source = MemoryWorldSource::default();
        source.insert(
            location,
            MemoryWorldRecord {
                header_bytes: Some(vec![0; 9]),
                metadata: Some(metadata),
            },
        );
        source.inject_fault(
            location,
            ReadOperation::Header,
            SourceReadError::PermissionDenied {
                operation: ReadOperation::Header,
            },
        );

        assert!(matches!(
            source.read_header_bounded(location, 8),
            Err(SourceReadError::PermissionDenied { .. })
        ));
        assert!(matches!(
            source.read_header_bounded(location, 8),
            Err(SourceReadError::LimitExceeded { .. })
        ));
        assert_eq!(source.audit().header_reads, 2);
    }
}
