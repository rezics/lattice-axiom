use std::num::NonZeroU32;

use latticeaxiom_core::{CanonicalHash, StableId};
use latticeaxiom_storage::{ChunkCoordinate, ChunkRevision, DimensionId};
use serde::{Deserialize, Serialize};

use crate::{
    GenerationEpochIdV1, SnapshotChecksumV1, WorldgenError, WorldgenLimitsV1, WorldgenResult,
    cave::snapshot_checksum,
};

const MAX_DECLARED_TRANSITION_WIDTH: u32 = 4_096;
const MAX_REQUIRED_CAVE_PORTALS_PER_DECLARATION: usize = 64;

/// Coarse two-dimensional planning-cell coordinate in `(x, z)` order.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PlanningCellCoordinateV1 {
    /// Horizontal cell coordinate increasing to world right.
    pub x: i64,
    /// Horizontal cell coordinate on the world depth axis.
    pub z: i64,
}

impl PlanningCellCoordinateV1 {
    /// Creates a planning-cell coordinate.
    #[must_use]
    pub const fn new(x: i64, z: i64) -> Self {
        Self { x, z }
    }

    /// Maps a chunk onto its planning cell using the D4 planning-cell edge.
    ///
    /// `planning_cell_edge_chunks` is the closed [`crate::WorldgenConfigV1`]
    /// field of the same name. A zero edge is treated as one chunk so the
    /// mapping remains total.
    #[must_use]
    pub fn from_chunk(coordinate: ChunkCoordinate, planning_cell_edge_chunks: u16) -> Self {
        let edge = i64::from(planning_cell_edge_chunks.max(1));
        Self {
            x: i64::from(coordinate.x).div_euclid(edge),
            z: i64::from(coordinate.z).div_euclid(edge),
        }
    }
}

/// Epoch state already assigned to the target planning cell.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum CellEpochStateV1 {
    /// The cell has never been durably materialized and may take the active epoch.
    Unassigned,
    /// The cell was frozen by its first durable materialization.
    Frozen(GenerationEpochIdV1),
}

/// Explicit authoritative state of one cardinally adjacent planning cell.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum AdjacentCellEpochStateV1 {
    /// The adjacent cell exists but has never been durably materialized.
    Unassigned,
    /// The adjacent cell has a locally frozen generation epoch.
    Frozen(GenerationEpochIdV1),
    /// The adjacent coordinate is outside the active dimension domain.
    OutsideDimension,
}

/// One entry in a complete four-cardinal adjacent-epoch snapshot.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AdjacentCellEpochV1 {
    cell: PlanningCellCoordinateV1,
    state: AdjacentCellEpochStateV1,
}

impl AdjacentCellEpochV1 {
    /// Records an adjacent cell that has not been materialized.
    #[must_use]
    pub const fn unassigned(cell: PlanningCellCoordinateV1) -> Self {
        Self {
            cell,
            state: AdjacentCellEpochStateV1::Unassigned,
        }
    }

    /// Records an adjacent cell with a frozen generation epoch.
    #[must_use]
    pub const fn frozen(cell: PlanningCellCoordinateV1, epoch: GenerationEpochIdV1) -> Self {
        Self {
            cell,
            state: AdjacentCellEpochStateV1::Frozen(epoch),
        }
    }

    /// Records an adjacent coordinate outside the dimension domain.
    #[must_use]
    pub const fn outside_dimension(cell: PlanningCellCoordinateV1) -> Self {
        Self {
            cell,
            state: AdjacentCellEpochStateV1::OutsideDimension,
        }
    }

    /// Returns the neighboring cell.
    #[must_use]
    pub const fn cell(self) -> PlanningCellCoordinateV1 {
        self.cell
    }

    /// Returns the explicit adjacent-cell state.
    #[must_use]
    pub const fn state(self) -> AdjacentCellEpochStateV1 {
        self.state
    }
}

