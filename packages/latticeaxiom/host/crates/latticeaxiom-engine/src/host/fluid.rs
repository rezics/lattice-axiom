//! Deterministic, bounded water/lava execution over the production spine.
//!
//! A versioned chunk continuation is the sole source of scheduled work.
//! Planning observes one immutable multi-chunk storage snapshot, runs on Bevy's
//! compute pool when it is available, and publishes every accepted voxel and
//! continuation replacement in one optimistic world transaction.

use std::{
    cmp::Ordering,
    collections::{BTreeMap, BTreeSet, btree_map::Entry},
    num::NonZeroU32,
};

use bevy::tasks::AsyncComputeTaskPool;
use latticeaxiom_content::{FluidFlowV1, FluidLevelV1, FluidStateV1, SolidOccupancyKindV1};
use latticeaxiom_core::{SchemaId, StableId, canonical_json_bytes};
use latticeaxiom_player::BlockEditRejectV1;
use latticeaxiom_storage::{
    AuthoritativeTransactionKernel, ChunkCoordinate, ChunkData, ChunkKey, ChunkRevision,
    ContinuationId, MemoryTransactionKernel, PayloadSchemaVersion, ReferenceWorldSnapshot,
    StoredChunk, VersionedPayload,
};
use latticeaxiom_voxel_runtime::{
    FluidFlow, FluidLayerCell, FluidRevisionStamp, FluidRuntimeError, FluidTickPlan,
    FluidUpdateBudget, SolidFluidRuntimeCell, StaleReason, admit_fluid_completion, plan_fluid_tick,
};
use serde::{Deserialize, Serialize};

use super::{
    spine::{
        HostVoxel, ProductionSpineInner, canonical_index, chunk_changed_domains, decode_cells,
        voxel_payload,
    },
    stream::{StreamClamps, is_simulation_chunk},
};

const FLUID_CONTINUATION_SCHEMA: &str = "latticeaxiom:schema/fluid-continuation@1";
const FLUID_CONTINUATION_SCHEMA_VERSION: u32 = 1;
// Frozen with voxel-runtime v1's conservative per-cell accounting. A future
// fluid schema must version both sides instead of silently changing this value.
const FLUID_CELL_ACCOUNT_BYTES_V1: u32 = 64;
const FLUID_CONTINUATION_ID: ContinuationId =
    ContinuationId::from_u128(0x4c41_5846_4c55_4944_0000_0000_0000_0001);

type LocalFluidCoordinate = (u16, u16, u16);

/// Result of one host-side bounded fluid apply.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HostFluidTickV1 {
    /// Chunk whose persisted continuation was planned.
    pub coordinate: ChunkCoordinate,
    /// Accepted cells attributed to this continuation.
    pub cells_changed: u32,
    /// Persisted frontier and deferred-intent depth after the tick.
    pub queue_depth: u32,
    /// Accounted in-flight bytes for the plan.
    pub in_flight_bytes: u32,
    /// New cross-chunk intents emitted by the plan.
    pub boundary_intents: u32,
    /// Cross-chunk intents still deferred because their neighbor is unloaded.
    pub deferred_intents: u32,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
