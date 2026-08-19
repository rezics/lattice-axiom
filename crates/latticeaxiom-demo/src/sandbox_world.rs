//! Streamed authoritative world backed by the storage facade.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context as _, Result};
use latticeaxiom_core::{BlockId, BlockPos, CHUNK_EDGE, Chunk, ChunkAddress, ChunkPos, Heightmap};
use latticeaxiom_storage::{
    BoundaryContractId, ChunkCommit, ChunkKey, ChunkSnapshot, CommitCondition, CommitId,
    ContentHash, DimensionId, Durability, GenerationEpoch, GenerationProvenance, ImplementationId,
    SchemaVersion, WorldId, WorldStorage,
};
use sha2::{Digest as _, Sha256};

use crate::block_catalog::BlockCatalog;
use crate::chunk_codec;

const HORIZONTAL_RADIUS: i32 = 2;
const VERTICAL_RADIUS: i32 = 1;
const SNAPSHOT_SCHEMA: SchemaVersion = SchemaVersion::new(1, 0);

#[derive(Debug)]
struct ResidentChunk {
    chunk: Chunk,
    revision: u64,
    provenance: GenerationProvenance,
}

/// Result of moving the bounded residency window.
#[derive(Debug, Default)]
pub struct ResidencyChange {
    /// Chunks removed from memory and therefore from the render world.
    pub unloaded: Vec<ChunkPos>,
}

/// M3 streamed world: authoritative chunks, deterministic generation, and
/// durable edits behind one storage contract.
pub struct SandboxWorld {
    storage: Arc<dyn WorldStorage>,
    world_id: WorldId,
    dimension: DimensionId,
    catalog: BlockCatalog,
    heightmap: Heightmap,
    producer_hash: ContentHash,
    chunks: BTreeMap<ChunkPos, ResidentChunk>,
    desired: BTreeSet<ChunkPos>,
    pending_loads: VecDeque<ChunkPos>,
    dirty_meshes: BTreeSet<ChunkPos>,
    edit_count: u64,
}

impl std::fmt::Debug for SandboxWorld {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("SandboxWorld")
            .field("world_id", &self.world_id)
            .field("dimension", &self.dimension)
            .field("chunks", &self.chunks.len())
            .field("desired", &self.desired.len())
            .field("pending_loads", &self.pending_loads.len())
            .field("dirty_meshes", &self.dirty_meshes.len())
            .field("edit_count", &self.edit_count)
            .finish_non_exhaustive()
    }
}

impl SandboxWorld {
    /// Creates a world runtime over a production or in-memory storage facade.
    pub fn new(
        storage: Arc<dyn WorldStorage>,
        catalog: BlockCatalog,
        seed: u64,
        producer_hash: ContentHash,
    ) -> Result<Self> {
        let heightmap = Heightmap::new(seed, 10, 6, 3)
            .context("the fixed first-demo heightmap configuration must be valid")?;
        Ok(Self {
            storage,
            world_id: WorldId::from_u128(1),
            dimension: DimensionId::from_u32(0),
            catalog,
            heightmap,
            producer_hash,
            chunks: BTreeMap::new(),
            desired: BTreeSet::new(),
            pending_loads: VecDeque::new(),
            dirty_meshes: BTreeSet::new(),
            edit_count: 0,
        })
    }

