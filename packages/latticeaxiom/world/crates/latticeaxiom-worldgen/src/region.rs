//! Bounded origin-region materialization through the D4 snapshot-candidate path.
//!
//! This module reuses [`GenerationPlanV1::generate`]. It does not add a second
//! generator, open a durable writer, stream interest, or assign territories.

use std::{
    collections::{BTreeMap, BTreeSet},
    str::FromStr,
};

use latticeaxiom_core::{IdentifierError, SchemaId, StableId, WorldId};
use latticeaxiom_storage::{
    ChangedDomains, ChunkCoordinate, ChunkData, ChunkKey, ChunkMutation, PayloadSchemaVersion,
    TransactionId, VersionedPayload, WorldRevision, WorldTransaction,
};

use crate::{
    ChunkGenerationOutcomeV1, D4SnapshotCandidateV1, GenerationPlanV1, WorldgenError,
    WorldgenResult,
};

/// Hard unique-chunk ceiling for one bounded generated region.
pub const MAX_BOUNDED_REGION_CHUNKS: usize = 16;

const SNAPSHOT_PAYLOAD_SCHEMA_ID: &str = "latticeaxiom:schema/d4-snapshot-candidate@1";
const WORLDGEN_PROVENANCE_ID: &str = "latticeaxiom:provenance/worldgen@1";
const SNAPSHOT_CHECKSUM_PROVENANCE_ID: &str = "latticeaxiom:provenance/d4-snapshot-checksum@1";

/// Origin neighborhood used by the worldgen-to-storage-candidate path.
///
/// Coordinates are canonical `(x, y, z)` with Bevy-native Y-up. The set covers
/// origin and the three adjacent chunks in the negative `x`/`z` quadrant of the
/// same vertical slice.
pub const ORIGIN_NEIGHBORHOOD_CHUNK_COORDINATES_V1: [ChunkCoordinate; 4] = [
    ChunkCoordinate::new(-1, 0, -1),
    ChunkCoordinate::new(-1, 0, 0),
    ChunkCoordinate::new(0, 0, -1),
    ChunkCoordinate::new(0, 0, 0),
];

/// Snapshot-first D4 candidates for a bounded set of chunks.
///
/// Materialization calls the compiled plan's generator once per unique chunk
/// and returns candidates only. Publication remains a caller-owned kernel
/// commit; this type never opens a writer.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BoundedGeneratedRegionV1 {
    candidates: BTreeMap<ChunkCoordinate, D4SnapshotCandidateV1>,
}

impl BoundedGeneratedRegionV1 {
    /// Materializes [`ORIGIN_NEIGHBORHOOD_CHUNK_COORDINATES_V1`] through D4 generation.
    ///
    /// # Errors
    ///
    /// Returns any generation failure for a neighborhood chunk.
    pub fn materialize(plan: &GenerationPlanV1) -> WorldgenResult<Self> {
        Self::materialize_coordinates(plan, ORIGIN_NEIGHBORHOOD_CHUNK_COORDINATES_V1)
    }

    /// Materializes a caller-supplied bounded set of unique chunk coordinates.
    ///
    /// Generation order is canonical `(x, y, z)` order. Request order cannot
    /// change candidate bytes.
    ///
    /// # Errors
    ///
    /// Fails for an empty set, a duplicate coordinate, a set larger than
    /// [`MAX_BOUNDED_REGION_CHUNKS`], reused snapshot evidence, or any D4
    /// generation error.
    pub fn materialize_coordinates(
        plan: &GenerationPlanV1,
        coordinates: impl IntoIterator<Item = ChunkCoordinate>,
    ) -> WorldgenResult<Self> {
        let mut unique = BTreeSet::new();
        for coordinate in coordinates {
            if !unique.insert(coordinate) {
                return Err(WorldgenError::DuplicateGeneratedRegionChunk { coordinate });
            }
        }
        if unique.is_empty() {
            return Err(WorldgenError::EmptyGeneratedRegion);
        }
        if unique.len() > MAX_BOUNDED_REGION_CHUNKS {
            return Err(WorldgenError::GeneratedRegionLimitExceeded {
                actual: unique.len(),
                limit: MAX_BOUNDED_REGION_CHUNKS,
            });
        }

        let mut candidates = BTreeMap::new();
        for coordinate in unique {
            let request = plan.vacant_generation_request(coordinate)?;
            let outcome = plan.generate(request)?;
            let ChunkGenerationOutcomeV1::Prepared(candidate) = outcome else {
                return Err(WorldgenError::ExistingSnapshotUnsupported { coordinate });
            };
            candidates.insert(coordinate, *candidate);
        }
        Ok(Self { candidates })
    }

