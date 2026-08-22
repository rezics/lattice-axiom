//! Bounded water/lava update planning with stale-revision rejection.
//!
//! This module never writes storage, never advances authoritative revisions,
//! and never starts a project-owned thread. Hosts dispatch the planner on Bevy
//! task pools and apply mutations only after [`admit_fluid_completion`].

use std::{collections::BTreeSet, num::NonZeroU32};

use latticeaxiom_core::StableId;
use latticeaxiom_storage::{ChunkCoordinate, ChunkRevision, VoxelRevision, WorldRevision};
use thiserror::Error;

use crate::StaleReason;

/// Frozen v1 cubic chunk edge used by fluid index math.
pub const FLUID_CHUNK_EDGE_V1: u16 = 32;

/// Conservative retained bytes charged for one queued or mutated fluid cell.
const CELL_ACCOUNT_BYTES: u32 = 64;

/// Result of a bounded fluid update.
pub type FluidRuntimeResult<T> = Result<T, FluidRuntimeError>;

/// Failures detected before a fluid plan is published or applied.
#[derive(Clone, Debug, Error, PartialEq, Eq)]
pub enum FluidRuntimeError {
    /// A configured hard bound was zero.
    #[error("fluid update limit {name} must be greater than zero")]
    InvalidLimit {
        /// Stable limit name.
        name: &'static str,
    },
    /// A fluid level was outside `0..=7`.
    #[error("fluid level {actual} is outside 0..=7")]
    InvalidLevel {
        /// Rejected level.
        actual: u8,
    },
    /// A local coordinate was outside the cubic chunk.
    #[error("fluid local coordinate ({x}, {y}, {z}) exceeds edge {edge}")]
    LocalOutOfRange {
        /// Rejected X.
        x: u16,
        /// Rejected Y.
        y: u16,
        /// Rejected Z.
        z: u16,
        /// Active cubic edge.
        edge: u16,
    },
    /// A per-tick cell, queue, or in-flight bound was crossed.
    #[error("{resource} count {actual} exceeds limit {limit}")]
    LimitExceeded {
        /// Bounded resource name.
        resource: &'static str,
        /// Observed count or bytes.
        actual: u32,
        /// Inclusive hard limit.
        limit: u32,
    },
    /// Two different fluids would occupy one cell.
    #[error("fluid mixing is forbidden")]
    Mixing,
    /// A completion no longer matches the captured chunk revision tuple.
    #[error("stale fluid task: {reason:?}")]
    Stale {
        /// Closed stale reason.
        reason: StaleReason,
    },
}

/// Versioned collision kind consumed at the runtime boundary.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum FluidCollisionKind {
    /// Fluid never occupies collision volume.
    None,
    /// Any non-empty fluid cell occupies collision volume.
    Volume,
}

/// Versioned selection kind consumed at the runtime boundary.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum FluidSelectionKind {
    /// Fluid is never selectable.
    None,
    /// Only source/full cells are selectable.
    Source,
    /// Any non-empty fluid cell is selectable.
    Volume,
}

/// Explicit authoritative flow; numeric tags are independent of Rust order.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum FluidFlow {
    /// No directional flow.
    Still,
    /// Negative Y.
    Down,
    /// Positive X.
    East,
    /// Negative X.
    West,
    /// Positive Z.
    South,
    /// Negative Z.
    North,
}

impl FluidFlow {
    /// Closed v1 numeric tags.
    pub const TAG_STILL: u16 = 0;
    /// Down tag.
    pub const TAG_DOWN: u16 = 1;
    /// East tag.
    pub const TAG_EAST: u16 = 2;
    /// West tag.
    pub const TAG_WEST: u16 = 3;
    /// South tag.
    pub const TAG_SOUTH: u16 = 4;
    /// North tag.
    pub const TAG_NORTH: u16 = 5;

    /// Horizontal spread order: east, west, south, north.
    pub const HORIZONTAL: [Self; 4] = [Self::East, Self::West, Self::South, Self::North];