struct FluidContinuationV1 {
    frontier: Vec<LocalFluidCoordinate>,
    deferred: Vec<DeferredBoundaryIntentV1>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct DeferredBoundaryIntentV1 {
    neighbor: ChunkCoordinate,
    local: LocalFluidCoordinate,
    fluid: FluidLayerCell,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct DurableFluidContinuationV1 {
    schema_version: u32,
    frontier: Vec<[u16; 3]>,
    deferred: Vec<DurableBoundaryIntentV1>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct DurableBoundaryIntentV1 {
    neighbor: [i32; 3],
    local: [u16; 3],
    fluid: String,
    level: u8,
    flow: u16,
}

struct WorkingChunk {
    revision: ChunkRevision,
    data: ChunkData,
    cells: Vec<HostVoxel>,
}

struct FluidPlanInput {
    coordinate: ChunkCoordinate,
    captured: FluidRevisionStamp,
    continuation: FluidContinuationV1,
    working: WorkingChunk,
    runtime_cells: Vec<SolidFluidRuntimeCell>,
}

struct PlannedChunk {
    coordinate: ChunkCoordinate,
    continuation: FluidContinuationV1,
    working: WorkingChunk,
    plan: Option<FluidTickPlan>,
}

#[derive(Clone)]
struct PendingIntent {
    origin: ChunkCoordinate,
    destination: ChunkCoordinate,
    local: LocalFluidCoordinate,
    fluid: FluidLayerCell,
    kind: u8,
    sequence: u32,
}

#[derive(Clone, Copy, Default)]
struct TickAccounting {
    cells_changed: u32,
    in_flight_bytes: u32,
    boundary_intents: u32,
}

pub(super) fn activate_persisted_frontier(
    continuations: &mut BTreeMap<ContinuationId, VersionedPayload>,
    local: LocalFluidCoordinate,
) -> Result<(), BlockEditRejectV1> {
    let mut continuation = continuations
        .get(&FLUID_CONTINUATION_ID)
        .map(decode_continuation)
        .transpose()?
        .unwrap_or_default();
    continuation.frontier.push(local);
    normalize_continuation(&mut continuation)?;
    continuations.insert(FLUID_CONTINUATION_ID, encode_continuation(&continuation)?);
    Ok(())
}

#[allow(
    clippy::too_many_lines,
    reason = "the two-phase tick keeps one snapshot, stable merge, budget validation, and atomic publication in one auditable transaction boundary"
)]
pub(super) fn tick_simulated(
    inner: &mut ProductionSpineInner,
    kernel: &MemoryTransactionKernel,
    origin: ChunkCoordinate,
    clamps: StreamClamps,
) -> Result<Vec<HostFluidTickV1>, BlockEditRejectV1> {
    let snapshot = kernel
        .reference_snapshot(inner.world)
        .map_err(|_| BlockEditRejectV1::StorageUnavailable)?;
    let budget = host_fluid_budget(inner);
    let resident = inner
        .runtime
        .resident_coordinates()
        .collect::<BTreeSet<_>>();
    let mut simulation = resident
        .iter()
        .copied()
        .filter(|coordinate| is_simulation_chunk(*coordinate, origin, clamps))
        .collect::<Vec<_>>();
    simulation.sort();

    let mut input_queue = 0_u32;
    let mut inputs = Vec::new();
    for coordinate in simulation {
        let key = ChunkKey::new(inner.world, inner.dimension.clone(), coordinate);
        let Some(stored) = snapshot.chunk(&key) else {
            return Err(BlockEditRejectV1::StorageUnavailable);
        };
        let Some(continuation) = persisted_continuation(stored)? else {
            continue;
        };
        input_queue = input_queue.saturating_add(continuation_entry_count(&continuation));
        enforce_host_limit("fluid_queue_depth", input_queue, budget.max_queue_depth())?;
        enforce_host_limit(
            "fluid_in_flight_bytes",
            input_queue.saturating_mul(FLUID_CELL_ACCOUNT_BYTES_V1),
            budget.max_in_flight_bytes(),
        )?;
        inputs.push(plan_input(inner, &snapshot, coordinate, continuation)?);
    }
    if inputs.is_empty() {
        return Ok(Vec::new());
    }

    let mut planned = plan_inputs(inputs, budget)?;
    planned.sort_by_key(|chunk| chunk.coordinate);
    validate_plan_revisions(inner, kernel, &planned)?;

    let mut working = BTreeMap::new();
    let mut continuations = BTreeMap::new();
    let mut accounting = BTreeMap::new();
    let mut pending = Vec::new();
    let mut total_cells_examined = 0_u32;
    let mut total_queue_depth = 0_u32;
    let mut total_in_flight_bytes = 0_u32;

    for chunk in planned {
        let coordinate = chunk.coordinate;
        let mut next = FluidContinuationV1::default();
        let mut metrics = TickAccounting::default();
        for (sequence, deferred) in chunk.continuation.deferred.into_iter().enumerate() {
            metrics.in_flight_bytes = metrics
                .in_flight_bytes
                .saturating_add(FLUID_CELL_ACCOUNT_BYTES_V1);
            stage_boundary_intent(
                coordinate,
                deferred,
                1,
                bounded_sequence(sequence),
                &resident,
                &mut pending,
                &mut next,
            );
        }
        if let Some(plan) = chunk.plan {
            total_cells_examined = total_cells_examined.saturating_add(plan.cells_examined());
            total_queue_depth = total_queue_depth.saturating_add(plan.queue_depth());
            total_in_flight_bytes = total_in_flight_bytes.saturating_add(plan.in_flight_bytes());
            metrics.in_flight_bytes = metrics
                .in_flight_bytes
                .saturating_add(plan.in_flight_bytes());
            metrics.boundary_intents = bounded_len(plan.boundaries().len());
            next.frontier.extend_from_slice(plan.next_frontier());
            for (sequence, mutation) in plan.mutations().iter().enumerate() {
                pending.push(PendingIntent {
                    origin: coordinate,
                    destination: coordinate,
                    local: (mutation.x(), mutation.y(), mutation.z()),
                    fluid: mutation.to().clone(),
                    kind: 0,
                    sequence: bounded_sequence(sequence),
                });
            }
            for (sequence, boundary) in plan.boundaries().iter().enumerate() {
                let deferred = DeferredBoundaryIntentV1 {
                    neighbor: boundary.neighbor(),
                    local: (boundary.x(), boundary.y(), boundary.z()),
                    fluid: boundary.fluid().clone(),
                };
                stage_boundary_intent(
                    coordinate,
                    deferred,
                    2,
                    bounded_sequence(sequence),
                    &resident,
                    &mut pending,
                    &mut next,
                );
            }
        }
        normalize_continuation(&mut next)?;
        accounting.insert(coordinate, metrics);
        continuations.insert(coordinate, next);
        working.insert(coordinate, chunk.working);
    }

    enforce_host_limit(
        "fluid_cells_per_tick",
        total_cells_examined,
        budget.max_cells_per_tick(),
    )?;
    enforce_host_limit(
        "fluid_queue_depth",
        total_queue_depth,
        budget.max_queue_depth(),
    )?;
    let deferred_input_bytes = input_queue.saturating_mul(FLUID_CELL_ACCOUNT_BYTES_V1);
    enforce_host_limit(
        "fluid_in_flight_bytes",
        total_in_flight_bytes.saturating_add(deferred_input_bytes),
        budget.max_in_flight_bytes(),
    )?;

    pending.sort_by(compare_pending_intents);
    let destinations = pending
        .iter()
        .map(|intent| intent.destination)
        .collect::<BTreeSet<_>>();
    for coordinate in destinations {
        if let Entry::Vacant(entry) = working.entry(coordinate) {
            entry.insert(load_working_chunk(inner, &snapshot, coordinate)?);
        }
    }

    let mut voxel_changed = BTreeSet::new();
    let mut start = 0_usize;
    while start < pending.len() {
        let mut end = start.saturating_add(1);
        while end < pending.len() && same_destination(&pending[start], &pending[end]) {
            end = end.saturating_add(1);
        }
        let group = &pending[start..end];
        let destination = group[0].destination;
        let local = group[0].local;
        let index = canonical_index(
            usize::from(inner.chunk_edge),
            usize::from(local.0),
            usize::from(local.1),
            usize::from(local.2),
        );
        let old = working
            .get(&destination)
            .and_then(|chunk| chunk.cells.get(index))
            .copied()
            .ok_or(BlockEditRejectV1::StorageUnavailable)?;
        let current = to_runtime_cell(inner, old)?;
        if let Some(chosen) = resolve_intent_group(group, &current)? {
            let fluid_index = fluid_layer_index(inner, &chosen.fluid)?;
            let new_voxel =
                HostVoxel::occupancy(old.palette_index, fluid_index, &inner.presentation);
            if new_voxel != old {
                let target = working
                    .get_mut(&destination)
                    .and_then(|chunk| chunk.cells.get_mut(index))
                    .ok_or(BlockEditRejectV1::StorageUnavailable)?;
                *target = new_voxel;
                voxel_changed.insert(destination);
                let metrics = accounting.entry(chosen.origin).or_default();
                metrics.cells_changed = metrics.cells_changed.saturating_add(1);
                ensure_continuation_update(&mut continuations, &working, destination)?
                    .frontier
                    .push(local);
            }
        }
        start = end;
    }

    let actual_changed = accounting
        .values()
        .fold(0_u32, |total, row| total.saturating_add(row.cells_changed));
    enforce_host_limit(
        "fluid_cells_per_tick",
        actual_changed,
        budget.max_cells_per_tick(),
    )?;
    let mut final_queue_depth = 0_u32;
    for continuation in continuations.values_mut() {
        normalize_continuation(continuation)?;
        final_queue_depth =
            final_queue_depth.saturating_add(continuation_entry_count(continuation));
    }
    enforce_host_limit(
        "fluid_queue_depth",
        final_queue_depth,
        budget.max_queue_depth(),
    )?;

    let affected = continuations
        .keys()
        .copied()
        .chain(voxel_changed.iter().copied())
        .collect::<BTreeSet<_>>();
    let mut replacements = Vec::new();
    for coordinate in affected {
        let chunk = working
            .get(&coordinate)
            .ok_or(BlockEditRejectV1::StorageUnavailable)?;
        let mut stored_continuations = chunk.data.continuations().clone();
        let continuation = match continuations.get(&coordinate) {
            Some(continuation) => continuation.clone(),
            None => persisted_continuation_from_data(&chunk.data)?,
        };
        if continuation.frontier.is_empty() && continuation.deferred.is_empty() {
            stored_continuations.remove(&FLUID_CONTINUATION_ID);
        } else {
            stored_continuations.insert(FLUID_CONTINUATION_ID, encode_continuation(&continuation)?);
        }
        let voxels = if voxel_changed.contains(&coordinate) {
            voxel_payload(
                chunk.data.voxels().schema(),
                chunk.data.voxels().schema_version(),
                &chunk.cells,
            )
        } else {
            chunk.data.voxels().clone()
        };
        let replacement = ChunkData::new(
            voxels,
            chunk.data.persistent_entities().clone(),
            stored_continuations,
            chunk.data.provenance().clone(),
        );
        let changed = chunk_changed_domains(Some(&chunk.data), &replacement);
        if !changed.is_empty() {
            replacements.push((coordinate, chunk.revision, changed, replacement));
        }
    }
    inner.commit_fluid_batch(kernel, snapshot.revision(), replacements)?;

    let mut reports = accounting
        .into_iter()
        .map(|(coordinate, metrics)| {
            let continuation = continuations.get(&coordinate).cloned().unwrap_or_default();
            HostFluidTickV1 {
                coordinate,
                cells_changed: metrics.cells_changed,
                queue_depth: continuation_entry_count(&continuation),
                in_flight_bytes: metrics.in_flight_bytes,
                boundary_intents: metrics.boundary_intents,
                deferred_intents: bounded_len(continuation.deferred.len()),
            }
        })
        .collect::<Vec<_>>();
    reports.sort_by_key(|report| report.coordinate);
    Ok(reports)
}

fn plan_input(
    inner: &ProductionSpineInner,
    snapshot: &ReferenceWorldSnapshot,
    coordinate: ChunkCoordinate,
    continuation: FluidContinuationV1,
) -> Result<FluidPlanInput, BlockEditRejectV1> {
    let working = load_working_chunk(inner, snapshot, coordinate)?;
    let captured = FluidRevisionStamp::new(
        snapshot.revision(),
        working.revision,
        snapshot
            .chunk(&ChunkKey::new(
                inner.world,
                inner.dimension.clone(),
                coordinate,
            ))
            .ok_or(BlockEditRejectV1::StorageUnavailable)?
            .domain_revisions()
            .voxels(),
    );
    let runtime_cells = working
        .cells
        .iter()
        .map(|cell| to_runtime_cell(inner, *cell))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(FluidPlanInput {
        coordinate,
        captured,
        continuation,
        working,
        runtime_cells,
    })
}

fn plan_inputs(
    inputs: Vec<FluidPlanInput>,
    budget: FluidUpdateBudget,
) -> Result<Vec<PlannedChunk>, BlockEditRejectV1> {
    if let Some(pool) = AsyncComputeTaskPool::try_get() {
        return pool
            .scope_with_executor(false, None, |scope| {
                for input in inputs {
                    scope.spawn(async move { plan_one(input, budget) });
                }
            })
            .into_iter()
            .collect();
    }
    inputs
        .into_iter()
        .map(|input| plan_one(input, budget))
        .collect()
}

fn plan_one(
    input: FluidPlanInput,
    budget: FluidUpdateBudget,
) -> Result<PlannedChunk, BlockEditRejectV1> {
    let plan = if input.continuation.frontier.is_empty() {
        None
    } else {
        Some(
            plan_fluid_tick(
                input.coordinate,
                input.captured,
                &input.runtime_cells,
                &input.continuation.frontier,
                budget,
            )
            .map_err(|error| map_fluid_error(&error))?,
        )
    };
    Ok(PlannedChunk {
        coordinate: input.coordinate,
        continuation: input.continuation,
        working: input.working,
        plan,
    })
}

fn validate_plan_revisions(
    inner: &ProductionSpineInner,
    kernel: &MemoryTransactionKernel,
    planned: &[PlannedChunk],
) -> Result<(), BlockEditRejectV1> {
    let current = kernel
        .reference_snapshot(inner.world)
        .map_err(|_| BlockEditRejectV1::StorageUnavailable)?;
    for chunk in planned {
        let Some(plan) = &chunk.plan else {
            continue;
        };
        let key = ChunkKey::new(inner.world, inner.dimension.clone(), chunk.coordinate);
        let stored = current
            .chunk(&key)
            .ok_or(BlockEditRejectV1::StorageUnavailable)?;
        let current_stamp = FluidRevisionStamp::new(
            current.revision(),
            stored.revision(),
            stored.domain_revisions().voxels(),
        );
        admit_fluid_completion(plan.captured(), current_stamp)
            .map_err(|error| map_stale_error(&error, plan.captured(), current_stamp))?;
    }
    Ok(())
}

fn load_working_chunk(
    inner: &ProductionSpineInner,
    snapshot: &ReferenceWorldSnapshot,
    coordinate: ChunkCoordinate,
) -> Result<WorkingChunk, BlockEditRejectV1> {
    let key = ChunkKey::new(inner.world, inner.dimension.clone(), coordinate);
    let stored = snapshot
        .chunk(&key)
        .ok_or(BlockEditRejectV1::StorageUnavailable)?;
    let cells = decode_cells(
        stored.data().voxels().bytes(),
        inner.chunk_edge,
        &inner.presentation,
    )
    .map_err(|_| BlockEditRejectV1::StorageUnavailable)?;
    Ok(WorkingChunk {
        revision: stored.revision(),
        data: stored.data().clone(),
        cells,
    })
}

fn ensure_continuation_update<'a>(
    continuations: &'a mut BTreeMap<ChunkCoordinate, FluidContinuationV1>,
    working: &BTreeMap<ChunkCoordinate, WorkingChunk>,
    coordinate: ChunkCoordinate,
) -> Result<&'a mut FluidContinuationV1, BlockEditRejectV1> {
    if let Entry::Vacant(entry) = continuations.entry(coordinate) {
        let chunk = working
            .get(&coordinate)
            .ok_or(BlockEditRejectV1::StorageUnavailable)?;
        let continuation = persisted_continuation_from_data(&chunk.data)?;
        entry.insert(continuation);
    }
    continuations
        .get_mut(&coordinate)
        .ok_or(BlockEditRejectV1::StorageUnavailable)
}

