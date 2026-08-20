//! Committed-projection Y-up voxel DDA selection.

use latticeaxiom_storage::{ChunkCoordinate, ChunkRevision, VoxelRevision, WorldRevision};
use latticeaxiom_voxel_mesh::Face;

use crate::{
    CollisionSemantics, MAX_AUTHORITATIVE_REACH_METERS, RetainedBytes, RuntimeError, RuntimeResult,
    VoxelCoordinate, VoxelRuntime,
};

/// Precision-preserving ray origin expressed as a chunk anchor plus local meters.
///
/// Keeping the fractional position local to a chunk avoids loss of sub-voxel
/// precision near the extrema of the canonical `i32` chunk-coordinate range.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DdaOrigin {
    chunk: ChunkCoordinate,
    local: [f64; 3],
}

impl DdaOrigin {
    /// Creates an origin.
    ///
    /// Local coordinates are validated against the runtime chunk edge when a
    /// query is executed.
    #[must_use]
    pub const fn new(chunk: ChunkCoordinate, local: [f64; 3]) -> Self {
        Self { chunk, local }
    }

    /// Returns the integer chunk anchor.
    #[must_use]
    pub const fn chunk(self) -> ChunkCoordinate {
        self.chunk
    }

    /// Returns the local meter position in native `(x, y, z)` order.
    #[must_use]
    pub const fn local(self) -> [f64; 3] {
        self.local
    }
}

/// Bounded selection query over resident committed voxel projections.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DdaQuery {
    origin: DdaOrigin,
    direction: [f64; 3],
    max_distance: f64,
}

impl DdaQuery {
    /// Creates a query that is fully validated when run against a working set.
    #[must_use]
    pub const fn new(origin: DdaOrigin, direction: [f64; 3], max_distance: f64) -> Self {
        Self {
            origin,
            direction,
            max_distance,
        }
    }

    /// Returns the chunk-relative origin.
    #[must_use]
    pub const fn origin(self) -> DdaOrigin {
        self.origin
    }

    /// Returns the caller direction; normalization is performed internally.
    #[must_use]
    pub const fn direction(self) -> [f64; 3] {
        self.direction
    }

    /// Returns the inclusive maximum traversal distance in meters.
    #[must_use]
    pub const fn max_distance(self) -> f64 {
        self.max_distance
    }
}

/// Selection policy for one committed voxel encountered by DDA.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CellSelection {
    /// Continue through this cell.
    PassThrough,
    /// Select this cell as the first valid gameplay target.
    Target,
    /// Stop because this cell blocks line of sight without being selectable.
    Occluder,
}

/// Revision-bearing committed cell observation returned by DDA.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DdaCell {
    coordinate: VoxelCoordinate,
    entered_face: Option<Face>,
    distance: f64,
    world_revision: WorldRevision,
    revision: ChunkRevision,
    voxel_revision: VoxelRevision,
}

impl DdaCell {
    const fn new(
        coordinate: VoxelCoordinate,
        entered_face: Option<Face>,
        distance: f64,
        world_revision: WorldRevision,
        revision: ChunkRevision,
        voxel_revision: VoxelRevision,
    ) -> Self {
        Self {
            coordinate,
            entered_face,
            distance,
            world_revision,
            revision,
            voxel_revision,
        }
    }

    /// Returns the selected or blocking world-space voxel.
    #[must_use]
    pub const fn coordinate(self) -> VoxelCoordinate {
        self.coordinate
    }

    /// Returns the face crossed to enter the cell.
    ///
    /// The face is absent for the cell containing the origin. For a simultaneous
    /// edge or corner crossing, X, then Y, then Z is the stable reporting order.
    #[must_use]
    pub const fn entered_face(self) -> Option<Face> {
        self.entered_face
    }

    /// Returns the normalized ray distance in meters.
    #[must_use]
    pub const fn distance(self) -> f64 {
        self.distance
    }

    /// Returns the committed world revision observed during selection.
    #[must_use]
    pub const fn world_revision(self) -> WorldRevision {
        self.world_revision
    }

    /// Returns the committed total chunk revision observed during selection.
    #[must_use]
    pub const fn revision(self) -> ChunkRevision {
        self.revision
    }

    /// Returns the committed voxel-domain revision observed during selection.
    #[must_use]
    pub const fn voxel_revision(self) -> VoxelRevision {
        self.voxel_revision
    }

