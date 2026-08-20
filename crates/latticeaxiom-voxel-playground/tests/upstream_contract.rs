//! Compile-time contract checks against `bevy_voxel_world` 0.17.
#![cfg(feature = "presentation-adapter")]

use bevy_voxel_world::prelude::{DefaultWorld, VoxelWorld, WorldVoxel};
use latticeaxiom_voxel_playground::{PresentationSink, PresentationVoxel, to_upstream_voxel};

fn assert_default_world_is_a_sink<'world>()
where
    VoxelWorld<'world, DefaultWorld>: PresentationSink<u8>,
{
}

#[test]
fn upstream_mapping_keeps_air_distinct_from_unset() {
    assert_default_world_is_a_sink();
    assert_eq!(
        to_upstream_voxel(PresentationVoxel::<u8>::Air),
        WorldVoxel::Air
    );
    assert_eq!(
        to_upstream_voxel(PresentationVoxel::Solid(7_u8)),
        WorldVoxel::Solid(7)
    );
}