fn stage_boundary_intent(
    origin: ChunkCoordinate,
    deferred: DeferredBoundaryIntentV1,
    kind: u8,
    sequence: u32,
    resident: &BTreeSet<ChunkCoordinate>,
    pending: &mut Vec<PendingIntent>,
    next: &mut FluidContinuationV1,
) {
    if resident.contains(&deferred.neighbor) {
        pending.push(PendingIntent {
            origin,
            destination: deferred.neighbor,
            local: deferred.local,
            fluid: deferred.fluid,
            kind,
            sequence,
        });
    } else {
        next.deferred.push(deferred);
    }
}

fn resolve_intent_group(
    group: &[PendingIntent],
    current: &SolidFluidRuntimeCell,
) -> Result<Option<PendingIntent>, BlockEditRejectV1> {
    if current.solid_occupied {
        return Ok(None);
    }
    let first_identity = group
        .first()
        .and_then(|intent| intent.fluid.identity())
        .ok_or(BlockEditRejectV1::ContentUnavailable)?;
    if group
        .iter()
        .any(|intent| intent.fluid.identity() != Some(first_identity))
    {
        return Err(map_fluid_error(&FluidRuntimeError::Mixing));
    }
    if let Some(existing) = current.fluid.identity()
        && existing != first_identity
    {
        return Err(map_fluid_error(&FluidRuntimeError::Mixing));
    }
    let chosen = group
        .iter()
        .min_by(|left, right| compare_intent_strength(left, right))
        .cloned()
        .ok_or(BlockEditRejectV1::ContentUnavailable)?;
    let desired_level = chosen
        .fluid
        .level()
        .ok_or(BlockEditRejectV1::ContentUnavailable)?;
    if current
        .fluid
        .level()
        .is_some_and(|existing| existing <= desired_level)
    {
        return Ok(None);
    }
    Ok(Some(chosen))
}

