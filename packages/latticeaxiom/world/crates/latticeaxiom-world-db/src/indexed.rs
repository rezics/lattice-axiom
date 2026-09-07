//! Incremental physical records. Legacy images remain intact as migration inputs.

use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

use latticeaxiom_core::{StableId, WorldId};
use latticeaxiom_storage::{ChunkKey, PersistentEntityId};
use latticeaxiom_world_wire::{ChunkRecordKey, RecordKind, WorldWireLimits};
use redb::{Durability, ReadTransaction, ReadableDatabase, ReadableTable, TableDefinition};

use crate::{
    DeterministicHeaderPublisher, DeterministicWorldStorage, DiskWorldEntryV1, DiskWorldError,
    DiskWorldStore, DurableWorldImageV1, PersistedChunkV1, WorldDbError, WorldDbResult,
    WorldFrontierV1, WorldReadView, WorldStorageLimitsV1,
    disk::{CATALOG, database_error},
    durable::{DurableRootImageV1, DurableStoreImageV1},
    keyspace::encode_record_key_for_write,
};

const HEADS: TableDefinition<'static, &str, &[u8]> = TableDefinition::new("world-heads-v2");
const RECORDS: TableDefinition<'static, &[u8], &[u8]> = TableDefinition::new("world-records-v2");
const ENTITIES: TableDefinition<'static, &[u8], &[u8]> = TableDefinition::new("world-entities-v2");
const CHECKPOINT_IMAGES: TableDefinition<'static, &[u8], &[u8]> =
    TableDefinition::new("checkpoint-images-v2");
const CHECKPOINT_RECEIPTS: TableDefinition<'static, &[u8], &[u8]> =
    TableDefinition::new("checkpoint-receipts-v2");
pub(crate) type IndexedCheckpoints =
    BTreeMap<crate::CheckpointId, (DurableStoreImageV1, crate::CheckpointReceiptV1)>;

pub(crate) fn physical(error: impl std::fmt::Display) -> WorldDbError {
    WorldDbError::PhysicalStorage {
        reason: error.to_string(),
    }
}

fn envelope(root: &DurableRootImageV1) -> WorldDbResult<DurableWorldImageV1> {
    let (bytes, digest) = root.encode()?;
    Ok(DurableWorldImageV1 {
        world: root.world().world(),
        bytes,
        digest,
    })
}

fn decode_head(bytes: &[u8]) -> WorldDbResult<DurableRootImageV1> {
    let value: DurableWorldImageV1 = postcard::from_bytes(bytes).map_err(physical)?;
    let root = DurableRootImageV1::decode(&value.bytes, value.digest)?;
    if value.world != root.world().world() || !root.world().records.is_empty() {
        return Err(physical(
            "indexed head contains records or a mismatched identity",
        ));
    }
    Ok(root)
}

fn entity_key(world: WorldId, id: PersistentEntityId) -> WorldDbResult<Vec<u8>> {
    postcard::to_allocvec(&(world, id)).map_err(physical)
}

