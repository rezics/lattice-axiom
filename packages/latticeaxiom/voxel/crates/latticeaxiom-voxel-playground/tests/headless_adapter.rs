//! Headless acceptance tests for the D2 upstream projection seam.

use latticeaxiom_gameplay::{BlockId, BlockPosition};
use latticeaxiom_player::{BlockEditActionV1, BlockEditSuccessV1, BlockFaceV1};
use latticeaxiom_storage::{ChunkCoordinate, ChunkRevision};
use latticeaxiom_voxel_playground::{
    ApplyOutcome, PresentationCoordinate, PresentationSink, PresentationVoxel, ProjectionBatch,
    ProjectionError, ProjectionWrite, RevisionGate, VoxelChunkEdge,
};
#[derive(Debug, Default)]
struct RecordingSink {
    writes: Vec<(PresentationCoordinate, PresentationVoxel<u8>)>,
}

impl PresentationSink<u8> for RecordingSink {
    type Error = &'static str;

    fn set_voxel(
        &mut self,
        coordinate: PresentationCoordinate,
        voxel: PresentationVoxel<u8>,
    ) -> Result<(), Self::Error> {
        self.writes.push((coordinate, voxel));
        Ok(())
    }
}

fn single_batch(
    coordinate: PresentationCoordinate,
    revision: u64,
    voxel: PresentationVoxel<u8>,
) -> ProjectionBatch<u8> {
    let edge = VoxelChunkEdge::D2;
    let result = ProjectionBatch::new(
        coordinate.chunk(edge),
        ChunkRevision::new(revision),
        edge,
        vec![ProjectionWrite::new(coordinate, voxel)],
    );
    let Ok(batch) = result else {
        panic!("single in-chunk write must form a valid batch");
    };
    batch
}

#[test]
fn stale_and_duplicate_results_never_reach_the_sink() {
    let coordinate = PresentationCoordinate::new(-1, 4, -33);
    let newest = single_batch(coordinate, 9, PresentationVoxel::Solid(7));
    let stale = single_batch(coordinate, 8, PresentationVoxel::Air);
    let duplicate = newest.clone();
    let mut gate = RevisionGate::default();
    let mut sink = RecordingSink::default();

    assert_eq!(
        gate.apply(&newest, &mut sink),
        Ok(ApplyOutcome::Applied { writes: 1 })
    );
    assert_eq!(
        gate.apply(&stale, &mut sink),
        Ok(ApplyOutcome::Stale {
            applied_revision: ChunkRevision::new(9),
        })
    );
    assert_eq!(
        gate.apply(&duplicate, &mut sink),
        Ok(ApplyOutcome::Duplicate)
    );
    assert_eq!(sink.writes, vec![(coordinate, PresentationVoxel::Solid(7))]);
}

#[test]
fn sink_failure_does_not_publish_a_revision_receipt() {
    #[derive(Debug, Default)]
    struct FailingSink;

    impl PresentationSink<u8> for FailingSink {
        type Error = &'static str;

        fn set_voxel(
            &mut self,
            _coordinate: PresentationCoordinate,
            _voxel: PresentationVoxel<u8>,
        ) -> Result<(), Self::Error> {
            Err("injected presentation failure")
        }
    }

    let coordinate = PresentationCoordinate::new(0, 0, 0);
    let batch = single_batch(coordinate, 3, PresentationVoxel::Solid(1));
    let mut gate = RevisionGate::default();

    assert_eq!(
        gate.apply(&batch, &mut FailingSink),
        Err("injected presentation failure")
    );
    assert_eq!(gate.applied_revision(ChunkCoordinate::new(0, 0, 0)), None);
}

#[test]
fn negative_coordinates_use_euclidean_chunk_ownership() {
    let edge = VoxelChunkEdge::D2;
    assert_eq!(
        PresentationCoordinate::new(-1, -32, -33).chunk(edge),
        ChunkCoordinate::new(-1, -1, -2)
    );
    assert_eq!(
        PresentationCoordinate::new(0, 31, 32).chunk(edge),
        ChunkCoordinate::new(0, 0, 1)
    );
}

#[test]
fn all_six_faces_preserve_native_y_up_xyz_adjacency() {
    let origin = BlockPosition {
        x: -4,
        y: -7,
        z: -9,
    };
    let cases = [
        (BlockFaceV1::PositiveX, (-3, -7, -9)),
        (BlockFaceV1::NegativeX, (-5, -7, -9)),
        (BlockFaceV1::PositiveY, (-4, -6, -9)),
        (BlockFaceV1::NegativeY, (-4, -8, -9)),
        (BlockFaceV1::PositiveZ, (-4, -7, -8)),
        (BlockFaceV1::NegativeZ, (-4, -7, -10)),
    ];

    for (face, expected) in cases {
        let Some(adjacent) = face.adjacent(origin) else {
            panic!("fixture adjacency stays inside coordinate bounds");
        };
        assert_eq!((adjacent.x, adjacent.y, adjacent.z), expected);
    }
}

