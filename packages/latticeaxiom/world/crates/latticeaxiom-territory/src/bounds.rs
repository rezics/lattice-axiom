//! Bounded planning-cell and vertical regions.

use latticeaxiom_worldgen::PlanningCellCoordinateV1;
use serde::{Deserialize, Serialize};

use crate::{TerritoryError, TerritoryResult};

/// A finite half-open rectangle of two-dimensional planning cells.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PlanningCellBoundsV1 {
    min_x: i64,
    min_z: i64,
    max_x_exclusive: i64,
    max_z_exclusive: i64,
}

impl PlanningCellBoundsV1 {
    /// Creates a non-empty half-open planning-cell rectangle.
    ///
    /// # Errors
    ///
    /// Returns an error for an empty or inverted axis.
    #[allow(
        clippy::similar_names,
        reason = "x and z half-open bounds are intentionally symmetric"
    )]
    pub fn new(
        min_x: i64,
        min_z: i64,
        max_x_exclusive: i64,
        max_z_exclusive: i64,
    ) -> TerritoryResult<Self> {
        if min_x >= max_x_exclusive {
            return Err(TerritoryError::InvalidBounds {
                kind: "planning-cell x bounds",
                minimum: min_x,
                maximum: max_x_exclusive,
            });
        }
        if min_z >= max_z_exclusive {
            return Err(TerritoryError::InvalidBounds {
                kind: "planning-cell z bounds",
                minimum: min_z,
                maximum: max_z_exclusive,
            });
        }
        Ok(Self {
            min_x,
            min_z,
            max_x_exclusive,
            max_z_exclusive,
        })
    }

    /// Returns the inclusive minimum x coordinate.
    #[must_use]
    pub const fn min_x(self) -> i64 {
        self.min_x
    }

    /// Returns the inclusive minimum z coordinate.
    #[must_use]
    pub const fn min_z(self) -> i64 {
        self.min_z
    }

    /// Returns the exclusive maximum x coordinate.
    #[must_use]
    pub const fn max_x_exclusive(self) -> i64 {
        self.max_x_exclusive
    }

    /// Returns the exclusive maximum z coordinate.
    #[must_use]
    pub const fn max_z_exclusive(self) -> i64 {
        self.max_z_exclusive
    }

    /// Returns whether the rectangle contains a planning cell.
    #[must_use]
    pub const fn contains(self, cell: PlanningCellCoordinateV1) -> bool {
        cell.x >= self.min_x
            && cell.x < self.max_x_exclusive
            && cell.z >= self.min_z
            && cell.z < self.max_z_exclusive
    }

    /// Returns whether this rectangle fully contains another rectangle.
    #[must_use]
    pub const fn contains_bounds(self, other: Self) -> bool {
        other.min_x >= self.min_x
            && other.min_z >= self.min_z
            && other.max_x_exclusive <= self.max_x_exclusive
            && other.max_z_exclusive <= self.max_z_exclusive
    }

    /// Returns whether two rectangles share at least one planning cell.
    #[must_use]
    pub const fn overlaps(self, other: Self) -> bool {
        self.min_x < other.max_x_exclusive
            && other.min_x < self.max_x_exclusive
            && self.min_z < other.max_z_exclusive
            && other.min_z < self.max_z_exclusive
    }

    /// Returns conservative distance in planning cells to this rectangle's boundary.
    ///
    /// Contained cells use the minimum distance to an edge, which is zero on the
    /// rim. Outside cells use Chebyshev distance to the nearest contained cell.
    #[must_use]
    pub fn boundary_distance_cells(self, cell: PlanningCellCoordinateV1) -> u32 {
        if self.contains(cell) {
            let west = cell.x.saturating_sub(self.min_x);
            let east = self
                .max_x_exclusive
                .saturating_sub(1)
                .saturating_sub(cell.x);
            let north = cell.z.saturating_sub(self.min_z);
            let south = self
                .max_z_exclusive
                .saturating_sub(1)
                .saturating_sub(cell.z);
            return u32::try_from(west.min(east).min(north.min(south))).unwrap_or(u32::MAX);
        }
        let dx = if cell.x < self.min_x {
            self.min_x.saturating_sub(cell.x)
        } else if cell.x >= self.max_x_exclusive {
            cell.x
                .saturating_sub(self.max_x_exclusive.saturating_sub(1))
        } else {
            0
        };
        let dz = if cell.z < self.min_z {
            self.min_z.saturating_sub(cell.z)
        } else if cell.z >= self.max_z_exclusive {
            cell.z
                .saturating_sub(self.max_z_exclusive.saturating_sub(1))
        } else {
            0
        };
        u32::try_from(dx.max(dz)).unwrap_or(u32::MAX)
    }

    /// Returns the number of planning cells in this finite rectangle.
    ///
    /// # Errors
    ///
    /// Returns an error if its area does not fit in an unsigned 64-bit value.
    pub fn area(self) -> TerritoryResult<u64> {
        let width = u64::try_from(i128::from(self.max_x_exclusive) - i128::from(self.min_x))
            .map_err(|_| TerritoryError::ArithmeticOverflow {
                kind: "planning-cell bounds width",
            })?;
        let depth = u64::try_from(i128::from(self.max_z_exclusive) - i128::from(self.min_z))
            .map_err(|_| TerritoryError::ArithmeticOverflow {
                kind: "planning-cell bounds depth",
            })?;
        width
            .checked_mul(depth)
            .ok_or(TerritoryError::ArithmeticOverflow {
                kind: "planning-cell bounds area",
            })
    }
}