impl DiskWorldStore {
    pub(crate) fn verify_indexed_records(
        &self,
        world: WorldId,
        owner: &StableId,
        limits: WorldWireLimits,
    ) -> WorldDbResult<()> {
        let tx = self.indexed_read().map_err(physical)?;
        let heads = tx.open_table(HEADS).map_err(physical)?;
        let head = heads
            .get(world.to_string().as_str())
            .map_err(physical)?
            .ok_or_else(|| physical("world head missing"))?;
        let frontier = decode_head(head.value())?.world().frontier();
        let records = tx.open_table(RECORDS).map_err(physical)?;
        let entities = tx.open_table(ENTITIES).map_err(physical)?;
        let prefix = latticeaxiom_world_wire::world_key_prefix(world);
        for row in records.range(prefix.as_slice()..).map_err(physical)? {
            let (key, value) = row.map_err(physical)?;
            if !key.value().starts_with(&prefix) {
                break;
            }
            let logical = latticeaxiom_world_wire::decode_chunk_record_key(key.value(), limits)
                .map_err(physical)?;
            if logical.owner() != owner || logical.record_kind() != RecordKind::CHUNK_SNAPSHOT {
                return Err(physical("record owner or kind mismatch"));
            }
            let chunk = crate::memory::decode_record(
                logical.chunk(),
                &BTreeMap::from([(key.value().to_vec(), value.value().to_vec())]),
                owner,
                limits,
            )?
            .ok_or_else(|| physical("indexed record disappeared"))?;
            if chunk.captured_world_revision() > frontier.durable() {
                return Err(physical("record exceeds durable frontier"));
            }
            for id in chunk.data().persistent_entities().keys() {
                let value = entities
                    .get(entity_key(world, *id)?.as_slice())
                    .map_err(physical)?
                    .ok_or_else(|| physical("persistent entity index missing"))?;
                let indexed: ChunkKey = postcard::from_bytes(value.value()).map_err(physical)?;
                if &indexed != chunk.key() {
                    return Err(physical("persistent entity index mismatch"));
                }
            }
        }
        let prefix = postcard::to_allocvec(&world).map_err(physical)?;
        for row in entities.range(prefix.as_slice()..).map_err(physical)? {
            let (key, value) = row.map_err(physical)?;
            if !key.value().starts_with(&prefix) {
                break;
            }
            let (_, id): (WorldId, PersistentEntityId) =
                postcard::from_bytes(key.value()).map_err(physical)?;
            let chunk_key: ChunkKey = postcard::from_bytes(value.value()).map_err(physical)?;
            let key = encode_record_key_for_write(
                &ChunkRecordKey::new(chunk_key.clone(), RecordKind::CHUNK_SNAPSHOT, owner.clone()),
                limits,
            )?;
            let record = records
                .get(key.as_slice())
                .map_err(physical)?
                .ok_or_else(|| physical("entity index points to missing chunk"))?;
            let chunk = crate::memory::decode_record(
                &chunk_key,
                &BTreeMap::from([(key, record.value().to_vec())]),
                owner,
                limits,
            )?
            .ok_or_else(|| physical("indexed entity chunk missing"))?;
            if !chunk.data().persistent_entities().contains_key(&id) {
                return Err(physical("orphan entity index"));
            }
        }
        Ok(())
    }
    pub(crate) fn checkpoint_receipts(
        &self,
        world: WorldId,
    ) -> WorldDbResult<BTreeMap<crate::CheckpointId, crate::CheckpointReceiptV1>> {
        let tx = self.indexed_read().map_err(physical)?;
        let table = tx.open_table(CHECKPOINT_RECEIPTS).map_err(physical)?;
        let prefix = postcard::to_allocvec(&world).map_err(physical)?;
        let mut result = BTreeMap::new();
        for item in table.range(prefix.as_slice()..).map_err(physical)? {
            let (key, value) = item.map_err(physical)?;
            if !key.value().starts_with(&prefix) {
                break;
            }
            let (owner, id): (WorldId, crate::CheckpointId) =
                postcard::from_bytes(key.value()).map_err(physical)?;
            if owner != world {
                return Err(physical("checkpoint world mismatch"));
            }
            result.insert(id, postcard::from_bytes(value.value()).map_err(physical)?);
            if result.len()
                > WorldStorageLimitsV1::D3_BOOTSTRAP.max_checkpoints_per_world() as usize
            {
                return Err(physical("checkpoint count exceeded"));
            }
        }
        Ok(result)
    }