fn compare_pending_intents(left: &PendingIntent, right: &PendingIntent) -> Ordering {
    left.destination
        .cmp(&right.destination)
        .then(left.local.1.cmp(&right.local.1))
        .then(left.local.2.cmp(&right.local.2))
        .then(left.local.0.cmp(&right.local.0))
        .then_with(|| compare_fluid(&left.fluid, &right.fluid))
        .then_with(|| compare_intent_strength(left, right))
}

fn compare_intent_strength(left: &PendingIntent, right: &PendingIntent) -> Ordering {
    left.fluid
        .level()
        .cmp(&right.fluid.level())
        .then(fluid_flow_tag(&left.fluid).cmp(&fluid_flow_tag(&right.fluid)))
        .then(left.origin.cmp(&right.origin))
        .then(left.kind.cmp(&right.kind))
        .then(left.sequence.cmp(&right.sequence))
}

fn compare_fluid(left: &FluidLayerCell, right: &FluidLayerCell) -> Ordering {
    left.identity()
        .cmp(&right.identity())
        .then(left.level().cmp(&right.level()))
        .then(fluid_flow_tag(left).cmp(&fluid_flow_tag(right)))
}

fn fluid_flow_tag(fluid: &FluidLayerCell) -> u16 {
    match fluid {
        FluidLayerCell::Empty => FluidFlow::TAG_STILL,
        FluidLayerCell::Fluid { flow, .. } => flow.tag(),
    }
}

