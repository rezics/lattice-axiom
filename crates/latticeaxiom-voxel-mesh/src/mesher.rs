//! The two meshing passes: visible-face culling and greedy merging.

use crate::geometry::{Face, Quad, QuadBuffer};
use crate::volume::{MergeVoxel, PaddedDims, Voxel};

/// Emits one unit quad per visible voxel face.
///
/// A face is visible when its voxel is opaque and the neighbor it points at
/// is not. Neighbors are read from the padded apron, so visibility at the
/// interior boundary follows the samples the caller put there.
///
/// # Panics
///
/// Panics when `voxels.len()` does not match [`PaddedDims::volume_len`];
/// that is a programmer error in the caller's volume assembly.
#[must_use]
pub fn visible_faces<V: Voxel>(voxels: &[V], dims: PaddedDims) -> QuadBuffer {
    check_len(voxels.len(), dims);
    let interior = dims.interior_size();
    let mut buffer = QuadBuffer::default();

    for z in 0..interior[2] {
        for y in 0..interior[1] {
            for x in 0..interior[0] {
                let padded = [x + 1, y + 1, z + 1];
                if !voxels[dims.linearize(padded)].is_opaque() {
                    continue;
                }
                for face in Face::ALL {
                    if !voxels[dims.linearize(step(padded, face))].is_opaque() {
                        buffer.push(
                            face,
                            Quad {
                                minimum: interior_u32([x, y, z]),
                                width: 1,
                                height: 1,
                            },
                        );
                    }
                }
            }
        }
    }
    buffer
}

/// Merges visible faces with equal [`MergeVoxel::merge_value`] into maximal
/// rectangles, sweeping each face direction layer by layer.
///
/// Invariants (checked by this crate's property tests):
///
/// - the summed quad area per face equals the visible face count, so nothing
///   is dropped or double-covered;
/// - every quad covers faces of a single merge value;
/// - output does not depend on anything but the samples (no hidden state).
///
/// # Panics
///
/// Panics when `voxels.len()` does not match [`PaddedDims::volume_len`];
/// that is a programmer error in the caller's volume assembly.
#[must_use]
pub fn greedy_quads<V: MergeVoxel>(voxels: &[V], dims: PaddedDims) -> QuadBuffer {
    check_len(voxels.len(), dims);
    let interior = dims.interior_size();
    let mut buffer = QuadBuffer::default();

    for face in Face::ALL {
        let axes = face.axes();
        let (extent_n, extent_u, extent_v) = (interior[axes.n], interior[axes.u], interior[axes.v]);
        // Mask of one layer perpendicular to the normal: `Some(value)` for a
        // still-unmerged visible face, `None` otherwise.
        let mut mask: Vec<Option<V::MergeValue>> = vec![None; extent_u * extent_v];

        for layer in 0..extent_n {
            fill_layer_mask(voxels, dims, face, layer, &mut mask);
            merge_layer_mask(&mut mask, (extent_u, extent_v), |cell_u, cell_v, w, h| {
                let mut minimum = [0usize; 3];
                minimum[axes.n] = layer;
                minimum[axes.u] = cell_u;
                minimum[axes.v] = cell_v;
                buffer.push(
                    face,
                    Quad {
                        minimum: interior_u32(minimum),
                        width: coord_u32(w),
                        height: coord_u32(h),
                    },
                );
            });
        }
    }
    buffer
}

/// Fills `mask` with the visible faces of one layer, in `u + extent_u * v`
/// order.
fn fill_layer_mask<V: MergeVoxel>(
    voxels: &[V],
    dims: PaddedDims,
    face: Face,
    layer: usize,
    mask: &mut [Option<V::MergeValue>],
) {
    let axes = face.axes();
    let interior = dims.interior_size();
    let (extent_u, extent_v) = (interior[axes.u], interior[axes.v]);

    for cell_v in 0..extent_v {
        for cell_u in 0..extent_u {
            let mut ipos = [0usize; 3];
            ipos[axes.n] = layer;
            ipos[axes.u] = cell_u;
            ipos[axes.v] = cell_v;
            let padded = [ipos[0] + 1, ipos[1] + 1, ipos[2] + 1];

            let voxel = &voxels[dims.linearize(padded)];
            let visible =
                voxel.is_opaque() && !voxels[dims.linearize(step(padded, face))].is_opaque();
            mask[cell_u + extent_u * cell_v] = visible.then(|| voxel.merge_value());
        }
    }
}