    /// Returns the explicit numeric tag.
    #[must_use]
    pub const fn tag(self) -> u16 {
        match self {
            Self::Still => Self::TAG_STILL,
            Self::Down => Self::TAG_DOWN,
            Self::East => Self::TAG_EAST,
            Self::West => Self::TAG_WEST,
            Self::South => Self::TAG_SOUTH,
            Self::North => Self::TAG_NORTH,
        }
    }

    /// Decodes an explicit numeric tag.
    #[must_use]
    pub const fn from_tag(tag: u16) -> Option<Self> {
        match tag {
            Self::TAG_STILL => Some(Self::Still),
            Self::TAG_DOWN => Some(Self::Down),
            Self::TAG_EAST => Some(Self::East),
            Self::TAG_WEST => Some(Self::West),
            Self::TAG_SOUTH => Some(Self::South),
            Self::TAG_NORTH => Some(Self::North),
            _ => None,
        }
    }

    const fn step(self) -> (i32, i32, i32) {
        match self {
            Self::Still => (0, 0, 0),
            Self::Down => (0, -1, 0),
            Self::East => (1, 0, 0),
            Self::West => (-1, 0, 0),
            Self::South => (0, 0, 1),
            Self::North => (0, 0, -1),
        }
    }
}

/// One orthogonal fluid layer value.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum FluidLayerCell {
    /// Canonical empty fluid.
    Empty,
    /// Exact fluid identity plus frozen per-cell state.
    Fluid {
        /// Exact fluid identity. Never a process-local integer.
        id: StableId,
        /// Inclusive `0..=7` level; zero is source/full.
        level: u8,
        /// Explicit flow.
        flow: FluidFlow,
    },
}

impl FluidLayerCell {
    /// Returns whether this cell has no fluid.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        matches!(self, Self::Empty)
    }

    /// Returns whether this is a source/full cell.
    #[must_use]
    pub const fn is_source(&self) -> bool {
        match self {
            Self::Empty => false,
            Self::Fluid { level, .. } => *level == 0,
        }
    }

    /// Returns the exact fluid identity when present.
    #[must_use]
    pub fn identity(&self) -> Option<&StableId> {
        match self {
            Self::Empty => None,
            Self::Fluid { id, .. } => Some(id),
        }
    }

    /// Returns the canonical numeric level when present.
    #[must_use]
    pub const fn level(&self) -> Option<u8> {
        match self {
            Self::Empty => None,
            Self::Fluid { level, .. } => Some(*level),
        }
    }
}

/// One dual-layer runtime cell.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SolidFluidRuntimeCell {
    /// Whether the solid layer occupies the cell.
    pub solid_occupied: bool,
    /// Orthogonal fluid layer.
    pub fluid: FluidLayerCell,
}

impl SolidFluidRuntimeCell {
    /// Empty solid and empty fluid.
    #[must_use]
    pub const fn empty() -> Self {
        Self {
            solid_occupied: false,
            fluid: FluidLayerCell::Empty,
        }
    }

    /// Returns whether collision queries must treat the cell as occupied.
    #[must_use]
    pub const fn collision_occupied(&self, fluid_kind: FluidCollisionKind) -> bool {
        self.solid_occupied
            || (matches!(fluid_kind, FluidCollisionKind::Volume) && !self.fluid.is_empty())
    }

    /// Returns whether the fluid layer is a gameplay selection target.
    #[must_use]
    pub const fn fluid_selectable(&self, fluid_kind: FluidSelectionKind) -> bool {
        if self.solid_occupied || self.fluid.is_empty() {
            return false;
        }
        match fluid_kind {
            FluidSelectionKind::None => false,
            FluidSelectionKind::Source => self.fluid.is_source(),
            FluidSelectionKind::Volume => true,
        }
    }
}

/// Hard per-tick cell, queue, and in-flight bounds.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[allow(
    clippy::struct_field_names,
    reason = "the max prefix makes each independently enforced fluid budget explicit"
)]
pub struct FluidUpdateBudget {
    max_cells_per_tick: u32,
    max_queue_depth: u32,
    max_in_flight_bytes: u32,
}

