//! Visible-face culling and deterministic greedy rectangle merging.

use thiserror::Error;

use crate::{Face, FaceDescriptor, MeshBuffer, MeshReceipt, MeshSource, PaddedChunk, Quad, Voxel};

/// A complete immutable derived mesh and its source receipt.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChunkMesh<K> {
    receipt: MeshReceipt,
    geometry: MeshBuffer<K>,
}

impl<K> ChunkMesh<K> {
    /// Receipt identifying the exact source used to derive this mesh.
    #[must_use]
    pub const fn receipt(&self) -> MeshReceipt {
        self.receipt
    }

    /// Renderer-independent quad geometry.
    #[must_use]
    pub const fn geometry(&self) -> &MeshBuffer<K> {
        &self.geometry
    }

    /// Splits the immutable result into its receipt and reusable geometry.
    #[must_use]
    pub fn into_parts(self) -> (MeshReceipt, MeshBuffer<K>) {
        (self.receipt, self.geometry)
    }
}

/// Invalid meshing input.
#[derive(Clone, Copy, Debug, Error, PartialEq, Eq)]
pub enum MeshError {
    /// The sample slice does not contain the exact padded volume.
    #[error("voxel sample length is {actual}, expected {expected} for the padded chunk")]
    InputLength {
        /// Required sample count.
        expected: usize,
        /// Supplied sample count.
        actual: usize,
    },
}

/// Emits one unit quad for every visible voxel face.
///
/// A candidate face is culled according to the opposite face descriptor in
/// the neighboring interior/halo sample. Output ordering is stable by mesh
/// group, face, normal layer, `v`, and `u` coordinate.
///
/// # Errors
///
/// Returns [`MeshError::InputLength`] when `voxels` does not exactly match
/// [`PaddedChunk::volume_len`].
pub fn visible_faces<V: Voxel>(
    voxels: &[V],
    dimensions: PaddedChunk,
    source: MeshSource,
) -> Result<ChunkMesh<V::MergeKey>, MeshError> {
    let mut geometry = MeshBuffer::default();
    let receipt = visible_faces_into(voxels, dimensions, source, &mut geometry)?;
    Ok(ChunkMesh { receipt, geometry })
}

/// Emits visible unit faces into reusable output storage.
///
/// The output is cleared only after input validation succeeds, and clearing
/// retains its allocation capacity. This is the allocation-conscious entry
/// point for benchmarks and task workers.
///
/// # Errors
///
/// Returns [`MeshError::InputLength`] when `voxels` does not exactly match
/// [`PaddedChunk::volume_len`].
pub fn visible_faces_into<V: Voxel>(
    voxels: &[V],
    dimensions: PaddedChunk,
    source: MeshSource,
    output: &mut MeshBuffer<V::MergeKey>,
) -> Result<MeshReceipt, MeshError> {
    check_input_length(voxels.len(), dimensions)?;
    output.clear();
    let interior = dimensions.interior_size();

    for face in Face::ALL {
        let axes = face.axes();
        for layer in 0..interior[axes.n] {
            for cell_v in 0..interior[axes.v] {
                for cell_u in 0..interior[axes.u] {
                    let position = position_from_axes(axes, layer, cell_u, cell_v);
                    let padded = [position[0] + 1, position[1] + 1, position[2] + 1];
                    if let Some(descriptor) = visible_descriptor(voxels, dimensions, padded, face) {
                        output.push(
                            descriptor.group(),
                            face,
                            Quad::new(position_u32(position), 1, 1, *descriptor.merge_key()),
                        );
                    }
                }
            }
        }
    }

    Ok(MeshReceipt::new(source))
}

/// Greedily merges visible faces with equal complete descriptors.
///
/// Convenience wrapper for one-off jobs. Repeated jobs should use
/// [`GreedyMesher::mesh_into`] to retain both scratch and output allocations.
///
/// # Errors
///
/// Returns [`MeshError::InputLength`] when `voxels` does not exactly match
/// [`PaddedChunk::volume_len`].
pub fn greedy_quads<V: Voxel>(
    voxels: &[V],
    dimensions: PaddedChunk,
    source: MeshSource,
) -> Result<ChunkMesh<V::MergeKey>, MeshError> {
    GreedyMesher::<V::MergeKey>::new().mesh(voxels, dimensions, source)
}

