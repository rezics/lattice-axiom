//! Bounded water/lava tick adapter over the production spine.
//!
//! Plans run on the host thread that already owns the memory kernel. Completions
//! apply only after [`admit_fluid_completion`] matches the captured chunk
//! revision tuple. Stale work never overwrites a newer revision.

use std::num::NonZeroU32;

use latticeaxiom_content::{FluidFlowV1, FluidLevelV1, FluidStateV1, SolidOccupancyKindV1};
use latticeaxiom_core::StableId;
use latticeaxiom_player::BlockEditRejectV1;
use latticeaxiom_storage::{ChunkCoordinate, MemoryTransactionKernel};
use latticeaxiom_voxel_runtime::{
    FluidFlow, FluidLayerCell, FluidRevisionStamp, FluidRuntimeError, FluidTickPlan,
    FluidUpdateBudget, SolidFluidRuntimeCell, admit_fluid_completion, plan_fluid_tick,
};

use super::spine::{HostVoxel, ProductionSpineInner, canonical_index, runtime_chunk_cells};

/// Result of one host-side bounded fluid apply.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HostFluidTickV1 {
    /// Chunk that was planned.
    pub coordinate: ChunkCoordinate,
    /// Cells whose fluid layer changed.
    pub cells_changed: u32,
    /// High-water frontier depth.
    pub queue_depth: u32,
    /// Accounted in-flight bytes.
    pub in_flight_bytes: u32,
}

pub(super) fn tick_chunk(
    inner: &mut ProductionSpineInner,
    kernel: &MemoryTransactionKernel,
    coordinate: ChunkCoordinate,
) -> Result<Option<HostFluidTickV1>, BlockEditRejectV1> {
    let Some((world, chunk, voxel)) = inner.runtime.chunk_revisions(coordinate) else {
        return Ok(None);
    };
    let captured = FluidRevisionStamp::new(world, chunk, voxel);
    let host_cells = runtime_chunk_cells(inner, coordinate);
    let cells = host_cells
        .iter()
        .map(|cell| to_runtime_cell(inner, *cell))
        .collect::<Result<Vec<_>, _>>()?;
    let frontier = fluid_frontier(&cells, inner.chunk_edge);
    if frontier.is_empty() {
        return Ok(None);
    }
    let budget = host_fluid_budget(inner);
    let plan = plan_fluid_tick(coordinate, captured, &cells, &frontier, budget)
        .map_err(|error| map_fluid_error(&error))?;
    let Some((current_world, current_chunk, current_voxel)) =
        inner.runtime.chunk_revisions(coordinate)
    else {
        return Ok(None);
    };
    admit_fluid_completion(
        plan.captured(),
        FluidRevisionStamp::new(current_world, current_chunk, current_voxel),
    )
    .map_err(|error| match error {
        FluidRuntimeError::Stale { .. } => BlockEditRejectV1::StaleRevision {
            expected: captured.chunk().get(),
            actual: current_chunk.get(),
        },
        other => map_fluid_error(&other),
    })?;
    apply_plan(inner, kernel, &plan)?;
    Ok(Some(HostFluidTickV1 {
        coordinate: plan.coordinate(),
        cells_changed: plan.cells_changed(),
        queue_depth: plan.queue_depth(),
        in_flight_bytes: plan.in_flight_bytes(),
    }))
}

pub(super) fn tick_resident(
    inner: &mut ProductionSpineInner,
    kernel: &MemoryTransactionKernel,
) -> Result<Vec<HostFluidTickV1>, BlockEditRejectV1> {
    let coordinates = inner.runtime.resident_coordinates().collect::<Vec<_>>();
    let mut applied = Vec::new();
    for coordinate in coordinates {
        if let Some(tick) = tick_chunk(inner, kernel, coordinate)? {
            applied.push(tick);
        }
    }
    Ok(applied)
}

fn apply_plan(
    inner: &mut ProductionSpineInner,
    kernel: &MemoryTransactionKernel,
    plan: &FluidTickPlan,
) -> Result<(), BlockEditRejectV1> {
    if plan.mutations().is_empty() {
        return Ok(());
    }
    let edge = inner.chunk_edge;
    let mut cells = runtime_chunk_cells(inner, plan.coordinate());
    let mut changed = false;
    for mutation in plan.mutations() {
        let index = canonical_index(
            usize::from(edge),
            usize::from(mutation.x()),
            usize::from(mutation.y()),
            usize::from(mutation.z()),
        );
        let Some(old) = cells.get(index).copied() else {
            return Err(BlockEditRejectV1::StorageUnavailable);
        };
        let fluid_index = fluid_layer_index(inner, mutation.to())?;
        let new_voxel = HostVoxel::occupancy(old.palette_index, fluid_index, &inner.presentation);
        if new_voxel == old {
            continue;
        }
        cells[index] = new_voxel;
        changed = true;
    }
    if !changed {
        return Ok(());
    }
    inner.commit_chunk_voxels(kernel, plan.coordinate(), &cells)
}