    pub(crate) fn indexed_checkpoint(
        &self,
        world: WorldId,
        id: crate::CheckpointId,
    ) -> WorldDbResult<(DurableStoreImageV1, crate::CheckpointReceiptV1)> {
        let tx = self.indexed_read().map_err(physical)?;
        let key = postcard::to_allocvec(&(world, id)).map_err(physical)?;
        let images = tx.open_table(CHECKPOINT_IMAGES).map_err(physical)?;
        let receipts = tx.open_table(CHECKPOINT_RECEIPTS).map_err(physical)?;
        let image = images.get(key.as_slice()).map_err(physical)?.ok_or(
            WorldDbError::CheckpointNotFound {
                world,
                checkpoint: id,
            },
        )?;
        let receipt = receipts
            .get(key.as_slice())
            .map_err(physical)?
            .ok_or_else(|| physical("checkpoint receipt missing"))?;
        Ok((
            postcard::from_bytes(image.value()).map_err(physical)?,
            postcard::from_bytes(receipt.value()).map_err(physical)?,
        ))
    }
    pub(crate) fn export_indexed(&self, world: WorldId) -> WorldDbResult<DurableWorldImageV1> {
        let tx = self.indexed_read().map_err(physical)?;
        let heads = tx.open_table(HEADS).map_err(physical)?;
        let bytes = heads
            .get(world.to_string().as_str())
            .map_err(physical)?
            .ok_or_else(|| physical("indexed world head missing"))?;
        let root = decode_head(bytes.value())?;
        let mut image = root.world().clone();
        let prefix = latticeaxiom_world_wire::world_key_prefix(world);
        let table = tx.open_table(RECORDS).map_err(physical)?;
        for item in table.range(prefix.as_slice()..).map_err(physical)? {
            let (key, value) = item.map_err(physical)?;
            if !key.value().starts_with(&prefix) {
                break;
            }
            image
                .records
                .insert(key.value().to_vec(), value.value().to_vec());
        }
        let receipts = self.checkpoint_receipts(world)?;
        let mut checkpoints = BTreeMap::new();
        for id in receipts.keys() {
            checkpoints.insert(*id, self.indexed_checkpoint(world, *id)?.0);
        }
        let root = DurableRootImageV1::new(image, checkpoints, receipts)?;
        envelope(&root)
    }
    /// Opens a world with lazy per-chunk reads and bounded database residency.
    /// Legacy snapshots are copied transactionally into a new keyspace once;
    /// their original image and checkpoint bytes are retained unchanged.
    ///
    /// # Errors
    /// Rejects missing/corrupt worlds and failed physical migration or reads.
    pub fn load_indexed(
        &self,
        world: WorldId,
    ) -> Result<DeterministicWorldStorage, DiskWorldError> {
        let root = if let Some(root) = self.indexed_head(world)? {
            root
        } else {
            let legacy = self.load(world)?;
            let original = DurableRootImageV1::decode(&legacy.bytes, legacy.digest)?;
            self.initialize_indexed(&original)?;
            self.indexed_head(world)?
                .ok_or(DiskWorldError::Missing(world))?
        };
        let storage = DeterministicWorldStorage::from_durable_image(
            &envelope(&root)?,
            "latticeaxiom:schema/world-db-chunk@1"
                .parse()
                .map_err(database_error)?,
            WorldWireLimits::default(),
            WorldStorageLimitsV1::D3_BOOTSTRAP,
            Arc::new(DeterministicHeaderPublisher::new()),
        )?;
        storage.attach_indexed(self.clone())?;
        Ok(storage)
    }

    /// Attaches a just-provisioned working world after its first physical save.
    ///
    /// # Errors
    /// Returns migration or snapshot-validation errors without discarding input.
    pub fn attach_indexed(
        &self,
        storage: &DeterministicWorldStorage,
    ) -> Result<(), DiskWorldError> {
        if storage.is_indexed() {
            return Ok(());
        }
        let image = storage.export_durable_image()?;
        let root = DurableRootImageV1::decode(&image.bytes, image.digest)?;
        self.initialize_indexed(&root)?;
        storage.attach_indexed(self.clone())?;
        Ok(())
    }