    /// Re-centers the bounded residency window around a world block.
    #[must_use]
    pub fn center_on(&mut self, block: BlockPos) -> ResidencyChange {
        let center = ChunkAddress::from_block(block).chunk;
        let mut desired = BTreeSet::new();
        for dz in -VERTICAL_RADIUS..=VERTICAL_RADIUS {
            for dy in -HORIZONTAL_RADIUS..=HORIZONTAL_RADIUS {
                for dx in -HORIZONTAL_RADIUS..=HORIZONTAL_RADIUS {
                    if let (Some(x), Some(y), Some(z)) = (
                        center.x.checked_add(dx),
                        center.y.checked_add(dy),
                        center.z.checked_add(dz),
                    ) {
                        desired.insert(ChunkPos::new(x, y, z));
                    }
                }
            }
        }

        let unloaded: Vec<_> = self
            .chunks
            .keys()
            .filter(|position| !desired.contains(position))
            .copied()
            .collect();
        for position in &unloaded {
            self.chunks.remove(position);
            self.dirty_meshes.remove(position);
        }
        // Remaining neighbors previously culled their shared faces against the
        // removed chunk's apron. Rebuild them so the residency boundary stays
        // visually closed instead of exposing a transient hole.
        for position in &unloaded {
            self.mark_mesh_neighborhood(*position);
        }

        let mut missing: Vec<_> = desired
            .iter()
            .filter(|position| !self.chunks.contains_key(position))
            .copied()
            .collect();
        missing.sort_by_key(|position| (chunk_distance(*position, center), *position));
        self.pending_loads = missing.into();
        self.desired = desired;
        ResidencyChange { unloaded }
    }

    /// Materializes at most `budget` pending chunks from storage or generation.
    pub fn stream(&mut self, budget: usize) -> Result<usize> {
        let mut loaded = 0;
        while loaded < budget {
            let Some(position) = self.pending_loads.pop_front() else {
                break;
            };
            if !self.desired.contains(&position) || self.chunks.contains_key(&position) {
                continue;
            }
            let resident = self.load_or_generate(position)?;
            self.chunks.insert(position, resident);
            self.mark_mesh_neighborhood(position);
            loaded += 1;
        }
        Ok(loaded)
    }

    /// Completes the current residency request; used at startup and by tests.
    pub fn stream_all(&mut self) -> Result<()> {
        while !self.pending_loads.is_empty() {
            self.stream(16)?;
        }
        Ok(())
    }

    /// Reads a block from resident authoritative state; absent chunks are air
    /// until their deterministic load request completes.
    #[must_use]
    pub fn block_at(&self, position: BlockPos) -> BlockId {
        let address = ChunkAddress::from_block(position);
        self.chunks
            .get(&address.chunk)
            .map_or(BlockId::AIR, |resident| resident.chunk.block(address.local))
    }

    /// Returns whether a resident block collides with the player.
    #[must_use]
    pub fn is_solid_at(&self, position: BlockPos) -> bool {
        self.catalog.is_solid(self.block_at(position))
    }

    /// Returns whether a block is solid for authoritative player collision.
    ///
    /// A chunk that is not resident is treated as a temporary barrier. This
    /// prevents the player entering unloaded terrain and becoming embedded
    /// when its authoritative snapshot arrives a few frames later.
    #[must_use]
    pub fn is_collision_solid_at(&self, position: BlockPos) -> bool {
        let address = ChunkAddress::from_block(position);
        self.chunks
            .get(&address.chunk)
            .is_none_or(|resident| self.catalog.is_solid(resident.chunk.block(address.local)))
    }

    /// Finds the nearest two-cell standing clearance around a preferred foot
    /// cell in one loaded column.
    ///
    /// The upward candidate wins at equal distance so an obstruction saved at
    /// the normal spawn point relocates the player above it. The one-chunk
    /// search radius stays inside the initial three-chunk vertical residency.
    #[must_use]
    pub fn find_standing_space(
        &self,
        column_x: i32,
        column_y: i32,
        preferred_feet_z: i32,
    ) -> Option<BlockPos> {
        for distance in 0..=CHUNK_EDGE {
            if let Some(z) = preferred_feet_z.checked_add(distance) {
                let candidate = BlockPos::new(column_x, column_y, z);
                if self.is_standing_space(candidate) {
                    return Some(candidate);
                }
            }
            if distance > 0
                && let Some(z) = preferred_feet_z.checked_sub(distance)
            {
                let candidate = BlockPos::new(column_x, column_y, z);
                if self.is_standing_space(candidate) {
                    return Some(candidate);
                }
            }
        }
        None
    }

