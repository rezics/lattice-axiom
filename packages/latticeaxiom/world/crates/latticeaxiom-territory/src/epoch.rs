//! Planning-cell epoch freezing and transition evidence.

use std::num::NonZeroU32;

use latticeaxiom_core::{CanonicalHash, StableId, canonical_json_hash};
use latticeaxiom_worldgen::{DimensionId, GenerationEpochIdV1, PlanningCellCoordinateV1};
use serde::{Deserialize, Serialize};

use crate::{
    CaveAdjacencyV1, CavePortalIdV1, TerritoryError, TerritoryResult, TransitionReceiptHashV1,
    cave::{validate_cardinal, validate_required_portals},
};

/// One durable planning-cell epoch assignment.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CellEpochAssignmentV1 {
    dimension: DimensionId,
    cell: PlanningCellCoordinateV1,
    epoch: GenerationEpochIdV1,
}

impl CellEpochAssignmentV1 {
    /// Returns the planning cell.
    #[must_use]
    pub const fn cell(&self) -> PlanningCellCoordinateV1 {
        self.cell
    }

    /// Returns the frozen generation epoch.
    #[must_use]
    pub const fn epoch(&self) -> GenerationEpochIdV1 {
        self.epoch
    }
}

#[derive(Serialize)]
struct LedgerHashPayloadV1<'a> {
    dimension: &'a DimensionId,
    assignments: &'a [CellEpochAssignmentV1],
}

/// Deterministic evidence emitted by an epoch freeze attempt.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CellEpochFreezeReceiptV1 {
    dimension: DimensionId,
    cell: PlanningCellCoordinateV1,
    epoch: GenerationEpochIdV1,
    created: bool,
    previous_ledger_hash: CanonicalHash,
    resulting_ledger_hash: CanonicalHash,
}

impl CellEpochFreezeReceiptV1 {
    /// Returns whether this attempt performed the first durable assignment.
    #[must_use]
    pub const fn created(&self) -> bool {
        self.created
    }

    /// Returns the resulting ledger hash.
    #[must_use]
    pub const fn resulting_ledger_hash(&self) -> &CanonicalHash {
        &self.resulting_ledger_hash
    }
}

/// A dimension-local ledger with an epoch frozen independently per 2D cell.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PlanningCellEpochLedgerV1 {
    dimension: DimensionId,
    assignments: Vec<CellEpochAssignmentV1>,
    ledger_hash: CanonicalHash,
}

impl PlanningCellEpochLedgerV1 {
    /// Creates an empty dimension-local epoch ledger.
    ///
    /// # Errors
    ///
    /// Returns an error if canonical hash encoding fails.
    pub fn empty(dimension: DimensionId) -> TerritoryResult<Self> {
        let assignments = Vec::new();
        let ledger_hash = ledger_hash(&dimension, &assignments)?;
        Ok(Self {
            dimension,
            assignments,
            ledger_hash,
        })
    }

    /// Freezes an epoch on first durable materialization.
    ///
    /// Repeating the same assignment is idempotent. A different epoch is
    /// rejected without changing any ledger state.
    ///
    /// # Errors
    ///
    /// Returns an error for an epoch conflict or canonical encoding failure.
    pub fn freeze_for_materialization(
        &mut self,
        cell: PlanningCellCoordinateV1,
        epoch: GenerationEpochIdV1,
    ) -> TerritoryResult<CellEpochFreezeReceiptV1> {
        let previous_ledger_hash = self.ledger_hash;
        match self
            .assignments
            .binary_search_by_key(&cell, CellEpochAssignmentV1::cell)
        {
            Ok(index) => {
                let frozen = self.assignments[index].epoch;
                if frozen != epoch {
                    return Err(TerritoryError::EpochAlreadyFrozen {
                        cell,
                        frozen,
                        requested: epoch,
                    });
                }
                Ok(CellEpochFreezeReceiptV1 {
                    dimension: self.dimension.clone(),
                    cell,
                    epoch,
                    created: false,
                    previous_ledger_hash,
                    resulting_ledger_hash: previous_ledger_hash,
                })
            }
            Err(index) => {
                let mut next_assignments = self.assignments.clone();
                next_assignments.insert(
                    index,
                    CellEpochAssignmentV1 {
                        dimension: self.dimension.clone(),
                        cell,
                        epoch,
                    },
                );
                let resulting_ledger_hash = ledger_hash(&self.dimension, &next_assignments)?;
                self.assignments = next_assignments;
                self.ledger_hash = resulting_ledger_hash;
                Ok(CellEpochFreezeReceiptV1 {
                    dimension: self.dimension.clone(),
                    cell,
                    epoch,
                    created: true,
                    previous_ledger_hash,
                    resulting_ledger_hash,
                })
            }
        }
    }

    /// Returns a frozen epoch, if the cell has durable materialization.
    #[must_use]
    pub fn epoch(&self, cell: PlanningCellCoordinateV1) -> Option<GenerationEpochIdV1> {
        self.assignments
            .binary_search_by_key(&cell, CellEpochAssignmentV1::cell)
            .ok()
            .map(|index| self.assignments[index].epoch)
    }

