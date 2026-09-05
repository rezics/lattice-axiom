//! Conformance evidence for the upstream `block-mesh` adoption decision.

#![allow(
    clippy::expect_used,
    reason = "fixed audit fixtures state their construction invariants"
)]

use block_mesh::ndshape::{ConstShape, ConstShape3u32};
use latticeaxiom_voxel_mesh::{
    ChunkCoordinate, Face, FaceDescriptor, FaceOcclusion, MeshGroup, MeshSource, PaddedChunk,
    SourceEpoch, SourceFingerprint, SourceRevision, Voxel, greedy_quads, visible_faces,
};

const INTERIOR_EDGE: usize = 4;
type AuditShape = ConstShape3u32<6, 6, 6>;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct AuditVoxel {
    material: Option<u8>,
    group: MeshGroup,
    occlusion: FaceOcclusion,
}

impl AuditVoxel {
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

    const fn matching(material: u8, group: MeshGroup) -> Self {
        Self {
            material: Some(material),
            group,
            occlusion: FaceOcclusion::Matching,
        }
    }
}

impl Voxel for AuditVoxel {
    type MergeKey = u8;

    fn face(&self, _face: Face) -> Option<FaceDescriptor<Self::MergeKey>> {
        self.material
            .map(|material| FaceDescriptor::new(self.group, material, self.occlusion))
    }
}

impl block_mesh::Voxel for AuditVoxel {
    fn get_visibility(&self) -> block_mesh::VoxelVisibility {
        match (self.material, self.occlusion) {
            (None, _) => block_mesh::VoxelVisibility::Empty,
            (Some(_), FaceOcclusion::Full) => block_mesh::VoxelVisibility::Opaque,
            (Some(_), FaceOcclusion::None | FaceOcclusion::Matching) => {
                block_mesh::VoxelVisibility::Translucent
            }
        }
    }
}

impl block_mesh::MergeVoxel for AuditVoxel {
    type MergeValue = (Option<u8>, MeshGroup);

    fn merge_value(&self) -> Self::MergeValue {
        (self.material, self.group)
    }
}

#[test]
fn opaque_subset_matches_upstream_face_and_greedy_counts() {
    for (name, voxels, expected) in [
        (
            "solid",
            volume(|_| AuditVoxel::opaque(1)),
            (6 * INTERIOR_EDGE * INTERIOR_EDGE, 6),
        ),
        (
            "layered",
            volume(|[_, y, _]| {
                AuditVoxel::opaque(u8::try_from(y + 1).expect("four layers fit u8"))
            }),
            (6 * INTERIOR_EDGE * INTERIOR_EDGE, 18),
        ),
        (
            "checker",
            volume(|[x, y, z]| {
                if (x + y + z) % 2 == 0 {
                    AuditVoxel::opaque(1)
                } else {
                    AuditVoxel::AIR
                }
            }),
            (192, 192),
        ),
    ] {
        let local = local_counts(&voxels);
        let upstream = upstream_counts(&voxels);
        assert_eq!(local, expected, "local {name} fixture guard");
        assert_eq!(upstream, expected, "upstream {name} fixture guard");
        assert_eq!(local, upstream, "opaque {name} conformance");
    }
}

#[test]
fn deterministic_opaque_corpora_match_upstream_visible_face_counts() {
    for seed in 0_u64..64 {
        let voxels = random_opaque_volume(seed);
        let local = local_counts(&voxels);
        let upstream = upstream_counts(&voxels);
        assert_eq!(
            local.0, upstream.0,
            "opaque visible-face corpus seed {seed}"
        );
    }
}

#[test]
fn upstream_greedy_partition_does_not_preserve_stable_output() {
    let voxels = random_opaque_volume(19);

    // Both implementations cover the same 122 visible unit faces, but their
    // deterministic rectangle partition differs. Production adoption would
    // therefore change the current stable quad stream even for opaque input.
    assert_eq!(local_counts(&voxels), (122, 100));
    assert_eq!(upstream_counts(&voxels), (122, 101));
}

#[test]
fn upstream_visibility_cannot_represent_matching_material_interfaces() {
    let voxels = volume(|[x, y, z]| match [x, y, z] {
        [0, 0, 0] => AuditVoxel::matching(1, MeshGroup::Translucent),
        [1, 0, 0] => AuditVoxel::matching(2, MeshGroup::Translucent),
        _ => AuditVoxel::AIR,
    });

    // Lattice Axiom retains both faces between different matching materials.
    // `block-mesh` has voxel-wide translucency and suppresses every interface
    // between two translucent voxels, irrespective of material identity.
    assert_eq!(local_counts(&voxels), (12, 12));
    assert_eq!(upstream_counts(&voxels), (10, 10));
}

fn random_opaque_volume(seed: u64) -> Vec<AuditVoxel> {
    let mut state = seed.wrapping_add(1);
    volume(|_| {
        state = state
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        match state % 5 {
            0 | 1 => AuditVoxel::AIR,
            material => {
                AuditVoxel::opaque(u8::try_from(material).expect("bounded audit material fits u8"))
            }
        }
    })
}

fn volume(mut voxel_at: impl FnMut([usize; 3]) -> AuditVoxel) -> Vec<AuditVoxel> {
    let dimensions = dimensions();
    assert_eq!(
        dimensions.volume_len(),
        usize::try_from(AuditShape::SIZE).expect("fixed shape size fits usize")
    );
    let mut voxels = vec![AuditVoxel::AIR; dimensions.volume_len()];
    for z in 0..INTERIOR_EDGE {
        for y in 0..INTERIOR_EDGE {
            for x in 0..INTERIOR_EDGE {
                let padded = [x + 1, y + 1, z + 1];
                let index = dimensions
                    .linearize(padded)
                    .expect("audit interior coordinate is padded in bounds");
                voxels[index] = voxel_at([x, y, z]);
            }
        }
    }
    voxels
}

fn local_counts(voxels: &[AuditVoxel]) -> (usize, usize) {
    let visible = visible_faces(voxels, dimensions(), source())
        .expect("audit volume length matches dimensions")
        .geometry()
        .quad_count();
    let greedy = greedy_quads(voxels, dimensions(), source())
        .expect("audit volume length matches dimensions")
        .geometry()
        .quad_count();
    (visible, greedy)
}

fn upstream_counts(voxels: &[AuditVoxel]) -> (usize, usize) {
    let shape = AuditShape {};
    let bounds = [5; 3];
    let mut visible = block_mesh::UnitQuadBuffer::new();
    block_mesh::visible_block_faces(
        voxels,
        &shape,
        [0; 3],
        bounds,
        &block_mesh::RIGHT_HANDED_Y_UP_CONFIG.faces,
        &mut visible,
    );
    let mut greedy = block_mesh::GreedyQuadsBuffer::new(voxels.len());
    block_mesh::greedy_quads(
        voxels,
        &shape,
        [0; 3],
        bounds,
        &block_mesh::RIGHT_HANDED_Y_UP_CONFIG.faces,
        &mut greedy,
    );
    (visible.num_quads(), greedy.quads.num_quads())
}

fn dimensions() -> PaddedChunk {
    PaddedChunk::new([INTERIOR_EDGE; 3]).expect("fixed audit dimensions are valid")
}

fn source() -> MeshSource {
    MeshSource::new(
        ChunkCoordinate::new(-2, 3, -5),
        SourceEpoch::new(11),
        SourceRevision::new(13),
        SourceFingerprint::new([17; 32]),
    )
}