/// Reusable greedy-meshing scratch storage.
///
/// A value may be kept per worker/task lane. It contains no world or renderer
/// state and performs no synchronization.
#[derive(Clone, Debug)]
pub struct GreedyMesher<K> {
    mask: Vec<Option<FaceDescriptor<K>>>,
}

impl<K> Default for GreedyMesher<K> {
    fn default() -> Self {
        Self { mask: Vec::new() }
    }
}

impl<K: Copy + Eq> GreedyMesher<K> {
    /// Creates empty reusable scratch storage.
    #[must_use]
    pub const fn new() -> Self {
        Self { mask: Vec::new() }
    }

    /// Derives a greedy mesh, allocating a fresh output buffer.
    ///
    /// # Errors
    ///
    /// Returns [`MeshError::InputLength`] when `voxels` does not exactly
    /// match [`PaddedChunk::volume_len`].
    pub fn mesh<V: Voxel<MergeKey = K>>(
        &mut self,
        voxels: &[V],
        dimensions: PaddedChunk,
        source: MeshSource,
    ) -> Result<ChunkMesh<K>, MeshError> {
        let mut geometry = MeshBuffer::default();
        let receipt = self.mesh_into(voxels, dimensions, source, &mut geometry)?;
        Ok(ChunkMesh { receipt, geometry })
    }

    /// Derives a greedy mesh into reusable output storage.
    ///
    /// The output is cleared only after validation succeeds. Both the layer
    /// mask and all output buckets retain capacity between calls.
    ///
    /// # Errors
    ///
    /// Returns [`MeshError::InputLength`] when `voxels` does not exactly
    /// match [`PaddedChunk::volume_len`].
    pub fn mesh_into<V: Voxel<MergeKey = K>>(
        &mut self,
        voxels: &[V],
        dimensions: PaddedChunk,
        source: MeshSource,
        output: &mut MeshBuffer<K>,
    ) -> Result<MeshReceipt, MeshError> {
        check_input_length(voxels.len(), dimensions)?;
        output.clear();
        let interior = dimensions.interior_size();

        for face in Face::ALL {
            let axes = face.axes();
            let extent_u = interior[axes.u];
            let extent_v = interior[axes.v];
            let mask_len = extent_u * extent_v;
            self.mask.clear();
            self.mask.resize(mask_len, None);

            for layer in 0..interior[axes.n] {
                fill_layer_mask(
                    voxels,
                    dimensions,
                    face,
                    layer,
                    (extent_u, extent_v),
                    &mut self.mask,
                );
                merge_layer_mask(
                    &mut self.mask,
                    (extent_u, extent_v),
                    |cell_u, cell_v, width, height, descriptor| {
                        let position = position_from_axes(axes, layer, cell_u, cell_v);
                        output.push(
                            descriptor.group(),
                            face,
                            Quad::new(
                                position_u32(position),
                                coordinate_u32(width),
                                coordinate_u32(height),
                                *descriptor.merge_key(),
                            ),
                        );
                    },
                );
            }
        }

        Ok(MeshReceipt::new(source))
    }
}

fn fill_layer_mask<V: Voxel>(
    voxels: &[V],
    dimensions: PaddedChunk,
    face: Face,
    layer: usize,
    (extent_u, extent_v): (usize, usize),
    mask: &mut [Option<FaceDescriptor<V::MergeKey>>],
) {
    let axes = face.axes();
    for cell_v in 0..extent_v {
        for cell_u in 0..extent_u {
            let position = position_from_axes(axes, layer, cell_u, cell_v);
            let padded = [position[0] + 1, position[1] + 1, position[2] + 1];
            mask[cell_u + extent_u * cell_v] = visible_descriptor(voxels, dimensions, padded, face);
        }
    }
}

fn merge_layer_mask<K: Copy + Eq>(
    mask: &mut [Option<FaceDescriptor<K>>],
    (extent_u, extent_v): (usize, usize),
    mut emit: impl FnMut(usize, usize, usize, usize, FaceDescriptor<K>),
) {
    for cell_v in 0..extent_v {
        let mut cell_u = 0;
        while cell_u < extent_u {
            let Some(descriptor) = mask[cell_u + extent_u * cell_v] else {
                cell_u += 1;
                continue;
            };

            let mut width = 1;
            while cell_u + width < extent_u
                && mask[cell_u + width + extent_u * cell_v] == Some(descriptor)
            {
                width += 1;
            }

            let mut height = 1;
            'grow: while cell_v + height < extent_v {
                for offset_u in 0..width {
                    if mask[cell_u + offset_u + extent_u * (cell_v + height)] != Some(descriptor) {
                        break 'grow;
                    }
                }
                height += 1;
            }

            for offset_v in 0..height {
                for offset_u in 0..width {
                    mask[cell_u + offset_u + extent_u * (cell_v + offset_v)] = None;
                }
            }

            emit(cell_u, cell_v, width, height, descriptor);
            cell_u += width;
        }
    }
}