impl FluidUpdateBudget {
    /// Validates nonzero hard bounds.
    ///
    /// # Errors
    ///
    /// Returns [`FluidRuntimeError::InvalidLimit`] when any bound is zero.
    #[must_use]
    pub const fn new(
        max_cells_per_tick: NonZeroU32,
        max_queue_depth: NonZeroU32,
        max_in_flight_bytes: NonZeroU32,
    ) -> Self {
        Self {
            max_cells_per_tick: max_cells_per_tick.get(),
            max_queue_depth: max_queue_depth.get(),
            max_in_flight_bytes: max_in_flight_bytes.get(),
        }
    }

    /// Maximum cells examined or changed in one tick.
    #[must_use]
    pub const fn max_cells_per_tick(self) -> u32 {
        self.max_cells_per_tick
    }

    /// Maximum queued frontier entries.
    #[must_use]
    pub const fn max_queue_depth(self) -> u32 {
        self.max_queue_depth
    }

    /// Maximum accounted in-flight bytes.
    #[must_use]
    pub const fn max_in_flight_bytes(self) -> u32 {
        self.max_in_flight_bytes
    }
}

/// Captured revision tuple used to reject stale fluid work.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FluidRevisionStamp {
    world: WorldRevision,
    chunk: ChunkRevision,
    voxel: VoxelRevision,
}

impl FluidRevisionStamp {
    /// Creates a captured or current revision tuple.
    #[must_use]
    pub const fn new(world: WorldRevision, chunk: ChunkRevision, voxel: VoxelRevision) -> Self {
        Self {
            world,
            chunk,
            voxel,
        }
    }

    /// Returns the world revision.
    #[must_use]
    pub const fn world(self) -> WorldRevision {
        self.world
    }

    /// Returns the complete chunk revision.
    #[must_use]
    pub const fn chunk(self) -> ChunkRevision {
        self.chunk
    }

    /// Returns the voxel-domain revision.
    #[must_use]
    pub const fn voxel(self) -> VoxelRevision {
        self.voxel
    }
}

/// Local cell mutation produced by a planned tick.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FluidCellMutation {
    x: u16,
    y: u16,
    z: u16,
    from: FluidLayerCell,
    to: FluidLayerCell,
}

impl FluidCellMutation {
    /// Local X.
    #[must_use]
    pub const fn x(&self) -> u16 {
        self.x
    }

    /// Local Y.
    #[must_use]
    pub const fn y(&self) -> u16 {
        self.y
    }

    /// Local Z.
    #[must_use]
    pub const fn z(&self) -> u16 {
        self.z
    }

    /// Fluid layer before the planned write.
    #[must_use]
    pub const fn from(&self) -> &FluidLayerCell {
        &self.from
    }

    /// Fluid layer after the planned write.
    #[must_use]
    pub const fn to(&self) -> &FluidLayerCell {
        &self.to
    }
}

/// Neighbor-chunk intent produced when flow crosses a chunk face.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FluidBoundaryIntent {
    neighbor: ChunkCoordinate,
    x: u16,
    y: u16,
    z: u16,
    fluid: FluidLayerCell,
}

impl FluidBoundaryIntent {
    /// Neighbor chunk coordinate.
    #[must_use]
    pub const fn neighbor(&self) -> ChunkCoordinate {
        self.neighbor
    }

    /// Neighbor-local X.
    #[must_use]
    pub const fn x(&self) -> u16 {
        self.x
    }

    /// Neighbor-local Y.
    #[must_use]
    pub const fn y(&self) -> u16 {
        self.y
    }

    /// Neighbor-local Z.
    #[must_use]
    pub const fn z(&self) -> u16 {
        self.z
    }

    /// Fluid that would occupy the neighbor cell.
    #[must_use]
    pub const fn fluid(&self) -> &FluidLayerCell {
        &self.fluid
    }
}

/// Bounded, non-authoritative fluid tick plan.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FluidTickPlan {
    captured: FluidRevisionStamp,
    coordinate: ChunkCoordinate,
    cells_examined: u32,
    cells_changed: u32,
    queue_depth: u32,
    in_flight_bytes: u32,
    mutations: Vec<FluidCellMutation>,
    boundaries: Vec<FluidBoundaryIntent>,
    next_frontier: Vec<(u16, u16, u16)>,
}

