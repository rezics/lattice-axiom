//! Renderer-independent output geometry.

use std::array;

/// Alpha policy implied by a [`MeshGroup`].
///
/// These values describe depth and coverage intent for a later material
/// adapter. This crate never creates GPU pipelines.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum MeshAlphaMode {
    /// Fully covered surfaces that write opaque depth.
    Opaque,
    /// Alpha-tested coverage that still writes depth when the test passes.
    Mask,
    /// Blended coverage that must not pretend to be a full occluder.
    Blend,
}

/// Stable render grouping for generated terrain faces.
///
/// The declaration order is part of the deterministic output contract. A
/// renderer may map these groups to its own materials or phases, but this
/// crate never creates or owns those renderer resources.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum MeshGroup {
    /// Fully opaque surfaces.
    Opaque,
    /// Alpha-tested or otherwise cutout surfaces.
    Cutout,
    /// Blended translucent surfaces.
    Translucent,
    /// Surfaces routed through the emissive terrain presentation group.
    Emissive,
}

impl MeshGroup {
    /// All mesh groups in stable output order.
    pub const ALL: [Self; 4] = [
        Self::Opaque,
        Self::Cutout,
        Self::Translucent,
        Self::Emissive,
    ];

    /// Stable index of this group within [`Self::ALL`].
    #[must_use]
    pub const fn index(self) -> usize {
        match self {
            Self::Opaque => 0,
            Self::Cutout => 1,
            Self::Translucent => 2,
            Self::Emissive => 3,
        }
    }

    /// Coverage policy for this group.
    #[must_use]
    pub const fn alpha_mode(self) -> MeshAlphaMode {
        match self {
            Self::Opaque | Self::Emissive => MeshAlphaMode::Opaque,
            Self::Cutout => MeshAlphaMode::Mask,
            Self::Translucent => MeshAlphaMode::Blend,
        }
    }

    /// Whether a later material adapter should write opaque depth.
    #[must_use]
    pub const fn writes_opaque_depth(self) -> bool {
        !matches!(self, Self::Translucent)
    }

    /// Whether a later material adapter should keep back-face culling.
    #[must_use]
    pub const fn culls_back_faces(self) -> bool {
        true
    }

    /// Whether this group is the emissive terrain pass.
    #[must_use]
    pub const fn is_emissive(self) -> bool {
        matches!(self, Self::Emissive)
    }
}

/// Merge identity for a terrain face selected from a locked layer table.
///
/// `layer_index` is the compiled table row. `face_variant` distinguishes
/// top/side/bottom or six-face slots that would emit different vertices or
/// material samples. Callers must include every value that changes those
/// outputs, including rotation when a consumer applies it.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct LayerMergeKey {
    layer_index: u16,
    face_variant: u8,
}

impl LayerMergeKey {
    /// Creates a merge key for one compiled layer row and face slot.
    #[must_use]
    pub const fn new(layer_index: u16, face_variant: u8) -> Self {
        Self {
            layer_index,
            face_variant,
        }
    }

    /// Creates a merge key whose variant follows [`Face`] output order.
    #[must_use]
    pub const fn for_face(layer_index: u16, face: Face) -> Self {
        Self::new(layer_index, face.layer_variant())
    }

    /// Compiled layer-table row.
    #[must_use]
    pub const fn layer_index(self) -> u16 {
        self.layer_index
    }

    /// Face-slot discriminant inside that row.
    #[must_use]
    pub const fn face_variant(self) -> u8 {
        self.face_variant
    }
}

/// One of the six axis-aligned voxel face directions.
///
/// Cardinal names follow ADR 0015: east is `+X`, west is `-X`, up is `+Y`,
/// down is `-Y`, south is `+Z`, and north is `-Z`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Face {
    /// East, `+X`.
    PosX,
    /// West, `-X`.
    NegX,
    /// Up, `+Y`.
    PosY,
    /// Down, `-Y`.
    NegY,
    /// South, `+Z`.
    PosZ,
    /// North (conventional forward), `-Z`.
    NegZ,
}

impl Face {
    /// All six faces in stable output order.
    pub const ALL: [Self; 6] = [
        Self::PosX,
        Self::NegX,
        Self::PosY,
        Self::NegY,
        Self::PosZ,
        Self::NegZ,
    ];

    /// Stable index of this face within [`Self::ALL`].
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

    /// Stable [`LayerMergeKey`] discriminant for this face.
    #[must_use]
    pub const fn layer_variant(self) -> u8 {
        match self {
            Self::PosX => 0,
            Self::NegX => 1,
            Self::PosY => 2,
            Self::NegY => 3,
            Self::PosZ => 4,
            Self::NegZ => 5,
        }
    }

