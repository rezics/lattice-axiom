//! Immutable source identity copied into every mesh result.

/// Stable integer coordinate of a source chunk, ordered `(x, y, z)`.
///
/// The signed representation preserves negative world coordinates without
/// converting them to floating point.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ChunkCoordinate([i64; 3]);

impl ChunkCoordinate {
    /// Creates a chunk coordinate in native Y-up axis order.
    #[must_use]
    pub const fn new(x: i64, y: i64, z: i64) -> Self {
        Self([x, y, z])
    }

    /// Returns `[x, y, z]`.
    #[must_use]
    pub const fn as_array(self) -> [i64; 3] {
        self.0
    }
}

/// Epoch of the authoritative source context captured for a mesh job.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct SourceEpoch(u64);

impl SourceEpoch {
    /// Creates an epoch value supplied by the caller.
    #[must_use]
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    /// Returns the numeric epoch.
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }
}

/// Revision of the authoritative source chunk captured for a mesh job.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct SourceRevision(u64);

impl SourceRevision {
    /// Creates a revision value supplied by the caller.
    #[must_use]
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    /// Returns the numeric revision.
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }
}

/// Canonical fingerprint of every caller-defined input that affects a mesh.
///
/// The mesher deliberately does not calculate this value: authoritative
/// voxel encoding, the halo, material semantics, and provider configuration
/// belong to their respective owners. The caller supplies their canonical
/// digest and compares the receipt before applying derived data.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct SourceFingerprint([u8; 32]);

impl SourceFingerprint {
    /// Creates a fingerprint from canonical digest bytes.
    #[must_use]
    pub const fn new(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    /// Returns the canonical digest bytes.
    #[must_use]
    pub const fn into_bytes(self) -> [u8; 32] {
        self.0
    }
}

/// Immutable source identity captured when a mesh job is created.
///
/// A fingerprint should cover the interior, its one-voxel halo, presentation
/// semantics, and any provider configuration that affects generated quads.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct MeshSource {
    chunk: ChunkCoordinate,
    epoch: SourceEpoch,
    revision: SourceRevision,
    fingerprint: SourceFingerprint,
}

impl MeshSource {
    /// Creates a complete mesh source identity.
    #[must_use]
    pub const fn new(
        chunk: ChunkCoordinate,
        epoch: SourceEpoch,
        revision: SourceRevision,
        fingerprint: SourceFingerprint,
    ) -> Self {
        Self {
            chunk,
            epoch,
            revision,
            fingerprint,
        }
    }

    /// Source chunk coordinate.
    #[must_use]
    pub const fn chunk(self) -> ChunkCoordinate {
        self.chunk
    }

    /// Source epoch.
    #[must_use]
    pub const fn epoch(self) -> SourceEpoch {
        self.epoch
    }

    /// Source revision.
    #[must_use]
    pub const fn revision(self) -> SourceRevision {
        self.revision
    }

    /// Source fingerprint.
    #[must_use]
    pub const fn fingerprint(self) -> SourceFingerprint {
        self.fingerprint
    }
}

/// Source identity returned with an immutable derived mesh result.
///
/// This type is evidence, not apply authorization. The owner of current
/// authoritative state must call [`Self::is_current_for`] (or perform an
/// equivalent comparison) at the point where it applies the result.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct MeshReceipt {
    source: MeshSource,
}

impl MeshReceipt {
    pub(crate) const fn new(source: MeshSource) -> Self {
        Self { source }
    }

    /// Returns the exact source identity used by the mesh job.
    #[must_use]
    pub const fn source(self) -> MeshSource {
        self.source
    }

    /// Whether this receipt still matches caller-supplied current state.
    ///
    /// The comparison is intentionally explicit and performed by the caller;
    /// this crate never applies a mesh or reads mutable world state.
    #[must_use]
    pub fn is_current_for(self, current: MeshSource) -> bool {
        self.source == current
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn negative_chunk_coordinate_is_preserved_exactly() {
        let source = MeshSource::new(
            ChunkCoordinate::new(-17, -3, -41),
            SourceEpoch::new(2),
            SourceRevision::new(9),
            SourceFingerprint::new([7; 32]),
        );
        let receipt = MeshReceipt::new(source);

        assert_eq!(receipt.source().chunk().as_array(), [-17, -3, -41]);
        assert!(receipt.is_current_for(source));
    }

    #[test]
    fn changed_revision_or_fingerprint_is_stale() {
        let original = MeshSource::new(
            ChunkCoordinate::new(1, 2, 3),
            SourceEpoch::new(4),
            SourceRevision::new(5),
            SourceFingerprint::new([6; 32]),
        );
        let receipt = MeshReceipt::new(original);
        let revised = MeshSource::new(
            original.chunk(),
            original.epoch(),
            SourceRevision::new(6),
            original.fingerprint(),
        );
        let refingerprinted = MeshSource::new(
            original.chunk(),
            original.epoch(),
            original.revision(),
            SourceFingerprint::new([8; 32]),
        );

        assert!(!receipt.is_current_for(revised));
        assert!(!receipt.is_current_for(refingerprinted));
    }
}