impl FluidTickPlan {
    /// Revision tuple captured when the plan was produced.
    #[must_use]
    pub const fn captured(&self) -> FluidRevisionStamp {
        self.captured
    }

    /// Chunk that was planned.
    #[must_use]
    pub const fn coordinate(&self) -> ChunkCoordinate {
        self.coordinate
    }

    /// Cells examined against the hard cell bound.
    #[must_use]
    pub const fn cells_examined(&self) -> u32 {
        self.cells_examined
    }

    /// Cells whose fluid layer would change.
    #[must_use]
    pub const fn cells_changed(&self) -> u32 {
        self.cells_changed
    }

    /// High-water frontier depth.
    #[must_use]
    pub const fn queue_depth(&self) -> u32 {
        self.queue_depth
    }

    /// Accounted in-flight bytes.
    #[must_use]
    pub const fn in_flight_bytes(&self) -> u32 {
        self.in_flight_bytes
    }

    /// Planned in-chunk mutations in `(y, z, x)` order.
    #[must_use]
    pub fn mutations(&self) -> &[FluidCellMutation] {
        &self.mutations
    }

    /// Planned chunk-boundary intents in `(y, z, x)` order.
    #[must_use]
    pub fn boundaries(&self) -> &[FluidBoundaryIntent] {
        &self.boundaries
    }

    /// Cells scheduled for the next tick, in `(y, z, x)` order.
    #[must_use]
    pub fn next_frontier(&self) -> &[(u16, u16, u16)] {
        &self.next_frontier
    }
}

/// Rejects a fluid completion that no longer matches committed revisions.
///
/// # Errors
///
/// Returns [`FluidRuntimeError::Stale`] when any captured revision disagrees
/// with the current committed tuple. The caller must not apply the plan.
pub fn admit_fluid_completion(
    captured: FluidRevisionStamp,
    current: FluidRevisionStamp,
) -> FluidRuntimeResult<()> {
    if captured.world != current.world {
        return Err(FluidRuntimeError::Stale {
            reason: StaleReason::WorldRevision,
        });
    }
    if captured.chunk != current.chunk {
        return Err(FluidRuntimeError::Stale {
            reason: StaleReason::ChunkRevision,
        });
    }
    if captured.voxel != current.voxel {
        return Err(FluidRuntimeError::Stale {
            reason: StaleReason::VoxelRevision,
        });
    }
    Ok(())
}

