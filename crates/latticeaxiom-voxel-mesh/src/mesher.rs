//! Visible-face culling and deterministic greedy rectangle merging.

use thiserror::Error;

use crate::{
    Face, FaceDescriptor, FaceOcclusion, FluidMeshFlow, FluidMeshIdentity, FluidSurfaceDescriptor,
    MeshBuffer, MeshGroup, MeshReceipt, MeshSource, PaddedChunk, Quad, Voxel,
};

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
    emit_fluid_surfaces(voxels, dimensions, output);

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
        emit_fluid_surfaces(voxels, dimensions, output);

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
    let voxel = &voxels[dimensions.linearize_unchecked(padded)];
    if voxel.fluid_surface(face).is_some() {
        return None;
    }
    let descriptor = voxel.face(face)?;
    let neighbor = step(padded, face);
    let neighbor_face = voxels[dimensions.linearize_unchecked(neighbor)].face(face.opposite());
    (!neighbor_face.is_some_and(|candidate| candidate.occludes(&descriptor))).then_some(descriptor)
}

fn emit_fluid_surfaces<V: Voxel>(
    voxels: &[V],
    dimensions: PaddedChunk,
    output: &mut MeshBuffer<V::MergeKey>,
) {
    let interior = dimensions.interior_size();
    let mut has_fluid = false;
    'scan: for z in 0..interior[2] {
        for y in 0..interior[1] {
            for x in 0..interior[0] {
                if fluid_at(voxels, dimensions, [x + 1, y + 1, z + 1], Face::PosY).is_some() {
                    has_fluid = true;
                    break 'scan;
                }
            }
        }
    }
    if !has_fluid {
        return;
    }
    for face in Face::ALL {
        if matches!(face, Face::NegY) {
            continue;
        }
        let axes = face.axes();
        for layer in 0..interior[axes.n] {
            for cell_v in 0..interior[axes.v] {
                for cell_u in 0..interior[axes.u] {
                    let position = position_from_axes(axes, layer, cell_u, cell_v);
                    let padded = [position[0] + 1, position[1] + 1, position[2] + 1];
                    let Some(surface) = fluid_at(voxels, dimensions, padded, face) else {
                        continue;
                    };
                    let heights = if matches!(face, Face::PosY) {
                        if !fluid_top_is_visible(voxels, dimensions, padded, surface.identity()) {
                            continue;
                        }
                        fluid_top_heights(voxels, dimensions, padded, surface)
                    } else {
                        let Some(heights) =
                            fluid_side_heights(voxels, dimensions, padded, face, surface)
                        else {
                            continue;
                        };
                        heights
                    };
                    output.push(
                        MeshGroup::Water,
                        face,
                        Quad::fluid(
                            position_u32(position),
                            *surface.merge_key(),
                            heights,
                            surface.identity(),
                            surface.level(),
                            surface.flow(),
                        ),
                    );
                }
            }
        }
    }
}

fn fluid_at<V: Voxel>(
    voxels: &[V],
    dimensions: PaddedChunk,
    padded: [usize; 3],
    face: Face,
) -> Option<FluidSurfaceDescriptor<V::MergeKey>> {
    let index = dimensions.linearize(padded)?;
    voxels.get(index)?.fluid_surface(face)
}

fn fluid_top_is_visible<V: Voxel>(
    voxels: &[V],
    dimensions: PaddedChunk,
    padded: [usize; 3],
    identity: FluidMeshIdentity,
) -> bool {
    let above = step(padded, Face::PosY);
    if fluid_at(voxels, dimensions, above, Face::PosY)
        .is_some_and(|candidate| candidate.identity() == identity)
    {
        return false;
    }
    dimensions
        .linearize(above)
        .and_then(|index| voxels.get(index))
        .and_then(|voxel| voxel.face(Face::NegY))
        .is_none_or(|face| !matches!(face.occlusion(), FaceOcclusion::Full))
}

