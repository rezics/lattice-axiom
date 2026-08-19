//! Load-time block metadata compiled into numeric hot-path tables.

use std::collections::BTreeMap;

use anyhow::{Result, bail};
use latticeaxiom_core::{BlockId, TerrainBlocks};

/// Immutable runtime properties for one registered block.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BlockProperties {
    /// Whether this block collides and occludes voxel faces.
    pub solid: bool,
    /// Linear RGBA used by the first-demo terrain renderer.
    pub color: [f32; 4],
}

/// Numeric block lookup built once after module registration.
#[derive(Clone, Debug)]
pub struct BlockCatalog {
    blocks: BTreeMap<BlockId, BlockProperties>,
    terrain: TerrainBlocks,
    selected: BlockId,
}

impl BlockCatalog {
    /// Builds and validates the numeric table used by simulation and meshing.
    ///
    /// # Errors
    ///
    /// Fails when IDs are duplicated, air is registered as solid, terrain IDs
    /// are missing/non-solid, or the placement selection is not solid.
    pub fn new(
        definitions: impl IntoIterator<Item = (BlockId, bool, [u8; 4])>,
        terrain: TerrainBlocks,
        selected: BlockId,
    ) -> Result<Self> {
        let mut blocks = BTreeMap::new();
        blocks.insert(
            BlockId::AIR,
            BlockProperties {
                solid: false,
                color: [0.0; 4],
            },
        );
        for (id, solid, color) in definitions {
            if id.is_air() {
                if solid {
                    bail!("the reserved air block cannot be solid");
                }
                continue;
            }
            let properties = BlockProperties {
                solid,
                color: rgba_to_float(color),
            };
            if blocks.insert(id, properties).is_some() {
                bail!("block id {id} was registered more than once");
            }
        }

        for (label, id) in [
            ("surface", terrain.surface),
            ("subsurface", terrain.subsurface),
            ("stone", terrain.stone),
            ("selected", selected),
        ] {
            let Some(properties) = blocks.get(&id) else {
                bail!("{label} block id {id} is not registered");
            };
            if !properties.solid {
                bail!("{label} block id {id} must be solid");
            }
        }

        Ok(Self {
            blocks,
            terrain,
            selected,
        })
    }

    /// Returns whether a numeric block participates in collision.
    #[must_use]
    pub fn is_solid(&self, id: BlockId) -> bool {
        self.blocks.get(&id).is_some_and(|block| block.solid)
    }

    /// Returns a registered color, using conspicuous magenta for corrupted or
    /// unknown non-air IDs.
    #[must_use]
    pub fn color(&self, id: BlockId) -> [f32; 4] {
        self.blocks
            .get(&id)
            .map_or([1.0, 0.0, 1.0, 1.0], |block| block.color)
    }

    /// Terrain layer identities resolved from the composed runtime image.
    #[must_use]
    pub const fn terrain(&self) -> TerrainBlocks {
        self.terrain
    }

    /// Block identity placed by the player's use action.
    #[must_use]
    pub const fn selected(&self) -> BlockId {
        self.selected
    }
}

fn rgba_to_float(color: [u8; 4]) -> [f32; 4] {
    color.map(|component| f32::from(component) / 255.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_resolves_only_numeric_hot_path_properties() {
        let stone = BlockId::from_raw(1);
        let dirt = BlockId::from_raw(2);
        let grass = BlockId::from_raw(3);
        let catalog = BlockCatalog::new(
            [
                (stone, true, [80, 90, 100, 255]),
                (dirt, true, [120, 80, 50, 255]),
                (grass, true, [50, 160, 70, 255]),
            ],
            TerrainBlocks::new(grass, dirt, stone),
            dirt,
        )
        .expect("complete solid terrain catalog must be valid");

        assert!(catalog.is_solid(stone));
        assert!(!catalog.is_solid(BlockId::AIR));
        assert_eq!(catalog.selected(), dirt);
    }
}