    /// Returns sorted assignments.
    #[must_use]
    pub fn assignments(&self) -> &[CellEpochAssignmentV1] {
        &self.assignments
    }

    /// Returns the canonical ledger hash.
    #[must_use]
    pub const fn ledger_hash(&self) -> &CanonicalHash {
        &self.ledger_hash
    }

    /// Revalidates ordering, dimension ownership, uniqueness, and hash.
    ///
    /// # Errors
    ///
    /// Returns an error if serialized ledger evidence is not canonical.
    pub fn validate(&self) -> TerritoryResult<()> {
        if self
            .assignments
            .windows(2)
            .any(|pair| pair[0].cell >= pair[1].cell)
            || self
                .assignments
                .iter()
                .any(|assignment| assignment.dimension != self.dimension)
            || ledger_hash(&self.dimension, &self.assignments)? != self.ledger_hash
        {
            return Err(TerritoryError::InvalidEpochLedger {
                reason: "assignments, dimension ownership, or hash are not canonical".to_owned(),
            });
        }
        Ok(())
    }
}

fn ledger_hash(
    dimension: &DimensionId,
    assignments: &[CellEpochAssignmentV1],
) -> TerritoryResult<CanonicalHash> {
    canonical_json_hash(&LedgerHashPayloadV1 {
        dimension,
        assignments,
    })
    .map_err(|error| TerritoryError::CanonicalEncoding {
        kind: "planning-cell epoch ledger",
        reason: error.to_string(),
    })
}

/// A finite adapter between two generation epochs.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PlanningCellTransitionAdapterV1 {
    adapter_id: StableId,
    adapter_version: NonZeroU32,
    adapter_hash: CanonicalHash,
    epoch_a: GenerationEpochIdV1,
    epoch_b: GenerationEpochIdV1,
    transition_width_cells: NonZeroU32,
    terrain_boundary_signature: CanonicalHash,
    required_cave_portals: Vec<CavePortalIdV1>,
    maximum_work_units: u64,
}

impl PlanningCellTransitionAdapterV1 {
    /// Creates a direction-independent, finite transition adapter.
    ///
    /// # Errors
    ///
    /// Returns an error for the wrong ID kind, equal epochs, duplicated portals,
    /// or a zero work budget.
    #[allow(clippy::too_many_arguments, reason = "transition evidence is explicit")]
    pub fn new(
        adapter_id: StableId,
        adapter_version: NonZeroU32,
        adapter_hash: CanonicalHash,
        first_epoch: GenerationEpochIdV1,
        second_epoch: GenerationEpochIdV1,
        transition_width_cells: NonZeroU32,
        terrain_boundary_signature: CanonicalHash,
        mut required_cave_portals: Vec<CavePortalIdV1>,
        maximum_work_units: u64,
    ) -> TerritoryResult<Self> {
        if adapter_id.kind() != "transition-adapter" {
            return Err(TerritoryError::InvalidStableKind {
                value: adapter_id.to_string(),
                expected: "transition-adapter",
            });
        }
        if first_epoch == second_epoch || maximum_work_units == 0 {
            return Err(TerritoryError::TransitionEpochMismatch);
        }
        required_cave_portals.sort();
        if required_cave_portals
            .windows(2)
            .any(|pair| pair[0] == pair[1])
        {
            return Err(TerritoryError::TransitionPortalMismatch);
        }
        let (epoch_a, epoch_b) = canonical_epoch_pair(first_epoch, second_epoch);
        Ok(Self {
            adapter_id,
            adapter_version,
            adapter_hash,
            epoch_a,
            epoch_b,
            transition_width_cells,
            terrain_boundary_signature,
            required_cave_portals,
            maximum_work_units,
        })
    }

    /// Returns the canonical epoch pair.
    #[must_use]
    pub const fn epochs(&self) -> (GenerationEpochIdV1, GenerationEpochIdV1) {
        (self.epoch_a, self.epoch_b)
    }

    /// Returns required cave portal IDs.
    #[must_use]
    pub fn required_cave_portals(&self) -> &[CavePortalIdV1] {
        &self.required_cave_portals
    }

    /// Revalidates canonical adapter evidence after deserialization.
    ///
    /// # Errors
    ///
    /// Returns an error for the wrong ID kind, non-canonical epochs or portals,
    /// or a zero work budget.
    pub fn validate(&self) -> TerritoryResult<()> {
        if self.adapter_id.kind() != "transition-adapter" {
            return Err(TerritoryError::InvalidStableKind {
                value: self.adapter_id.to_string(),
                expected: "transition-adapter",
            });
        }
        if self.epoch_a >= self.epoch_b || self.maximum_work_units == 0 {
            return Err(TerritoryError::TransitionEpochMismatch);
        }
        if self
            .required_cave_portals
            .windows(2)
            .any(|pair| pair[0] >= pair[1])
        {
            return Err(TerritoryError::TransitionPortalMismatch);
        }
        Ok(())
    }
}