/// Plans one bounded fluid tick over a dense dual-layer chunk.
///
/// v1 rules: no upward or diagonal flow, no mixing, no waterlogging of a solid
/// cell, down first, then east/west/south/north. Source cells remain. Spreading
/// from level 7 is refused. Work stops with a typed error when a hard bound is
/// crossed rather than silently truncating.
///
/// # Errors
///
/// Returns a bound, range, mixing, or level error before producing a plan.
#[allow(
    clippy::too_many_lines,
    reason = "one tick keeps frontier accounting, in-chunk spread, and boundary intents atomic"
)]
pub fn plan_fluid_tick(
    coordinate: ChunkCoordinate,
    captured: FluidRevisionStamp,
    cells: &[SolidFluidRuntimeCell],
    frontier: &[(u16, u16, u16)],
    budget: FluidUpdateBudget,
) -> FluidRuntimeResult<FluidTickPlan> {
    let edge = FLUID_CHUNK_EDGE_V1;
    let expected = volume_cell_count(edge)?;
    if cells.len() != expected {
        return Err(FluidRuntimeError::LocalOutOfRange {
            x: 0,
            y: 0,
            z: 0,
            edge,
        });
    }

    let mut queue = BTreeSet::new();
    for &(x, y, z) in frontier {
        let _ = linear_index(edge, x, y, z)?;
        enqueue(&mut queue, x, y, z, budget)?;
    }

    let mut cells_examined = 0_u32;
    let mut cells_changed = 0_u32;
    let mut mutations = Vec::new();
    let mut boundaries = Vec::new();
    let mut next_frontier = BTreeSet::new();
    let mut planned = cells.to_vec();
    for (y, z, x) in queue {
        cells_examined = saturating_add(cells_examined, 1);
        enforce(
            "fluid_cells_per_tick",
            cells_examined,
            budget.max_cells_per_tick,
        )?;
        let index = linear_index(edge, x, y, z)?;
        let source = planned[index].clone();
        if source.solid_occupied || source.fluid.is_empty() {
            continue;
        }
        let FluidLayerCell::Fluid { id, level, flow: _ } = source.fluid.clone() else {
            continue;
        };
        if level > 7 {
            return Err(FluidRuntimeError::InvalidLevel { actual: level });
        }

        if try_spread(
            coordinate,
            edge,
            &mut planned,
            x,
            y,
            z,
            &id,
            level,
            FluidFlow::Down,
            &mut mutations,
            &mut boundaries,
            &mut next_frontier,
            &mut cells_changed,
            budget,
        )? {
            continue;
        }
        if level >= 7 {
            continue;
        }
        for flow in FluidFlow::HORIZONTAL {
            let _ = try_spread(
                coordinate,
                edge,
                &mut planned,
                x,
                y,
                z,
                &id,
                level,
                flow,
                &mut mutations,
                &mut boundaries,
                &mut next_frontier,
                &mut cells_changed,
                budget,
            )?;
        }
    }

    mutations.sort_by(|left, right| {
        left.y
            .cmp(&right.y)
            .then(left.z.cmp(&right.z))
            .then(left.x.cmp(&right.x))
    });
    boundaries.sort_by(|left, right| {
        left.neighbor
            .y
            .cmp(&right.neighbor.y)
            .then(left.neighbor.z.cmp(&right.neighbor.z))
            .then(left.neighbor.x.cmp(&right.neighbor.x))
            .then(left.y.cmp(&right.y))
            .then(left.z.cmp(&right.z))
            .then(left.x.cmp(&right.x))
    });
    let next_frontier = next_frontier
        .into_iter()
        .map(|(y, z, x)| (x, y, z))
        .collect::<Vec<_>>();

    let queue_depth =
        u32::try_from(frontier.len().saturating_add(next_frontier.len())).unwrap_or(u32::MAX);
    let in_flight_bytes = saturating_mul(
        saturating_add(
            u32::try_from(mutations.len()).unwrap_or(u32::MAX),
            u32::try_from(boundaries.len()).unwrap_or(u32::MAX),
        ),
        CELL_ACCOUNT_BYTES,
    );
    enforce(
        "fluid_in_flight_bytes",
        in_flight_bytes,
        budget.max_in_flight_bytes,
    )?;

    Ok(FluidTickPlan {
        captured,
        coordinate,
        cells_examined,
        cells_changed,
        queue_depth,
        in_flight_bytes,
        mutations,
        boundaries,
        next_frontier,
    })
}

#[allow(
    clippy::too_many_arguments,
    reason = "spread keeps source cell, destination flow, and accounting in one transaction"
)]
fn try_spread(
    coordinate: ChunkCoordinate,
    edge: u16,
    planned: &mut [SolidFluidRuntimeCell],
    x: u16,
    y: u16,
    z: u16,
    id: &StableId,
    level: u8,
    flow: FluidFlow,
    mutations: &mut Vec<FluidCellMutation>,
    boundaries: &mut Vec<FluidBoundaryIntent>,
    next_frontier: &mut BTreeSet<(u16, u16, u16)>,
    cells_changed: &mut u32,
    budget: FluidUpdateBudget,
) -> FluidRuntimeResult<bool> {
    let dest_level = if flow == FluidFlow::Down {
        level
    } else {
        let Some(next) = level.checked_add(1).filter(|value| *value <= 7) else {
            return Ok(false);
        };
        next
    };
    let dest = match neighbor_cell(coordinate, edge, x, y, z, flow) {
        Neighbor::Inside {
            x: nx,
            y: ny,
            z: nz,
        } => {
            let index = linear_index(edge, nx, ny, nz)?;
            let current = planned[index].clone();
            if current.solid_occupied {
                return Ok(false);
            }
            match &current.fluid {
                FluidLayerCell::Empty => {}
                FluidLayerCell::Fluid {
                    id: existing,
                    level: existing_level,
                    ..
                } => {
                    if existing != id {
                        return Err(FluidRuntimeError::Mixing);
                    }
                    if *existing_level <= dest_level {
                        return Ok(false);
                    }
                }
            }
            let from = current.fluid.clone();
            let to = FluidLayerCell::Fluid {
                id: id.clone(),
                level: dest_level,
                flow,
            };
            planned[index].fluid = to.clone();
            *cells_changed = saturating_add(*cells_changed, 1);
            enforce(
                "fluid_cells_per_tick",
                *cells_changed,
                budget.max_cells_per_tick,
            )?;
            mutations.push(FluidCellMutation {
                x: nx,
                y: ny,
                z: nz,
                from,
                to,
            });
            enqueue(next_frontier, nx, ny, nz, budget)?;
            true
        }
        Neighbor::Boundary {
            neighbor,
            x: nx,
            y: ny,
            z: nz,
        } => {
            boundaries.push(FluidBoundaryIntent {
                neighbor,
                x: nx,
                y: ny,
                z: nz,
                fluid: FluidLayerCell::Fluid {
                    id: id.clone(),
                    level: dest_level,
                    flow,
                },
            });
            false
        }
        Neighbor::OutOfWorld => false,
    };
    Ok(dest)
}