    /// Returns the cell adjacent to the entered face for placement.
    ///
    /// `None` is returned when the ray started in this cell or the adjacent
    /// coordinate cannot be represented by [`VoxelCoordinate`].
    #[must_use]
    pub fn adjacent_placement(self) -> Option<VoxelCoordinate> {
        let [dx, dy, dz] = self.entered_face?.normal_offset();
        Some(VoxelCoordinate::new(
            self.coordinate.x.checked_add(i64::try_from(dx).ok()?)?,
            self.coordinate.y.checked_add(i64::try_from(dy).ok()?)?,
            self.coordinate.z.checked_add(i64::try_from(dz).ok()?)?,
        ))
    }
}

/// First unavailable committed cell encountered by a selection query.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DdaUnavailable {
    coordinate: VoxelCoordinate,
    distance: f64,
}

impl DdaUnavailable {
    /// Returns the first coordinate whose committed projection was not resident.
    #[must_use]
    pub const fn coordinate(self) -> VoxelCoordinate {
        self.coordinate
    }

    /// Returns the normalized ray distance to the unavailable cell.
    #[must_use]
    pub const fn distance(self) -> f64 {
        self.distance
    }
}

/// Deterministic result of a bounded committed voxel traversal.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum DdaOutcome {
    /// First cell accepted by the caller's selection policy.
    Hit(DdaCell),
    /// First line-of-sight blocker rejected by the selection policy.
    Occluded(DdaCell),
    /// Traversal reached a chunk outside the resident committed projection set.
    Unavailable(DdaUnavailable),
    /// No target or blocker was encountered within the inclusive reach.
    NoTarget,
}

impl<V> VoxelRuntime<V>
where
    V: Clone + Eq + RetainedBytes + CollisionSemantics,
{
    /// Traverses resident committed projections with native Y-up face semantics.
    ///
    /// The policy closure sees decoded values from storage-committed projections.
    /// A missing projection stops traversal instead of being interpreted as air.
    /// The direction is normalized internally, so distances are meters. A cell
    /// entered exactly at `max_distance` is visited; a cell entered later is not.
    ///
    /// Origins on an integer boundary belong to the cell in the ray direction.
    /// Axes whose boundary times tie are advanced together. The reported entered
    /// face uses stable X/Y/Z priority while `+Y` remains world up and conventional
    /// forward remains `-Z`.
    ///
    /// # Errors
    ///
    /// Returns [`RuntimeError::InvalidDdaQuery`] for non-finite, out-of-chunk, or
    /// zero-direction input; [`RuntimeError::ReachExceeded`] above the accepted
    /// five-meter reach; or [`RuntimeError::CoordinateOutOfRange`] if traversal
    /// leaves the canonical `i32` chunk-coordinate domain.
    pub fn raycast_committed(
        &self,
        query: DdaQuery,
        mut classify: impl FnMut(&V) -> CellSelection,
    ) -> RuntimeResult<DdaOutcome> {
        let validated = self.validate_dda(query)?;
        let mut cell = initial_cell(
            query.origin.chunk,
            self.chunk_edge(),
            query.origin.local,
            validated.direction,
        );
        let steps = validated.direction.map(step_for_component);
        let mut t_delta = [f64::INFINITY; 3];
        let mut t_max = [f64::INFINITY; 3];
        for axis in 0..3 {
            if steps[axis] == 0 {
                continue;
            }
            t_delta[axis] = validated.direction[axis].abs().recip();
            t_max[axis] = first_boundary_distance(
                query.origin.local[axis],
                validated.direction[axis],
                steps[axis],
            );
        }

        let mut distance = 0.0;
        let mut entered_face = None;
        loop {
            let coordinate = VoxelCoordinate::new(cell[0], cell[1], cell[2]);
            let Some((value, world_revision, revision, voxel_revision)) =
                self.resident_sample(coordinate)?
            else {
                return Ok(DdaOutcome::Unavailable(DdaUnavailable {
                    coordinate,
                    distance,
                }));
            };
            let observed = DdaCell::new(
                coordinate,
                entered_face,
                distance,
                world_revision,
                revision,
                voxel_revision,
            );
            match classify(value) {
                CellSelection::PassThrough => {}
                CellSelection::Target => return Ok(DdaOutcome::Hit(observed)),
                CellSelection::Occluder => return Ok(DdaOutcome::Occluded(observed)),
            }

            let next_distance = t_max.into_iter().fold(f64::INFINITY, f64::min);
            if !next_distance.is_finite() || next_distance > validated.max_distance {
                return Ok(DdaOutcome::NoTarget);
            }

            let mut next_cell = cell;
            let mut first_axis = None;
            for axis in 0..3 {
                if t_max[axis] <= validated.max_distance
                    && boundary_times_tie(t_max[axis], next_distance)
                {
                    first_axis.get_or_insert(axis);
                    let Some(next_coordinate) = next_cell[axis].checked_add(i64::from(steps[axis]))
                    else {
                        return Ok(DdaOutcome::NoTarget);
                    };
                    next_cell[axis] = next_coordinate;
                    t_max[axis] += t_delta[axis];
                }
            }
            cell = next_cell;
            distance = next_distance.max(0.0);
            entered_face = first_axis.map(|axis| entered_face_for_step(axis, steps[axis]));
        }
    }

    fn validate_dda(&self, query: DdaQuery) -> RuntimeResult<ValidatedDda> {
        let local = query.origin.local;
        let edge = f64::from(self.chunk_edge());
        if local
            .into_iter()
            .any(|component| !component.is_finite() || component < 0.0 || component >= edge)
        {
            return Err(RuntimeError::InvalidDdaQuery {
                reason: "origin local coordinates must be finite and inside the chunk",
            });
        }
        if !query.max_distance.is_finite() || query.max_distance < 0.0 {
            return Err(RuntimeError::InvalidDdaQuery {
                reason: "maximum distance must be finite and nonnegative",
            });
        }
        if query.max_distance > MAX_AUTHORITATIVE_REACH_METERS {
            return Err(RuntimeError::ReachExceeded {
                requested_millimeters: requested_millimeters(query.max_distance),
                maximum_millimeters: 5_000,
            });
        }
        if query.direction.into_iter().any(|value| !value.is_finite()) {
            return Err(RuntimeError::InvalidDdaQuery {
                reason: "direction components must be finite",
            });
        }

        let direction_scale = query
            .direction
            .into_iter()
            .map(f64::abs)
            .fold(0.0, f64::max);
        if direction_scale <= 0.0 {
            return Err(RuntimeError::InvalidDdaQuery {
                reason: "direction must have nonzero length",
            });
        }
        let scaled_direction = query.direction.map(|value| value / direction_scale);
        let scaled_length = scaled_direction
            .into_iter()
            .map(|value| value * value)
            .sum::<f64>()
            .sqrt();
        let direction = scaled_direction.map(|value| value / scaled_length);

        Ok(ValidatedDda {
            direction,
            max_distance: query.max_distance,
        })
    }
}