    /// Returns the number of prepared snapshot candidates.
    #[must_use]
    pub fn len(&self) -> usize {
        self.candidates.len()
    }

    /// Returns whether the region contains no candidates.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.candidates.is_empty()
    }

    /// Returns the candidate prepared for `coordinate`.
    #[must_use]
    pub fn candidate(&self, coordinate: ChunkCoordinate) -> Option<&D4SnapshotCandidateV1> {
        self.candidates.get(&coordinate)
    }

    /// Iterates candidates in canonical `(x, y, z)` order.
    #[must_use]
    pub fn candidates(
        &self,
    ) -> impl ExactSizeIterator<Item = (ChunkCoordinate, &D4SnapshotCandidateV1)> {
        self.candidates
            .iter()
            .map(|(coordinate, candidate)| (*coordinate, candidate))
    }

    /// Converts every candidate into kernel chunk replacements without committing.
    ///
    /// # Errors
    ///
    /// Returns a storage-identity or canonical-encoding failure from any
    /// candidate conversion.
    pub fn to_storage_mutations(&self, world: WorldId) -> WorldgenResult<Vec<ChunkMutation>> {
        self.candidates
            .values()
            .map(|candidate| candidate.to_storage_mutation(world))
            .collect()
    }

    /// Builds one atomic transaction of the region without opening a writer.
    ///
    /// # Errors
    ///
    /// Returns a storage-identity or canonical-encoding failure from any
    /// candidate conversion.
    pub fn to_storage_transaction(
        &self,
        world: WorldId,
        transaction_id: TransactionId,
        base_world_revision: WorldRevision,
    ) -> WorldgenResult<WorldTransaction> {
        Ok(WorldTransaction::new(
            transaction_id,
            world,
            base_world_revision,
            self.to_storage_mutations(world)?,
        ))
    }
}

impl D4SnapshotCandidateV1 {
    /// Stores these exact snapshot bytes plus the receipt sidecar as chunk data.
    ///
    /// The result is a kernel candidate payload. It does not open a writer.
    ///
    /// # Errors
    ///
    /// Returns an invalid storage-identity error or a receipt encoding failure.
    pub fn to_storage_chunk_data(&self) -> WorldgenResult<ChunkData> {
        let voxels = VersionedPayload::new(
            parse_schema(SNAPSHOT_PAYLOAD_SCHEMA_ID)?,
            snapshot_schema_version()?,
            self.snapshot_bytes().to_vec(),
        );
        let mut provenance = BTreeMap::new();
        provenance.insert(
            parse_stable_id(WORLDGEN_PROVENANCE_ID)?,
            self.receipt().canonical_hash()?,
        );
        provenance.insert(
            parse_stable_id(SNAPSHOT_CHECKSUM_PROVENANCE_ID)?,
            *self.checksum().as_hash(),
        );
        Ok(ChunkData::new(
            voxels,
            BTreeMap::new(),
            BTreeMap::new(),
            provenance,
        ))
    }

    /// Converts this candidate into a first-publication chunk replacement.
    ///
    /// # Errors
    ///
    /// Returns an invalid storage-identity error or a receipt encoding failure.
    pub fn to_storage_mutation(&self, world: WorldId) -> WorldgenResult<ChunkMutation> {
        Ok(ChunkMutation::new(
            ChunkKey::new(
                world,
                self.receipt().dimension().clone(),
                self.receipt().chunk(),
            ),
            self.expected_chunk_revision(),
            ChangedDomains::ALL,
            self.to_storage_chunk_data()?,
        ))
    }
}

fn parse_schema(value: &'static str) -> WorldgenResult<SchemaId> {
    SchemaId::from_str(value).map_err(|error| invalid_storage_identity(value, &error))
}

fn parse_stable_id(value: &'static str) -> WorldgenResult<StableId> {
    StableId::from_str(value).map_err(|error| invalid_storage_identity(value, &error))
}

fn invalid_storage_identity(value: &'static str, error: &IdentifierError) -> WorldgenError {
    WorldgenError::InvalidStorageIdentity {
        value: value.to_owned(),
        reason: error.to_string(),
    }
}

fn snapshot_schema_version() -> WorldgenResult<PayloadSchemaVersion> {
    PayloadSchemaVersion::new(1).map_err(|_| WorldgenError::InvalidStorageIdentity {
        value: SNAPSHOT_PAYLOAD_SCHEMA_ID.to_owned(),
        reason: "payload schema version one must be positive".to_owned(),
    })
}