fn same_destination(left: &PendingIntent, right: &PendingIntent) -> bool {
    left.destination == right.destination && left.local == right.local
}

fn persisted_continuation(
    stored: &StoredChunk,
) -> Result<Option<FluidContinuationV1>, BlockEditRejectV1> {
    stored
        .data()
        .continuations()
        .get(&FLUID_CONTINUATION_ID)
        .map(decode_continuation)
        .transpose()
}

fn persisted_continuation_from_data(
    data: &ChunkData,
) -> Result<FluidContinuationV1, BlockEditRejectV1> {
    data.continuations()
        .get(&FLUID_CONTINUATION_ID)
        .map(decode_continuation)
        .transpose()
        .map(Option::unwrap_or_default)
}

fn decode_continuation(
    payload: &VersionedPayload,
) -> Result<FluidContinuationV1, BlockEditRejectV1> {
    if payload.schema().as_str() != FLUID_CONTINUATION_SCHEMA
        || payload.schema_version().get() != FLUID_CONTINUATION_SCHEMA_VERSION
    {
        return Err(BlockEditRejectV1::ContentUnavailable);
    }
    let durable = serde_json::from_slice::<DurableFluidContinuationV1>(payload.bytes())
        .map_err(|_| BlockEditRejectV1::ContentUnavailable)?;
    if durable.schema_version != FLUID_CONTINUATION_SCHEMA_VERSION {
        return Err(BlockEditRejectV1::ContentUnavailable);
    }
    let mut continuation = FluidContinuationV1 {
        frontier: durable
            .frontier
            .into_iter()
            .map(|local| (local[0], local[1], local[2]))
            .collect(),
        deferred: durable
            .deferred
            .into_iter()
            .map(|durable| decode_deferred(&durable))
            .collect::<Result<Vec<_>, _>>()?,
    };
    normalize_continuation(&mut continuation)?;
    Ok(continuation)
}

