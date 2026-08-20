//! Bounded cache of storage-committed voxel projections and derived work.
#![allow(
    clippy::expect_used,
    clippy::missing_panics_doc,
    reason = "private synchronous invariants are stated at each expectation and cannot be violated through the public API"
)]

use std::{
    collections::{BTreeMap, BTreeSet},
    mem,
    panic::{AssertUnwindSafe, catch_unwind},
};

use latticeaxiom_storage::{ChunkCoordinate, ChunkRevision, VoxelRevision, WorldRevision};
use latticeaxiom_voxel_mesh::{Face, PaddedChunk};
use sha2::{Digest, Sha256};

use crate::{
    ApplyByteDeclaration, CancellationAckOutcome, CancellationReleaseReceipt, ColliderFailure,
    ColliderSafetyState, ColliderSemanticFingerprint, CollisionSemantics, CommittedChunkProjection,
    CompletionOutcome, DerivedApplyReceipt, DerivedEnqueueReceipt, DerivedInput, DerivedJobId,
    DerivedJobKey, DerivedKind, DerivedRequest, DerivedRequestSet, DerivedSemanticFingerprint,
    DerivedSourceFingerprint, DispatchOutcome, EnqueueDecision, EvictionLeaseGeneration,
    EvictionPermit, EvictionReceipt, FixedTick, InterestWindow, MemoryStage,
    MeshSemanticFingerprint, NeighborRevision, NeighborRevisions, ProjectionDecision,
    ProjectionReceipt, RetainedBytes, RuntimeDiagnostics, RuntimeError, RuntimeGeneration,
    RuntimeLimits, RuntimeResult, StaleReason, VoxelCoordinate, WorkingSetScope,
    model::ProjectionParts,
    queue::{DerivedQueue, JobState, PendingJob},
};

const SOURCE_FINGERPRINT_DOMAIN: &[u8] = b"latticeaxiom.voxel-derived-source.v2\0";

#[derive(Clone, Debug)]
struct RuntimeChunk<V> {
    world_revision: WorldRevision,
    revision: ChunkRevision,
    voxel_revision: VoxelRevision,
    cells: Vec<V>,
    mesh_semantics: MeshSemanticFingerprint,
    collider_semantics: ColliderSemanticFingerprint,
    last_applied: [Option<DerivedJobKey>; 2],
    collider_safety: ColliderSafetyState,
}

impl<V> RuntimeChunk<V> {
    fn from_parts(parts: ProjectionParts<V>, last_applied: [Option<DerivedJobKey>; 2]) -> Self {
        Self {
            world_revision: parts.world_revision,
            revision: parts.revision,
            voxel_revision: parts.voxel_revision,
            cells: parts.cells,
            mesh_semantics: parts.mesh_semantics,
            collider_semantics: parts.collider_semantics,
            last_applied,
            collider_safety: ColliderSafetyState::PendingConservative {
                world_revision: parts.world_revision,
                voxel_revision: parts.voxel_revision,
            },
        }
    }
}

/// Bounded runtime cache and coordinator over storage-committed voxel state.
///
/// The runtime cannot author authoritative edits or advance revisions. Storage
/// must first publish a commit or snapshot and issue a
/// [`CommittedChunkProjection`]. Meshes, colliders, renderer resources, task
/// execution, and persistence remain caller-owned.
#[derive(Debug)]
pub struct VoxelRuntime<V> {
    scope: WorkingSetScope,
    runtime_generation: RuntimeGeneration,
    edge: u16,
    dimensions: PaddedChunk,
    empty: V,
    limits: RuntimeLimits,
    chunks: BTreeMap<ChunkCoordinate, RuntimeChunk<V>>,
    dirty: BTreeSet<ChunkCoordinate>,
    interest: Option<InterestWindow>,
    queues: [DerivedQueue; 2],
    next_job_id: u64,
    next_eviction_lease: u64,
    diagnostics: RuntimeDiagnostics,
}

impl<V: Clone + Eq + RetainedBytes + CollisionSemantics> VoxelRuntime<V> {
    /// Creates an empty committed-projection cache.
    ///
    /// `runtime_generation` must be unique while executors can still return
    /// tickets issued by an older runtime instance.
    ///
    /// # Errors
    ///
    /// Returns [`RuntimeError::InvalidChunkEdge`] when `edge` cannot satisfy
    /// the mesh crate's one-voxel-halo contract.
    pub fn new(
        scope: WorkingSetScope,
        runtime_generation: RuntimeGeneration,
        edge: u16,
        empty: V,
        limits: RuntimeLimits,
    ) -> RuntimeResult<Self> {
        let dimensions = PaddedChunk::new([usize::from(edge); 3])
            .map_err(|_| RuntimeError::InvalidChunkEdge { edge })?;
        Ok(Self {
            scope,
            runtime_generation,
            edge,
            dimensions,
            empty,
            limits,
            chunks: BTreeMap::new(),
            dirty: BTreeSet::new(),
            interest: None,
            queues: [
                DerivedQueue::new(limits.mesh()),
                DerivedQueue::new(limits.collider()),
            ],
            next_job_id: 0,
            next_eviction_lease: 1,
            diagnostics: RuntimeDiagnostics::default(),
        })
    }

    /// World, dimension, and epoch shared by resident projections.
    #[must_use]
    pub const fn scope(&self) -> &WorkingSetScope {
        &self.scope
    }

    /// Process-local instance generation bound into every ticket.
    #[must_use]
    pub const fn runtime_generation(&self) -> RuntimeGeneration {
        self.runtime_generation
    }

    /// Cubic committed chunk edge in voxels.
    #[must_use]
    pub const fn chunk_edge(&self) -> u16 {
        self.edge
    }

    /// Hard resident, queue, and combined memory limits.
    #[must_use]
    pub const fn limits(&self) -> RuntimeLimits {
        self.limits
    }