    /// Replaces one resident block and synchronously acknowledges a durable
    /// snapshot commit before reporting success.
    pub fn set_block_durable(&mut self, position: BlockPos, block: BlockId) -> Result<BlockId> {
        let address = ChunkAddress::from_block(position);
        let resident = self
            .chunks
            .get_mut(&address.chunk)
            .with_context(|| format!("chunk {:?} is not resident", address.chunk))?;
        let previous = resident.chunk.set_block(address.local, block);
        if previous == block {
            return Ok(previous);
        }

        if let Err(error) = self.persist_chunk(address.chunk, Durability::Sync) {
            let resident = self
                .chunks
                .get_mut(&address.chunk)
                .context("edited chunk disappeared during persistence rollback")?;
            resident.chunk.set_block(address.local, previous);
            return Err(error);
        }
        self.edit_count = self.edit_count.saturating_add(1);
        self.mark_mesh_neighborhood(address.chunk);
        Ok(previous)
    }

    /// Removes and returns up to `budget` deterministic remesh requests.
    pub fn take_dirty_meshes(&mut self, budget: usize) -> Vec<ChunkPos> {
        let mut positions = Vec::with_capacity(budget.min(self.dirty_meshes.len()));
        while positions.len() < budget {
            let Some(position) = self.dirty_meshes.pop_first() else {
                break;
            };
            if self.chunks.contains_key(&position) {
                positions.push(position);
            }
        }
        positions
    }

    /// Numeric block metadata compiled at module registration.
    #[must_use]
    pub const fn catalog(&self) -> &BlockCatalog {
        &self.catalog
    }

    /// Current number of resident authoritative chunks.
    #[must_use]
    pub fn loaded_chunks(&self) -> usize {
        self.chunks.len()
    }

    /// Number of chunks still waiting for storage/generation.
    #[must_use]
    pub fn pending_loads(&self) -> usize {
        self.pending_loads.len()
    }

    /// Number of resident chunks waiting for a derived mesh rebuild.
    #[must_use]
    pub fn pending_meshes(&self) -> usize {
        self.dirty_meshes.len()
    }

    /// Durable block edits acknowledged in this process.
    #[must_use]
    pub const fn edit_count(&self) -> u64 {
        self.edit_count
    }

    /// Heightmap used for deterministic first materialization.
    #[must_use]
    pub const fn heightmap(&self) -> Heightmap {
        self.heightmap
    }

    fn load_or_generate(&self, position: ChunkPos) -> Result<ResidentChunk> {
        let key = ChunkKey::new(self.world_id, self.dimension, position);
        if let Some(stored) = self
            .storage
            .load_chunk(key)
            .with_context(|| format!("failed to load chunk {position:?}"))?
        {
            if stored.snapshot.schema != SNAPSHOT_SCHEMA {
                anyhow::bail!(
                    "chunk {position:?} uses unsupported snapshot schema {:?}",
                    stored.snapshot.schema
                );
            }
            if stored.snapshot.provenance.configuration_hash != self.producer_hash {
                anyhow::bail!(
                    "chunk {position:?} was generated by a different configuration; \
                     reopen the matching package graph or start a new world"
                );
            }
            let chunk = chunk_codec::decode(position, &stored.snapshot.payload)?;
            return Ok(ResidentChunk {
                chunk,
                revision: stored.revision,
                provenance: stored.snapshot.provenance,
            });
        }

        let chunk = self
            .heightmap
            .generate_chunk(position, self.catalog.terrain())
            .with_context(|| format!("failed to generate chunk {position:?}"))?;
        let provenance = self.generation_provenance()?;
        let payload = chunk_codec::encode(&chunk)?;
        let commit = ChunkCommit {
            id: commit_id(key, &payload, 0),
            key,
            condition: CommitCondition::IfAbsent,
            durability: Durability::Wal,
            snapshot: ChunkSnapshot {
                schema: SNAPSHOT_SCHEMA,
                provenance: provenance.clone(),
                payload,
            },
            spatial_entities: BTreeMap::new(),
            continuation: BTreeMap::new(),
            artifact_receipts: BTreeMap::new(),
        };
        let receipt = self
            .storage
            .commit_chunk(commit)
            .with_context(|| format!("failed to materialize chunk {position:?}"))?;
        Ok(ResidentChunk {
            chunk,
            revision: receipt.revision,
            provenance,
        })
    }