/// Consumes a layer mask into maximal rectangles: grow width first, then
/// height while every row cell still matches, then clear the covered cells.
fn merge_layer_mask<M: Copy + Eq>(
    mask: &mut [Option<M>],
    (extent_u, extent_v): (usize, usize),
    mut emit: impl FnMut(usize, usize, usize, usize),
) {
    for cell_v in 0..extent_v {
        let mut cell_u = 0;
        while cell_u < extent_u {
            let Some(value) = mask[cell_u + extent_u * cell_v] else {
                cell_u += 1;
                continue;
            };

            let mut width = 1;
            while cell_u + width < extent_u
                && mask[cell_u + width + extent_u * cell_v] == Some(value)
            {
                width += 1;
            }

            let mut height = 1;
            'grow: while cell_v + height < extent_v {
                for du in 0..width {
                    if mask[cell_u + du + extent_u * (cell_v + height)] != Some(value) {
                        break 'grow;
                    }
                }
                height += 1;
            }

            for dv in 0..height {
                for du in 0..width {
                    mask[cell_u + du + extent_u * (cell_v + dv)] = None;
                }
            }

            emit(cell_u, cell_v, width, height);
            cell_u += width;
        }
    }
}

/// Neighbor position one step along the face normal (stays inside the
/// padded volume because interior positions are at least 1 from the border).
fn step([x, y, z]: [usize; 3], face: Face) -> [usize; 3] {
    let [dx, dy, dz] = face.normal_offset();
    [
        x.wrapping_add_signed(dx),
        y.wrapping_add_signed(dy),
        z.wrapping_add_signed(dz),
    ]
}

fn check_len(len: usize, dims: PaddedDims) {
    assert_eq!(
        len,
        dims.volume_len(),
        "voxel slice length must match PaddedDims::volume_len()"
    );
}

fn interior_u32(position: [usize; 3]) -> [u32; 3] {
    [
        coord_u32(position[0]),
        coord_u32(position[1]),
        coord_u32(position[2]),
    ]
}