#[test]
fn break_and_place_successes_share_the_revision_gated_command_path() {
    let Ok(stone) = BlockId::parse("example:block/stone") else {
        panic!("fixture block ID must be canonical");
    };
    let position = BlockPosition { x: -32, y: 5, z: 1 };
    let break_success = BlockEditSuccessV1 {
        position,
        old_content: Some(stone.clone()),
        new_content: None,
        committed_chunk_revision: ChunkRevision::new(11),
    };
    let place_success = BlockEditSuccessV1 {
        position,
        old_content: None,
        new_content: Some(stone),
        committed_chunk_revision: ChunkRevision::new(12),
    };
    let break_batch =
        ProjectionBatch::from_block_edit(BlockEditActionV1::Break, &break_success, |_| Some(99_u8));
    let place_batch =
        ProjectionBatch::from_block_edit(BlockEditActionV1::Place, &place_success, |_| Some(4_u8));
    let (Ok(break_batch), Ok(place_batch)) = (break_batch, place_batch) else {
        panic!("authoritative fixture successes must translate");
    };
    let mut gate = RevisionGate::default();
    let mut sink = RecordingSink::default();

    assert_eq!(
        gate.apply(&break_batch, &mut sink),
        Ok(ApplyOutcome::Applied { writes: 1 })
    );
    assert_eq!(
        gate.apply(&place_batch, &mut sink),
        Ok(ApplyOutcome::Applied { writes: 1 })
    );
    assert_eq!(
        sink.writes,
        vec![
            (
                PresentationCoordinate::new(-32, 5, 1),
                PresentationVoxel::Air
            ),
            (
                PresentationCoordinate::new(-32, 5, 1),
                PresentationVoxel::Solid(4),
            ),
        ]
    );
}

#[test]
fn malformed_success_cannot_mutate_presentation() {
    let success = BlockEditSuccessV1 {
        position: BlockPosition { x: 0, y: 0, z: 0 },
        old_content: None,
        new_content: None,
        committed_chunk_revision: ChunkRevision::new(1),
    };
    assert!(matches!(
        ProjectionBatch::<u8>::from_block_edit(BlockEditActionV1::Break, &success, |_| Some(1)),
        Err(ProjectionError::ActionResultMismatch {
            action: BlockEditActionV1::Break,
        })
    ));
}

#[test]
fn batches_cannot_smuggle_a_write_across_a_chunk_boundary() {
    let result = ProjectionBatch::new(
        ChunkCoordinate::new(-1, 0, 0),
        ChunkRevision::new(1),
        VoxelChunkEdge::D2,
        vec![ProjectionWrite::new(
            PresentationCoordinate::new(0, 0, 0),
            PresentationVoxel::<u8>::Air,
        )],
    );
    assert!(matches!(result, Err(ProjectionError::ChunkMismatch { .. })));
}

#[test]
fn batch_writes_are_canonical_across_discovery_order() {
    let edge = VoxelChunkEdge::D2;
    let chunk = ChunkCoordinate::new(0, 0, 0);
    let coordinates = [
        PresentationCoordinate::new(3, 1, 2),
        PresentationCoordinate::new(1, 3, 2),
        PresentationCoordinate::new(1, 2, 3),
    ];
    let batch = ProjectionBatch::new(
        chunk,
        ChunkRevision::new(1),
        edge,
        coordinates
            .into_iter()
            .map(|coordinate| ProjectionWrite::new(coordinate, PresentationVoxel::<u8>::Air))
            .collect(),
    )
    .unwrap_or_else(|error| panic!("canonical batch fixture: {error}"));
    let actual = batch
        .writes()
        .iter()
        .map(ProjectionWrite::coordinate)
        .collect::<Vec<_>>();
    let mut expected = coordinates.to_vec();
    expected.sort_unstable();
    assert_eq!(actual, expected);
}

#[test]
fn batch_rejects_duplicate_coordinates() {
    let coordinate = PresentationCoordinate::new(1, 2, 3);
    let result = ProjectionBatch::new(
        ChunkCoordinate::new(0, 0, 0),
        ChunkRevision::new(1),
        VoxelChunkEdge::D2,
        vec![
            ProjectionWrite::new(coordinate, PresentationVoxel::<u8>::Air),
            ProjectionWrite::new(coordinate, PresentationVoxel::Solid(7)),
        ],
    );
    assert_eq!(
        result,
        Err(ProjectionError::DuplicateCoordinate { coordinate })
    );
}