    fn persist_chunk(&mut self, position: ChunkPos, durability: Durability) -> Result<()> {
        let resident = self
            .chunks
            .get(&position)
            .context("cannot persist a non-resident chunk")?;
        let payload = chunk_codec::encode(&resident.chunk)?;
        let key = ChunkKey::new(self.world_id, self.dimension, position);
        let expected_revision = resident.revision;
        let commit = ChunkCommit {
            id: commit_id(key, &payload, expected_revision),
            key,
            condition: CommitCondition::IfRevision(expected_revision),
            durability,
            snapshot: ChunkSnapshot {
                schema: SNAPSHOT_SCHEMA,
                provenance: resident.provenance.clone(),
                payload,
            },
            spatial_entities: BTreeMap::new(),
            continuation: BTreeMap::new(),
            artifact_receipts: BTreeMap::new(),
        };
        let receipt = self
            .storage
            .commit_chunk(commit)
            .with_context(|| format!("failed to persist edited chunk {position:?}"))?;
        self.chunks
            .get_mut(&position)
            .context("persisted chunk disappeared before revision update")?
            .revision = receipt.revision;
        Ok(())
    }

    fn generation_provenance(&self) -> Result<GenerationProvenance> {
        let producer = ImplementationId::new("latticeaxiom.official.heightmap@1")
            .context("fixed producer identifier must be valid")?;
        let boundary_contract = BoundaryContractId::new("chunk.apron@1")
            .context("fixed boundary contract identifier must be valid")?;
        let mut implementations = BTreeMap::new();
        implementations.insert(producer, self.producer_hash);
        Ok(GenerationProvenance {
            created_at_unix_millis: unix_millis(),
            epoch: GenerationEpoch::from_u64(1),
            configuration_hash: self.producer_hash,
            implementations,
            upstream_plan_hash: self.producer_hash,
            boundary_contract,
        })
    }

    fn mark_mesh_neighborhood(&mut self, position: ChunkPos) {
        for (dx, dy, dz) in [
            (0, 0, 0),
            (-1, 0, 0),
            (1, 0, 0),
            (0, -1, 0),
            (0, 1, 0),
            (0, 0, -1),
            (0, 0, 1),
        ] {
            if let (Some(x), Some(y), Some(z)) = (
                position.x.checked_add(dx),
                position.y.checked_add(dy),
                position.z.checked_add(dz),
            ) {
                let neighbor = ChunkPos::new(x, y, z);
                if self.chunks.contains_key(&neighbor) {
                    self.dirty_meshes.insert(neighbor);
                }
            }
        }
    }

    fn is_standing_space(&self, feet: BlockPos) -> bool {
        let Some(floor_z) = feet.z.checked_sub(1) else {
            return false;
        };
        let Some(head_z) = feet.z.checked_add(1) else {
            return false;
        };
        self.is_solid_at(BlockPos::new(feet.x, feet.y, floor_z))
            && !self.is_solid_at(feet)
            && !self.is_solid_at(BlockPos::new(feet.x, feet.y, head_z))
    }
}

fn chunk_distance(left: ChunkPos, right: ChunkPos) -> i64 {
    (i64::from(left.x) - i64::from(right.x)).abs()
        + (i64::from(left.y) - i64::from(right.y)).abs()
        + (i64::from(left.z) - i64::from(right.z)).abs()
}