fn visible_descriptor<V: Voxel>(
    voxels: &[V],
    dimensions: PaddedChunk,
    padded: [usize; 3],
    face: Face,
) -> Option<FaceDescriptor<V::MergeKey>> {
    let descriptor = voxels[dimensions.linearize_unchecked(padded)].face(face)?;
    let neighbor = step(padded, face);
    let neighbor_face = voxels[dimensions.linearize_unchecked(neighbor)].face(face.opposite());
    (!neighbor_face.is_some_and(|candidate| candidate.occludes(&descriptor))).then_some(descriptor)
}

fn step([x, y, z]: [usize; 3], face: Face) -> [usize; 3] {
    let [dx, dy, dz] = face.normal_offset();
    [
        x.wrapping_add_signed(dx),
        y.wrapping_add_signed(dy),
        z.wrapping_add_signed(dz),
    ]
}

fn position_from_axes(
    axes: crate::geometry::FaceAxes,
    normal: usize,
    u: usize,
    v: usize,
) -> [usize; 3] {
    let mut position = [0; 3];
    position[axes.n] = normal;
    position[axes.u] = u;
    position[axes.v] = v;
    position
}

fn check_input_length(len: usize, dimensions: PaddedChunk) -> Result<(), MeshError> {
    let expected = dimensions.volume_len();
    if len == expected {
        Ok(())
    } else {
        Err(MeshError::InputLength {
            expected,
            actual: len,
        })
    }
}

fn position_u32(position: [usize; 3]) -> [u32; 3] {
    [
        coordinate_u32(position[0]),
        coordinate_u32(position[1]),
        coordinate_u32(position[2]),
    ]
}

#[expect(
    clippy::cast_possible_truncation,
    reason = "PaddedChunk bounds every coordinate and extent below 4096"
)]
fn coordinate_u32(value: usize) -> u32 {
    value as u32
}

#[cfg(test)]
mod tests {
    use std::array;

    use super::*;
    use crate::{
        Aabb, ChunkCoordinate, FaceOcclusion, MeshGroup, SourceEpoch, SourceFingerprint,
        SourceRevision,
    };

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    struct TestVoxel {
        material: Option<u8>,
        group: MeshGroup,
        occlusion: FaceOcclusion,
    }

    impl TestVoxel {
        const AIR: Self = Self {
            material: None,
            group: MeshGroup::Opaque,
            occlusion: FaceOcclusion::None,
        };

        const fn opaque(material: u8) -> Self {
            Self {
                material: Some(material),
                group: MeshGroup::Opaque,
                occlusion: FaceOcclusion::Full,
            }
        }

        const fn grouped(material: u8, group: MeshGroup) -> Self {
            Self {
                material: Some(material),
                group,
                occlusion: FaceOcclusion::Matching,
            }
        }
    }

    impl Voxel for TestVoxel {
        type MergeKey = u8;

        fn face(&self, _face: Face) -> Option<FaceDescriptor<Self::MergeKey>> {
            self.material
                .map(|material| FaceDescriptor::new(self.group, material, self.occlusion))
        }
    }

    fn source() -> MeshSource {
        MeshSource::new(
            ChunkCoordinate::new(-2, 3, -5),
            SourceEpoch::new(11),
            SourceRevision::new(13),
            SourceFingerprint::new([17; 32]),
        )
    }

    fn volume(
        interior: [usize; 3],
        voxel_at: impl Fn([usize; 3]) -> TestVoxel,
    ) -> (Vec<TestVoxel>, PaddedChunk) {
        let dimensions = PaddedChunk::new(interior).expect("valid test dimensions");
        let mut voxels = vec![TestVoxel::AIR; dimensions.volume_len()];
        for z in 0..interior[2] {
            for y in 0..interior[1] {
                for x in 0..interior[0] {
                    let padded = dimensions
                        .pad_interior([x, y, z])
                        .expect("loop coordinate is in the interior");
                    let index = dimensions
                        .linearize(padded)
                        .expect("padded interior coordinate is in bounds");
                    voxels[index] = voxel_at([x, y, z]);
                }
            }
        }
        (voxels, dimensions)
    }

