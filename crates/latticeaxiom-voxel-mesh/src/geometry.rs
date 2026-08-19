//! Output geometry: face directions, quads, and vertex emission helpers.

/// Per-face plane axes: `u` and `v` span the face plane and are chosen so
/// that `u x v` points along the outward normal, which makes the emitted
/// corner order counter-clockwise seen from outside.
pub(crate) struct FaceAxes {
    /// Axis index (0 = x, 1 = y, 2 = z) of the outward normal.
    pub n: usize,
    /// Axis index of the quad `width` direction.
    pub u: usize,
    /// Axis index of the quad `height` direction.
    pub v: usize,
    /// Whether the normal points toward positive `n`.
    pub positive: bool,
}

/// One of the six axis-aligned voxel face directions (ADR 0011 naming).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Face {
    /// `+X`, east.
    PosX,
    /// `-X`, west.
    NegX,
    /// `+Y`, north.
    PosY,
    /// `-Y`, south.
    NegY,
    /// `+Z`, up.
    PosZ,
    /// `-Z`, down.
    NegZ,
}

impl Face {
    /// All six faces, in [`QuadBuffer`] group order.
    pub const ALL: [Self; 6] = [
        Self::PosX,
        Self::NegX,
        Self::PosY,
        Self::NegY,
        Self::PosZ,
        Self::NegZ,
    ];

    /// Stable index of this face within [`Self::ALL`] and [`QuadBuffer`].
    #[must_use]
    pub const fn index(self) -> usize {
        match self {
            Self::PosX => 0,
            Self::NegX => 1,
            Self::PosY => 2,
            Self::NegY => 3,
            Self::PosZ => 4,
            Self::NegZ => 5,
        }
    }

    /// Outward normal as a per-axis step, usable for neighbor lookups.
    #[must_use]
    pub const fn normal_offset(self) -> [isize; 3] {
        match self {
            Self::PosX => [1, 0, 0],
            Self::NegX => [-1, 0, 0],
            Self::PosY => [0, 1, 0],
            Self::NegY => [0, -1, 0],
            Self::PosZ => [0, 0, 1],
            Self::NegZ => [0, 0, -1],
        }
    }

    /// Outward unit normal.
    #[must_use]
    pub const fn normal(self) -> [f32; 3] {
        match self {
            Self::PosX => [1.0, 0.0, 0.0],
            Self::NegX => [-1.0, 0.0, 0.0],
            Self::PosY => [0.0, 1.0, 0.0],
            Self::NegY => [0.0, -1.0, 0.0],
            Self::PosZ => [0.0, 0.0, 1.0],
            Self::NegZ => [0.0, 0.0, -1.0],
        }
    }

    /// Plane axes for this face; `u x v` equals the outward normal.
    pub(crate) const fn axes(self) -> FaceAxes {
        match self {
            Self::PosX => FaceAxes {
                n: 0,
                u: 1,
                v: 2,
                positive: true,
            },
            Self::NegX => FaceAxes {
                n: 0,
                u: 2,
                v: 1,
                positive: false,
            },
            Self::PosY => FaceAxes {
                n: 1,
                u: 2,
                v: 0,
                positive: true,
            },
            Self::NegY => FaceAxes {
                n: 1,
                u: 0,
                v: 2,
                positive: false,
            },
            Self::PosZ => FaceAxes {
                n: 2,
                u: 0,
                v: 1,
                positive: true,
            },
            Self::NegZ => FaceAxes {
                n: 2,
                u: 1,
                v: 0,
                positive: false,
            },
        }
    }

    /// The four corner positions of `quad`, counter-clockwise seen from
    /// outside, in the same interior-local space as [`Quad::minimum`]
    /// (meters; one voxel edge is one meter).
    #[must_use]
    pub fn quad_positions(self, quad: &Quad) -> [[f32; 3]; 4] {
        let axes = self.axes();
        let mut base = [
            coord_f32(quad.minimum[0]),
            coord_f32(quad.minimum[1]),
            coord_f32(quad.minimum[2]),
        ];
        if axes.positive {
            base[axes.n] += 1.0;
        }

        let with_offset = |du: u32, dv: u32| {
            let mut corner = base;
            corner[axes.u] += coord_f32(du);
            corner[axes.v] += coord_f32(dv);
            corner
        };

        [
            with_offset(0, 0),
            with_offset(quad.width, 0),
            with_offset(quad.width, quad.height),
            with_offset(0, quad.height),
        ]
    }