fn commit_id(key: ChunkKey, payload: &[u8], expected_revision: u64) -> CommitId {
    let mut digest = Sha256::new();
    digest.update(b"latticeaxiom.chunk-commit.v1\0");
    digest.update(key.world.to_u128().to_be_bytes());
    digest.update(key.dimension.to_u32().to_be_bytes());
    digest.update(key.position.x.to_be_bytes());
    digest.update(key.position.y.to_be_bytes());
    digest.update(key.position.z.to_be_bytes());
    digest.update(expected_revision.to_be_bytes());
    digest.update(payload);
    let bytes = digest.finalize();
    let mut id = [0; 16];
    id.copy_from_slice(&bytes[..16]);
    CommitId::from_u128(u128::from_be_bytes(id))
}

fn unix_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| {
            u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
        })
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::sync::atomic::{AtomicU64, Ordering};

    use latticeaxiom_core::TerrainBlocks;
    use latticeaxiom_storage::MemoryWorldStorage;
    use latticeaxiom_storage_rocksdb::RocksDbWorldStorage;

    use super::*;

    static NEXT_TEMP_WORLD: AtomicU64 = AtomicU64::new(0);

    fn catalog() -> BlockCatalog {
        let stone = BlockId::from_raw(1);
        let dirt = BlockId::from_raw(2);
        let grass = BlockId::from_raw(3);
        BlockCatalog::new(
            [
                (stone, true, [90, 100, 110, 255]),
                (dirt, true, [120, 80, 50, 255]),
                (grass, true, [55, 150, 65, 255]),
            ],
            TerrainBlocks::new(grass, dirt, stone),
            dirt,
        )
        .expect("test catalog is complete")
    }

    #[test]
    fn durable_edit_survives_runtime_recreation() {
        let storage = Arc::new(MemoryWorldStorage::new());
        let height = Heightmap::new(7, 10, 6, 3)
            .expect("test heightmap configuration is valid")
            .height_at(0, 0);
        let surface = BlockPos::new(0, 0, height);
        {
            let mut world = SandboxWorld::new(storage.clone(), catalog(), 7, ContentHash::ZERO)
                .expect("test world configuration must be valid");
            let _ = world.center_on(BlockPos::new(0, 0, height + 2));
            world.stream_all().expect("initial chunks must materialize");
            assert!(!world.block_at(surface).is_air());
            world
                .set_block_durable(surface, BlockId::AIR)
                .expect("edit must commit atomically");
        }

        let mut reopened = SandboxWorld::new(storage, catalog(), 7, ContentHash::ZERO)
            .expect("test world configuration must be valid");
        let _ = reopened.center_on(BlockPos::new(0, 0, height + 2));
        reopened.stream_all().expect("stored chunks must reload");
        assert_eq!(reopened.block_at(surface), BlockId::AIR);
    }

    #[test]
    fn stored_chunk_rejects_configuration_mismatch() {
        let storage = Arc::new(MemoryWorldStorage::new());
        let first_hash = ContentHash::from_bytes([1; 32]);
        let second_hash = ContentHash::from_bytes([2; 32]);

        let mut original = SandboxWorld::new(storage.clone(), catalog(), 7, first_hash)
            .expect("test world configuration must be valid");
        let _ = original.center_on(BlockPos::new(0, 0, 12));
        assert_eq!(original.stream(1).expect("one chunk must materialize"), 1);

        let mut reopened = SandboxWorld::new(storage, catalog(), 7, second_hash)
            .expect("test world configuration must be valid");
        let _ = reopened.center_on(BlockPos::new(0, 0, 12));
        let error = reopened
            .stream(1)
            .expect_err("a different package graph must not reuse stored terrain");
        assert!(
            error.to_string().contains("different configuration"),
            "configuration mismatch must be actionable: {error:#}"
        );
    }

    #[test]
    fn standing_space_moves_above_a_persisted_spawn_obstruction() {
        let storage = Arc::new(MemoryWorldStorage::new());
        let height = Heightmap::new(7, 10, 6, 3)
            .expect("test heightmap configuration is valid")
            .height_at(0, 0);
        let mut world = SandboxWorld::new(storage, catalog(), 7, ContentHash::ZERO)
            .expect("test world configuration must be valid");
        let _ = world.center_on(BlockPos::new(0, 0, height + 2));
        world.stream_all().expect("initial chunks must materialize");

        let preferred = BlockPos::new(0, 0, height + 1);
        assert_eq!(
            world.find_standing_space(0, 0, preferred.z),
            Some(preferred)
        );
        world
            .set_block_durable(preferred, world.catalog().selected())
            .expect("spawn obstruction must persist");
        assert_eq!(
            world.find_standing_space(0, 0, preferred.z),
            Some(BlockPos::new(0, 0, preferred.z + 1))
        );
    }

    #[test]
    fn unloaded_chunks_are_collision_barriers() {
        let world = SandboxWorld::new(
            Arc::new(MemoryWorldStorage::new()),
            catalog(),
            7,
            ContentHash::ZERO,
        )
        .expect("test world configuration must be valid");
        assert!(world.is_collision_solid_at(BlockPos::new(0, 0, 0)));
    }

    #[test]
    fn rocksdb_round_trip_preserves_negative_coordinate_edits() {
        let sequence = NEXT_TEMP_WORLD.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "latticeaxiom-demo-world-{}-{sequence}",
            std::process::id()
        ));
        let x = -33;
        let y = -1;
        let height = Heightmap::new(7, 10, 6, 3)
            .expect("test heightmap configuration is valid")
            .height_at(x, y);
        let broken = BlockPos::new(x, y, height);
        let placed = BlockPos::new(x, y, height + 1);
        let selected = catalog().selected();

        {
            let storage = Arc::new(
                RocksDbWorldStorage::open(&path).expect("fresh production world must open"),
            );
            let mut world = SandboxWorld::new(storage, catalog(), 7, ContentHash::ZERO)
                .expect("test world configuration must be valid");
            let _ = world.center_on(BlockPos::new(x, y, height + 2));
            world
                .stream_all()
                .expect("negative-coordinate chunks must materialize");
            world
                .set_block_durable(broken, BlockId::AIR)
                .expect("production break must commit");
            world
                .set_block_durable(placed, selected)
                .expect("production placement must commit");
        }

        {
            let storage =
                Arc::new(RocksDbWorldStorage::open(&path).expect("production world must reopen"));
            let mut reopened = SandboxWorld::new(storage, catalog(), 7, ContentHash::ZERO)
                .expect("test world configuration must be valid");
            let _ = reopened.center_on(BlockPos::new(x, y, height + 2));
            reopened
                .stream_all()
                .expect("stored production chunks must reload");
            assert_eq!(reopened.block_at(broken), BlockId::AIR);
            assert_eq!(reopened.block_at(placed), selected);
        }

        fs::remove_dir_all(path).expect("closed production test world must be removable");
    }

    #[test]
    fn residency_unloads_chunks_after_crossing_window() {
        let storage = Arc::new(MemoryWorldStorage::new());
        let mut world = SandboxWorld::new(storage, catalog(), 7, ContentHash::ZERO)
            .expect("test world configuration must be valid");
        let _ = world.center_on(BlockPos::new(0, 0, 12));
        world.stream_all().expect("initial chunks must materialize");
        let first_count = world.loaded_chunks();
        let _ = world.take_dirty_meshes(usize::MAX);
        let change = world.center_on(BlockPos::new(96, 0, 12));
        assert!(!change.unloaded.is_empty());
        let remeshes = world.take_dirty_meshes(usize::MAX);
        assert!(
            remeshes.contains(&ChunkPos::new(1, 0, 0)),
            "resident neighbors must expose faces after their apron chunk unloads"
        );
        world.stream_all().expect("new chunks must materialize");
        assert_eq!(world.loaded_chunks(), first_count);
    }
}