fn fluid_top_heights<V: Voxel>(
    voxels: &[V],
    dimensions: PaddedChunk,
    padded: [usize; 3],
    surface: FluidSurfaceDescriptor<V::MergeKey>,
) -> [u8; 4] {
    let own_height = surface.level().height_eighths();
    if matches!(surface.flow(), FluidMeshFlow::Down) {
        return [own_height; 4];
    }
    [
        smoothed_corner_height(
            voxels,
            dimensions,
            padded,
            surface.identity(),
            [(-1, -1), (-1, 0), (0, -1), (0, 0)],
            own_height,
        ),
        smoothed_corner_height(
            voxels,
            dimensions,
            padded,
            surface.identity(),
            [(-1, 0), (-1, 1), (0, 0), (0, 1)],
            own_height,
        ),
        smoothed_corner_height(
            voxels,
            dimensions,
            padded,
            surface.identity(),
            [(0, 0), (0, 1), (1, 0), (1, 1)],
            own_height,
        ),
        smoothed_corner_height(
            voxels,
            dimensions,
            padded,
            surface.identity(),
            [(0, -1), (0, 0), (1, -1), (1, 0)],
            own_height,
        ),
    ]
}

fn smoothed_corner_height<V: Voxel>(
    voxels: &[V],
    dimensions: PaddedChunk,
    padded: [usize; 3],
    identity: FluidMeshIdentity,
    offsets: [(isize, isize); 4],
    fallback: u8,
) -> u8 {
    let mut sum = 0_u16;
    let mut count = 0_u16;
    for (offset_x, offset_z) in offsets {
        let Some(x) = padded[0].checked_add_signed(offset_x) else {
            continue;
        };
        let Some(z) = padded[2].checked_add_signed(offset_z) else {
            continue;
        };
        let sample_position = [x, padded[1], z];
        let Some(sample) = fluid_at(voxels, dimensions, sample_position, Face::PosY) else {
            continue;
        };
        if sample.identity() != identity {
            continue;
        }
        if matches!(sample.flow(), FluidMeshFlow::Down) {
            continue;
        }
        let height = sample.level().height_eighths();
        let stacked = fluid_at(
            voxels,
            dimensions,
            step(sample_position, Face::PosY),
            Face::PosY,
        )
        .is_some_and(|above| above.identity() == identity);
        if height == 8 || stacked {
            return 8;
        }
        sum = sum.saturating_add(u16::from(height));
        count = count.saturating_add(1);
    }
    let rounded_sum = sum.saturating_add(count.saturating_sub(1));
    match rounded_sum
        .checked_div(count)
        .and_then(|height| u8::try_from(height).ok())
    {
        Some(height) => height,
        None => fallback,
    }
}

fn fluid_side_heights<V: Voxel>(
    voxels: &[V],
    dimensions: PaddedChunk,
    padded: [usize; 3],
    face: Face,
    surface: FluidSurfaceDescriptor<V::MergeKey>,
) -> Option<[u8; 4]> {
    let top = fluid_top_heights(voxels, dimensions, padded, surface);
    let upper = top_edge_heights(top, face)?;
    let neighbor = fluid_at(voxels, dimensions, step(padded, face), face.opposite());
    let lower = match neighbor {
        Some(neighbor) if neighbor.identity() == surface.identity() => {
            let current_height = surface.level().height_eighths();
            let neighbor_height = neighbor.level().height_eighths();
            if !matches!(surface.flow(), FluidMeshFlow::Down) || current_height <= neighbor_height {
                return None;
            }
            [neighbor_height; 2]
        }
        _ => [0; 2],
    };
    Some(side_vertex_heights(face, lower, upper))
}