    fn initialize_indexed(&self, original: &DurableRootImageV1) -> Result<(), DiskWorldError> {
        let db = self
            .database
            .lock()
            .map_err(database_error)?
            .as_ref()
            .cloned()
            .ok_or_else(|| database_error("connection is closed"))?;
        let mut tx = db.begin_write().map_err(database_error)?;
        tx.set_durability(Durability::Immediate)
            .map_err(database_error)?;
        let world = original.world().world();
        {
            let mut heads = tx.open_table(HEADS).map_err(database_error)?;
            if heads
                .get(world.to_string().as_str())
                .map_err(database_error)?
                .is_some()
            {
                return Ok(());
            }
            let mut records = tx.open_table(RECORDS).map_err(database_error)?;
            let mut entities = tx.open_table(ENTITIES).map_err(database_error)?;
            let mut checkpoint_images = tx.open_table(CHECKPOINT_IMAGES).map_err(database_error)?;
            let mut checkpoint_receipts =
                tx.open_table(CHECKPOINT_RECEIPTS).map_err(database_error)?;
            for (id, image) in original.checkpoints() {
                let key = postcard::to_allocvec(&(world, id)).map_err(database_error)?;
                checkpoint_images
                    .insert(
                        key.as_slice(),
                        postcard::to_allocvec(image)
                            .map_err(database_error)?
                            .as_slice(),
                    )
                    .map_err(database_error)?;
                let receipt = original
                    .checkpoint_receipts()
                    .get(id)
                    .cloned()
                    .ok_or_else(|| database_error("migration checkpoint receipt missing"))?;
                checkpoint_receipts
                    .insert(
                        key.as_slice(),
                        postcard::to_allocvec(&receipt)
                            .map_err(database_error)?
                            .as_slice(),
                    )
                    .map_err(database_error)?;
            }
            let owner: StableId = "latticeaxiom:schema/world-db-chunk@1"
                .parse()
                .map_err(database_error)?;
            for (key, value) in &original.world().records {
                let logical = latticeaxiom_world_wire::decode_chunk_record_key(
                    key,
                    WorldWireLimits::default(),
                )
                .map_err(database_error)?;
                let chunk = crate::memory::decode_record(
                    logical.chunk(),
                    &BTreeMap::from([(key.clone(), value.clone())]),
                    &owner,
                    WorldWireLimits::default(),
                )?
                .ok_or_else(|| database_error("migration record missing"))?;
                for id in chunk.data().persistent_entities().keys() {
                    let key = entity_key(world, *id)?;
                    let value = postcard::to_allocvec(chunk.key()).map_err(database_error)?;
                    if entities
                        .get(key.as_slice())
                        .map_err(database_error)?
                        .is_some()
                    {
                        return Err(database_error(
                            "duplicate persistent entity during migration",
                        ));
                    }
                    entities
                        .insert(key.as_slice(), value.as_slice())
                        .map_err(database_error)?;
                }
                records
                    .insert(key.as_slice(), value.as_slice())
                    .map_err(database_error)?;
            }
            let mut metadata = original.world().clone();
            metadata.records.clear();
            let head = DurableRootImageV1::new(metadata, BTreeMap::new(), BTreeMap::new())?;
            let bytes = postcard::to_allocvec(&envelope(&head)?).map_err(database_error)?;
            heads
                .insert(world.to_string().as_str(), bytes.as_slice())
                .map_err(database_error)?;
        }
        tx.commit().map_err(database_error)
    }

    pub(crate) fn indexed_head(
        &self,
        world: WorldId,
    ) -> Result<Option<DurableRootImageV1>, DiskWorldError> {
        let tx = self.indexed_read()?;
        let table = match tx.open_table(HEADS) {
            Ok(table) => table,
            Err(redb::TableError::TableDoesNotExist(_)) => return Ok(None),
            Err(error) => return Err(database_error(error)),
        };
        table
            .get(world.to_string().as_str())
            .map_err(database_error)?
            .map(|value| decode_head(value.value()).map_err(DiskWorldError::from))
            .transpose()
    }

    pub(crate) fn indexed_read(&self) -> Result<ReadTransaction, DiskWorldError> {
        self.database
            .lock()
            .map_err(database_error)?
            .as_ref()
            .ok_or_else(|| database_error("connection is closed"))?
            .begin_read()
            .map_err(database_error)
    }

    pub(crate) fn indexed_view(
        &self,
        world: WorldId,
        frontier: WorldFrontierV1,
        overlay: Arc<BTreeMap<Vec<u8>, Vec<u8>>>,
        owner: StableId,
        limits: WorldWireLimits,
    ) -> WorldDbResult<Box<dyn WorldReadView>> {
        Ok(Box::new(IndexedReadView {
            tx: self.indexed_read().map_err(physical)?,
            world,
            frontier,
            overlay,
            owner,
            limits,
        }))
    }