fn decode_deferred(
    durable: &DurableBoundaryIntentV1,
) -> Result<DeferredBoundaryIntentV1, BlockEditRejectV1> {
    if durable.level > 7 {
        return Err(BlockEditRejectV1::ContentUnavailable);
    }
    let id = durable
        .fluid
        .parse::<StableId>()
        .map_err(|_| BlockEditRejectV1::ContentUnavailable)?;
    let flow = FluidFlow::from_tag(durable.flow).ok_or(BlockEditRejectV1::ContentUnavailable)?;
    Ok(DeferredBoundaryIntentV1 {
        neighbor: ChunkCoordinate::new(
            durable.neighbor[0],
            durable.neighbor[1],
            durable.neighbor[2],
        ),
        local: (durable.local[0], durable.local[1], durable.local[2]),
        fluid: FluidLayerCell::Fluid {
            id,
            level: durable.level,
            flow,
        },
    })
}

fn encode_continuation(
    continuation: &FluidContinuationV1,
) -> Result<VersionedPayload, BlockEditRejectV1> {
    let schema = FLUID_CONTINUATION_SCHEMA
        .parse::<SchemaId>()
        .map_err(|_| BlockEditRejectV1::ContentUnavailable)?;
    let version = PayloadSchemaVersion::new(FLUID_CONTINUATION_SCHEMA_VERSION)
        .map_err(|_| BlockEditRejectV1::ContentUnavailable)?;
    let durable = DurableFluidContinuationV1 {
        schema_version: FLUID_CONTINUATION_SCHEMA_VERSION,
        frontier: continuation
            .frontier
            .iter()
            .map(|&(x, y, z)| [x, y, z])
            .collect(),
        deferred: continuation
            .deferred
            .iter()
            .map(encode_deferred)
            .collect::<Result<Vec<_>, _>>()?,
    };
    let bytes =
        canonical_json_bytes(&durable).map_err(|_| BlockEditRejectV1::ContentUnavailable)?;
    Ok(VersionedPayload::new(schema, version, bytes))
}

fn encode_deferred(
    deferred: &DeferredBoundaryIntentV1,
) -> Result<DurableBoundaryIntentV1, BlockEditRejectV1> {
    let FluidLayerCell::Fluid { id, level, flow } = &deferred.fluid else {
        return Err(BlockEditRejectV1::ContentUnavailable);
    };
    if *level > 7 {
        return Err(BlockEditRejectV1::ContentUnavailable);
    }
    Ok(DurableBoundaryIntentV1 {
        neighbor: [
            deferred.neighbor.x,
            deferred.neighbor.y,
            deferred.neighbor.z,
        ],
        local: [deferred.local.0, deferred.local.1, deferred.local.2],
        fluid: id.as_str().to_owned(),
        level: *level,
        flow: flow.tag(),
    })
}