const fn top_edge_heights(top: [u8; 4], face: Face) -> Option<[u8; 2]> {
    match face {
        Face::PosX => Some([top[3], top[2]]),
        Face::NegX => Some([top[0], top[1]]),
        Face::PosZ => Some([top[1], top[2]]),
        Face::NegZ => Some([top[0], top[3]]),
        Face::PosY | Face::NegY => None,
    }
}

const fn side_vertex_heights(face: Face, lower: [u8; 2], upper: [u8; 2]) -> [u8; 4] {
    match face {
        Face::PosX | Face::NegZ => [lower[0], upper[0], upper[1], lower[1]],
        Face::NegX | Face::PosZ => [lower[0], lower[1], upper[1], upper[0]],
        Face::PosY | Face::NegY => [0; 4],
    }
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
        Aabb, ChunkCoordinate, FaceOcclusion, FluidMeshLevel, LayerMergeKey, MeshGroup,
        SourceEpoch, SourceFingerprint, SourceRevision,
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

    #[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
    struct FluidVoxel {
        level: Option<FluidMeshLevel>,
        flow: FluidMeshFlow,
    }

    impl FluidVoxel {
        fn water(level: u8, flow: FluidMeshFlow) -> Self {
            Self {
                level: Some(FluidMeshLevel::new(level).expect("fixture fluid level is valid")),
                flow,
            }
        }
    }

    impl Voxel for FluidVoxel {
        type MergeKey = u8;

        fn face(&self, _face: Face) -> Option<FaceDescriptor<Self::MergeKey>> {
            None
        }

        fn fluid_surface(&self, face: Face) -> Option<FluidSurfaceDescriptor<Self::MergeKey>> {
            self.level.map(|level| {
                FluidSurfaceDescriptor::new(
                    FluidMeshIdentity::new(41),
                    level,
                    self.flow,
                    u8::try_from(face.index()).expect("six face indices fit u8"),
                )
            })
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

    fn fluid_volume(
        interior: [usize; 3],
        voxel_at: impl Fn([usize; 3]) -> FluidVoxel,
    ) -> (Vec<FluidVoxel>, PaddedChunk) {
        let dimensions = PaddedChunk::new(interior).expect("valid test dimensions");
        let mut voxels = vec![FluidVoxel::default(); dimensions.volume_len()];
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

    fn fluid_world_volume(
        origin_x: i32,
        voxel_at: impl Fn([i32; 3]) -> FluidVoxel,
    ) -> (Vec<FluidVoxel>, PaddedChunk) {
        let dimensions = PaddedChunk::new([1, 1, 1]).expect("valid test dimensions");
        let padded_size = dimensions.padded_size();
        let mut voxels = vec![FluidVoxel::default(); dimensions.volume_len()];
        for z in 0..padded_size[2] {
            for y in 0..padded_size[1] {
                for x in 0..padded_size[0] {
                    let index = dimensions
                        .linearize([x, y, z])
                        .expect("padded loop coordinate is in bounds");
                    let local = [x, y, z].map(|value| {
                        i32::try_from(value).expect("fixture coordinate fits i32") - 1
                    });
                    voxels[index] = voxel_at([origin_x + local[0], local[1], local[2]]);
                }
            }
        }
        (voxels, dimensions)
    }

    fn assert_height_eq(actual: f32, expected: f32) {
        assert!((actual - expected).abs() < f32::EPSILON);
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
        assert_eq!(mesh.geometry().vertex_count(), 24);
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
            assert_halo_culls_only_boundary_face(mesh.geometry(), face);
            let greedy = greedy_quads(&voxels, dimensions, source()).expect("valid samples");
            assert_halo_culls_only_boundary_face(greedy.geometry(), face);
        }
    }

    fn assert_halo_culls_only_boundary_face(geometry: &MeshBuffer<u8>, face: Face) {
        assert!(
            geometry.group(MeshGroup::Opaque, face).is_empty(),
            "halo failed to cull {face:?}"
        );
        assert_eq!(geometry.quad_count(), 5, "halo at {face:?}");
        for other in Face::ALL.into_iter().filter(|candidate| *candidate != face) {
            assert_eq!(
                geometry.group(MeshGroup::Opaque, other).len(),
                1,
                "halo at {face:?} incorrectly culled {other:?}"
            );
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
    fn layer_merge_keys_keep_distinct_face_slots_from_greedy_merging() {
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        struct LayerVoxel {
            key: Option<LayerMergeKey>,
            group: MeshGroup,
        }

        impl Voxel for LayerVoxel {
            type MergeKey = LayerMergeKey;

            fn face(&self, _face: Face) -> Option<FaceDescriptor<Self::MergeKey>> {
                self.key
                    .map(|key| FaceDescriptor::new(self.group, key, FaceOcclusion::Full))
            }
        }

        let dimensions = PaddedChunk::new([2, 1, 1]).expect("valid test dimensions");
        let mut voxels = vec![
            LayerVoxel {
                key: None,
                group: MeshGroup::Opaque,
            };
            dimensions.volume_len()
        ];
        let left = dimensions
            .linearize([1, 1, 1])
            .expect("interior coordinate is in bounds");
        let right = dimensions
            .linearize([2, 1, 1])
            .expect("interior coordinate is in bounds");
        voxels[left] = LayerVoxel {
            key: Some(LayerMergeKey::new(1, 0)),
            group: MeshGroup::Cutout,
        };
        voxels[right] = LayerVoxel {
            key: Some(LayerMergeKey::new(1, 1)),
            group: MeshGroup::Cutout,
        };

        let mesh = greedy_quads(&voxels, dimensions, source()).expect("valid samples");
        let tops = mesh.geometry().group(MeshGroup::Cutout, Face::PosY);
        assert_eq!(tops.len(), 2);
        assert_ne!(tops[0].merge_key(), tops[1].merge_key());
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
            MeshGroup::Water,
            MeshGroup::Translucent,
            MeshGroup::Cutout,
        ];
        let (voxels, dimensions) = volume([9, 1, 1], |position| {
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

    #[test]
    fn fluid_levels_emit_exact_top_heights_and_preserve_state() {
        for (level, expected_height) in [(0, 1.0), (7, 0.125)] {
            let (voxels, dimensions) =
                fluid_volume([1, 1, 1], |_| FluidVoxel::water(level, FluidMeshFlow::East));
            let visible = visible_faces(&voxels, dimensions, source()).expect("valid samples");
            let greedy = greedy_quads(&voxels, dimensions, source()).expect("valid samples");
            assert_eq!(visible.geometry(), greedy.geometry());

            let tops = greedy.geometry().group(MeshGroup::Water, Face::PosY);
            assert_eq!(tops.len(), 1);
            let top = &tops[0];
            assert!(
                top.positions(Face::PosY)
                    .into_iter()
                    .all(|position| (position[1] - expected_height).abs() < f32::EPSILON)
            );
            assert_eq!(top.fluid_identity(), Some(FluidMeshIdentity::new(41)));
            assert_eq!(top.fluid_level().map(FluidMeshLevel::get), Some(level));
            assert_eq!(top.fluid_flow(), Some(FluidMeshFlow::East));
            assert_eq!(top.normals(Face::PosY), [[0.0, 1.0, 0.0]; 4]);
        }
    }

    #[test]
    fn adjacent_and_stacked_water_cull_interior_faces() {
        let (horizontal, horizontal_dimensions) =
            fluid_volume([2, 1, 1], |_| FluidVoxel::water(0, FluidMeshFlow::Still));
        let horizontal_mesh = greedy_quads(&horizontal, horizontal_dimensions, source())
            .expect("valid horizontal samples");
        let geometry = horizontal_mesh.geometry();
        assert_eq!(geometry.group(MeshGroup::Water, Face::PosY).len(), 2);
        assert_eq!(geometry.group(MeshGroup::Water, Face::PosX).len(), 1);
        assert_eq!(geometry.group(MeshGroup::Water, Face::NegX).len(), 1);
        assert_eq!(geometry.quad_count(), 8);

        let (vertical, vertical_dimensions) =
            fluid_volume([1, 2, 1], |_| FluidVoxel::water(0, FluidMeshFlow::Still));
        let vertical_mesh =
            greedy_quads(&vertical, vertical_dimensions, source()).expect("valid vertical samples");
        let tops = vertical_mesh.geometry().group(MeshGroup::Water, Face::PosY);
        assert_eq!(tops.len(), 1);
        assert_eq!(tops[0].minimum()[1], 1);
    }

    #[test]
    fn shared_halo_produces_identical_water_seam_vertices_and_normals() {
        let world = |position: [i32; 3]| match position {
            [0, 0, 0] => FluidVoxel::water(2, FluidMeshFlow::Still),
            [1, 0, 0] => FluidVoxel::water(4, FluidMeshFlow::Still),
            _ => FluidVoxel::default(),
        };
        let (left, dimensions) = fluid_world_volume(0, world);
        let (right, right_dimensions) = fluid_world_volume(1, world);
        assert_eq!(dimensions, right_dimensions);
        let left_mesh = greedy_quads(&left, dimensions, source()).expect("valid left samples");
        let right_mesh = greedy_quads(&right, dimensions, source()).expect("valid right samples");
        let left_top = &left_mesh.geometry().group(MeshGroup::Water, Face::PosY)[0];
        let right_top = &right_mesh.geometry().group(MeshGroup::Water, Face::PosY)[0];
        let left_positions = left_top.positions(Face::PosY);
        let right_positions = right_top.positions(Face::PosY);

        assert_height_eq(left_positions[3][1], right_positions[0][1]);
        assert_height_eq(left_positions[2][1], right_positions[1][1]);
        assert_eq!(left_top.normals(Face::PosY), right_top.normals(Face::PosY));
    }

    #[test]
    fn downward_flow_emits_an_explicit_sheet_to_lower_water() {
        let (voxels, dimensions) = fluid_volume([2, 1, 1], |position| {
            if position[0] == 0 {
                FluidVoxel::water(0, FluidMeshFlow::Down)
            } else {
                FluidVoxel::water(4, FluidMeshFlow::Still)
            }
        });
        let mesh = greedy_quads(&voxels, dimensions, source()).expect("valid samples");
        let positive_x = mesh.geometry().group(MeshGroup::Water, Face::PosX);
        let sheet = positive_x
            .iter()
            .find(|quad| quad.minimum()[0] == 0)
            .expect("downward cell emits its internal waterfall sheet");
        assert_eq!(sheet.fluid_flow(), Some(FluidMeshFlow::Down));
        assert_eq!(sheet.fluid_vertex_heights_eighths(), Some([4, 8, 8, 4]));
    }

    #[test]
    fn still_level_transitions_use_a_continuous_smoothed_top() {
        let (voxels, dimensions) = fluid_volume([2, 1, 1], |position| {
            FluidVoxel::water(if position[0] == 0 { 2 } else { 4 }, FluidMeshFlow::Still)
        });
        let mesh = greedy_quads(&voxels, dimensions, source()).expect("valid samples");
        assert_eq!(
            mesh.geometry().group(MeshGroup::Water, Face::PosX).len(),
            1,
            "only the outer +X shoreline emits a side"
        );
        let tops = mesh.geometry().group(MeshGroup::Water, Face::PosY);
        let left = tops
            .iter()
            .find(|quad| quad.minimum()[0] == 0)
            .expect("left water top");
        let right = tops
            .iter()
            .find(|quad| quad.minimum()[0] == 1)
            .expect("right water top");
        assert_height_eq(
            left.positions(Face::PosY)[3][1],
            right.positions(Face::PosY)[0][1],
        );
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
