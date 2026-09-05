//! Physical, atomic publication of world images and their catalog metadata.
//!
//! The contract oracle does not claim physical durability on its own. This
//! adapter acknowledges it only after a redb Immediate transaction commits.

use std::{path::Path, sync::Arc};

use latticeaxiom_core::{CanonicalHash, StableId, WorldId};
use redb::{Database, Durability, ReadableDatabase, ReadableTable, TableDefinition};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{DeterministicWorldStorage, DigestV1, DisplayName, WorldDbError};

const CATALOG: TableDefinition<'static, &str, &[u8]> = TableDefinition::new("world-catalog-v1");
const IMAGES: TableDefinition<'static, &str, &[u8]> = TableDefinition::new("world-images-v1");
const PREVIOUS: TableDefinition<'static, &str, &[u8]> =
    TableDefinition::new("previous-world-images-v1");

/// An independently verified, portable world-store image, never a writer lease.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DurableWorldImageV1 {
    /// World identity bound inside the image.
    pub world: WorldId,
    /// Portable encoded storage records and metadata.
    pub bytes: Vec<u8>,
    /// Domain-separated image checksum.
    pub digest: DigestV1,
}

/// Small catalog record readable without loading voxel payloads.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DiskWorldEntryV1 {
    /// Persisted world identity.
    pub world: WorldId,
    /// Validated display name.
    pub display_name: DisplayName,
    /// Initial creation timestamp.
    pub created_at_ms: u64,
    /// Last successful durable publication timestamp.
    pub last_played_at_ms: u64,
    /// Frozen gameplay lock; client resource locks are deliberately excluded.
    pub game_lock: CanonicalHash,
    /// Creation profile supplied by the gameplay package.
    pub generation_profile: Option<StableId>,
}

