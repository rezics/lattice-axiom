use latticeaxiom_storage::{ChunkCoordinate, DimensionId, PersistentEntityId};

use crate::BlockPosition;

macro_rules! persistent_entity_wrapper {
    ($(#[$meta:meta])* $name:ident) => {
        $(#[$meta])*
        #[repr(transparent)]
        #[derive(Clone, Copy, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd)]
        pub struct $name(PersistentEntityId);

        impl $name {
            /// Creates a deterministic identifier for fixtures and reference-oracle input.
            #[must_use]
            pub const fn new(value: u64) -> Self {
                Self(PersistentEntityId::from_u128(value as u128))
            }

            /// Creates an identifier from stable network-order bytes.
            #[must_use]
            pub const fn from_bytes(bytes: [u8; 16]) -> Self {
                Self(PersistentEntityId::from_bytes(bytes))
            }

            /// Borrows the canonical storage identifier without conversion.
            #[must_use]
            pub const fn as_persistent_entity_id(&self) -> &PersistentEntityId {
                &self.0
            }

            /// Consumes the domain wrapper and returns the canonical storage identifier.
            #[must_use]
            pub const fn into_persistent_entity_id(self) -> PersistentEntityId {
                self.0
            }

            /// Returns stable network-order bytes.
            #[must_use]
            pub const fn as_bytes(self) -> [u8; 16] {
                *self.0.as_bytes()
            }


        }
    };
}

persistent_entity_wrapper!(
    /// Persistent player entity identity used by the gameplay reference oracle.
    PlayerId
);
persistent_entity_wrapper!(
    /// Persistent dropped-item entity identity used by gameplay plans.
    DropEntityId
);
persistent_entity_wrapper!(
    /// Persistent container entity identity used by gameplay plans.
    ContainerId
);

/// Dimension-qualified chunk key observed by a gameplay plan.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct DimensionChunkKey {
    /// Registered dimension containing the chunk.
    pub dimension: DimensionId,
    /// Canonical storage chunk coordinate.
    pub coordinate: ChunkCoordinate,
}

impl DimensionChunkKey {
    /// Creates a complete gameplay-visible chunk key.
    #[must_use]
    pub const fn new(dimension: DimensionId, coordinate: ChunkCoordinate) -> Self {
        Self {
            dimension,
            coordinate,
        }
    }
}

/// Complete block-cell identity; positions never imply a default dimension.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct BlockKey {
    /// Registered dimension containing the cell.
    pub dimension: DimensionId,
    /// Integer voxel position in right-handed Y-up coordinates.
    pub position: BlockPosition,
}

impl BlockKey {
    /// Creates a dimension-qualified block key.
    #[must_use]
    pub const fn new(dimension: DimensionId, position: BlockPosition) -> Self {
        Self {
            dimension,
            position,
        }
    }

    /// Returns the dimension-qualified authoritative chunk containing the cell.
    #[must_use]
    pub fn chunk(&self) -> DimensionChunkKey {
        self.chunk_in(32)
    }

    /// Returns the chunk key for a host-selected cubic edge.
    #[must_use]
    pub fn chunk_in(&self, edge: u16) -> DimensionChunkKey {
        DimensionChunkKey::new(self.dimension.clone(), self.position.chunk_in(edge))
    }
}

/// Storage domain touched by one runtime-staged gameplay edit.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum GameplayStorageDomain {
    /// Canonical voxel payload.
    Voxels,
    /// Persistent entity payloads.
    PersistentEntities,
    /// Deterministic continuation payloads.
    Continuations,
}

/// Explicit storage capture target for one runtime-staged gameplay edit.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct GameplayEditTarget {
    /// Dimension-qualified chunk to replace in the eventual storage transaction.
    pub chunk: DimensionChunkKey,
    /// Authoritative storage domain changed by the edit.
    pub domain: GameplayStorageDomain,
}

impl GameplayEditTarget {
    /// Creates an explicit capture target.
    #[must_use]
    pub const fn new(chunk: DimensionChunkKey, domain: GameplayStorageDomain) -> Self {
        Self { chunk, domain }
    }
}