#[derive(Clone, Copy, Debug)]
struct ValidatedDda {
    direction: [f64; 3],
    max_distance: f64,
}

fn step_for_component(component: f64) -> i8 {
    if component > 0.0 {
        1
    } else if component < 0.0 {
        -1
    } else {
        0
    }
}

fn entered_face_for_step(axis: usize, step: i8) -> Face {
    match (axis, step.is_positive()) {
        (0, true) => Face::NegX,
        (0, false) => Face::PosX,
        (1, true) => Face::NegY,
        (1, false) => Face::PosY,
        (2, true) => Face::NegZ,
        (2, false) => Face::PosZ,
        _ => unreachable!("DDA axes are limited to X/Y/Z and zero steps are skipped"),
    }
}

fn boundary_times_tie(left: f64, right: f64) -> bool {
    if !left.is_finite() || !right.is_finite() {
        return false;
    }
    let scale = left.abs().max(right.abs()).max(1.0);
    (left - right).abs() <= f64::EPSILON * 8.0 * scale
}

#[allow(
    clippy::float_cmp,
    reason = "exact local integer boundaries define deterministic negative-ray ownership"
)]
fn first_boundary_distance(local: f64, direction: f64, step: i8) -> f64 {
    let fraction = local - local.floor();
    if step > 0 {
        (1.0 - fraction) / direction
    } else if fraction == 0.0 {
        -1.0 / direction
    } else {
        -fraction / direction
    }
}

#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "the diagnostic saturates an already validated nonnegative finite distance"
)]
fn requested_millimeters(distance: f64) -> u32 {
    (distance * 1_000.0).ceil() as u32
}

#[allow(
    clippy::cast_possible_truncation,
    clippy::float_cmp,
    reason = "validated local coordinates are finite and below u16::MAX; exact boundaries define ownership"
)]
fn initial_cell(
    chunk: ChunkCoordinate,
    edge: u16,
    local: [f64; 3],
    direction: [f64; 3],
) -> [i64; 3] {
    let chunk_axes = [chunk.x, chunk.y, chunk.z];
    std::array::from_fn(|axis| {
        let anchor = i64::from(chunk_axes[axis]) * i64::from(edge);
        let local_floor = local[axis].floor();
        let containing = anchor + local_floor as i64;
        if direction[axis].is_sign_negative() && local[axis] == local_floor {
            containing - 1
        } else {
            containing
        }
    })
}