    /// The opposite face direction.
    #[must_use]
    pub const fn opposite(self) -> Self {
        match self {
            Self::PosX => Self::NegX,
            Self::NegX => Self::PosX,
            Self::PosY => Self::NegY,
            Self::NegY => Self::PosY,
            Self::PosZ => Self::NegZ,
            Self::NegZ => Self::PosZ,
        }
    }

    /// Outward normal as an integer neighbor step.
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

    /// Outward unit normal in right-handed Y-up world space.
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

    /// Four copies of [`Self::normal`], one per quad vertex.
    #[must_use]
    pub const fn quad_normals(self) -> [[f32; 3]; 4] {
        [self.normal(); 4]
    }

    /// Triangle-list indices for a quad whose first vertex is `base`.
    ///
    /// The two triangles wind counter-clockwise when viewed from outside.
    /// Returns `None` when adding the four local vertices to `base` would
    /// overflow `u32`.
    #[must_use]
    pub fn quad_indices(base: u32) -> Option<[u32; 6]> {
        let second = base.checked_add(1)?;
        let third = base.checked_add(2)?;
        let fourth = base.checked_add(3)?;
        Some([base, second, third, base, third, fourth])
    }

    pub(crate) const fn axes(self) -> FaceAxes {
        match self {
            Self::PosX => FaceAxes::new(0, 1, 2, true),
            Self::NegX => FaceAxes::new(0, 2, 1, false),
            Self::PosY => FaceAxes::new(1, 2, 0, true),
            Self::NegY => FaceAxes::new(1, 0, 2, false),
            Self::PosZ => FaceAxes::new(2, 0, 1, true),
            Self::NegZ => FaceAxes::new(2, 1, 0, false),
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct FaceAxes {
    pub n: usize,
    pub u: usize,
    pub v: usize,
    pub positive: bool,
}

impl FaceAxes {
    const fn new(n: usize, u: usize, v: usize, positive: bool) -> Self {
        Self { n, u, v, positive }
    }
}

/// One axis-aligned rectangle of coplanar voxel faces.
///
/// `minimum` is the interior-local minimum-corner voxel. `width` extends
/// along the face's stable `u` axis and `height` along its `v` axis. The
/// merge key must contain every caller-defined value that changes emitted
/// vertex data, such as a material table entry or texture orientation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Quad<K> {
    /// Interior-local coordinates of the minimum-corner voxel.
    minimum: [u32; 3],
    /// Extent in voxels along the face's `u` axis.
    width: u32,
    /// Extent in voxels along the face's `v` axis.
    height: u32,
    /// Caller-defined identity shared by every unit face in the quad.
    merge_key: K,
}

impl<K> Quad<K> {
    pub(crate) const fn new(minimum: [u32; 3], width: u32, height: u32, merge_key: K) -> Self {
        Self {
            minimum,
            width,
            height,
            merge_key,
        }
    }

    /// Interior-local coordinates of the minimum-corner voxel.
    #[must_use]
    pub const fn minimum(&self) -> [u32; 3] {
        self.minimum
    }

    /// Extent in voxels along the face's stable plane-width axis.
    #[must_use]
    pub const fn width(&self) -> u32 {
        self.width
    }

    /// Extent in voxels along the face's stable plane-height axis.
    #[must_use]
    pub const fn height(&self) -> u32 {
        self.height
    }

    /// Caller-defined identity shared by every unit face in the quad.
    #[must_use]
    pub const fn merge_key(&self) -> &K {
        &self.merge_key
    }

    /// Number of unit voxel faces covered by this quad.
    #[must_use]
    pub fn area(&self) -> u64 {
        u64::from(self.width) * u64::from(self.height)
    }

    /// Quad positions in interior-local meters.
    ///
    /// Corners wind counter-clockwise when the face is viewed from outside.
    #[must_use]
    pub fn positions(&self, face: Face) -> [[f32; 3]; 4] {
        let axes = face.axes();
        let mut base = [
            coordinate_f32(self.minimum[0]),
            coordinate_f32(self.minimum[1]),
            coordinate_f32(self.minimum[2]),
        ];
        if axes.positive {
            base[axes.n] += 1.0;
        }

        let with_offset = |du: u32, dv: u32| {
            let mut corner = base;
            corner[axes.u] += coordinate_f32(du);
            corner[axes.v] += coordinate_f32(dv);
            corner
        };

        [
            with_offset(0, 0),
            with_offset(self.width, 0),
            with_offset(self.width, self.height),
            with_offset(0, self.height),
        ]
    }

    /// Quad texture coordinates covering `0..width` / `0..height`.
    ///
    /// Corners match [`Self::positions`] for `face`: `u` follows the face
    /// width and `v` follows the face height, so a unit face occupies `0..1`
    /// and a greedy rectangle covers that many unit tiles. A renderer maps
    /// these coordinates into atlas space.
    #[must_use]
    pub fn uvs(&self, _face: Face) -> [[f32; 2]; 4] {
        [
            [0.0, 0.0],
            [coordinate_f32(self.width), 0.0],
            [coordinate_f32(self.width), coordinate_f32(self.height)],
            [0.0, coordinate_f32(self.height)],
        ]
    }
}

/// Axis-aligned bounds in interior-local meters.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Aabb {
    /// Component-wise minimum.
    pub minimum: [f32; 3],
    /// Component-wise maximum.
    pub maximum: [f32; 3],
}

/// Reusable quad storage grouped by [`MeshGroup`] and [`Face`].
///
/// [`Self::clear`] retains all vector capacity, allowing callers and
/// benchmarks to reuse allocations across chunk jobs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MeshBuffer<K> {
    groups: [[Vec<Quad<K>>; 6]; 4],
}