/// Failure to open, read or atomically publish a physical world store.
#[derive(Debug, Error)]
pub enum DiskWorldError {
    /// Filesystem failure.
    #[error(transparent)]
    Io(#[from] std::io::Error),
    /// World codec or storage-contract failure.
    #[error(transparent)]
    World(#[from] WorldDbError),
    /// Invalid catalog metadata.
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    /// redb rejected the operation; a failed commit must not be assumed rolled back.
    #[error("physical world store failed: {0}")]
    Database(String),
    /// Requested world does not exist.
    #[error("saved world {0} is absent")]
    Missing(WorldId),
    /// Catalog and payload disagree on world identity.
    #[error("catalog and world image identities disagree")]
    Identity,
}

fn database_error(error: impl std::fmt::Display) -> DiskWorldError {
    DiskWorldError::Database(error.to_string())
}

/// Process-exclusive physical repository with atomic catalog/image publication.
#[derive(Clone, Debug)]
pub struct DiskWorldStore {
    database: Arc<Database>,
}

impl DiskWorldStore {
    /// Opens or initializes a local repository. Existing invalid files are not replaced.
    ///
    /// # Errors
    /// Returns [`DiskWorldError`] for I/O, format, lock or transaction failures.
    pub fn open(path: &Path) -> Result<Self, DiskWorldError> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let database = Database::create(path).map_err(database_error)?;
        let mut transaction = database.begin_write().map_err(database_error)?;
        transaction
            .set_durability(Durability::Immediate)
            .map_err(database_error)?;
        transaction.open_table(CATALOG).map_err(database_error)?;
        transaction.open_table(IMAGES).map_err(database_error)?;
        transaction.open_table(PREVIOUS).map_err(database_error)?;
        transaction.commit().map_err(database_error)?;
        Ok(Self {
            database: Arc::new(database),
        })
    }

    /// Lists catalog metadata without decoding any world's voxel payload.
    ///
    /// # Errors
    /// Returns [`DiskWorldError`] for corrupt catalog records or database errors.
    pub fn entries(&self) -> Result<Vec<DiskWorldEntryV1>, DiskWorldError> {
        let transaction = self.database.begin_read().map_err(database_error)?;
        let table = transaction.open_table(CATALOG).map_err(database_error)?;
        table
            .iter()
            .map_err(database_error)?
            .map(|item| {
                let (_, value) = item.map_err(database_error)?;
                Ok(serde_json::from_slice(value.value())?)
            })
            .collect()
    }

    /// Loads one current world image, retaining the checksum for bounded codec validation.
    ///
    /// # Errors
    /// Returns [`DiskWorldError`] when the world is absent or its envelope is invalid.
    pub fn load(&self, world: WorldId) -> Result<DurableWorldImageV1, DiskWorldError> {
        let transaction = self.database.begin_read().map_err(database_error)?;
        let table = transaction.open_table(IMAGES).map_err(database_error)?;
        let key = world.to_string();
        let value = table
            .get(key.as_str())
            .map_err(database_error)?
            .ok_or(DiskWorldError::Missing(world))?;
        let image: DurableWorldImageV1 =
            postcard::from_bytes(value.value()).map_err(database_error)?;
        if image.world != world {
            return Err(DiskWorldError::Identity);
        }
        crate::durable::DurableRootImageV1::decode(&image.bytes, image.digest)?;
        Ok(image)
    }

    /// Atomically publishes the synchronized world and its catalog entry.
    ///
    /// Keeps one previous image for recovery. Success means redb's Immediate
    /// durability boundary completed; an error never becomes a success receipt.
    ///
    /// # Errors
    /// Returns [`DiskWorldError`] for identity, codec, I/O or commit failures.
    pub fn publish(
        &self,
        storage: &DeterministicWorldStorage,
        entry: &DiskWorldEntryV1,
    ) -> Result<(), DiskWorldError> {
        let image = storage.export_durable_image()?;
        if image.world != entry.world {
            return Err(DiskWorldError::Identity);
        }
        let encoded = postcard::to_allocvec(&image).map_err(database_error)?;
        let metadata = serde_json::to_vec(entry)?;
        let key = entry.world.to_string();
        let mut transaction = self.database.begin_write().map_err(database_error)?;
        transaction
            .set_durability(Durability::Immediate)
            .map_err(database_error)?;
        {
            let mut images = transaction.open_table(IMAGES).map_err(database_error)?;
            let previous = images
                .get(key.as_str())
                .map_err(database_error)?
                .map(|value| value.value().to_vec());
            if let Some(previous) = previous {
                transaction
                    .open_table(PREVIOUS)
                    .map_err(database_error)?
                    .insert(key.as_str(), previous.as_slice())
                    .map_err(database_error)?;
            }
            images
                .insert(key.as_str(), encoded.as_slice())
                .map_err(database_error)?;
            transaction
                .open_table(CATALOG)
                .map_err(database_error)?
                .insert(key.as_str(), metadata.as_slice())
                .map_err(database_error)?;
        }
        transaction.commit().map_err(database_error)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::property_tests::{durable_store, provision_ready};

    #[test]
    fn physical_world_and_catalog_survive_all_original_handles_being_dropped() {
        let directory = tempfile::tempdir().expect("temporary repository");
        let path = directory.path().join("worlds.redb");
        let (storage, world, metadata) = durable_store();
        provision_ready(&storage, world, &metadata);
        let original = storage.export_durable_image().expect("logical image");
        let entry = DiskWorldEntryV1 {
            world,
            display_name: DisplayName::new("Saved world").expect("name"),
            created_at_ms: 1,
            last_played_at_ms: 2,
            game_lock: CanonicalHash::digest(b"world-lock"),
            generation_profile: None,
        };
        {
            let disk = DiskWorldStore::open(&path).expect("physical repository");
            disk.publish(&storage, &entry).expect("Immediate commit");
        }
        drop(storage);
        let disk = DiskWorldStore::open(&path).expect("independent reopen");
        assert_eq!(disk.entries().expect("catalog"), vec![entry]);
        assert_eq!(disk.load(world).expect("world image"), original);
    }

    #[test]
    fn uncommitted_catalog_change_is_not_published() {
        let directory = tempfile::tempdir().expect("temporary repository");
        let path = directory.path().join("worlds.redb");
        let disk = DiskWorldStore::open(&path).expect("repository");
        {
            let transaction = disk.database.begin_write().expect("transaction");
            transaction
                .open_table(CATALOG)
                .expect("catalog")
                .insert("uncommitted", b"invalid metadata".as_slice())
                .expect("stage write");
        }
        drop(disk);
        assert!(
            DiskWorldStore::open(&path)
                .expect("reopen")
                .entries()
                .expect("catalog")
                .is_empty()
        );
    }
}