#[derive(Serialize)]
struct TransitionHashPayloadV1<'a> {
    dimension: &'a DimensionId,
    first_cell: PlanningCellCoordinateV1,
    second_cell: PlanningCellCoordinateV1,
    epoch_a: GenerationEpochIdV1,
    epoch_b: GenerationEpochIdV1,
    adapter_id: &'a StableId,
    adapter_version: NonZeroU32,
    adapter_hash: CanonicalHash,
    transition_width_cells: NonZeroU32,
    terrain_boundary_signature: CanonicalHash,
    required_cave_portals: &'a [CavePortalIdV1],
    maximum_work_units: u64,
}

/// Validated, direction-independent transition evidence for an adjacent cell pair.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PlanningCellTransitionReceiptV1 {
    receipt_hash: TransitionReceiptHashV1,
    dimension: DimensionId,
    first_cell: PlanningCellCoordinateV1,
    second_cell: PlanningCellCoordinateV1,
    epoch_a: GenerationEpochIdV1,
    epoch_b: GenerationEpochIdV1,
    adapter_id: StableId,
    adapter_version: NonZeroU32,
    adapter_hash: CanonicalHash,
    transition_width_cells: NonZeroU32,
    terrain_boundary_signature: CanonicalHash,
    required_cave_portals: Vec<CavePortalIdV1>,
    maximum_work_units: u64,
}

impl PlanningCellTransitionReceiptV1 {
    /// Issues receipt evidence after checking frozen epochs and cave portals.
    ///
    /// # Errors
    ///
    /// Returns an error for non-adjacent or unfrozen cells, an epoch mismatch,
    /// portal mismatch, or canonical encoding failure.
    pub fn issue(
        ledger: &PlanningCellEpochLedgerV1,
        first_cell: PlanningCellCoordinateV1,
        second_cell: PlanningCellCoordinateV1,
        adapter: &PlanningCellTransitionAdapterV1,
        cave_adjacency: &CaveAdjacencyV1,
    ) -> TerritoryResult<Self> {
        ledger.validate()?;
        adapter.validate()?;
        cave_adjacency.validate()?;
        validate_cardinal(first_cell, second_cell)?;
        let first_epoch = ledger
            .epoch(first_cell)
            .ok_or(TerritoryError::CellEpochUnassigned { cell: first_cell })?;
        let second_epoch = ledger
            .epoch(second_cell)
            .ok_or(TerritoryError::CellEpochUnassigned { cell: second_cell })?;
        if canonical_epoch_pair(first_epoch, second_epoch) != adapter.epochs() {
            return Err(TerritoryError::TransitionEpochMismatch);
        }
        let (first_cell, second_cell) = canonical_cell_pair(first_cell, second_cell);
        validate_required_portals(adapter.required_cave_portals(), cave_adjacency.portal_ids())?;
        if cave_adjacency.cells() != (first_cell, second_cell)
            || cave_adjacency.portal_ids() != adapter.required_cave_portals
        {
            return Err(TerritoryError::TransitionPortalMismatch);
        }
        let payload = TransitionHashPayloadV1 {
            dimension: &ledger.dimension,
            first_cell,
            second_cell,
            epoch_a: adapter.epoch_a,
            epoch_b: adapter.epoch_b,
            adapter_id: &adapter.adapter_id,
            adapter_version: adapter.adapter_version,
            adapter_hash: adapter.adapter_hash,
            transition_width_cells: adapter.transition_width_cells,
            terrain_boundary_signature: adapter.terrain_boundary_signature,
            required_cave_portals: &adapter.required_cave_portals,
            maximum_work_units: adapter.maximum_work_units,
        };
        let hash =
            canonical_json_hash(&payload).map_err(|error| TerritoryError::CanonicalEncoding {
                kind: "planning-cell transition receipt",
                reason: error.to_string(),
            })?;
        Ok(Self {
            receipt_hash: TransitionReceiptHashV1::from_hash(hash),
            dimension: ledger.dimension.clone(),
            first_cell,
            second_cell,
            epoch_a: adapter.epoch_a,
            epoch_b: adapter.epoch_b,
            adapter_id: adapter.adapter_id.clone(),
            adapter_version: adapter.adapter_version,
            adapter_hash: adapter.adapter_hash,
            transition_width_cells: adapter.transition_width_cells,
            terrain_boundary_signature: adapter.terrain_boundary_signature,
            required_cave_portals: adapter.required_cave_portals.clone(),
            maximum_work_units: adapter.maximum_work_units,
        })
    }

    /// Returns the direction-independent receipt hash.
    #[must_use]
    pub const fn receipt_hash(&self) -> TransitionReceiptHashV1 {
        self.receipt_hash
    }

    /// Returns the canonical cell pair.
    #[must_use]
    pub const fn cells(&self) -> (PlanningCellCoordinateV1, PlanningCellCoordinateV1) {
        (self.first_cell, self.second_cell)
    }
}

fn canonical_epoch_pair(
    first: GenerationEpochIdV1,
    second: GenerationEpochIdV1,
) -> (GenerationEpochIdV1, GenerationEpochIdV1) {
    if first <= second {
        (first, second)
    } else {
        (second, first)
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