    #[test]
    fn all_air_emits_no_geometry() {
        let (voxels, dimensions) = volume([4, 3, 2], |_| TestVoxel::AIR);
        assert!(
            visible_faces(&voxels, dimensions, source())
                .expect("valid samples")
                .geometry()
                .is_empty()
        );
        assert!(
            greedy_quads(&voxels, dimensions, source())
                .expect("valid samples")
                .geometry()
                .is_empty()
        );
    }

    #[test]
    fn one_block_emits_six_outward_faces_and_unit_aabb() {
        let (voxels, dimensions) = volume([1, 1, 1], |_| TestVoxel::opaque(7));
        let mesh = greedy_quads(&voxels, dimensions, source()).expect("valid samples");

        assert_eq!(mesh.geometry().quad_count(), 6);
        assert_eq!(mesh.geometry().triangle_count(), 12);
        assert_eq!(mesh.geometry().index_count(), 36);
        assert_eq!(
            mesh.geometry().bounds(),
            Some(Aabb {
                minimum: [0.0, 0.0, 0.0],
                maximum: [1.0, 1.0, 1.0],
            })
        );
        for face in Face::ALL {
            assert_eq!(mesh.geometry().group(MeshGroup::Opaque, face).len(), 1);
        }
    }

    #[test]
    fn adjacent_full_faces_are_removed_before_greedy_merging() {
        let (voxels, dimensions) = volume([2, 1, 1], |position| {
            TestVoxel::opaque(u8::try_from(position[0] + 1).expect("small test coordinate"))
        });

        let culled = visible_faces(&voxels, dimensions, source()).expect("valid samples");
        assert_eq!(culled.geometry().quad_count(), 10);
        let greedy = greedy_quads(&voxels, dimensions, source()).expect("valid samples");
        // Different materials prevent the side rectangles from merging, but
        // the two shared faces remain absent.
        assert_eq!(greedy.geometry().quad_count(), 10);
    }

    #[test]
    fn equal_adjacent_blocks_merge_to_six_rectangles() {
        let (voxels, dimensions) = volume([2, 1, 1], |_| TestVoxel::opaque(3));
        let mesh = greedy_quads(&voxels, dimensions, source()).expect("valid samples");

        assert_eq!(mesh.geometry().quad_count(), 6);
        assert_eq!(
            mesh.geometry().group(MeshGroup::Opaque, Face::PosY),
            // +Y uses u = z and v = x.
            &[Quad::new([0, 0, 0], 1, 2, 3)]
        );
    }

    #[test]
    fn each_of_the_six_halo_directions_culls_only_its_boundary_face() {
        for face in Face::ALL {
            let (mut voxels, dimensions) = volume([1, 1, 1], |_| TestVoxel::opaque(1));
            let padded = step([1, 1, 1], face);
            let index = dimensions
                .linearize(padded)
                .expect("halo coordinate is in bounds");
            voxels[index] = TestVoxel::opaque(2);

            let mesh = visible_faces(&voxels, dimensions, source()).expect("valid samples");
            assert!(
                mesh.geometry().group(MeshGroup::Opaque, face).is_empty(),
                "halo failed to cull {face:?}"
            );
            assert_eq!(mesh.geometry().quad_count(), 5, "halo at {face:?}");
            for other in Face::ALL.into_iter().filter(|candidate| *candidate != face) {
                assert_eq!(
                    mesh.geometry().group(MeshGroup::Opaque, other).len(),
                    1,
                    "halo at {face:?} incorrectly culled {other:?}"
                );
            }
        }
    }

    #[test]
    fn full_and_matching_occlusion_are_asymmetric_at_an_interface() {
        let (voxels, dimensions) = volume([2, 1, 1], |position| {
            if position[0] == 0 {
                TestVoxel::grouped(1, MeshGroup::Cutout)
            } else {
                TestVoxel::opaque(2)
            }
        });
        let mesh = visible_faces(&voxels, dimensions, source()).expect("valid samples");

        // Opaque fully hides the adjacent cutout face.
        assert!(
            mesh.geometry()
                .group(MeshGroup::Cutout, Face::PosX)
                .is_empty()
        );
        // Matching cutout does not hide a different opaque descriptor.
        assert_eq!(
            mesh.geometry().group(MeshGroup::Opaque, Face::NegX).len(),
            1
        );
        assert_eq!(mesh.geometry().quad_count(), 11);
    }

