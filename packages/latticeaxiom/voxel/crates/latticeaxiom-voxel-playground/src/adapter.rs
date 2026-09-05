//! Revision gate shared by the upstream sink and headless fakes.

use std::collections::BTreeMap;

use latticeaxiom_storage::{ChunkCoordinate, ChunkRevision};

use crate::{PresentationCoordinate, PresentationVoxel, ProjectionBatch};

/// Narrow sink for discardable voxel presentation state.
///
/// Implementations may buffer writes. They must never publish their material
/// index or internal voxel map as authoritative or persistent state.
pub trait PresentationSink<M> {
    /// Backend-specific write failure.
    type Error;

    /// Queues or applies one presentation-only voxel value.
    ///
    /// # Errors
    ///
    /// Returns a backend error without advancing [`RevisionGate`]. A batch may
    /// have partially reached a sink, so retry must be idempotent.
    fn set_voxel(
        &mut self,
        coordinate: PresentationCoordinate,
        voxel: PresentationVoxel<M>,
    ) -> Result<(), Self::Error>;
}

/// Outcome of comparing a source revision with the last fully applied revision.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ApplyOutcome {
    /// Every write reached the sink and the source revision advanced.
    Applied {
        /// Number of writes sent to the sink.
        writes: usize,
    },
    /// The exact source revision was already fully applied.
    Duplicate,
    /// A newer source revision was already fully applied.
    Stale {
        /// Newer revision held by the presentation gate.
        applied_revision: ChunkRevision,
    },
}

/// Per-chunk source-revision gate in deterministic key order.
///
/// The gate is presentation state only. Losing it causes harmless replay from
/// authoritative chunks; it is never serialized into a world snapshot.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct RevisionGate {
    applied: BTreeMap<ChunkCoordinate, ChunkRevision>,
}

impl RevisionGate {
    /// Applies a batch only when its authoritative revision is newer.
    ///
    /// The revision advances after every sink write succeeds. If the sink
    /// fails after a partial batch, replaying the same batch is safe because
    /// voxel writes are idempotent.
    ///
    /// # Errors
    ///
    /// Returns the sink error and leaves the source revision unadvanced.
    pub fn apply<M: Clone, S: PresentationSink<M>>(
        &mut self,
        batch: &ProjectionBatch<M>,
        sink: &mut S,
    ) -> Result<ApplyOutcome, S::Error> {
        if let Some(applied) = self.applied.get(&batch.chunk()).copied() {
            if batch.revision() < applied {
                return Ok(ApplyOutcome::Stale {
                    applied_revision: applied,
                });
            }
            if batch.revision() == applied {
                return Ok(ApplyOutcome::Duplicate);
            }
        }

        for write in batch.writes() {
            sink.set_voxel(write.coordinate(), write.voxel().clone())?;
        }
        self.applied.insert(batch.chunk(), batch.revision());
        Ok(ApplyOutcome::Applied {
            writes: batch.writes().len(),
        })
    }

    /// Returns the last fully applied source revision for a chunk.
    #[must_use]
    pub fn applied_revision(&self, chunk: ChunkCoordinate) -> Option<ChunkRevision> {
        self.applied.get(&chunk).copied()
    }

    /// Drops presentation state for an unloaded chunk.
    ///
    /// Reload must submit a complete authoritative chunk batch before deltas.
    pub fn forget_chunk(&mut self, chunk: ChunkCoordinate) {
        self.applied.remove(&chunk);
    }

    /// Number of chunks carrying discardable presentation receipts.
    #[must_use]
    pub fn tracked_chunks(&self) -> usize {
        self.applied.len()
    }
}