fn normalize_continuation(continuation: &mut FluidContinuationV1) -> Result<(), BlockEditRejectV1> {
    for &(x, y, z) in &continuation.frontier {
        if x >= latticeaxiom_voxel_runtime::FLUID_CHUNK_EDGE_V1
            || y >= latticeaxiom_voxel_runtime::FLUID_CHUNK_EDGE_V1
            || z >= latticeaxiom_voxel_runtime::FLUID_CHUNK_EDGE_V1
        {
            return Err(BlockEditRejectV1::ContentUnavailable);
        }
    }
    continuation.frontier.sort_by_key(|&(x, y, z)| (y, z, x));
    continuation.frontier.dedup();
    continuation.deferred.sort_by(compare_deferred);
    continuation.deferred.dedup();
    Ok(())
}

fn compare_deferred(left: &DeferredBoundaryIntentV1, right: &DeferredBoundaryIntentV1) -> Ordering {
    left.neighbor
        .cmp(&right.neighbor)
        .then(left.local.1.cmp(&right.local.1))
        .then(left.local.2.cmp(&right.local.2))
        .then(left.local.0.cmp(&right.local.0))
        .then_with(|| compare_fluid(&left.fluid, &right.fluid))
}

fn continuation_entry_count(continuation: &FluidContinuationV1) -> u32 {
    bounded_len(
        continuation
            .frontier
            .len()
            .saturating_add(continuation.deferred.len()),
    )
}

fn bounded_len(length: usize) -> u32 {
    u32::try_from(length).unwrap_or(u32::MAX)
}

fn bounded_sequence(index: usize) -> u32 {
    u32::try_from(index).unwrap_or(u32::MAX)
}