    #[test]
    fn matching_occlusion_removes_same_translucent_internal_faces_only() {
        let (same, dimensions) =
            volume([2, 1, 1], |_| TestVoxel::grouped(1, MeshGroup::Translucent));
        assert_eq!(
            visible_faces(&same, dimensions, source())
                .expect("valid samples")
                .geometry()
                .quad_count(),
            10
        );

        let (different, dimensions) = volume([2, 1, 1], |position| {
            TestVoxel::grouped(
                u8::try_from(position[0] + 1).expect("small test coordinate"),
                MeshGroup::Translucent,
            )
        });
        assert_eq!(
            visible_faces(&different, dimensions, source())
                .expect("valid samples")
                .geometry()
                .quad_count(),
            12
        );
    }

    #[test]
    fn output_groups_have_a_fixed_order() {
        let positions = [
            MeshGroup::Emissive,
            MeshGroup::Opaque,
            MeshGroup::Translucent,
            MeshGroup::Cutout,
        ];
        let (voxels, dimensions) = volume([7, 1, 1], |position| {
            if position[0] % 2 == 0 {
                let group = positions[position[0] / 2];
                TestVoxel::grouped(
                    u8::try_from(position[0]).expect("small test coordinate"),
                    group,
                )
            } else {
                TestVoxel::AIR
            }
        });
        let mesh = greedy_quads(&voxels, dimensions, source()).expect("valid samples");
        let visited: Vec<MeshGroup> = mesh.geometry().iter().map(|(group, _, _)| group).collect();

        assert!(visited.windows(2).all(|pair| pair[0] <= pair[1]));
        for group in MeshGroup::ALL {
            let count = mesh
                .geometry()
                .iter()
                .filter(|(candidate, _, _)| *candidate == group)
                .count();
            assert_eq!(count, 6, "group {group:?}");
        }
    }

    #[test]
    fn construction_order_does_not_change_output_order() {
        let dimensions = PaddedChunk::new([3, 2, 2]).expect("valid test dimensions");
        let mut forward = vec![TestVoxel::AIR; dimensions.volume_len()];
        let mut reverse = forward.clone();
        let mut assignments = Vec::new();
        for z in 0..2 {
            for y in 0..2 {
                for x in 0..3 {
                    let padded = [x + 1, y + 1, z + 1];
                    let index = dimensions
                        .linearize(padded)
                        .expect("interior coordinate is in bounds");
                    let material =
                        u8::try_from((x + 2 * y + 3 * z) % 3 + 1).expect("small test material");
                    assignments.push((index, TestVoxel::opaque(material)));
                }
            }
        }
        for &(index, voxel) in &assignments {
            forward[index] = voxel;
        }
        for &(index, voxel) in assignments.iter().rev() {
            reverse[index] = voxel;
        }

        let first = greedy_quads(&forward, dimensions, source()).expect("valid samples");
        let second = greedy_quads(&reverse, dimensions, source()).expect("valid samples");
        assert_eq!(first, second);
    }

    #[test]
    fn invalid_input_does_not_clear_reusable_output() {
        let (voxels, dimensions) = volume([1, 1, 1], |_| TestVoxel::opaque(1));
        let mut output = MeshBuffer::default();
        visible_faces_into(&voxels, dimensions, source(), &mut output)
            .expect("valid initial samples");
        let original = output.clone();

        assert_eq!(
            visible_faces_into(
                &voxels[..voxels.len() - 1],
                dimensions,
                source(),
                &mut output
            ),
            Err(MeshError::InputLength {
                expected: voxels.len(),
                actual: voxels.len() - 1,
            })
        );
        assert_eq!(output, original);
    }

    #[test]
    fn receipt_requires_caller_to_reject_a_stale_result() {
        let (voxels, dimensions) = volume([1, 1, 1], |_| TestVoxel::opaque(1));
        let mesh = greedy_quads(&voxels, dimensions, source()).expect("valid samples");
        let changed = MeshSource::new(
            source().chunk(),
            source().epoch(),
            SourceRevision::new(source().revision().get() + 1),
            source().fingerprint(),
        );

        assert!(!mesh.receipt().is_current_for(changed));
    }