    /// Current diagnostics snapshot.
    #[must_use]
    pub fn diagnostics(&self) -> RuntimeDiagnostics {
        let mut diagnostics = self.diagnostics;
        diagnostics.mesh = self.queues[DerivedKind::Mesh.index()].diagnostics();
        diagnostics.collider = self.queues[DerivedKind::Collider.index()].diagnostics();
        diagnostics.conservative_colliders = self
            .chunks
            .values()
            .filter(|chunk| !matches!(chunk.collider_safety, ColliderSafetyState::Ready { .. }))
            .count();
        diagnostics.visible_chunks = self
            .chunks
            .values()
            .filter(|chunk| chunk.last_applied[DerivedKind::Mesh.index()].is_some())
            .count();
        diagnostics.active_chunks = self
            .chunks
            .values()
            .filter(|chunk| {
                chunk.last_applied[DerivedKind::Mesh.index()].is_some()
                    && chunk.last_applied[DerivedKind::Collider.index()].is_some()
            })
            .count();
        diagnostics.in_flight_chunks = diagnostics
            .mesh
            .in_flight()
            .saturating_add(diagnostics.collider.in_flight());
        diagnostics.saving_chunks = 0;
        diagnostics.byte_budget = self.limits.max_combined_reserved_bytes();
        diagnostics
    }

    /// Returns whether one committed projection is resident.
    #[must_use]
    pub fn is_resident(&self, coordinate: ChunkCoordinate) -> bool {
        self.chunks.contains_key(&coordinate)
    }

    /// Returns whether a resident projection is pinned by an edit.
    #[must_use]
    pub fn is_dirty(&self, coordinate: ChunkCoordinate) -> bool {
        self.dirty.contains(&coordinate)
    }