    pub(crate) fn current_indexed_view(
        &self,
        world: WorldId,
        owner: StableId,
        limits: WorldWireLimits,
    ) -> WorldDbResult<Box<dyn WorldReadView>> {
        let tx = self.indexed_read().map_err(physical)?;
        let frontier = {
            let heads = tx.open_table(HEADS).map_err(physical)?;
            let head = heads
                .get(world.to_string().as_str())
                .map_err(physical)?
                .ok_or(WorldDbError::WorldNotFound { world })?;
            decode_head(head.value())?.world().frontier()
        };
        Ok(Box::new(IndexedReadView {
            tx,
            world,
            frontier,
            owner,
            limits,
            overlay: Arc::new(BTreeMap::new()),
        }))
    }

    pub(crate) fn indexed_entity(
        &self,
        world: WorldId,
        id: PersistentEntityId,
    ) -> WorldDbResult<Option<ChunkKey>> {
        let tx = self.indexed_read().map_err(physical)?;
        let table = tx.open_table(ENTITIES).map_err(physical)?;
        table
            .get(entity_key(world, id)?.as_slice())
            .map_err(physical)?
            .map(|value| postcard::from_bytes(value.value()).map_err(physical))
            .transpose()
    }

    pub(crate) fn commit_indexed(
        &self,
        metadata: DurableStoreImageV1,
        records: &BTreeMap<Vec<u8>, Vec<u8>>,
        old_entities: &BTreeSet<PersistentEntityId>,
        entities: &BTreeMap<PersistentEntityId, ChunkKey>,
        expected_epoch: u64,
        checkpoints: &IndexedCheckpoints,
    ) -> WorldDbResult<(Vec<u8>, crate::DigestV1)> {
        let root = DurableRootImageV1::new(metadata, BTreeMap::new(), BTreeMap::new())?;
        let value = envelope(&root)?;
        let bytes = postcard::to_allocvec(&value).map_err(physical)?;
        let db = self
            .database
            .lock()
            .map_err(physical)?
            .as_ref()
            .cloned()
            .ok_or_else(|| physical("connection is closed"))?;
        let mut tx = db.begin_write().map_err(physical)?;
        tx.set_durability(Durability::Immediate).map_err(physical)?;
        let world = root.world().world();
        {
            let mut heads = tx.open_table(HEADS).map_err(physical)?;
            let current = heads
                .get(world.to_string().as_str())
                .map_err(physical)?
                .ok_or_else(|| physical("indexed world head missing"))?;
            let epoch = decode_head(current.value())?.world().metadata_epoch().get();
            drop(current);
            if epoch != expected_epoch {
                return Err(physical("indexed head changed; reconcile before retry"));
            }
            let mut table = tx.open_table(RECORDS).map_err(physical)?;
            for (key, value) in records {
                table
                    .insert(key.as_slice(), value.as_slice())
                    .map_err(physical)?;
            }
            let mut index = tx.open_table(ENTITIES).map_err(physical)?;
            for id in old_entities {
                index
                    .remove(entity_key(world, *id)?.as_slice())
                    .map_err(physical)?;
            }
            for (id, key) in entities {
                index
                    .insert(
                        entity_key(world, *id)?.as_slice(),
                        postcard::to_allocvec(key).map_err(physical)?.as_slice(),
                    )
                    .map_err(physical)?;
            }
            heads
                .insert(world.to_string().as_str(), bytes.as_slice())
                .map_err(physical)?;
            let mut checkpoint_images = tx.open_table(CHECKPOINT_IMAGES).map_err(physical)?;
            let mut checkpoint_receipts = tx.open_table(CHECKPOINT_RECEIPTS).map_err(physical)?;
            for (id, (image, receipt)) in checkpoints {
                let key = postcard::to_allocvec(&(world, id)).map_err(physical)?;
                checkpoint_images
                    .insert(
                        key.as_slice(),
                        postcard::to_allocvec(image).map_err(physical)?.as_slice(),
                    )
                    .map_err(physical)?;
                checkpoint_receipts
                    .insert(
                        key.as_slice(),
                        postcard::to_allocvec(receipt).map_err(physical)?.as_slice(),
                    )
                    .map_err(physical)?;
            }
            let mut catalog = tx.open_table(CATALOG).map_err(physical)?;
            let existing = catalog
                .get(world.to_string().as_str())
                .map_err(physical)?
                .ok_or_else(|| physical("indexed catalog missing"))?;
            let mut entry: DiskWorldEntryV1 =
                serde_json::from_slice(existing.value()).map_err(physical)?;
            drop(existing);
            entry.metadata_epoch = root.world().metadata_epoch().get();
            entry.durable_revision = root.world().frontier().durable().get();
            catalog
                .insert(
                    world.to_string().as_str(),
                    serde_json::to_vec(&entry).map_err(physical)?.as_slice(),
                )
                .map_err(physical)?;
        }
        tx.commit().map_err(physical)?;
        Ok((value.bytes, value.digest))
    }