/// Structurally complete snapshot of all four cardinal adjacent cells.
///
/// A storage/coordinator adapter remains responsible for the truth of the
/// observations. This type prevents a sparse list from being mistaken for a
/// complete view; it does not authenticate the external source.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AdjacentEpochSnapshotV1 {
    target_cell: PlanningCellCoordinateV1,
    neighbors: [AdjacentCellEpochV1; 4],
}

impl AdjacentEpochSnapshotV1 {
    /// Validates exactly one entry for every cardinal neighbor.
    ///
    /// # Errors
    ///
    /// Returns an adjacency error for a non-cardinal, duplicate, missing, or
    /// unrepresentable neighbor coordinate.
    pub fn new(
        target_cell: PlanningCellCoordinateV1,
        mut neighbors: [AdjacentCellEpochV1; 4],
    ) -> WorldgenResult<Self> {
        for neighbor in neighbors {
            validate_cardinal_neighbor(target_cell, neighbor.cell)?;
        }
        neighbors.sort_by_key(|neighbor| neighbor.cell);
        if let Some(duplicate) = neighbors
            .windows(2)
            .find(|pair| pair[0].cell == pair[1].cell)
            .map(|pair| pair[0].cell)
        {
            return Err(WorldgenError::DuplicateAdjacentEpochCell { cell: duplicate });
        }
        let mut expected = cardinal_cells(target_cell)?;
        expected.sort();
        if neighbors.map(|neighbor| neighbor.cell) != expected {
            return Err(WorldgenError::IncompleteAdjacentEpochSnapshot { cell: target_cell });
        }
        Ok(Self {
            target_cell,
            neighbors,
        })
    }

    /// Creates a complete snapshot in which all four neighbors are unassigned.
    ///
    /// # Errors
    ///
    /// Returns an arithmetic error if a cardinal neighbor is not representable.
    pub fn all_unassigned(target_cell: PlanningCellCoordinateV1) -> WorldgenResult<Self> {
        let neighbors = cardinal_cells(target_cell)?.map(AdjacentCellEpochV1::unassigned);
        Self::new(target_cell, neighbors)
    }

    /// Returns the target planning cell captured by this snapshot.
    #[must_use]
    pub const fn target_cell(&self) -> PlanningCellCoordinateV1 {
        self.target_cell
    }

    /// Returns the four canonically ordered neighbor observations.
    #[must_use]
    pub const fn neighbors(&self) -> &[AdjacentCellEpochV1; 4] {
        &self.neighbors
    }
}

/// Bounded, untrusted declaration of a possible epoch-boundary adapter.
///
/// This declaration is intentionally not a verification receipt. D4 refuses a
/// cross-epoch write even when a matching declaration is present; a later
/// boundary verifier must prove the terrain and cave contracts and mint the
/// publication evidence.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct BoundaryAdapterDeclarationV1 {
    adapter_id: StableId,
    adapter_version: NonZeroU32,
    adapter_hash: CanonicalHash,
    epoch_a: GenerationEpochIdV1,
    epoch_b: GenerationEpochIdV1,
    transition_width: NonZeroU32,
    terrain_boundary_signature: CanonicalHash,
    required_cave_portals: Vec<CanonicalHash>,
}