impl<K> Default for MeshBuffer<K> {
    fn default() -> Self {
        Self {
            groups: array::from_fn(|_| array::from_fn(|_| Vec::new())),
        }
    }
}

impl<K> MeshBuffer<K> {
    /// Removes all quads while retaining allocated capacity.
    pub fn clear(&mut self) {
        for by_face in &mut self.groups {
            for quads in by_face {
                quads.clear();
            }
        }
    }

    /// Quads in one stable render-group and face-direction bucket.
    #[must_use]
    pub fn group(&self, group: MeshGroup, face: Face) -> &[Quad<K>] {
        &self.groups[group.index()][face.index()]
    }

    /// Iterates quads in stable group, face, and scan order.
    pub fn iter(&self) -> impl Iterator<Item = (MeshGroup, Face, &Quad<K>)> {
        MeshGroup::ALL.into_iter().flat_map(move |group| {
            Face::ALL.into_iter().flat_map(move |face| {
                self.group(group, face)
                    .iter()
                    .map(move |quad| (group, face, quad))
            })
        })
    }

    /// Total number of quads.
    #[must_use]
    pub fn quad_count(&self) -> usize {
        self.groups
            .iter()
            .flat_map(|by_face| by_face.iter())
            .map(Vec::len)
            .sum()
    }

    /// Total number of triangle-list vertices, four per quad.
    #[must_use]
    pub fn vertex_count(&self) -> usize {
        self.quad_count() * 4
    }

    /// Total number of triangle-list indices.
    #[must_use]
    pub fn index_count(&self) -> usize {
        self.quad_count() * 6
    }

    /// Total number of triangles.
    #[must_use]
    pub fn triangle_count(&self) -> usize {
        self.quad_count() * 2
    }

    /// Whether the buffer contains no geometry.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.quad_count() == 0
    }

    /// Bounds of all emitted geometry, or `None` for an empty mesh.
    #[must_use]
    pub fn bounds(&self) -> Option<Aabb> {
        let mut positions = self
            .iter()
            .flat_map(|(_, face, quad)| quad.positions(face).into_iter());
        let first = positions.next()?;
        let mut bounds = Aabb {
            minimum: first,
            maximum: first,
        };
        for position in positions {
            for (axis, coordinate) in position.into_iter().enumerate() {
                bounds.minimum[axis] = bounds.minimum[axis].min(coordinate);
                bounds.maximum[axis] = bounds.maximum[axis].max(coordinate);
            }
        }
        Some(bounds)
    }

    pub(crate) fn push(&mut self, group: MeshGroup, face: Face, quad: Quad<K>) {
        self.groups[group.index()][face.index()].push(quad);
    }
}