    pub(crate) fn publish_indexed_catalog(
        &self,
        storage: &DeterministicWorldStorage,
        entry: &DiskWorldEntryV1,
    ) -> Result<(), DiskWorldError> {
        let frontier = crate::WorldStorage::begin_read(storage, entry.world)?.frontier();
        let root = self
            .indexed_head(entry.world)?
            .ok_or(DiskWorldError::Missing(entry.world))?;
        if frontier.durable() != root.world().frontier().durable() {
            return Err(database_error("unsynchronized indexed frontier"));
        }
        let mut entry = entry.clone();
        entry.metadata_epoch = root.world().metadata_epoch().get();
        entry.durable_revision = frontier.durable().get();
        let db = self
            .database
            .lock()
            .map_err(database_error)?
            .as_ref()
            .cloned()
            .ok_or_else(|| database_error("connection is closed"))?;
        let mut tx = db.begin_write().map_err(database_error)?;
        tx.set_durability(Durability::Immediate)
            .map_err(database_error)?;
        tx.open_table(CATALOG)
            .map_err(database_error)?
            .insert(
                entry.world.to_string().as_str(),
                serde_json::to_vec(&entry)?.as_slice(),
            )
            .map_err(database_error)?;
        tx.commit().map_err(database_error)
    }
}

struct IndexedReadView {
    tx: ReadTransaction,
    world: WorldId,
    frontier: WorldFrontierV1,
    overlay: Arc<BTreeMap<Vec<u8>, Vec<u8>>>,
    owner: StableId,
    limits: WorldWireLimits,
}

