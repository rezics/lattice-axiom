//! Concrete `bevy_voxel_world` presentation sink.

use std::convert::Infallible;

use bevy::prelude::IVec3;
use bevy_voxel_world::prelude::{VoxelWorld, VoxelWorldConfig, WorldVoxel};

use crate::{PresentationCoordinate, PresentationSink, PresentationVoxel};

/// Converts a Lattice presentation value to the upstream voxel enum.
///
/// Known authoritative air maps to `Air`, never `Unset`; `Unset` remains an
/// upstream cache/generation detail and has no persistence meaning.
#[must_use]
pub fn to_upstream_voxel<M>(voxel: PresentationVoxel<M>) -> WorldVoxel<M> {
    match voxel {
        PresentationVoxel::Air => WorldVoxel::Air,
        PresentationVoxel::Solid(material) => WorldVoxel::Solid(material),
    }
}

impl<C: VoxelWorldConfig> PresentationSink<C::MaterialIndex> for VoxelWorld<'_, C> {
    type Error = Infallible;

    fn set_voxel(
        &mut self,
        coordinate: PresentationCoordinate,
        voxel: PresentationVoxel<C::MaterialIndex>,
    ) -> Result<(), Self::Error> {
        VoxelWorld::set_voxel(
            self,
            IVec3::new(coordinate.x, coordinate.y, coordinate.z),
            to_upstream_voxel(voxel),
        );
        Ok(())
    }
}