fn to_runtime_cell(
    inner: &ProductionSpineInner,
    voxel: HostVoxel,
) -> Result<SolidFluidRuntimeCell, BlockEditRejectV1> {
    Ok(SolidFluidRuntimeCell {
        solid_occupied: solid_occupied(inner, voxel)?,
        fluid: fluid_layer(inner, voxel.fluid_palette_index),
    })
}

fn solid_occupied(
    inner: &ProductionSpineInner,
    voxel: HostVoxel,
) -> Result<bool, BlockEditRejectV1> {
    let Some(block) = inner.palette.get(usize::from(voxel.palette_index)) else {
        return Ok(false);
    };
    let id: StableId = block
        .as_str()
        .parse()
        .map_err(|_| BlockEditRejectV1::ContentUnavailable)?;
    let semantics = inner
        .block_semantics(&id)
        .ok_or(BlockEditRejectV1::ContentUnavailable)?;
    Ok(matches!(
        SolidOccupancyKindV1::classify(&semantics.solid_occupancy)
            .map_err(|_| BlockEditRejectV1::ContentUnavailable)?,
        SolidOccupancyKindV1::Full | SolidOccupancyKindV1::Partial
    ))
}

fn fluid_layer(inner: &ProductionSpineInner, palette_index: u16) -> FluidLayerCell {
    let Some(entry) = inner
        .fluid_palette
        .entries()
        .get(usize::from(palette_index))
    else {
        return FluidLayerCell::Empty;
    };
    match (entry.fluid(), entry.state()) {
        (Some(id), Some(state)) => FluidLayerCell::Fluid {
            id: id.clone(),
            level: state.level.get(),
            flow: to_runtime_flow(state.flow),
        },
        _ => FluidLayerCell::Empty,
    }
}

fn fluid_layer_index(
    inner: &ProductionSpineInner,
    layer: &FluidLayerCell,
) -> Result<u16, BlockEditRejectV1> {
    match layer {
        FluidLayerCell::Empty => Ok(0),
        FluidLayerCell::Fluid { id, level, flow } => {
            let state = FluidStateV1 {
                level: FluidLevelV1::new(*level)
                    .map_err(|_| BlockEditRejectV1::ContentUnavailable)?,
                flow: from_runtime_flow(*flow),
            };
            inner
                .fluid_palette
                .entries()
                .iter()
                .position(|entry| entry.fluid() == Some(id) && entry.state() == Some(&state))
                .and_then(|index| u16::try_from(index).ok())
                .ok_or(BlockEditRejectV1::ContentUnavailable)
        }
    }
}

fn fluid_frontier(cells: &[SolidFluidRuntimeCell], edge: u16) -> Vec<(u16, u16, u16)> {
    let mut frontier = Vec::new();
    for y in 0..edge {
        for z in 0..edge {
            for x in 0..edge {
                let index = canonical_index(
                    usize::from(edge),
                    usize::from(x),
                    usize::from(y),
                    usize::from(z),
                );
                if cells.get(index).is_some_and(|cell| !cell.fluid.is_empty()) {
                    frontier.push((x, y, z));
                }
            }
        }
    }
    frontier
}

fn host_fluid_budget(inner: &ProductionSpineInner) -> FluidUpdateBudget {
    let mut cells = u32::MAX;
    let mut queue = u32::MAX;
    let mut bytes = u32::MAX;
    for fluid in inner.content.fluids() {
        cells = cells.min(fluid.update_policy.max_cells_per_tick.get());
        queue = queue.min(fluid.update_policy.max_queue_depth.get());
        bytes = bytes.min(fluid.update_policy.max_in_flight_bytes.get());
    }
    FluidUpdateBudget::new(nonzero(cells), nonzero(queue), nonzero(bytes))
}

fn nonzero(value: u32) -> NonZeroU32 {
    match NonZeroU32::new(value.max(1)) {
        Some(bound) => bound,
        None => NonZeroU32::MIN,
    }
}

const fn to_runtime_flow(flow: FluidFlowV1) -> FluidFlow {
    match flow {
        FluidFlowV1::Still => FluidFlow::Still,
        FluidFlowV1::Down => FluidFlow::Down,
        FluidFlowV1::East => FluidFlow::East,
        FluidFlowV1::West => FluidFlow::West,
        FluidFlowV1::South => FluidFlow::South,
        FluidFlowV1::North => FluidFlow::North,
    }
}

const fn from_runtime_flow(flow: FluidFlow) -> FluidFlowV1 {
    match flow {
        FluidFlow::Still => FluidFlowV1::Still,
        FluidFlow::Down => FluidFlowV1::Down,
        FluidFlow::East => FluidFlowV1::East,
        FluidFlow::West => FluidFlowV1::West,
        FluidFlow::South => FluidFlowV1::South,
        FluidFlow::North => FluidFlowV1::North,
    }
}

fn map_fluid_error(error: &FluidRuntimeError) -> BlockEditRejectV1 {
    match error {
        FluidRuntimeError::Stale { .. } => BlockEditRejectV1::StaleRevision {
            expected: 0,
            actual: 0,
        },
        _ => BlockEditRejectV1::ContentUnavailable,
    }
}