impl WorldReadView for IndexedReadView {
    fn world(&self) -> WorldId {
        self.world
    }
    fn frontier(&self) -> WorldFrontierV1 {
        self.frontier
    }
    fn load_chunk(&self, key: &ChunkKey) -> WorldDbResult<Option<PersistedChunkV1>> {
        if key.world != self.world {
            return Err(WorldDbError::ChunkWorldMismatch {
                transaction_world: self.world,
                key: Box::new(key.clone()),
            });
        }
        let encoded = encode_record_key_for_write(
            &ChunkRecordKey::new(key.clone(), RecordKind::CHUNK_SNAPSHOT, self.owner.clone()),
            self.limits,
        )?;
        if self.overlay.contains_key(&encoded) {
            return crate::memory::decode_record(key, &self.overlay, &self.owner, self.limits);
        }
        let table = self.tx.open_table(RECORDS).map_err(physical)?;
        let Some(value) = table.get(encoded.as_slice()).map_err(physical)? else {
            return Ok(None);
        };
        let records = BTreeMap::from([(encoded, value.value().to_vec())]);
        let chunk = crate::memory::decode_record(key, &records, &self.owner, self.limits)?;
        if chunk
            .as_ref()
            .is_some_and(|chunk| chunk.captured_world_revision() > self.frontier.current())
        {
            return Err(physical("chunk is newer than its captured frontier"));
        }
        Ok(chunk)
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, reason = "validated storage fixtures")]
    use super::*;
    use crate::property_tests::{
        durable_store, fixture_data, fixture_key, provision_ready, sealed_activation,
    };
    use crate::{CommitDurabilityV1, DisplayName, WorldCommitRequestV1, WorldStorage};
    use latticeaxiom_core::CanonicalHash;
    use latticeaxiom_storage::{
        ChangedDomains, ChunkCoordinate, ChunkMutation, ChunkRevisionExpectation, TransactionId,
        WorldTransaction,
    };

    #[test]
    #[allow(
        clippy::too_many_lines,
        reason = "one physical lifecycle proves snapshot isolation, bounded residency and independent reopen"
    )]
    fn indexed_traversal_persists_untouched_chunks_without_retaining_payloads() {
        let dir = tempfile::tempdir().expect("directory");
        let path = dir.path().join("worlds.redb");
        let (storage, world, metadata) = durable_store();
        provision_ready(&storage, world, &metadata);
        let original = storage.export_durable_image().expect("original");
        let disk = DiskWorldStore::open(&path).expect("database");
        disk.publish(
            &storage,
            &DiskWorldEntryV1 {
                world,
                display_name: DisplayName::new("Indexed").expect("name"),
                created_at_ms: 1,
                last_played_at_ms: 1,
                game_lock: CanonicalHash::digest(b"lock"),
                generation_profile: None,
                metadata_epoch: 1,
                durable_revision: 0,
            },
        )
        .expect("initial publication");
        disk.attach_indexed(&storage).expect("indexed migration");
        let permit = storage
            .preflight(world)
            .expect("preflight")
            .activation_permit()
            .expect("permit")
            .clone();
        let mut writer = storage
            .activate_writer(sealed_activation(world, permit))
            .expect("writer");
        let empty_view = storage.begin_read(world).expect("old snapshot");
        for n in 1..=48_u8 {
            let base = storage
                .begin_read(world)
                .expect("view")
                .frontier()
                .current();
            writer
                .commit(WorldCommitRequestV1::new(
                    WorldTransaction::new(
                        TransactionId::from_u128(u128::from(n)),
                        world,
                        base,
                        vec![ChunkMutation::new(
                            fixture_key(world, ChunkCoordinate::new(i32::from(n), 0, 0)),
                            ChunkRevisionExpectation::Absent,
                            ChangedDomains::ALL,
                            fixture_data(n),
                        )],
                    ),
                    metadata.clone(),
                    CommitDurabilityV1::Durable,
                ))
                .expect("incremental commit");
            assert_eq!(
                storage
                    .keyspace_stats()
                    .expect("cache stats")
                    .record_entries(),
                0
            );
        }
        let key = fixture_key(world, ChunkCoordinate::new(48, 0, 0));
        assert!(
            empty_view
                .load_chunk(&key)
                .expect("isolated old read")
                .is_none()
        );
        assert_eq!(
            storage
                .begin_read(world)
                .expect("read")
                .load_chunk(&key)
                .expect("chunk")
                .expect("present")
                .data(),
            &fixture_data(48)
        );
        writer.close().expect("close");
        drop(empty_view);
        drop(storage);
        drop(disk);
        let disk = DiskWorldStore::open(&path).expect("independent reopen");
        assert_eq!(
            disk.load_legacy(world).expect("legacy recovery image"),
            original
        );
        let restored = disk.load_indexed(world).expect("lazy open");
        assert_eq!(
            restored
                .keyspace_stats()
                .expect("cold cache stats")
                .record_entries(),
            0
        );
        let view = restored.begin_read(world).expect("read view");
        assert_eq!(
            view.load_chunk(&key).expect("read").expect("stored").data(),
            &fixture_data(48)
        );
        assert!(
            view.load_chunk(&fixture_key(world, ChunkCoordinate::new(49, 0, 0)))
                .expect("absent")
                .is_none()
        );
        let exported = disk.load(world).expect("complete export");
        let decoded =
            DurableRootImageV1::decode(&exported.bytes, exported.digest).expect("valid export");
        assert_eq!(decoded.world().records.len(), 48);
    }

    #[test]
    #[allow(
        clippy::too_many_lines,
        reason = "physical failure and checkpoint recovery share one isolated fixture"
    )]
    fn indexed_checkpoint_survives_reopen_and_corruption_never_means_absence() {
        use crate::{CheckpointId, CheckpointKindV1, CheckpointRequestV1, DatabaseFaultPointV1};
        let dir = tempfile::tempdir().expect("directory");
        let path = dir.path().join("worlds.redb");
        let (storage, world, metadata) = durable_store();
        provision_ready(&storage, world, &metadata);
        let disk = DiskWorldStore::open(&path).expect("database");
        disk.publish(
            &storage,
            &DiskWorldEntryV1 {
                world,
                display_name: DisplayName::new("Recovery").expect("name"),
                created_at_ms: 1,
                last_played_at_ms: 1,
                game_lock: CanonicalHash::digest(b"lock"),
                generation_profile: None,
                metadata_epoch: 1,
                durable_revision: 0,
            },
        )
        .expect("initial save");
        disk.attach_indexed(&storage).expect("attach");
        let permit = storage
            .preflight(world)
            .expect("preflight")
            .activation_permit()
            .expect("permit")
            .clone();
        let mut writer = storage
            .activate_writer(sealed_activation(world, permit))
            .expect("writer");
        let key = fixture_key(world, ChunkCoordinate::new(1, 0, 0));
        let request = |number: u128, key: ChunkKey| {
            WorldCommitRequestV1::new(
                WorldTransaction::new(
                    TransactionId::from_u128(number),
                    world,
                    storage
                        .begin_read(world)
                        .expect("view")
                        .frontier()
                        .current(),
                    vec![ChunkMutation::new(
                        key,
                        ChunkRevisionExpectation::Absent,
                        ChangedDomains::ALL,
                        fixture_data(1),
                    )],
                ),
                metadata.clone(),
                CommitDurabilityV1::Durable,
            )
        };
        writer.commit(request(1, key.clone())).expect("chunk saved");
        let checkpoint = CheckpointId::from_u128(7);
        writer
            .create_checkpoint(CheckpointRequestV1::new(
                checkpoint,
                CheckpointKindV1::Protected,
                "before failure",
            ))
            .expect("checkpoint");
        storage
            .inject_database_fault_once(DatabaseFaultPointV1::BeforeBatchPublication)
            .expect("fault");
        // A new chunk without entities avoids an unrelated duplicate-entity rejection.
        let absent = fixture_key(world, ChunkCoordinate::new(2, 0, 0));
        let data = fixture_data(2);
        let failure = WorldCommitRequestV1::new(
            WorldTransaction::new(
                TransactionId::from_u128(2),
                world,
                storage
                    .begin_read(world)
                    .expect("view")
                    .frontier()
                    .current(),
                vec![ChunkMutation::new(
                    absent.clone(),
                    ChunkRevisionExpectation::Absent,
                    ChangedDomains::ALL,
                    data,
                )],
            ),
            metadata,
            CommitDurabilityV1::Durable,
        );
        assert!(writer.commit(failure).is_err());
        assert!(
            storage
                .begin_read(world)
                .expect("read")
                .load_chunk(&absent)
                .expect("absent")
                .is_none()
        );
        writer.close().expect("close");
        drop(storage);
        drop(disk);
        let disk = DiskWorldStore::open(&path).expect("independent open");
        let storage = disk.load_indexed(world).expect("lazy storage");
        assert!(
            storage
                .verify_checkpoint(world, checkpoint)
                .expect("retained checkpoint")
                .restore_verified()
        );
        let exported = disk.load(world).expect("export with checkpoint");
        let root = DurableRootImageV1::decode(&exported.bytes, exported.digest).expect("root");
        assert_eq!(root.world().records.len(), 1);
        assert_eq!(root.checkpoints()[&checkpoint].records.len(), 1);
        let record_key = root.world().records.keys().next().expect("key").clone();
        {
            let guard = disk.database.lock().expect("lock");
            let tx = guard.as_ref().expect("db").begin_write().expect("write");
            tx.open_table(RECORDS)
                .expect("records")
                .insert(record_key.as_slice(), b"damaged".as_slice())
                .expect("corrupt fixture");
            tx.commit().expect("commit corruption");
        }
        assert!(
            storage
                .begin_read(world)
                .expect("read")
                .load_chunk(&key)
                .is_err()
        );
        assert!(storage.verify_checkpoint(world, checkpoint).is_ok());
        assert!(storage.verify_crash_recovery(world).is_err());
    }
}