    /// Resident coordinates in canonical `(x, y, z)` order.
    pub fn resident_coordinates(&self) -> impl Iterator<Item = ChunkCoordinate> + '_ {
        self.chunks.keys().copied()
    }

    /// Dirty edited coordinates in canonical `(x, y, z)` order.
    pub fn dirty_coordinates(&self) -> impl Iterator<Item = ChunkCoordinate> + '_ {
        self.dirty.iter().copied()
    }

    /// Current streaming interest window, if the host has set one.
    #[must_use]
    pub const fn interest(&self) -> Option<InterestWindow> {
        self.interest
    }

    /// Sets the inclusive Chebyshev cube used to choose clean eviction victims.
    pub fn set_interest(&mut self, interest: InterestWindow) {
        self.interest = Some(interest);
    }

    /// Pins a resident edited chunk so streaming eviction cannot drop it.
    ///
    /// Dirty membership is bounded by [`RuntimeLimits::max_resident_chunks`]: a
    /// pinned chunk occupies a resident slot until the host unloads the world.
    ///
    /// # Errors
    ///
    /// Returns [`RuntimeError::ChunkNotResident`] when the target is absent.
    pub fn mark_dirty(&mut self, coordinate: ChunkCoordinate) -> RuntimeResult<()> {
        if !self.chunks.contains_key(&coordinate) {
            return Err(RuntimeError::ChunkNotResident { coordinate });
        }
        self.dirty.insert(coordinate);
        self.refresh_resident_diagnostics();
        Ok(())
    }

    /// Returns storage publication, total, and voxel-domain revisions.
    #[must_use]
    pub fn chunk_revisions(
        &self,
        coordinate: ChunkCoordinate,
    ) -> Option<(WorldRevision, ChunkRevision, VoxelRevision)> {
        self.chunks
            .get(&coordinate)
            .map(|chunk| (chunk.world_revision, chunk.revision, chunk.voxel_revision))
    }

    /// Returns a decoded committed cell.
    ///
    /// # Errors
    ///
    /// Returns an error when the coordinate exceeds the canonical chunk range
    /// or its chunk projection is not resident.
    pub fn cell(&self, coordinate: VoxelCoordinate) -> RuntimeResult<&V> {
        let (chunk_coordinate, local) = self.split_coordinate(coordinate)?;
        let chunk = self
            .chunks
            .get(&chunk_coordinate)
            .ok_or(RuntimeError::ChunkNotResident {
                coordinate: chunk_coordinate,
            })?;
        Ok(&chunk.cells[self.cell_index(local)])
    }

    /// Returns projected occupancy used while a collider is pending or failed.
    ///
    /// # Errors
    ///
    /// Returns the same addressing errors as [`Self::cell`].
    pub fn projected_collision_occupied(&self, coordinate: VoxelCoordinate) -> RuntimeResult<bool> {
        self.cell(coordinate)
            .map(CollisionSemantics::collision_occupied)
    }

    /// Returns collider safety for one resident projection.
    #[must_use]
    pub fn collider_safety(&self, coordinate: ChunkCoordinate) -> Option<&ColliderSafetyState> {
        self.chunks
            .get(&coordinate)
            .map(|chunk| &chunk.collider_safety)
    }

    /// Returns the last successfully applied key, including an older safe fallback.
    #[must_use]
    pub fn last_applied_key(
        &self,
        coordinate: ChunkCoordinate,
        kind: DerivedKind,
    ) -> Option<&DerivedJobKey> {
        self.chunks
            .get(&coordinate)
            .and_then(|chunk| chunk.last_applied[kind.index()].as_ref())
    }

    /// Returns the conservative retained-byte estimate for one halo capture.
    #[must_use]
    pub fn estimated_derived_input_bytes(&self, coordinate: ChunkCoordinate) -> Option<u64> {
        self.chunks
            .contains_key(&coordinate)
            .then(|| self.halo_retained_bytes(coordinate))
    }

    /// Projects one storage-issued committed publication and schedules invalidation.
    ///
    /// Revision advancement and cell mutation must already have succeeded in
    /// storage. An equal publication is idempotent; conflicting or regressing
    /// authority is rejected before resident state changes.
    ///
    /// # Errors
    ///
    /// Returns an error for scope/edge mismatch, capacity exhaustion,
    /// conflicting same-revision state, or revision regression.
    #[allow(
        clippy::too_many_lines,
        reason = "the atomic projection state transition keeps validation and mutation in one auditable path"
    )]
    pub fn project_committed(
        &mut self,
        projection: CommittedChunkProjection<V>,
        tick: FixedTick,
        requests: DerivedRequestSet,
    ) -> RuntimeResult<ProjectionReceipt> {
        let coordinate = projection.key().coordinate;
        if projection.key().world != self.scope.world()
            || projection.key().dimension != *self.scope.dimension()
        {
            return Err(RuntimeError::ScopeMismatch { coordinate });
        }
        if projection.edge() != self.edge {
            return Err(RuntimeError::ChunkEdgeMismatch {
                coordinate,
                expected: self.edge,
                actual: projection.edge(),
            });
        }
        self.make_room_for_admission(coordinate, tick, requests)?;

        let parts = projection.into_parts();
        let existing = self.chunks.get(&coordinate);
        if let Some(current) = existing {
            if parts.world_revision < current.world_revision {
                return Err(RuntimeError::StaleProjection {
                    coordinate,
                    current: current.world_revision,
                    incoming: parts.world_revision,
                });
            }
            if parts.world_revision == current.world_revision
                && (parts.revision != current.revision
                    || parts.voxel_revision != current.voxel_revision
                    || parts.cells != current.cells)
            {
                return Err(RuntimeError::ConflictingProjection {
                    coordinate,
                    world_revision: parts.world_revision,
                });
            }
            if parts.world_revision > current.world_revision
                && (parts.revision < current.revision
                    || parts.voxel_revision < current.voxel_revision)
            {
                return Err(RuntimeError::ProjectionRevisionRegression {
                    coordinate,
                    chunk_revision: parts.revision,
                    voxel_revision: parts.voxel_revision,
                });
            }
        }

        let authority_equal = existing.is_some_and(|current| {
            parts.world_revision == current.world_revision
                && parts.revision == current.revision
                && parts.voxel_revision == current.voxel_revision
                && parts.cells == current.cells
        });
        let exact_replay = existing.is_some_and(|current| {
            authority_equal
                && parts.mesh_semantics == current.mesh_semantics
                && parts.collider_semantics == current.collider_semantics
        });
        if exact_replay {
            self.diagnostics.projection_replays =
                self.diagnostics.projection_replays.saturating_add(1);
            let current = self
                .chunks
                .get(&coordinate)
                .expect("exact replay requires the resident projection just compared");
            return Ok(ProjectionReceipt {
                coordinate,
                decision: ProjectionDecision::Replay,
                previous_world_revision: Some(current.world_revision),
                world_revision: current.world_revision,
                revision: current.revision,
                voxel_revision: current.voxel_revision,
                changed_cells: 0,
                derived: Vec::new(),
            });
        }

        let decision = if existing.is_some() {
            ProjectionDecision::Updated
        } else {
            ProjectionDecision::Admitted
        };
        let previous_world_revision = existing.map(|chunk| chunk.world_revision);
        let changed_cells = existing.map_or(parts.cells.len(), |chunk| {
            chunk
                .cells
                .iter()
                .zip(&parts.cells)
                .filter(|(left, right)| left != right)
                .count()
        });
        let mesh_changed = existing.is_none_or(|current| {
            parts.world_revision != current.world_revision
                || parts.revision != current.revision
                || parts.voxel_revision != current.voxel_revision
                || parts.cells != current.cells
                || parts.mesh_semantics != current.mesh_semantics
        });
        let collider_changed = existing.is_none_or(|current| {
            parts.world_revision != current.world_revision
                || parts.revision != current.revision
                || parts.voxel_revision != current.voxel_revision
                || parts.cells != current.cells
                || parts.collider_semantics != current.collider_semantics
        });
        let neighbor_source_changed = existing.is_none_or(|current| {
            parts.world_revision != current.world_revision
                || parts.revision != current.revision
                || parts.voxel_revision != current.voxel_revision
                || parts.cells != current.cells
        });
        let last_applied = existing.map_or([None, None], |chunk| chunk.last_applied.clone());
        let previous_collider_safety = existing.map(|chunk| chunk.collider_safety.clone());
        let world_revision = parts.world_revision;
        let revision = parts.revision;
        let voxel_revision = parts.voxel_revision;
        let mut replacement = RuntimeChunk::from_parts(parts, last_applied);
        if !collider_changed && let Some(previous) = previous_collider_safety {
            replacement.collider_safety = previous;
        }
        self.chunks.insert(coordinate, replacement);
        if decision == ProjectionDecision::Updated && changed_cells > 0 {
            self.dirty.insert(coordinate);
        }
        self.diagnostics.projected_commits = self.diagnostics.projected_commits.saturating_add(1);
        self.refresh_resident_diagnostics();

        let mut work = BTreeSet::new();
        if mesh_changed {
            work.insert((coordinate, DerivedKind::Mesh));
        }
        if collider_changed {
            work.insert((coordinate, DerivedKind::Collider));
        }
        if neighbor_source_changed {
            for neighbor in self.resident_face_neighbors(coordinate) {
                for kind in DerivedKind::ALL {
                    work.insert((neighbor, kind));
                }
            }
        }
        let derived = self.enqueue_work(&work, tick, requests);
        Ok(ProjectionReceipt {
            coordinate,
            decision,
            previous_world_revision,
            world_revision,
            revision,
            voxel_revision,
            changed_cells,
            derived,
        })
    }

    /// Creates a single-use permit for the exact clean committed projection.
    ///
    /// Generated projections start clean. A voxel-changing update or
    /// [`Self::mark_dirty`] pins the chunk in the bounded dirty set. The
    /// caller-supplied lifecycle generation lets higher-level streaming reject
    /// a permit after lease ownership changes.
    ///
    /// # Errors
    ///
    /// Returns [`RuntimeError::ChunkNotResident`] when the target is absent, or
    /// [`RuntimeError::DirtyChunkPinned`] when the projection was edited.
    pub fn prepare_eviction(
        &self,
        coordinate: ChunkCoordinate,
        lease: EvictionLeaseGeneration,
    ) -> RuntimeResult<EvictionPermit> {
        let chunk = self
            .chunks
            .get(&coordinate)
            .ok_or(RuntimeError::ChunkNotResident { coordinate })?;
        if self.dirty.contains(&coordinate) {
            return Err(RuntimeError::DirtyChunkPinned { coordinate });
        }
        Ok(EvictionPermit {
            key: latticeaxiom_storage::ChunkKey::new(
                self.scope.world(),
                self.scope.dimension().clone(),
                coordinate,
            ),
            world_revision: chunk.world_revision,
            revision: chunk.revision,
            voxel_revision: chunk.voxel_revision,
            lease,
        })
    }

    /// Evicts the exact clean projection named by a runtime-issued permit.
    ///
    /// In-flight resources enter `CancelRequested` and remain fully accounted
    /// until [`Self::acknowledge_cancelled`] or [`Self::complete_derived`]
    /// consumes their owned input/result buffers.
    ///
    /// # Errors
    ///
    /// Returns an error when the target disappeared or the permit became stale.
    #[allow(
        clippy::needless_pass_by_value,
        reason = "consuming the permit enforces its single-use lifecycle contract"
    )]
    pub fn evict_committed(
        &mut self,
        permit: EvictionPermit,
        tick: FixedTick,
        requests: DerivedRequestSet,
    ) -> RuntimeResult<EvictionReceipt> {
        let coordinate = permit.key.coordinate;
        let Some(chunk) = self.chunks.get(&coordinate) else {
            return Err(RuntimeError::ChunkNotResident { coordinate });
        };
        if permit.key.world != self.scope.world()
            || permit.key.dimension != *self.scope.dimension()
            || permit.world_revision != chunk.world_revision
            || permit.revision != chunk.revision
            || permit.voxel_revision != chunk.voxel_revision
        {
            return Err(RuntimeError::StaleEvictionPermit { coordinate });
        }
        if self.dirty.contains(&coordinate) {
            return Err(RuntimeError::DirtyChunkPinned { coordinate });
        }

        let neighbors = self.resident_face_neighbors(coordinate);
        self.chunks.remove(&coordinate);
        self.dirty.remove(&coordinate);
        self.diagnostics.evictions = self.diagnostics.evictions.saturating_add(1);
        self.refresh_resident_diagnostics();

        let mut cancellations = Vec::new();
        for kind in DerivedKind::ALL {
            cancellations.extend(self.queues[kind.index()].request_cancel_target(coordinate));
        }
        let work = neighbors
            .into_iter()
            .flat_map(|neighbor| {
                DerivedKind::ALL
                    .into_iter()
                    .map(move |kind| (neighbor, kind))
            })
            .collect();
        let derived = self.enqueue_work(&work, tick, requests);
        Ok(EvictionReceipt {
            coordinate,
            lease: permit.lease,
            cancellations,
            derived,
        })
    }

    /// Evicts every clean generated projection currently outside interest.
    ///
    /// Dirty edited chunks remain resident even when they leave the interest
    /// cube. This does not retain every visited chunk: clean generated
    /// projections are discarded and may later be regenerated identically.
    ///
    /// # Errors
    ///
    /// Returns [`RuntimeError::InterestWindowRequired`] when the host has not
    /// set interest, or an eviction error if a selected victim becomes stale.
    pub fn evict_clean_outside_interest(
        &mut self,
        lease: EvictionLeaseGeneration,
        tick: FixedTick,
        requests: DerivedRequestSet,
    ) -> RuntimeResult<Vec<EvictionReceipt>> {
        if self.interest.is_none() {
            return Err(RuntimeError::InterestWindowRequired);
        }
        let victims = self.clean_outside_interest_victims();
        let mut receipts = Vec::with_capacity(victims.len());
        for coordinate in victims {
            if !self.chunks.contains_key(&coordinate) || self.dirty.contains(&coordinate) {
                continue;
            }
            let permit = self.prepare_eviction(coordinate, lease)?;
            receipts.push(self.evict_committed(permit, tick, requests)?);
        }
        Ok(receipts)
    }

    /// Explicitly requests current derived work for a resident projection.
    ///
    /// # Errors
    ///
    /// Returns [`RuntimeError::ChunkNotResident`] when the target is absent.
    pub fn request_derived(
        &mut self,
        coordinate: ChunkCoordinate,
        kind: DerivedKind,
        tick: FixedTick,
        request: DerivedRequest,
    ) -> RuntimeResult<DerivedEnqueueReceipt> {
        let key = self
            .current_job_key(coordinate, kind)
            .ok_or(RuntimeError::ChunkNotResident { coordinate })?;
        Ok(self.enqueue_key(key, tick, request))
    }

    /// Starts the next stable-priority job if count and byte budgets allow it.
    ///
    /// The returned input owns a full chunk plus one voxel on every side. It is
    /// deliberately non-cloneable and must later be returned to completion or
    /// cancellation acknowledgement.
    ///
    /// # Errors
    ///
    /// Returns [`RuntimeError::CounterOverflow`] if process-local job identity
    /// is exhausted.
    pub fn dispatch_next(&mut self, kind: DerivedKind) -> RuntimeResult<DispatchOutcome<V>> {
        let global_available = self
            .limits
            .max_combined_reserved_bytes()
            .saturating_sub(self.diagnostics.combined_reserved_bytes);
        let candidate = match self.queues[kind.index()].next_candidate(global_available) {
            Ok(candidate) => candidate,
            Err(None) => return Ok(DispatchOutcome::Empty),
            Err(Some(reason)) => return Ok(DispatchOutcome::Backpressured { reason }),
        };
        let (samples, actual_input_bytes) = self.capture_halo(candidate.key.coordinate());
        if actual_input_bytes > candidate.input_bytes {
            let removed = self.queues[kind.index()].reject_pending_memory_contract(&candidate.key);
            debug_assert!(removed, "selected pending job must remain until dispatch");
            if kind == DerivedKind::Collider {
                self.set_collider_failure_if_current(
                    &candidate.key,
                    ColliderFailure::MemoryContractViolation,
                );
            }
            return Ok(DispatchOutcome::MemoryContractViolation {
                key: candidate.key,
                reserved_input_bytes: candidate.input_bytes,
                actual_input_bytes,
            });
        }

        let next_job_id = self
            .next_job_id
            .checked_add(1)
            .ok_or(RuntimeError::CounterOverflow {
                counter: "derived job ID",
                coordinate: candidate.key.coordinate(),
            })?;
        let ticket = self.queues[kind.index()].start(
            &candidate,
            DerivedJobId::new(next_job_id),
            self.scope.epoch(),
            self.runtime_generation,
        );
        self.next_job_id = next_job_id;
        self.diagnostics.combined_reserved_bytes = self
            .diagnostics
            .combined_reserved_bytes
            .checked_add(ticket.reserved_bytes())
            .expect("dispatch admission proved the global reservation fits");
        self.diagnostics.combined_reserved_bytes_high_water = self
            .diagnostics
            .combined_reserved_bytes_high_water
            .max(self.diagnostics.combined_reserved_bytes);
        Ok(DispatchOutcome::Started(DerivedInput::new(
            ticket,
            self.dimensions,
            samples,
            actual_input_bytes,
        )))
    }

    /// Revalidates, applies, then releases one externally completed result.
    ///
    /// `apply` runs only for an active exact source. Its typed failure and panic
    /// are contained; either leaves the previous derived state unchanged.
    /// Result and input declarations are checked before apply. The caller must
    /// ensure `apply_bytes` conservatively covers temporary apply staging.
    #[allow(
        clippy::too_many_lines,
        reason = "the completion state machine keeps resource release adjacent to every terminal outcome"
    )]
    pub fn complete_derived<R, A, E>(
        &mut self,
        input: DerivedInput<V>,
        result: R,
        apply_bytes: ApplyByteDeclaration,
        completed_tick: FixedTick,
        apply: impl FnOnce(R) -> Result<A, E>,
    ) -> CompletionOutcome<A, E, V, R>
    where
        R: RetainedBytes,
    {
        let ticket = input.ticket.clone();
        let kind = ticket.key.kind();
        let Some(state) = self.queues[kind.index()].state(&ticket) else {
            return CompletionOutcome::UnknownJob {
                job: ticket.id(),
                input,
                result,
            };
        };
        let result_bytes = result.retained_bytes();
        if state == JobState::CancelRequested {
            if result_bytes > ticket.result_byte_budget() {
                drop(result);
                drop(input);
                self.queues[kind.index()].mark_memory_contract_violation();
                self.release_ticket(&ticket);
                self.queues[kind.index()].mark_cancel_acknowledged();
                return CompletionOutcome::MemoryContractViolation {
                    job: ticket.id(),
                    stage: MemoryStage::Result,
                    budget: ticket.result_byte_budget(),
                    actual: result_bytes,
                };
            }
            drop(result);
            let input_bytes = input.actual_retained_bytes();
            drop(input);
            let released = self.release_ticket(&ticket);
            self.queues[kind.index()].mark_cancel_acknowledged();
            return CompletionOutcome::Cancelled {
                receipt: CancellationReleaseReceipt::new(
                    ticket.id(),
                    input_bytes,
                    result_bytes,
                    released,
                ),
            };
        }
        if result_bytes > ticket.result_byte_budget() {
            drop(result);
            drop(input);
            self.queues[kind.index()].mark_memory_contract_violation();
            self.set_collider_failure_if_current(
                &ticket.key,
                ColliderFailure::MemoryContractViolation,
            );
            self.release_ticket(&ticket);
            return CompletionOutcome::MemoryContractViolation {
                job: ticket.id(),
                stage: MemoryStage::Result,
                budget: ticket.result_byte_budget(),
                actual: result_bytes,
            };
        }
        if apply_bytes.get() > ticket.apply_byte_budget() {
            drop(result);
            drop(input);
            self.queues[kind.index()].mark_memory_contract_violation();
            self.set_collider_failure_if_current(
                &ticket.key,
                ColliderFailure::MemoryContractViolation,
            );
            self.release_ticket(&ticket);
            return CompletionOutcome::MemoryContractViolation {
                job: ticket.id(),
                stage: MemoryStage::Apply,
                budget: ticket.apply_byte_budget(),
                actual: apply_bytes.get(),
            };
        }
        if let Some(reason) = self.stale_reason(&ticket.key) {
            drop(result);
            drop(input);
            self.queues[kind.index()].mark_stale_rejected();
            self.release_ticket(&ticket);
            return CompletionOutcome::StaleRejected {
                job: ticket.id(),
                reason,
            };
        }

        let applied = catch_unwind(AssertUnwindSafe(|| apply(result)));
        match applied {
            Ok(Ok(value)) => {
                drop(input);
                let chunk = self
                    .chunks
                    .get_mut(&ticket.key.coordinate())
                    .expect("successful current-source validation requires a resident target");
                chunk.last_applied[kind.index()] = Some(ticket.key.clone());
                if kind == DerivedKind::Collider {
                    chunk.collider_safety = ColliderSafetyState::Ready {
                        source_fingerprint: ticket.key.source_fingerprint(),
                        world_revision: ticket.key.world_revision(),
                        voxel_revision: ticket.key.voxel_revision(),
                    };
                }
                self.queues[kind.index()].mark_applied();
                let released = self.release_ticket(&ticket);
                let latency = completed_tick
                    .get()
                    .saturating_sub(ticket.projected_tick().get());
                self.diagnostics.last_commit_to_apply_ticks = Some(latency);
                self.diagnostics.max_commit_to_apply_ticks =
                    self.diagnostics.max_commit_to_apply_ticks.max(latency);
                CompletionOutcome::Applied {
                    receipt: DerivedApplyReceipt::new(
                        ticket.id(),
                        ticket.key,
                        completed_tick,
                        latency,
                        result_bytes,
                        apply_bytes.get(),
                        released,
                    ),
                    value,
                }
            }
            Ok(Err(error)) => {
                drop(input);
                self.queues[kind.index()].mark_apply_failed();
                self.set_collider_failure_if_current(&ticket.key, ColliderFailure::ApplyRejected);
                self.release_ticket(&ticket);
                CompletionOutcome::ApplyFailed {
                    job: ticket.id(),
                    error,
                }
            }
            Err(payload) => {
                drop(payload);
                drop(input);
                self.queues[kind.index()].mark_apply_panicked();
                self.set_collider_failure_if_current(&ticket.key, ColliderFailure::ApplyPanicked);
                self.release_ticket(&ticket);
                CompletionOutcome::ApplyPanicked { job: ticket.id() }
            }
        }
    }

    /// Acknowledges executor cancellation and releases owned buffers and budget.
    ///
    /// Active or foreign tickets are returned untouched. A provided result is
    /// still checked against the ticket's declared result budget.
    pub fn acknowledge_cancelled<R>(
        &mut self,
        input: DerivedInput<V>,
        result: Option<R>,
    ) -> CancellationAckOutcome<V, R>
    where
        R: RetainedBytes,
    {
        let ticket = input.ticket.clone();
        let kind = ticket.key.kind();
        let Some(state) = self.queues[kind.index()].state(&ticket) else {
            return CancellationAckOutcome::UnknownJob { input, result };
        };
        if state == JobState::Active {
            return CancellationAckOutcome::NotRequested { input, result };
        }
        let result_bytes = result.as_ref().map_or(0, RetainedBytes::retained_bytes);
        let input_bytes = input.actual_retained_bytes();
        drop(result);
        drop(input);
        let released = self.release_ticket(&ticket);
        self.queues[kind.index()].mark_cancel_acknowledged();
        if result_bytes > ticket.result_byte_budget() {
            self.queues[kind.index()].mark_memory_contract_violation();
            return CancellationAckOutcome::MemoryContractViolation {
                job: ticket.id(),
                budget: ticket.result_byte_budget(),
                actual: result_bytes,
            };
        }
        CancellationAckOutcome::Released(CancellationReleaseReceipt::new(
            ticket.id(),
            input_bytes,
            result_bytes,
            released,
        ))
    }

    pub(crate) fn resident_sample(
        &self,
        coordinate: VoxelCoordinate,
    ) -> RuntimeResult<Option<(&V, WorldRevision, ChunkRevision, VoxelRevision)>> {
        let (chunk_coordinate, local) = self.split_coordinate(coordinate)?;
        Ok(self.chunks.get(&chunk_coordinate).map(|chunk| {
            (
                &chunk.cells[self.cell_index(local)],
                chunk.world_revision,
                chunk.revision,
                chunk.voxel_revision,
            )
        }))
    }

    fn enqueue_work(
        &mut self,
        work: &BTreeSet<(ChunkCoordinate, DerivedKind)>,
        tick: FixedTick,
        requests: DerivedRequestSet,
    ) -> Vec<DerivedEnqueueReceipt> {
        work.iter()
            .filter_map(|&(coordinate, kind)| {
                self.current_job_key(coordinate, kind)
                    .map(|key| self.enqueue_key(key, tick, requests.get(kind)))
            })
            .collect()
    }

    fn enqueue_key(
        &mut self,
        key: DerivedJobKey,
        tick: FixedTick,
        request: DerivedRequest,
    ) -> DerivedEnqueueReceipt {
        let input_bytes = self.halo_retained_bytes(key.coordinate());
        let memory = request.memory();
        let reserved_bytes = input_bytes
            .checked_add(memory.result_bytes())
            .and_then(|bytes| bytes.checked_add(memory.apply_bytes()))
            .unwrap_or(u64::MAX);
        let decision = if reserved_bytes > self.limits.max_combined_reserved_bytes() {
            self.queues[key.kind().index()].mark_rejected_memory_budget();
            EnqueueDecision::RejectedMemoryBudget
        } else {
            self.queues[key.kind().index()].enqueue(PendingJob {
                key: key.clone(),
                priority: request.priority(),
                owner: request.owner(),
                input_bytes,
                result_bytes: memory.result_bytes(),
                apply_bytes: memory.apply_bytes(),
                reserved_bytes,
                projected_tick: tick,
            })
        };
        DerivedEnqueueReceipt::new(key, decision, reserved_bytes)
    }

    fn current_job_key(
        &self,
        coordinate: ChunkCoordinate,
        kind: DerivedKind,
    ) -> Option<DerivedJobKey> {
        let chunk = self.chunks.get(&coordinate)?;
        let neighbors = self.neighbor_revisions(coordinate);
        let semantics = match kind {
            DerivedKind::Mesh => DerivedSemanticFingerprint::Mesh(chunk.mesh_semantics),
            DerivedKind::Collider => DerivedSemanticFingerprint::Collider(chunk.collider_semantics),
        };
        let source_fingerprint = source_fingerprint(
            &self.scope,
            self.edge,
            kind,
            coordinate,
            chunk.world_revision,
            chunk.revision,
            chunk.voxel_revision,
            neighbors,
            semantics,
        );
        Some(DerivedJobKey::new(
            kind,
            coordinate,
            self.scope.epoch(),
            chunk.world_revision,
            chunk.revision,
            chunk.voxel_revision,
            neighbors,
            semantics,
            source_fingerprint,
        ))
    }

    fn stale_reason(&self, key: &DerivedJobKey) -> Option<StaleReason> {
        if key.epoch() != self.scope.epoch() {
            return Some(StaleReason::WorldEpoch);
        }
        let Some(chunk) = self.chunks.get(&key.coordinate()) else {
            return Some(StaleReason::TargetEvicted);
        };
        if key.world_revision() != chunk.world_revision {
            return Some(StaleReason::WorldRevision);
        }
        if key.revision() != chunk.revision {
            return Some(StaleReason::ChunkRevision);
        }
        if key.voxel_revision() != chunk.voxel_revision {
            return Some(StaleReason::VoxelRevision);
        }
        let current_neighbors = self.neighbor_revisions(key.coordinate());
        for face in Face::ALL {
            if key.neighbors().get(face) != current_neighbors.get(face) {
                return Some(StaleReason::NeighborRevision { face });
            }
        }
        match key.semantics() {
            DerivedSemanticFingerprint::Mesh(value) if value != chunk.mesh_semantics => {
                return Some(StaleReason::MeshSemantics);
            }
            DerivedSemanticFingerprint::Collider(value) if value != chunk.collider_semantics => {
                return Some(StaleReason::ColliderSemantics);
            }
            DerivedSemanticFingerprint::Mesh(_) | DerivedSemanticFingerprint::Collider(_) => {}
        }
        let current = self
            .current_job_key(key.coordinate(), key.kind())
            .expect("resident source must produce a current derived key");
        debug_assert_eq!(
            key.source_fingerprint(),
            current.source_fingerprint(),
            "canonical fingerprint must be a pure function of validated fields"
        );
        None
    }

    fn neighbor_revisions(&self, coordinate: ChunkCoordinate) -> NeighborRevisions {
        NeighborRevisions::new(Face::ALL.map(|face| {
            neighbor_coordinate(coordinate, face)
                .and_then(|neighbor| self.chunks.get(&neighbor))
                .map_or(NeighborRevision::Missing, |chunk| {
                    NeighborRevision::Resident {
                        world_revision: chunk.world_revision,
                        revision: chunk.revision,
                        voxel_revision: chunk.voxel_revision,
                    }
                })
        }))
    }

    fn resident_face_neighbors(&self, coordinate: ChunkCoordinate) -> BTreeSet<ChunkCoordinate> {
        Face::ALL
            .into_iter()
            .filter_map(|face| neighbor_coordinate(coordinate, face))
            .filter(|neighbor| self.chunks.contains_key(neighbor))
            .collect()
    }

    fn halo_retained_bytes(&self, coordinate: ChunkCoordinate) -> u64 {
        let edge = usize::from(self.edge);
        let mut bytes = u64::try_from(mem::size_of::<DerivedInput<V>>()).unwrap_or(u64::MAX);
        for z in 0..edge + 2 {
            for y in 0..edge + 2 {
                for x in 0..edge + 2 {
                    bytes = bytes
                        .saturating_add(self.padded_sample(coordinate, [x, y, z]).retained_bytes());
                }
            }
        }
        bytes
    }

    fn capture_halo(&self, coordinate: ChunkCoordinate) -> (Vec<V>, u64) {
        let edge = usize::from(self.edge);
        let mut samples = Vec::with_capacity(self.dimensions.volume_len());
        let mut bytes = u64::try_from(mem::size_of::<DerivedInput<V>>()).unwrap_or(u64::MAX);
        for z in 0..edge + 2 {
            for y in 0..edge + 2 {
                for x in 0..edge + 2 {
                    let sample = self.padded_sample(coordinate, [x, y, z]).clone();
                    bytes = bytes.saturating_add(sample.retained_bytes());
                    samples.push(sample);
                }
            }
        }
        debug_assert_eq!(samples.len(), self.dimensions.volume_len());
        (samples, bytes)
    }

    fn padded_sample(&self, coordinate: ChunkCoordinate, [x, y, z]: [usize; 3]) -> &V {
        let edge = usize::from(self.edge);
        let outside = [x, y, z]
            .into_iter()
            .filter(|value| *value == 0 || *value == edge + 1)
            .count();
        if outside == 0 {
            let chunk = self
                .chunks
                .get(&coordinate)
                .expect("only resident targets may be queued");
            return &chunk.cells[canonical_index(edge, x - 1, y - 1, z - 1)];
        }
        if outside != 1 {
            return &self.empty;
        }

        let (face, local) = if x == 0 {
            (Face::NegX, [edge - 1, y - 1, z - 1])
        } else if x == edge + 1 {
            (Face::PosX, [0, y - 1, z - 1])
        } else if y == 0 {
            (Face::NegY, [x - 1, edge - 1, z - 1])
        } else if y == edge + 1 {
            (Face::PosY, [x - 1, 0, z - 1])
        } else if z == 0 {
            (Face::NegZ, [x - 1, y - 1, edge - 1])
        } else {
            (Face::PosZ, [x - 1, y - 1, 0])
        };
        let Some(neighbor) =
            neighbor_coordinate(coordinate, face).and_then(|neighbor| self.chunks.get(&neighbor))
        else {
            return &self.empty;
        };
        &neighbor.cells[canonical_index(edge, local[0], local[1], local[2])]
    }

    fn split_coordinate(
        &self,
        coordinate: VoxelCoordinate,
    ) -> RuntimeResult<(ChunkCoordinate, [usize; 3])> {
        let edge = i64::from(self.edge);
        let values = coordinate.as_array();
        let chunks = values.map(|value| value.div_euclid(edge));
        let x = i32::try_from(chunks[0])
            .map_err(|_| RuntimeError::CoordinateOutOfRange { coordinate })?;
        let y = i32::try_from(chunks[1])
            .map_err(|_| RuntimeError::CoordinateOutOfRange { coordinate })?;
        let z = i32::try_from(chunks[2])
            .map_err(|_| RuntimeError::CoordinateOutOfRange { coordinate })?;
        let local = values.map(|value| {
            usize::try_from(value.rem_euclid(edge))
                .expect("Euclidean remainder of a positive u16 edge fits usize")
        });
        Ok((ChunkCoordinate::new(x, y, z), local))
    }

    fn cell_index(&self, [x, y, z]: [usize; 3]) -> usize {
        canonical_index(usize::from(self.edge), x, y, z)
    }

    fn release_ticket(&mut self, ticket: &crate::DerivedTicket) -> u64 {
        let released = self.queues[ticket.key.kind().index()]
            .release(ticket)
            .expect("ticket state was validated before owned resources were dropped");
        self.diagnostics.combined_reserved_bytes = self
            .diagnostics
            .combined_reserved_bytes
            .checked_sub(released)
            .expect("released ticket bytes must remain in the combined ledger");
        released
    }

    fn set_collider_failure_if_current(&mut self, key: &DerivedJobKey, failure: ColliderFailure) {
        if key.kind() != DerivedKind::Collider || self.stale_reason(key).is_some() {
            return;
        }
        let chunk = self
            .chunks
            .get_mut(&key.coordinate())
            .expect("current collider key requires a resident target");
        chunk.collider_safety = ColliderSafetyState::FailedConservative {
            world_revision: chunk.world_revision,
            voxel_revision: chunk.voxel_revision,
            failure,
        };
    }

    fn refresh_resident_diagnostics(&mut self) {
        self.diagnostics.resident_chunks = self.chunks.len();
        self.diagnostics.resident_high_water = self
            .diagnostics
            .resident_high_water
            .max(self.diagnostics.resident_chunks);
        self.diagnostics.dirty_chunks = self.dirty.len();
    }

    fn make_room_for_admission(
        &mut self,
        incoming: ChunkCoordinate,
        tick: FixedTick,
        requests: DerivedRequestSet,
    ) -> RuntimeResult<()> {
        if self.chunks.contains_key(&incoming) {
            return Ok(());
        }
        while self.chunks.len() >= self.limits.max_resident_chunks() {
            let Some(victim) = self.next_clean_outside_interest_victim() else {
                return Err(RuntimeError::ResidentLimitExceeded {
                    limit: self.limits.max_resident_chunks(),
                });
            };
            let lease = EvictionLeaseGeneration::new(self.next_eviction_lease);
            self.next_eviction_lease = self.next_eviction_lease.saturating_add(1);
            let permit = self.prepare_eviction(victim, lease)?;
            self.evict_committed(permit, tick, requests)?;
        }
        Ok(())
    }

    fn next_clean_outside_interest_victim(&self) -> Option<ChunkCoordinate> {
        self.clean_outside_interest_victims().into_iter().next()
    }

    fn clean_outside_interest_victims(&self) -> Vec<ChunkCoordinate> {
        let Some(interest) = self.interest else {
            return Vec::new();
        };
        let mut victims: Vec<ChunkCoordinate> = self
            .chunks
            .keys()
            .copied()
            .filter(|coordinate| {
                !self.dirty.contains(coordinate) && !interest.contains(*coordinate)
            })
            .collect();
        victims.sort_by(|left, right| {
            interest
                .chebyshev_distance(*right)
                .cmp(&interest.chebyshev_distance(*left))
                .then_with(|| left.cmp(right))
        });
        victims
    }
}