impl BoundaryAdapterDeclarationV1 {
    /// Validates a direction-independent, bounded adapter declaration.
    ///
    /// # Errors
    ///
    /// Returns [`WorldgenError::InvalidBoundaryAdapterDeclaration`] when both
    /// epochs are equal, the transition width exceeds 4096, portals are
    /// duplicated, or the portal list exceeds 64.
    #[allow(
        clippy::too_many_arguments,
        reason = "the declaration has eight independent contract fields"
    )]
    pub fn new(
        adapter_id: StableId,
        adapter_version: NonZeroU32,
        adapter_hash: CanonicalHash,
        first_epoch: GenerationEpochIdV1,
        second_epoch: GenerationEpochIdV1,
        transition_width: NonZeroU32,
        terrain_boundary_signature: CanonicalHash,
        mut required_cave_portals: Vec<CanonicalHash>,
    ) -> WorldgenResult<Self> {
        if first_epoch == second_epoch {
            return Err(WorldgenError::InvalidBoundaryAdapterDeclaration {
                adapter: adapter_id,
                reason: "adapter epochs must differ".to_owned(),
            });
        }
        if transition_width.get() > MAX_DECLARED_TRANSITION_WIDTH {
            return Err(WorldgenError::InvalidBoundaryAdapterDeclaration {
                adapter: adapter_id,
                reason: format!(
                    "transition width {transition_width} exceeds {MAX_DECLARED_TRANSITION_WIDTH}"
                ),
            });
        }
        let portal_count = required_cave_portals.len();
        if portal_count > MAX_REQUIRED_CAVE_PORTALS_PER_DECLARATION {
            return Err(WorldgenError::InvalidBoundaryAdapterDeclaration {
                adapter: adapter_id,
                reason: format!(
                    "required cave portal count {portal_count} exceeds {MAX_REQUIRED_CAVE_PORTALS_PER_DECLARATION}"
                ),
            });
        }
        required_cave_portals.sort();
        if required_cave_portals
            .windows(2)
            .any(|pair| pair[0] == pair[1])
        {
            return Err(WorldgenError::InvalidBoundaryAdapterDeclaration {
                adapter: adapter_id,
                reason: "required cave portals must be unique".to_owned(),
            });
        }
        let (epoch_a, epoch_b) = canonical_epoch_pair(first_epoch, second_epoch);
        Ok(Self {
            adapter_id,
            adapter_version,
            adapter_hash,
            epoch_a,
            epoch_b,
            transition_width,
            terrain_boundary_signature,
            required_cave_portals,
        })
    }

    /// Returns the stable declaration identity.
    #[must_use]
    pub const fn adapter_id(&self) -> &StableId {
        &self.adapter_id
    }

    /// Returns the canonical pair of declared epochs.
    #[must_use]
    pub const fn epochs(&self) -> (GenerationEpochIdV1, GenerationEpochIdV1) {
        (self.epoch_a, self.epoch_b)
    }

    /// Returns the adapter implementation hash.
    #[must_use]
    pub const fn adapter_hash(&self) -> CanonicalHash {
        self.adapter_hash
    }

    /// Returns the declared adapter version.
    #[must_use]
    pub const fn adapter_version(&self) -> NonZeroU32 {
        self.adapter_version
    }
}

/// Verified, direction-independent evidence that two planning-cell epochs may meet.
///
/// Unlike [`BoundaryAdapterDeclarationV1`], this receipt is the applied adapter
/// proof. A matching receipt allows a new epoch cell to generate beside a frozen
/// neighbor without rewriting the neighbor's durable snapshot.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct BoundaryReceiptV1 {
    boundary_id: crate::BoundaryIdV1,
    cell_a: PlanningCellCoordinateV1,
    cell_b: PlanningCellCoordinateV1,
    epoch_a: GenerationEpochIdV1,
    epoch_b: GenerationEpochIdV1,
    adapter_id: StableId,
    adapter_version: NonZeroU32,
    adapter_hash: CanonicalHash,
    transition_width: NonZeroU32,
    terrain_boundary_signature: CanonicalHash,
    required_cave_portals: Vec<CanonicalHash>,
}