enum Neighbor {
    Inside {
        x: u16,
        y: u16,
        z: u16,
    },
    Boundary {
        neighbor: ChunkCoordinate,
        x: u16,
        y: u16,
        z: u16,
    },
    OutOfWorld,
}

fn neighbor_cell(
    coordinate: ChunkCoordinate,
    edge: u16,
    x: u16,
    y: u16,
    z: u16,
    flow: FluidFlow,
) -> Neighbor {
    let (dx, dy, dz) = flow.step();
    let edge_i = i32::from(edge);
    let mut nx = i32::from(x) + dx;
    let mut ny = i32::from(y) + dy;
    let mut nz = i32::from(z) + dz;
    let mut chunk = coordinate;
    if nx < 0 {
        nx += edge_i;
        let Some(value) = chunk.x.checked_sub(1) else {
            return Neighbor::OutOfWorld;
        };
        chunk.x = value;
    } else if nx >= edge_i {
        nx -= edge_i;
        let Some(value) = chunk.x.checked_add(1) else {
            return Neighbor::OutOfWorld;
        };
        chunk.x = value;
    }
    if ny < 0 {
        ny += edge_i;
        let Some(value) = chunk.y.checked_sub(1) else {
            return Neighbor::OutOfWorld;
        };
        chunk.y = value;
    } else if ny >= edge_i {
        ny -= edge_i;
        let Some(value) = chunk.y.checked_add(1) else {
            return Neighbor::OutOfWorld;
        };
        chunk.y = value;
    }
    if nz < 0 {
        nz += edge_i;
        let Some(value) = chunk.z.checked_sub(1) else {
            return Neighbor::OutOfWorld;
        };
        chunk.z = value;
    } else if nz >= edge_i {
        nz -= edge_i;
        let Some(value) = chunk.z.checked_add(1) else {
            return Neighbor::OutOfWorld;
        };
        chunk.z = value;
    }
    let Ok(x) = u16::try_from(nx) else {
        return Neighbor::OutOfWorld;
    };
    let Ok(y) = u16::try_from(ny) else {
        return Neighbor::OutOfWorld;
    };
    let Ok(z) = u16::try_from(nz) else {
        return Neighbor::OutOfWorld;
    };
    if chunk == coordinate {
        Neighbor::Inside { x, y, z }
    } else {
        Neighbor::Boundary {
            neighbor: chunk,
            x,
            y,
            z,
        }
    }
}

fn enqueue(
    queue: &mut BTreeSet<(u16, u16, u16)>,
    x: u16,
    y: u16,
    z: u16,
    budget: FluidUpdateBudget,
) -> FluidRuntimeResult<()> {
    queue.insert((y, z, x));
    let depth = u32::try_from(queue.len()).unwrap_or(u32::MAX);
    enforce("fluid_queue_depth", depth, budget.max_queue_depth)
}