/// A finite half-open vertical range in world meters.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(deny_unknown_fields)]
pub struct VerticalRangeV1 {
    min_y: i32,
    max_y_exclusive: i32,
}

impl VerticalRangeV1 {
    /// Creates a non-empty half-open vertical range.
    ///
    /// # Errors
    ///
    /// Returns an error when the minimum is not below the exclusive maximum.
    pub fn new(min_y: i32, max_y_exclusive: i32) -> TerritoryResult<Self> {
        if min_y >= max_y_exclusive {
            return Err(TerritoryError::InvalidBounds {
                kind: "vertical bounds",
                minimum: i64::from(min_y),
                maximum: i64::from(max_y_exclusive),
            });
        }
        Ok(Self {
            min_y,
            max_y_exclusive,
        })
    }

    /// Returns the inclusive minimum y coordinate.
    #[must_use]
    pub const fn min_y(self) -> i32 {
        self.min_y
    }

    /// Returns the exclusive maximum y coordinate.
    #[must_use]
    pub const fn max_y_exclusive(self) -> i32 {
        self.max_y_exclusive
    }

    /// Returns whether this range contains a y coordinate.
    #[must_use]
    pub const fn contains(self, y: i32) -> bool {
        y >= self.min_y && y < self.max_y_exclusive
    }

    /// Returns whether this range contains another range.
    #[must_use]
    pub const fn contains_range(self, other: Self) -> bool {
        other.min_y >= self.min_y && other.max_y_exclusive <= self.max_y_exclusive
    }

    /// Returns whether two ranges share at least one integer y coordinate.
    #[must_use]
    pub const fn overlaps(self, other: Self) -> bool {
        self.min_y < other.max_y_exclusive && other.min_y < self.max_y_exclusive
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn half_open_bounds_have_exact_edges_and_area() {
        let bounds = PlanningCellBoundsV1::new(-2, 4, 3, 7)
            .unwrap_or_else(|error| panic!("valid bounds were rejected: {error}"));
        assert!(bounds.contains(PlanningCellCoordinateV1::new(-2, 4)));
        assert!(bounds.contains(PlanningCellCoordinateV1::new(2, 6)));
        assert!(!bounds.contains(PlanningCellCoordinateV1::new(3, 6)));
        assert_eq!(bounds.area().ok(), Some(15));
        assert_eq!(
            bounds.boundary_distance_cells(PlanningCellCoordinateV1::new(-2, 4)),
            0
        );
        assert_eq!(
            bounds.boundary_distance_cells(PlanningCellCoordinateV1::new(0, 5)),
            1
        );
        assert_eq!(
            bounds.boundary_distance_cells(PlanningCellCoordinateV1::new(4, 5)),
            2
        );
    }

    #[test]
    fn invalid_ranges_are_rejected() {
        assert!(PlanningCellBoundsV1::new(0, 0, 0, 1).is_err());
        assert!(VerticalRangeV1::new(4, 4).is_err());
    }
}