    /// Per-vertex normals matching [`Self::quad_positions`].
    #[must_use]
    pub fn quad_normals(self) -> [[f32; 3]; 4] {
        [self.normal(); 4]
    }

    /// Triangle-list indices for one quad whose corners start at `base`,
    /// counter-clockwise seen from outside.
    #[must_use]
    pub const fn quad_indices(base: u32) -> [u32; 6] {
        [base, base + 1, base + 2, base, base + 2, base + 3]
    }
}

/// An axis-aligned rectangle of coplanar, same-material voxel faces.
///
/// `minimum` is the interior-local position of the minimum-corner voxel
/// (the apron is already subtracted). `width` extends along the face's `u`
/// axis and `height` along its `v` axis — see [`Face::quad_positions`] for
/// turning a quad into vertices without knowing those axes yourself.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Quad {
    /// Interior-local coordinates of the minimum-corner voxel.
    pub minimum: [u32; 3],
    /// Extent in voxels along the face's `u` axis (at least 1).
    pub width: u32,
    /// Extent in voxels along the face's `v` axis (at least 1).
    pub height: u32,
}

impl Quad {
    /// Number of unit voxel faces this quad covers.
    #[must_use]
    pub fn area(&self) -> u64 {
        u64::from(self.width) * u64::from(self.height)
    }
}

/// Quads produced by one meshing pass, grouped by face direction.
#[derive(Clone, Debug, Default)]
pub struct QuadBuffer {
    groups: [Vec<Quad>; 6],
}

impl QuadBuffer {
    /// Quads whose outward normal is `face`.
    #[must_use]
    pub fn group(&self, face: Face) -> &[Quad] {
        &self.groups[face.index()]
    }

    /// Iterates all quads with their face direction.
    pub fn iter(&self) -> impl Iterator<Item = (Face, &Quad)> {
        Face::ALL
            .into_iter()
            .flat_map(move |face| self.group(face).iter().map(move |quad| (face, quad)))
    }

    /// Total quad count across all faces.
    #[must_use]
    pub fn num_quads(&self) -> usize {
        self.groups.iter().map(Vec::len).sum()
    }

    pub(crate) fn push(&mut self, face: Face, quad: Quad) {
        self.groups[face.index()].push(quad);
    }
}

#[expect(
    clippy::cast_precision_loss,
    reason = "coordinates are bounded by MAX_PADDED_EDGE (4096), far below f32 integer precision"
)]
fn coord_f32(value: u32) -> f32 {
    value as f32
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
        [
            a[1] * b[2] - a[2] * b[1],
            a[2] * b[0] - a[0] * b[2],
            a[0] * b[1] - a[1] * b[0],
        ]
    }

    fn sub(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
        [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
    }

    fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
        a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
    }

    #[test]
    #[allow(
        clippy::float_cmp,
        reason = "cross products of unit basis vectors are exactly 0.0 or 1.0"
    )]
    fn plane_axes_cross_to_the_outward_normal() {
        for face in Face::ALL {
            let axes = face.axes();
            let mut u = [0.0; 3];
            let mut v = [0.0; 3];
            u[axes.u] = 1.0;
            v[axes.v] = 1.0;
            assert_eq!(cross(u, v), face.normal(), "axes of {face:?}");
        }
    }

    #[test]
    fn quad_corners_wind_counter_clockwise_from_outside() {
        let quad = Quad {
            minimum: [2, 3, 4],
            width: 2,
            height: 3,
        };
        for face in Face::ALL {
            let corners = face.quad_positions(&quad);
            // Both triangles (0,1,2) and (0,2,3) must face along the normal.
            let n1 = cross(sub(corners[1], corners[0]), sub(corners[2], corners[0]));
            let n2 = cross(sub(corners[2], corners[0]), sub(corners[3], corners[0]));
            assert!(dot(n1, face.normal()) > 0.0, "triangle 1 of {face:?}");
            assert!(dot(n2, face.normal()) > 0.0, "triangle 2 of {face:?}");

            // All corners lie on the face plane.
            let axes = face.axes();
            let expected_plane = coord_f32(quad.minimum[axes.n] + u32::from(axes.positive));
            for corner in corners {
                assert!(
                    (corner[axes.n] - expected_plane).abs() < f32::EPSILON,
                    "corner off-plane for {face:?}"
                );
            }
        }
    }

    #[test]
    fn quad_indices_reference_four_corners() {
        assert_eq!(Face::quad_indices(8), [8, 9, 10, 8, 10, 11]);
    }
}