impl BoundaryReceiptV1 {
    /// Mints verified application evidence from a bounded adapter declaration.
    ///
    /// # Errors
    ///
    /// Returns an arithmetic error when the two cells are not cardinally
    /// adjacent.
    pub fn from_verified_declaration(
        declaration: &BoundaryAdapterDeclarationV1,
        first_cell: PlanningCellCoordinateV1,
        second_cell: PlanningCellCoordinateV1,
    ) -> WorldgenResult<Self> {
        validate_cardinal_neighbor(first_cell, second_cell)?;
        let (cell_a, cell_b) = canonical_cell_pair(first_cell, second_cell);
        let (epoch_a, epoch_b) = declaration.epochs();
        let boundary_id = crate::BoundaryIdV1::from_hash(crate::hashes::domain_hash(
            b"latticeaxiom.generation-boundary.v1\0",
            &[
                &cell_a.x.to_be_bytes(),
                &cell_a.z.to_be_bytes(),
                &cell_b.x.to_be_bytes(),
                &cell_b.z.to_be_bytes(),
                epoch_a.as_bytes(),
                epoch_b.as_bytes(),
                declaration.adapter_id.as_str().as_bytes(),
                &declaration.adapter_version.get().to_be_bytes(),
                declaration.adapter_hash.as_bytes(),
            ],
        ));
        Ok(Self {
            boundary_id,
            cell_a,
            cell_b,
            epoch_a,
            epoch_b,
            adapter_id: declaration.adapter_id.clone(),
            adapter_version: declaration.adapter_version,
            adapter_hash: declaration.adapter_hash,
            transition_width: declaration.transition_width,
            terrain_boundary_signature: declaration.terrain_boundary_signature,
            required_cave_portals: declaration.required_cave_portals.clone(),
        })
    }

    /// Returns the direction-independent boundary identity.
    #[must_use]
    pub const fn boundary_id(&self) -> crate::BoundaryIdV1 {
        self.boundary_id
    }

    /// Returns the canonical pair of connected planning cells.
    #[must_use]
    pub const fn cells(&self) -> (PlanningCellCoordinateV1, PlanningCellCoordinateV1) {
        (self.cell_a, self.cell_b)
    }

    /// Returns the canonical pair of connected epochs.
    #[must_use]
    pub const fn epochs(&self) -> (GenerationEpochIdV1, GenerationEpochIdV1) {
        (self.epoch_a, self.epoch_b)
    }

    /// Returns the verified adapter identity.
    #[must_use]
    pub const fn adapter_id(&self) -> &StableId {
        &self.adapter_id
    }
}

const fn canonical_cell_pair(
    first: PlanningCellCoordinateV1,
    second: PlanningCellCoordinateV1,
) -> (PlanningCellCoordinateV1, PlanningCellCoordinateV1) {
    if first.x < second.x || (first.x == second.x && first.z <= second.z) {
        (first, second)
    } else {
        (second, first)
    }
}

/// Caller-trusted evidence returned by an authoritative storage read.
///
/// This value is not a durability proof. A storage adapter must construct it
/// only after reading an authoritative snapshot and its metadata; worldgen
/// verifies internal byte checksum and request consistency before reuse.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExistingSnapshotEvidenceV1 {
    dimension: DimensionId,
    coordinate: ChunkCoordinate,
    planning_cell: PlanningCellCoordinateV1,
    epoch: GenerationEpochIdV1,
    revision: ChunkRevision,
    writer_count: u64,
    bytes: Vec<u8>,
    checksum: SnapshotChecksumV1,
}