#[expect(
    clippy::cast_precision_loss,
    reason = "coordinates are bounded below f32's exact integer range"
)]
fn coordinate_f32(value: u32) -> f32 {
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

    fn subtract(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
        [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
    }

    fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
        a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
    }

    fn assert_vector_eq(actual: [f32; 3], expected: [f32; 3]) {
        for (actual, expected) in actual.into_iter().zip(expected) {
            assert!((actual - expected).abs() < f32::EPSILON);
        }
    }

    #[test]
    fn y_is_up_and_negative_z_is_forward() {
        assert_vector_eq(Face::PosY.normal(), [0.0, 1.0, 0.0]);
        assert_vector_eq(Face::NegY.normal(), [0.0, -1.0, 0.0]);
        assert_vector_eq(Face::NegZ.normal(), [0.0, 0.0, -1.0]);
    }

    #[test]
    fn mesh_groups_preserve_depth_alpha_and_culling_policies() {
        assert_eq!(MeshGroup::ALL.map(MeshGroup::index), [0, 1, 2, 3]);
        assert_eq!(MeshGroup::Opaque.alpha_mode(), MeshAlphaMode::Opaque);
        assert_eq!(MeshGroup::Cutout.alpha_mode(), MeshAlphaMode::Mask);
        assert_eq!(MeshGroup::Translucent.alpha_mode(), MeshAlphaMode::Blend);
        assert_eq!(MeshGroup::Emissive.alpha_mode(), MeshAlphaMode::Opaque);
        assert!(MeshGroup::Opaque.writes_opaque_depth());
        assert!(MeshGroup::Cutout.writes_opaque_depth());
        assert!(!MeshGroup::Translucent.writes_opaque_depth());
        assert!(MeshGroup::Emissive.writes_opaque_depth());
        assert!(MeshGroup::Emissive.is_emissive());
        for group in MeshGroup::ALL {
            assert!(group.culls_back_faces());
        }
    }

    #[test]
    fn layer_merge_keys_include_face_specific_slots() {
        let top = LayerMergeKey::for_face(7, Face::PosY);
        let side = LayerMergeKey::for_face(7, Face::PosX);
        assert_eq!(top.layer_index(), 7);
        assert_ne!(top, side);
        assert_eq!(top.face_variant(), Face::PosY.layer_variant());
        assert_eq!(Face::ALL.map(Face::layer_variant), [0, 1, 2, 3, 4, 5]);
    }

    #[test]
    fn all_faces_have_outward_counter_clockwise_winding_and_normals() {
        let quad = Quad::new([2, 3, 4], 2, 3, 7_u8);

        for face in Face::ALL {
            let corners = quad.positions(face);
            let first = cross(
                subtract(corners[1], corners[0]),
                subtract(corners[2], corners[0]),
            );
            let second = cross(
                subtract(corners[2], corners[0]),
                subtract(corners[3], corners[0]),
            );
            assert!(dot(first, face.normal()) > 0.0, "first triangle: {face:?}");
            assert!(
                dot(second, face.normal()) > 0.0,
                "second triangle: {face:?}"
            );
            let indices = [0_u32, 1, 2, 0, 2, 3];
            assert_eq!(Face::quad_indices(0), Some(indices));
            for triangle in indices.chunks_exact(3) {
                let a = corners[triangle[0] as usize];
                let b = corners[triangle[1] as usize];
                let c = corners[triangle[2] as usize];
                let winding = cross(subtract(b, a), subtract(c, a));
                assert!(
                    dot(winding, face.normal()) > 0.0,
                    "indexed triangle {triangle:?} for {face:?}"
                );
            }
            for normal in face.quad_normals() {
                assert_vector_eq(normal, face.normal());
            }
            let uvs = quad.uvs(face);
            assert_eq!(uvs.len(), corners.len());
            assert_eq!(uvs, [[0.0, 0.0], [2.0, 0.0], [2.0, 3.0], [0.0, 3.0]]);
        }
    }

    #[test]
    fn quad_indices_reject_an_overflowing_base() {
        assert_eq!(Face::quad_indices(8), Some([8, 9, 10, 8, 10, 11]));
        assert_eq!(
            Face::quad_indices(u32::MAX - 3),
            Some([
                u32::MAX - 3,
                u32::MAX - 2,
                u32::MAX - 1,
                u32::MAX - 3,
                u32::MAX - 1,
                u32::MAX,
            ])
        );
        assert_eq!(Face::quad_indices(u32::MAX - 2), None);
    }

    #[test]
    fn bounds_include_all_quad_corners() {
        let mut mesh = MeshBuffer::default();
        mesh.push(
            MeshGroup::Opaque,
            Face::PosY,
            Quad::new([2, 3, 4], 5, 7, 1_u8),
        );

        assert_eq!(
            mesh.bounds(),
            Some(Aabb {
                // +Y uses u = z and v = x.
                minimum: [2.0, 4.0, 4.0],
                maximum: [9.0, 4.0, 9.0],
            })
        );
    }
}