fn enforce_host_limit(
    resource: &'static str,
    actual: u32,
    limit: u32,
) -> Result<(), BlockEditRejectV1> {
    if actual > limit {
        Err(map_fluid_error(&FluidRuntimeError::LimitExceeded {
            resource,
            actual,
            limit,
        }))
    } else {
        Ok(())
    }
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

fn map_stale_error(
    error: &FluidRuntimeError,
    captured: FluidRevisionStamp,
    current: FluidRevisionStamp,
) -> BlockEditRejectV1 {
    match error {
        FluidRuntimeError::Stale {
            reason: StaleReason::WorldRevision,
        } => BlockEditRejectV1::StaleRevision {
            expected: captured.world().get(),
            actual: current.world().get(),
        },
        FluidRuntimeError::Stale {
            reason: StaleReason::ChunkRevision,
        } => BlockEditRejectV1::StaleRevision {
            expected: captured.chunk().get(),
            actual: current.chunk().get(),
        },
        FluidRuntimeError::Stale {
            reason: StaleReason::VoxelRevision,
        } => BlockEditRejectV1::StaleRevision {
            expected: captured.voxel().get(),
            actual: current.voxel().get(),
        },
        other => map_fluid_error(other),
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

#[cfg(test)]
mod tests {
    use super::*;

    fn water(level: u8, flow: FluidFlow) -> FluidLayerCell {
        FluidLayerCell::Fluid {
            id: "fixture:fluid/water"
                .parse()
                .unwrap_or_else(|error| panic!("fixture water id is invalid: {error}")),
            level,
            flow,
        }
    }

    fn lava(level: u8) -> FluidLayerCell {
        FluidLayerCell::Fluid {
            id: "fixture:fluid/lava"
                .parse()
                .unwrap_or_else(|error| panic!("fixture lava id is invalid: {error}")),
            level,
            flow: FluidFlow::Still,
        }
    }

    fn intent(
        origin: ChunkCoordinate,
        destination: ChunkCoordinate,
        fluid: FluidLayerCell,
    ) -> PendingIntent {
        PendingIntent {
            origin,
            destination,
            local: (0, 3, 2),
            fluid,
            kind: 2,
            sequence: 0,
        }
    }

    #[test]
    fn continuation_round_trip_is_canonical_and_versioned() {
        let mut value = FluidContinuationV1 {
            frontier: vec![(4, 2, 1), (1, 0, 2), (4, 2, 1)],
            deferred: vec![DeferredBoundaryIntentV1 {
                neighbor: ChunkCoordinate::new(-1, 0, 2),
                local: (31, 2, 7),
                fluid: water(3, FluidFlow::West),
            }],
        };
        normalize_continuation(&mut value).expect("fixture continuation normalizes");
        let first = encode_continuation(&value).expect("continuation encodes");
        let decoded = decode_continuation(&first).expect("continuation decodes");
        let second = encode_continuation(&decoded).expect("continuation re-encodes");
        assert_eq!(decoded, value);
        assert_eq!(first, second);
        assert_eq!(decoded.frontier, vec![(1, 0, 2), (4, 2, 1)]);
    }

    #[test]
    fn stable_merge_prefers_stronger_level_independent_of_input_order() {
        let destination = ChunkCoordinate::new(1, 0, 0);
        let weak = intent(
            ChunkCoordinate::new(2, 0, 0),
            destination,
            water(6, FluidFlow::West),
        );
        let strong = intent(
            ChunkCoordinate::new(0, 0, 0),
            destination,
            water(2, FluidFlow::East),
        );
        let current = SolidFluidRuntimeCell::empty();
        for mut group in [vec![weak.clone(), strong.clone()], vec![strong, weak]] {
            group.sort_by(compare_pending_intents);
            let chosen = resolve_intent_group(&group, &current)
                .expect("same-fluid merge succeeds")
                .expect("empty destination accepts fluid");
            assert_eq!(chosen.fluid.level(), Some(2));
            assert_eq!(chosen.origin, ChunkCoordinate::new(0, 0, 0));
        }
    }

    #[test]
    fn stable_merge_rejects_mixing_before_any_apply() {
        let destination = ChunkCoordinate::new(1, 0, 0);
        let group = vec![
            intent(
                ChunkCoordinate::new(0, 0, 0),
                destination,
                water(2, FluidFlow::East),
            ),
            intent(ChunkCoordinate::new(2, 0, 0), destination, lava(1)),
        ];
        assert!(matches!(
            resolve_intent_group(&group, &SolidFluidRuntimeCell::empty()),
            Err(BlockEditRejectV1::ContentUnavailable)
        ));
    }

    #[test]
    fn activation_preserves_other_continuations_and_deduplicates_frontier() {
        let schema: SchemaId = "fixture:schema/other@1"
            .parse()
            .unwrap_or_else(|error| panic!("fixture schema is invalid: {error}"));
        let version = PayloadSchemaVersion::new(1)
            .unwrap_or_else(|error| panic!("fixture version is invalid: {error}"));
        let other_id = ContinuationId::from_u128(9);
        let mut continuations = BTreeMap::from([(
            other_id,
            VersionedPayload::new(schema, version, vec![1, 2, 3]),
        )]);
        activate_persisted_frontier(&mut continuations, (31, 2, 7))
            .expect("first activation succeeds");
        activate_persisted_frontier(&mut continuations, (31, 2, 7))
            .expect("duplicate activation succeeds");
        assert!(continuations.contains_key(&other_id));
        let active = continuations
            .get(&FLUID_CONTINUATION_ID)
            .map(decode_continuation)
            .transpose()
            .expect("continuation decodes")
            .expect("fluid continuation exists");
        assert_eq!(active.frontier, vec![(31, 2, 7)]);
    }

    #[test]
    fn unloaded_boundary_is_persisted_then_consumed_after_reload() {
        let origin = ChunkCoordinate::new(0, 0, 0);
        let neighbor = ChunkCoordinate::new(1, 0, 0);
        let deferred = DeferredBoundaryIntentV1 {
            neighbor,
            local: (0, 3, 2),
            fluid: water(1, FluidFlow::East),
        };
        let mut pending = Vec::new();
        let mut next = FluidContinuationV1::default();
        stage_boundary_intent(
            origin,
            deferred,
            2,
            0,
            &BTreeSet::new(),
            &mut pending,
            &mut next,
        );
        assert!(pending.is_empty());
        assert_eq!(next.deferred.len(), 1);

        let payload = encode_continuation(&next).expect("deferred intent persists");
        let reopened = decode_continuation(&payload).expect("deferred intent reopens");
        let mut retry_pending = Vec::new();
        let mut retry_next = FluidContinuationV1::default();
        for (sequence, intent) in reopened.deferred.into_iter().enumerate() {
            stage_boundary_intent(
                origin,
                intent,
                1,
                bounded_sequence(sequence),
                &BTreeSet::from([neighbor]),
                &mut retry_pending,
                &mut retry_next,
            );
        }
        assert_eq!(retry_pending.len(), 1);
        assert!(retry_next.deferred.is_empty());
        assert_eq!(retry_pending[0].destination, neighbor);
    }
}