impl ExistingSnapshotEvidenceV1 {
    /// Validates caller-trusted storage evidence against its exact checksum.
    ///
    /// # Errors
    ///
    /// Returns [`WorldgenError::SnapshotChecksumMismatch`] when `bytes` do not
    /// match `checksum`.
    #[allow(
        clippy::too_many_arguments,
        reason = "existing snapshot evidence is intentionally explicit"
    )]
    pub fn new(
        dimension: DimensionId,
        coordinate: ChunkCoordinate,
        planning_cell: PlanningCellCoordinateV1,
        epoch: GenerationEpochIdV1,
        revision: ChunkRevision,
        writer_count: u64,
        bytes: Vec<u8>,
        checksum: SnapshotChecksumV1,
    ) -> WorldgenResult<Self> {
        if snapshot_checksum(&bytes) != checksum {
            return Err(WorldgenError::SnapshotChecksumMismatch);
        }
        Ok(Self {
            dimension,
            coordinate,
            planning_cell,
            epoch,
            revision,
            writer_count,
            bytes,
            checksum,
        })
    }

    /// Returns the dimension owning the snapshot.
    #[must_use]
    pub const fn dimension(&self) -> &DimensionId {
        &self.dimension
    }

    /// Returns the chunk coordinate.
    #[must_use]
    pub const fn coordinate(&self) -> ChunkCoordinate {
        self.coordinate
    }

    /// Returns the frozen planning cell.
    #[must_use]
    pub const fn planning_cell(&self) -> PlanningCellCoordinateV1 {
        self.planning_cell
    }

    /// Returns the locally frozen epoch.
    #[must_use]
    pub const fn epoch(&self) -> GenerationEpochIdV1 {
        self.epoch
    }

    /// Returns the persisted revision supplied by storage.
    #[must_use]
    pub const fn revision(&self) -> ChunkRevision {
        self.revision
    }

    /// Returns the writer count supplied by storage.
    #[must_use]
    pub const fn writer_count(&self) -> u64 {
        self.writer_count
    }

    /// Returns the exact persisted bytes.
    #[must_use]
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    /// Returns the checksum of [`Self::bytes`].
    #[must_use]
    pub const fn checksum(&self) -> SnapshotChecksumV1 {
        self.checksum
    }
}

pub(crate) fn validate_epoch_boundaries(
    cell: PlanningCellCoordinateV1,
    active_epoch: GenerationEpochIdV1,
    adjacent: &AdjacentEpochSnapshotV1,
    declarations: &[BoundaryAdapterDeclarationV1],
    receipts: &[BoundaryReceiptV1],
    limits: WorldgenLimitsV1,
) -> WorldgenResult<()> {
    preflight_count(
        "complete adjacent epoch states",
        adjacent.neighbors.len(),
        usize::from(limits.max_adjacent_epochs.get()),
    )?;
    preflight_count(
        "boundary adapter declarations",
        declarations.len(),
        usize::from(limits.max_boundary_adapters.get()),
    )?;
    preflight_count(
        "boundary adapter receipts",
        receipts.len(),
        usize::from(limits.max_boundary_adapters.get()),
    )?;
    if adjacent.target_cell != cell {
        return Err(WorldgenError::AdjacentEpochSnapshotCellMismatch {
            expected: cell,
            actual: adjacent.target_cell,
        });
    }

    for neighbor in adjacent.neighbors {
        let AdjacentCellEpochStateV1::Frozen(neighbor_epoch) = neighbor.state else {
            continue;
        };
        if neighbor_epoch == active_epoch {
            continue;
        }
        let (epoch_a, epoch_b) = canonical_epoch_pair(active_epoch, neighbor_epoch);
        let (cell_a, cell_b) = canonical_cell_pair(cell, neighbor.cell);
        let matching_receipts = receipts
            .iter()
            .filter(|receipt| {
                receipt.epoch_a == epoch_a
                    && receipt.epoch_b == epoch_b
                    && receipt.cell_a == cell_a
                    && receipt.cell_b == cell_b
            })
            .collect::<Vec<_>>();
        match matching_receipts.as_slice() {
            [_] => continue,
            [_, _, ..] => {
                return Err(WorldgenError::ConflictingBoundaryAdapters { epoch_a, epoch_b });
            }
            [] => {}
        }
        let candidates = declarations
            .iter()
            .filter(|declaration| declaration.epoch_a == epoch_a && declaration.epoch_b == epoch_b)
            .collect::<Vec<_>>();
        match candidates.as_slice() {
            [] => return Err(WorldgenError::MissingBoundaryAdapter { epoch_a, epoch_b }),
            [declaration] => {
                return Err(WorldgenError::BoundaryAdapterNotVerified {
                    epoch_a,
                    epoch_b,
                    adapter: Box::new(declaration.adapter_id.clone()),
                });
            }
            _ => {
                return Err(WorldgenError::ConflictingBoundaryAdapters { epoch_a, epoch_b });
            }
        }
    }
    Ok(())
}