fn linear_index(edge: u16, x: u16, y: u16, z: u16) -> FluidRuntimeResult<usize> {
    if x >= edge || y >= edge || z >= edge {
        return Err(FluidRuntimeError::LocalOutOfRange { x, y, z, edge });
    }
    let edge = usize::from(edge);
    Ok(usize::from(x) + edge * (usize::from(z) + edge * usize::from(y)))
}

fn volume_cell_count(edge: u16) -> FluidRuntimeResult<usize> {
    let edge = usize::from(edge);
    edge.checked_mul(edge)
        .and_then(|area| area.checked_mul(edge))
        .ok_or(FluidRuntimeError::LocalOutOfRange {
            x: 0,
            y: 0,
            z: 0,
            edge: 0,
        })
}

fn enforce(resource: &'static str, actual: u32, limit: u32) -> FluidRuntimeResult<()> {
    if actual > limit {
        Err(FluidRuntimeError::LimitExceeded {
            resource,
            actual,
            limit,
        })
    } else {
        Ok(())
    }
}

fn saturating_add(left: u32, right: u32) -> u32 {
    left.saturating_add(right)
}

fn saturating_mul(left: u32, right: u32) -> u32 {
    left.saturating_mul(right)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn budget(cells: u32, queue: u32, bytes: u32) -> FluidUpdateBudget {
        FluidUpdateBudget::new(
            NonZeroU32::new(cells).unwrap_or_else(|| panic!("cells")),
            NonZeroU32::new(queue).unwrap_or_else(|| panic!("queue")),
            NonZeroU32::new(bytes).unwrap_or_else(|| panic!("bytes")),
        )
    }

    fn stamp(world: u64, chunk: u64, voxel: u64) -> FluidRevisionStamp {
        FluidRevisionStamp::new(
            WorldRevision::new(world),
            ChunkRevision::new(chunk),
            VoxelRevision::new(voxel),
        )
    }

    fn fluid_id() -> StableId {
        "fixture:fluid/alpha"
            .parse()
            .unwrap_or_else(|error| panic!("fixture fluid id is invalid: {error}"))
    }

    fn lava_id() -> StableId {
        "fixture:fluid/beta"
            .parse()
            .unwrap_or_else(|error| panic!("fixture lava id is invalid: {error}"))
    }

    fn source_cell() -> SolidFluidRuntimeCell {
        SolidFluidRuntimeCell {
            solid_occupied: false,
            fluid: FluidLayerCell::Fluid {
                id: fluid_id(),
                level: 0,
                flow: FluidFlow::Still,
            },
        }
    }

    fn empty_volume() -> Vec<SolidFluidRuntimeCell> {
        let count = volume_cell_count(FLUID_CHUNK_EDGE_V1)
            .unwrap_or_else(|error| panic!("fluid chunk cell count is invalid: {error}"));
        vec![SolidFluidRuntimeCell::empty(); count]
    }

    fn place(
        cells: &mut [SolidFluidRuntimeCell],
        x: u16,
        y: u16,
        z: u16,
        cell: SolidFluidRuntimeCell,
    ) {
        let index = linear_index(FLUID_CHUNK_EDGE_V1, x, y, z)
            .unwrap_or_else(|error| panic!("place index failed: {error}"));
        cells[index] = cell;
    }

    #[test]
    fn stale_completion_is_rejected_and_does_not_apply() {
        let captured = stamp(4, 2, 2);
        let current = stamp(5, 3, 3);
        let error = admit_fluid_completion(captured, current)
            .expect_err("newer committed revisions must stale-reject");
        assert!(matches!(
            error,
            FluidRuntimeError::Stale {
                reason: StaleReason::WorldRevision
            }
        ));
        assert!(admit_fluid_completion(captured, captured).is_ok());
        let chunk_mismatch = admit_fluid_completion(captured, stamp(4, 3, 2)).expect_err("chunk");
        assert!(matches!(
            chunk_mismatch,
            FluidRuntimeError::Stale {
                reason: StaleReason::ChunkRevision
            }
        ));
    }

    #[test]
    fn source_spreads_down_then_east_across_chunk_boundary() {
        let mut cells = empty_volume();
        place(&mut cells, 31, 1, 0, source_cell());
        let plan = plan_fluid_tick(
            ChunkCoordinate::new(-2, -3, 4),
            stamp(9, 4, 4),
            &cells,
            &[(31, 1, 0)],
            budget(64, 64, 65_536),
        )
        .unwrap_or_else(|error| panic!("plan failed: {error}"));
        assert_eq!(plan.cells_changed(), 1);
        assert_eq!(plan.mutations()[0].x(), 31);
        assert_eq!(plan.mutations()[0].y(), 0);
        assert_eq!(plan.mutations()[0].z(), 0);
        match plan.mutations()[0].to() {
            FluidLayerCell::Fluid { flow, level, .. } => {
                assert_eq!(*flow, FluidFlow::Down);
                assert_eq!(*level, 0);
            }
            FluidLayerCell::Empty => panic!("down spread must write fluid"),
        }
        assert_eq!(plan.coordinate(), ChunkCoordinate::new(-2, -3, 4));
        assert!(plan.boundaries().is_empty());

        let mut edge_cells = empty_volume();
        place(&mut edge_cells, 31, 0, 0, source_cell());
        let edge_plan = plan_fluid_tick(
            ChunkCoordinate::new(7, -1, 0),
            stamp(1, 1, 1),
            &edge_cells,
            &[(31, 0, 0)],
            budget(64, 64, 65_536),
        )
        .unwrap_or_else(|error| panic!("edge plan failed: {error}"));
        assert!(edge_plan.boundaries().iter().any(|intent| intent.neighbor()
            == ChunkCoordinate::new(8, -1, 0)
            && intent.x() == 0
            && intent.y() == 0
            && intent.z() == 0));
    }

    #[test]
    fn mixing_and_cell_bounds_fail_closed() {
        let mut cells = empty_volume();
        place(&mut cells, 0, 1, 0, source_cell());
        place(
            &mut cells,
            0,
            0,
            0,
            SolidFluidRuntimeCell {
                solid_occupied: false,
                fluid: FluidLayerCell::Fluid {
                    id: lava_id(),
                    level: 3,
                    flow: FluidFlow::Still,
                },
            },
        );
        let mixed = plan_fluid_tick(
            ChunkCoordinate::new(0, 0, 0),
            stamp(1, 1, 1),
            &cells,
            &[(0, 1, 0)],
            budget(64, 64, 65_536),
        );
        assert!(matches!(mixed, Err(FluidRuntimeError::Mixing)));

        let mut bounded = empty_volume();
        place(&mut bounded, 4, 1, 4, source_cell());
        place(&mut bounded, 8, 1, 8, source_cell());
        let over = plan_fluid_tick(
            ChunkCoordinate::new(0, 0, 0),
            stamp(1, 1, 1),
            &bounded,
            &[(4, 1, 4), (8, 1, 8)],
            budget(1, 64, 65_536),
        );
        assert!(matches!(
            over,
            Err(FluidRuntimeError::LimitExceeded {
                resource: "fluid_cells_per_tick",
                limit: 1,
                ..
            })
        ));
    }

    #[test]
    fn collision_and_selection_honor_source_policy() {
        let source = source_cell();
        let flowing = SolidFluidRuntimeCell {
            solid_occupied: false,
            fluid: FluidLayerCell::Fluid {
                id: fluid_id(),
                level: 2,
                flow: FluidFlow::East,
            },
        };
        let solid = SolidFluidRuntimeCell {
            solid_occupied: true,
            fluid: FluidLayerCell::Empty,
        };
        assert!(source.collision_occupied(FluidCollisionKind::Volume));
        assert!(!source.collision_occupied(FluidCollisionKind::None));
        assert!(source.fluid_selectable(FluidSelectionKind::Source));
        assert!(!flowing.fluid_selectable(FluidSelectionKind::Source));
        assert!(flowing.fluid_selectable(FluidSelectionKind::Volume));
        assert!(!solid.fluid_selectable(FluidSelectionKind::Volume));
        assert!(solid.collision_occupied(FluidCollisionKind::None));
    }
}