#[expect(
    clippy::cast_possible_truncation,
    reason = "PaddedDims bounds every coordinate and extent below MAX_PADDED_EDGE (4096)"
)]
fn coord_u32(value: usize) -> u32 {
    value as u32
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    /// Test voxel: `None` is empty, `Some(id)` is an opaque palette entry.
    #[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
    struct Vox(Option<u8>);

    impl Voxel for Vox {
        fn is_opaque(&self) -> bool {
            self.0.is_some()
        }
    }

    impl MergeVoxel for Vox {
        type MergeValue = u8;

        fn merge_value(&self) -> u8 {
            self.0.unwrap_or(0)
        }
    }

    /// Builds a padded volume from a function over interior coordinates;
    /// the apron stays empty.
    fn volume(
        interior: [usize; 3],
        voxel_at: impl Fn([usize; 3]) -> Option<u8>,
    ) -> (Vec<Vox>, PaddedDims) {
        let dims = PaddedDims::new([interior[0] + 2, interior[1] + 2, interior[2] + 2]);
        let mut voxels = vec![Vox(None); dims.volume_len()];
        for z in 0..interior[2] {
            for y in 0..interior[1] {
                for x in 0..interior[0] {
                    voxels[dims.linearize([x + 1, y + 1, z + 1])] = Vox(voxel_at([x, y, z]));
                }
            }
        }
        (voxels, dims)
    }

    /// Brute-force count of visible faces per face direction.
    fn exposed_faces(voxels: &[Vox], dims: PaddedDims) -> [usize; 6] {
        let interior = dims.interior_size();
        let mut counts = [0usize; 6];
        for z in 0..interior[2] {
            for y in 0..interior[1] {
                for x in 0..interior[0] {
                    let padded = [x + 1, y + 1, z + 1];
                    if !voxels[dims.linearize(padded)].is_opaque() {
                        continue;
                    }
                    for face in Face::ALL {
                        if !voxels[dims.linearize(step(padded, face))].is_opaque() {
                            counts[face.index()] += 1;
                        }
                    }
                }
            }
        }
        counts
    }

    #[test]
    fn empty_volume_emits_nothing() {
        let (voxels, dims) = volume([4, 4, 4], |_| None);
        assert_eq!(visible_faces(&voxels, dims).num_quads(), 0);
        assert_eq!(greedy_quads(&voxels, dims).num_quads(), 0);
    }

    #[test]
    fn single_voxel_emits_six_faces() {
        let (voxels, dims) = volume([1, 1, 1], |_| Some(7));
        assert_eq!(visible_faces(&voxels, dims).num_quads(), 6);
        assert_eq!(greedy_quads(&voxels, dims).num_quads(), 6);
    }

    #[test]
    fn quad_coordinates_are_interior_local() {
        // A voxel away from the origin pins the "apron already subtracted"
        // output contract.
        let (voxels, dims) = volume([4, 5, 6], |p| (p == [2, 3, 4]).then_some(1));
        let quads = greedy_quads(&voxels, dims);
        assert_eq!(quads.num_quads(), 6);
        for (_, quad) in quads.iter() {
            assert_eq!(quad.minimum, [2, 3, 4]);
        }
    }

    #[test]
    fn adjacent_equal_voxels_merge() {
        let (voxels, dims) = volume([2, 1, 1], |_| Some(7));
        // Two shared faces are hidden: 12 - 2 = 10 unit faces.
        assert_eq!(visible_faces(&voxels, dims).num_quads(), 10);

        // Greedy merges each side pair into one quad: 6 total.
        let quads = greedy_quads(&voxels, dims);
        assert_eq!(quads.num_quads(), 6);
        let up = quads.group(Face::PosZ);
        assert_eq!(
            up,
            &[Quad {
                minimum: [0, 0, 0],
                // +Z axes are u = x, v = y: the pair extends along width.
                width: 2,
                height: 1,
            }]
        );
    }

    #[test]
    fn adjacent_different_voxels_do_not_merge() {
        let (voxels, dims) = volume([2, 1, 1], |p| Some(u8::try_from(p[0]).unwrap()));
        assert_eq!(greedy_quads(&voxels, dims).num_quads(), 10);
    }

    #[test]
    fn solid_cube_merges_to_one_quad_per_face() {
        let (voxels, dims) = volume([3, 3, 3], |_| Some(1));
        assert_eq!(visible_faces(&voxels, dims).num_quads(), 6 * 9);

        let quads = greedy_quads(&voxels, dims);
        assert_eq!(quads.num_quads(), 6);
        for (_, quad) in quads.iter() {
            assert_eq!((quad.width, quad.height), (3, 3));
        }
    }

    #[test]
    fn checkerboard_cannot_merge() {
        let (voxels, dims) = volume([4, 4, 4], |p| ((p[0] + p[1] + p[2]) % 2 == 0).then_some(1));
        let culled = visible_faces(&voxels, dims);
        let greedy = greedy_quads(&voxels, dims);
        assert_eq!(greedy.num_quads(), culled.num_quads());
    }

    /// Strategy: a random palette volume with interior 3x4x5 — deliberately
    /// asymmetric so axis mix-ups cannot cancel out.
    fn arb_volume() -> impl Strategy<Value = (Vec<Vox>, PaddedDims)> {
        proptest::collection::vec(proptest::option::weighted(0.5, 0u8..3), 3 * 4 * 5)
            .prop_map(|cells| volume([3, 4, 5], |p| cells[p[0] + 3 * (p[1] + 4 * p[2])]))
    }

    proptest! {
        /// Culling emits exactly one unit quad per exposed face.
        #[test]
        fn culled_faces_match_brute_force((voxels, dims) in arb_volume()) {
            let expected = exposed_faces(&voxels, dims);
            let culled = visible_faces(&voxels, dims);
            for face in Face::ALL {
                prop_assert_eq!(culled.group(face).len(), expected[face.index()]);
                for quad in culled.group(face) {
                    prop_assert_eq!((quad.width, quad.height), (1, 1));
                }
            }
        }

        /// Greedy quads tile the visible faces exactly: same total area, no
        /// overlap, no leakage onto hidden or empty cells, uniform values.
        #[test]
        fn greedy_quads_tile_visible_faces_exactly((voxels, dims) in arb_volume()) {
            let interior = dims.interior_size();
            let greedy = greedy_quads(&voxels, dims);

            for face in Face::ALL {
                let axes = face.axes();

                // Paint every covered cell and check uniform merge values.
                let mut covered = vec![0u32; interior[0] * interior[1] * interior[2]];
                let paint_index = |p: [usize; 3]| p[0] + interior[0] * (p[1] + interior[1] * p[2]);

                for quad in greedy.group(face) {
                    let minimum = [
                        quad.minimum[0] as usize,
                        quad.minimum[1] as usize,
                        quad.minimum[2] as usize,
                    ];
                    let value = voxels[dims.linearize([
                        minimum[0] + 1,
                        minimum[1] + 1,
                        minimum[2] + 1,
                    ])];
                    for du in 0..quad.width as usize {
                        for dv in 0..quad.height as usize {
                            let mut p = minimum;
                            p[axes.u] += du;
                            p[axes.v] += dv;
                            covered[paint_index(p)] += 1;
                            let cell = voxels[dims.linearize([p[0] + 1, p[1] + 1, p[2] + 1])];
                            prop_assert_eq!(cell, value, "quad covers mixed merge values");
                        }
                    }
                }

                // Every visible face cell is covered exactly once; everything
                // else is untouched.
                for z in 0..interior[2] {
                    for y in 0..interior[1] {
                        for x in 0..interior[0] {
                            let padded = [x + 1, y + 1, z + 1];
                            let visible = voxels[dims.linearize(padded)].is_opaque()
                                && !voxels[dims.linearize(step(padded, face))].is_opaque();
                            prop_assert_eq!(
                                covered[paint_index([x, y, z])],
                                u32::from(visible),
                                "coverage mismatch at ({}, {}, {}) for {:?}",
                                x, y, z, face
                            );
                        }
                    }
                }
            }
        }
    }
}