fn canonical_index(edge: usize, x: usize, y: usize, z: usize) -> usize {
    x + edge * (z + edge * y)
}

fn neighbor_coordinate(coordinate: ChunkCoordinate, face: Face) -> Option<ChunkCoordinate> {
    match face {
        Face::PosX => coordinate
            .x
            .checked_add(1)
            .map(|x| ChunkCoordinate::new(x, coordinate.y, coordinate.z)),
        Face::NegX => coordinate
            .x
            .checked_sub(1)
            .map(|x| ChunkCoordinate::new(x, coordinate.y, coordinate.z)),
        Face::PosY => coordinate
            .y
            .checked_add(1)
            .map(|y| ChunkCoordinate::new(coordinate.x, y, coordinate.z)),
        Face::NegY => coordinate
            .y
            .checked_sub(1)
            .map(|y| ChunkCoordinate::new(coordinate.x, y, coordinate.z)),
        Face::PosZ => coordinate
            .z
            .checked_add(1)
            .map(|z| ChunkCoordinate::new(coordinate.x, coordinate.y, z)),
        Face::NegZ => coordinate
            .z
            .checked_sub(1)
            .map(|z| ChunkCoordinate::new(coordinate.x, coordinate.y, z)),
    }
}

#[allow(
    clippy::too_many_arguments,
    reason = "the canonical source receipt lists every invalidation input explicitly"
)]
fn source_fingerprint(
    scope: &WorkingSetScope,
    edge: u16,
    kind: DerivedKind,
    coordinate: ChunkCoordinate,
    world_revision: WorldRevision,
    revision: ChunkRevision,
    voxel_revision: VoxelRevision,
    neighbors: NeighborRevisions,
    semantics: DerivedSemanticFingerprint,
) -> DerivedSourceFingerprint {
    let mut hash = Sha256::new();
    hash.update(SOURCE_FINGERPRINT_DOMAIN);
    hash.update(scope.world().as_bytes());
    let dimension = scope.dimension().as_str().as_bytes();
    hash.update(
        u64::try_from(dimension.len())
            .unwrap_or(u64::MAX)
            .to_be_bytes(),
    );
    hash.update(dimension);
    hash.update(scope.epoch().get().to_be_bytes());
    hash.update(edge.to_be_bytes());
    hash.update([match kind {
        DerivedKind::Mesh => 0,
        DerivedKind::Collider => 1,
    }]);
    hash.update(coordinate.x.to_be_bytes());
    hash.update(coordinate.y.to_be_bytes());
    hash.update(coordinate.z.to_be_bytes());
    hash.update(world_revision.get().to_be_bytes());
    hash.update(revision.get().to_be_bytes());
    hash.update(voxel_revision.get().to_be_bytes());
    for neighbor in neighbors.as_array() {
        match neighbor {
            NeighborRevision::Missing => hash.update([0]),
            NeighborRevision::Resident {
                world_revision,
                revision,
                voxel_revision,
            } => {
                hash.update([1]);
                hash.update(world_revision.get().to_be_bytes());
                hash.update(revision.get().to_be_bytes());
                hash.update(voxel_revision.get().to_be_bytes());
            }
        }
    }
    match semantics {
        DerivedSemanticFingerprint::Mesh(value) => {
            hash.update([0]);
            hash.update(value.into_bytes());
        }
        DerivedSemanticFingerprint::Collider(value) => {
            hash.update([1]);
            hash.update(value.into_bytes());
        }
    }
    DerivedSourceFingerprint::new(hash.finalize().into())
}

#[cfg(test)]
mod tests {
    use super::{canonical_index, neighbor_coordinate};
    use latticeaxiom_storage::ChunkCoordinate;
    use latticeaxiom_voxel_mesh::Face;

    #[test]
    fn canonical_layout_is_y_layers_z_rows_x_fastest() {
        assert_eq!(canonical_index(4, 3, 0, 0), 3);
        assert_eq!(canonical_index(4, 0, 0, 1), 4);
        assert_eq!(canonical_index(4, 0, 1, 0), 16);
    }

    #[test]
    fn neighbor_steps_are_checked_at_i32_extremes() {
        assert_eq!(
            neighbor_coordinate(ChunkCoordinate::new(i32::MAX, 0, 0), Face::PosX),
            None
        );
        assert_eq!(
            neighbor_coordinate(ChunkCoordinate::new(i32::MIN, 0, 0), Face::NegX),
            None
        );
    }
}