    /// Exhaustive small-volume property test: greedy quads must tile each
    /// visible face exactly once without leaking across material groups.
    #[test]
    fn property_greedy_quads_exactly_tile_all_two_by_two_by_two_volumes() {
        for occupancy in 0_u16..=u8::MAX.into() {
            let (voxels, dimensions) = volume([2, 2, 2], |position| {
                let bit = position[0] + 2 * (position[1] + 2 * position[2]);
                if occupancy & (1 << bit) == 0 {
                    TestVoxel::AIR
                } else {
                    TestVoxel::opaque(
                        u8::try_from((position[0] + position[1] + position[2]) % 2 + 1)
                            .expect("small test material"),
                    )
                }
            });
            assert_exact_tiling(&voxels, dimensions);
        }
    }

    /// Deterministic generated property corpus deliberately uses asymmetric
    /// dimensions so axis swaps and ordering bugs cannot cancel out.
    #[test]
    fn property_greedy_quads_exactly_tile_generated_asymmetric_volumes() {
        let dimensions = PaddedChunk::new([3, 4, 5]).expect("valid test dimensions");
        for seed in 0_u64..96 {
            let mut state = seed.wrapping_add(0x9e37_79b9_7f4a_7c15);
            let mut voxels = vec![TestVoxel::AIR; dimensions.volume_len()];
            for z in 0..5 {
                for y in 0..4 {
                    for x in 0..3 {
                        state ^= state << 13;
                        state ^= state >> 7;
                        state ^= state << 17;
                        let selector = state % 7;
                        let voxel = match selector {
                            0 | 1 => TestVoxel::AIR,
                            2 | 3 => TestVoxel::opaque(
                                u8::try_from(state % 3).expect("bounded material"),
                            ),
                            4 => TestVoxel::grouped(
                                u8::try_from(state % 2).expect("bounded material"),
                                MeshGroup::Cutout,
                            ),
                            5 => TestVoxel::grouped(
                                u8::try_from(state % 2).expect("bounded material"),
                                MeshGroup::Translucent,
                            ),
                            _ => TestVoxel::grouped(
                                u8::try_from(state % 2).expect("bounded material"),
                                MeshGroup::Emissive,
                            ),
                        };
                        let index = dimensions
                            .linearize([x + 1, y + 1, z + 1])
                            .expect("interior coordinate is in bounds");
                        voxels[index] = voxel;
                    }
                }
            }
            assert_exact_tiling(&voxels, dimensions);
        }
    }

    fn assert_exact_tiling(voxels: &[TestVoxel], dimensions: PaddedChunk) {
        let mesh = greedy_quads(voxels, dimensions, source()).expect("valid samples");
        let interior = dimensions.interior_size();
        let cell_count = interior[0] * interior[1] * interior[2];

        for face in Face::ALL {
            let axes = face.axes();
            let mut covered: Vec<Option<(MeshGroup, u8)>> = vec![None; cell_count];
            for group in MeshGroup::ALL {
                for quad in mesh.geometry().group(group, face) {
                    let quad_minimum = quad.minimum();
                    let minimum = array::from_fn(|axis| {
                        usize::try_from(quad_minimum[axis]).expect("small test coordinate")
                    });
                    for offset_v in 0..usize::try_from(quad.height()).expect("small test extent") {
                        for offset_u in 0..usize::try_from(quad.width()).expect("small test extent")
                        {
                            let mut position = minimum;
                            position[axes.u] += offset_u;
                            position[axes.v] += offset_v;
                            let index = interior_index(interior, position);
                            assert_eq!(covered[index], None, "overlap at {position:?} {face:?}");
                            covered[index] = Some((group, *quad.merge_key()));
                        }
                    }
                }
            }

            for z in 0..interior[2] {
                for y in 0..interior[1] {
                    for x in 0..interior[0] {
                        let position = [x, y, z];
                        let padded = [x + 1, y + 1, z + 1];
                        let expected = visible_descriptor(voxels, dimensions, padded, face)
                            .map(|descriptor| (descriptor.group(), *descriptor.merge_key()));
                        assert_eq!(
                            covered[interior_index(interior, position)],
                            expected,
                            "coverage mismatch at {position:?} {face:?}"
                        );
                    }
                }
            }
        }
    }

    fn interior_index(interior: [usize; 3], [x, y, z]: [usize; 3]) -> usize {
        x + interior[0] * (y + interior[1] * z)
    }
}