const fn canonical_epoch_pair(
    first: GenerationEpochIdV1,
    second: GenerationEpochIdV1,
) -> (GenerationEpochIdV1, GenerationEpochIdV1) {
    if first.as_hash().as_bytes()[0] < second.as_hash().as_bytes()[0] {
        (first, second)
    } else if first.as_hash().as_bytes()[0] > second.as_hash().as_bytes()[0] {
        (second, first)
    } else {
        canonical_epoch_pair_slow(first, second)
    }
}

const fn canonical_epoch_pair_slow(
    first: GenerationEpochIdV1,
    second: GenerationEpochIdV1,
) -> (GenerationEpochIdV1, GenerationEpochIdV1) {
    let mut index = 1;
    while index < 32 {
        let left = first.as_hash().as_bytes()[index];
        let right = second.as_hash().as_bytes()[index];
        if left < right {
            return (first, second);
        }
        if left > right {
            return (second, first);
        }
        index += 1;
    }
    (first, second)
}

fn cardinal_cells(cell: PlanningCellCoordinateV1) -> WorldgenResult<[PlanningCellCoordinateV1; 4]> {
    let west = cell
        .x
        .checked_sub(1)
        .ok_or(WorldgenError::ArithmeticOverflow {
            operation: "west planning-cell neighbor",
        })?;
    let east = cell
        .x
        .checked_add(1)
        .ok_or(WorldgenError::ArithmeticOverflow {
            operation: "east planning-cell neighbor",
        })?;
    let north = cell
        .z
        .checked_sub(1)
        .ok_or(WorldgenError::ArithmeticOverflow {
            operation: "north planning-cell neighbor",
        })?;
    let south = cell
        .z
        .checked_add(1)
        .ok_or(WorldgenError::ArithmeticOverflow {
            operation: "south planning-cell neighbor",
        })?;
    Ok([
        PlanningCellCoordinateV1::new(west, cell.z),
        PlanningCellCoordinateV1::new(east, cell.z),
        PlanningCellCoordinateV1::new(cell.x, north),
        PlanningCellCoordinateV1::new(cell.x, south),
    ])
}

fn validate_cardinal_neighbor(
    cell: PlanningCellCoordinateV1,
    neighbor: PlanningCellCoordinateV1,
) -> WorldgenResult<()> {
    let delta_x = i128::from(cell.x)
        .saturating_sub(i128::from(neighbor.x))
        .abs();
    let delta_z = i128::from(cell.z)
        .saturating_sub(i128::from(neighbor.z))
        .abs();
    if delta_x.saturating_add(delta_z) == 1 {
        Ok(())
    } else {
        Err(WorldgenError::NonAdjacentEpochCell { cell, neighbor })
    }
}

fn preflight_count(kind: &'static str, actual: usize, limit: usize) -> WorldgenResult<()> {
    if actual <= limit {
        Ok(())
    } else {
        Err(WorldgenError::CollectionLimitExceeded {
            kind,
            actual,
            limit,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::PlanningCellCoordinateV1;
    use latticeaxiom_storage::ChunkCoordinate;

    #[test]
    fn chunk_mapping_uses_planning_cell_edge_and_ignores_y() {
        let edge = 8_u16;
        assert_eq!(
            PlanningCellCoordinateV1::from_chunk(ChunkCoordinate::new(-1, 12, 7), edge),
            PlanningCellCoordinateV1::new(-1, 0)
        );
        assert_eq!(
            PlanningCellCoordinateV1::from_chunk(ChunkCoordinate::new(8, -4, -9), edge),
            PlanningCellCoordinateV1::new(1, -2)
        );
        assert_eq!(
            PlanningCellCoordinateV1::from_chunk(ChunkCoordinate::new(0, 0, 0), 0),
            PlanningCellCoordinateV1::new(0, 0)
        );
    }
}
