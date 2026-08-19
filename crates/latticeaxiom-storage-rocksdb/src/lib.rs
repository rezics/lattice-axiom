//! `RocksDB` implementation of the authoritative [`WorldStorage`] facade.
//!
//! This crate is the only direct `RocksDB` dependent in the workspace. Its
//! public surface accepts paths and facade DTOs only; database handles,
//! options, batches, snapshots, and errors remain backend details.

use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use latticeaxiom_storage::backend::{
    CommitPreparation, decode_scanned_receipt, load_chunk_with, prepare_commit_with,
    receipt_scan_prefix,
};
use latticeaxiom_storage::{
    ArtifactKey, ArtifactReceipt, CheckpointId, CheckpointTarget, ChunkCommit, ChunkKey,
    CommitReceipt, Durability, ReceiptQuery, StorageError, StorageResult, StoredChunk,
    WorldStorage,
};
use rocksdb::checkpoint::Checkpoint;
use rocksdb::{DB, Direction, IteratorMode, Options, WriteBatch, WriteOptions};

/// Production authoritative world store backed by one embedded `RocksDB`.
///
/// Opening the same database from another process fails through `RocksDB`'s
/// database lock, preserving ADR 0009's single-authoritative-writer rule.
pub struct RocksDbWorldStorage {
    db: DB,
    path: PathBuf,
    commit_gate: Mutex<()>,
}

impl fmt::Debug for RocksDbWorldStorage {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RocksDbWorldStorage")
            .field("path", &self.path)
            .finish_non_exhaustive()
    }
}

impl RocksDbWorldStorage {
    /// Opens an existing world store or creates a new one at `path`.
    ///
    /// `RocksDB`'s default column family is intentionally used for the first
    /// measured implementation. Logical key spaces remain separate and can be
    /// moved to column families later without changing the facade.
    ///
    /// # Errors
    ///
    /// Returns a backend error when `RocksDB` cannot acquire the single-writer
    /// lock, create the store, or recover its WAL.
    pub fn open(path: impl AsRef<Path>) -> StorageResult<Self> {
        let path = path.as_ref().to_path_buf();
        let mut options = Options::default();
        options.create_if_missing(true);
        let db = DB::open(&options, &path).map_err(|error| StorageError::Backend {
            operation: "open RocksDB world store",
            message: error.to_string(),
        })?;
        Ok(Self {
            db,
            path,
            commit_gate: Mutex::new(()),
        })
    }

    fn lock_commits(&self) -> StorageResult<std::sync::MutexGuard<'_, ()>> {
        self.commit_gate
            .lock()
            .map_err(|_| StorageError::LockPoisoned {
                operation: "serialize RocksDB commits",
            })
    }

    fn get(&self, key: &[u8], operation: &'static str) -> StorageResult<Option<Vec<u8>>> {
        self.db.get(key).map_err(|error| StorageError::Backend {
            operation,
            message: error.to_string(),
        })
    }
}

impl WorldStorage for RocksDbWorldStorage {
    fn load_chunk(&self, key: ChunkKey) -> StorageResult<Option<StoredChunk>> {
        let snapshot = self.db.snapshot();
        #[cfg(test)]
        let mut record_reads = 0_u8;
        load_chunk_with(key, |persistent_key| {
            #[cfg(test)]
            {
                record_reads = record_reads.saturating_add(1);
                if record_reads == 2 {
                    injected_kill("during_load");
                }
            }
            snapshot
                .get(persistent_key)
                .map_err(|error| StorageError::Backend {
                    operation: "read consistent RocksDB chunk view",
                    message: error.to_string(),
                })
        })
    }

    fn commit_chunk(&self, commit: ChunkCommit) -> StorageResult<CommitReceipt> {
        let _guard = self.lock_commits()?;
        let preparation = prepare_commit_with(&commit, |persistent_key| {
            self.get(persistent_key, "prepare RocksDB chunk commit")
        })?;
        match preparation {
            CommitPreparation::Replay(receipt) => Ok(receipt),
            CommitPreparation::Write(write) => {
                let parts = write.into_parts();
                let mut batch = WriteBatch::default();
                for key in parts.deletes {
                    batch.delete(key);
                }
                for (key, value) in parts.puts {
                    batch.put(key, value);
                }

                #[cfg(test)]
                injected_kill("before_write");

                let mut options = WriteOptions::default();
                options.disable_wal(false);
                options.set_sync(matches!(commit.durability, Durability::Sync));
                self.db
                    .write_opt(batch, &options)
                    .map_err(|error| StorageError::Backend {
                        operation: "atomically write RocksDB chunk batch",
                        message: error.to_string(),
                    })?;

                #[cfg(test)]
                injected_kill("after_write");

                Ok(parts.receipt)
            }
        }
    }

    fn scan_receipts(
        &self,
        query: &ReceiptQuery,
    ) -> StorageResult<Vec<(ChunkKey, ArtifactKey, ArtifactReceipt)>> {
        let prefix = receipt_scan_prefix(query)?;
        let snapshot = self.db.snapshot();
        let iterator = snapshot.iterator(IteratorMode::From(&prefix, Direction::Forward));
        let mut receipts = Vec::new();
        for item in iterator {
            let (key, value) = item.map_err(|error| StorageError::Backend {
                operation: "scan RocksDB artifact receipts",
                message: error.to_string(),
            })?;
            if !key.starts_with(&prefix) {
                break;
            }
            receipts.push(decode_scanned_receipt(&key, &value)?);
        }
        Ok(receipts)
    }

    fn create_checkpoint(&self, target: &CheckpointTarget) -> StorageResult<CheckpointId> {
        let _guard = self.lock_commits()?;
        #[cfg(test)]
        injected_kill("before_checkpoint");
        let checkpoint = Checkpoint::new(&self.db).map_err(|error| StorageError::Backend {
            operation: "initialize RocksDB checkpoint",
            message: error.to_string(),
        })?;
        checkpoint
            .create_checkpoint(target.directory())
            .map_err(|error| StorageError::Backend {
                operation: "create RocksDB checkpoint",
                message: error.to_string(),
            })?;
        #[cfg(test)]
        injected_kill("after_checkpoint");
        Ok(target.id())
    }
}

#[cfg(test)]
fn injected_kill(point: &str) {
    if std::env::var("LATTICEAXIOM_ROCKSDB_KILL_POINT").as_deref() == Ok(point) {
        std::process::abort();
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::fs;
    use std::process::Command;
    use std::sync::atomic::{AtomicU64, Ordering};

    use latticeaxiom_storage::conformance;
    use latticeaxiom_storage::{CheckpointId, CheckpointTarget, CommitCondition, WorldStorage};

    use super::*;

    static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);

    #[test]
    fn conforms_to_world_storage_contract() {
        let paths = RefCell::new(Vec::new());
        conformance::run_all(|| {
            let path = temporary_path("conformance");
            paths.borrow_mut().push(path.clone());
            RocksDbWorldStorage::open(path).expect("fresh RocksDB test store must open")
        });
        for path in paths.into_inner() {
            fs::remove_dir_all(path).expect("closed RocksDB test store must be removable");
        }
    }

    #[test]
    fn committed_chunk_survives_reopen() {
        let path = temporary_path("reopen");
        let commit = conformance::sample_commit(200);
        {
            let storage =
                RocksDbWorldStorage::open(&path).expect("fresh RocksDB test store must open");
            storage
                .commit_chunk(commit.clone())
                .expect("sample commit must succeed");
        }
        {
            let reopened =
                RocksDbWorldStorage::open(&path).expect("committed RocksDB store must reopen");
            let chunk = reopened
                .load_chunk(commit.key)
                .expect("reopened chunk read must succeed")
                .expect("committed chunk must survive reopen");
            assert_eq!(chunk.revision, 1);
            assert_eq!(chunk.snapshot, commit.snapshot);
            assert_eq!(chunk.spatial_entities, commit.spatial_entities);
            assert_eq!(chunk.continuation, commit.continuation);
            assert_eq!(chunk.artifact_receipts, commit.artifact_receipts);
        }
        fs::remove_dir_all(path).expect("closed RocksDB test store must be removable");
    }

    #[test]
    fn checkpoint_opens_as_independent_world_store() {
        let source_path = temporary_path("checkpoint-source");
        let checkpoint_path = temporary_path("checkpoint-copy");
        let commit = conformance::sample_commit(300);
        {
            let storage = RocksDbWorldStorage::open(&source_path)
                .expect("fresh RocksDB source store must open");
            storage
                .commit_chunk(commit.clone())
                .expect("sample commit must succeed");
            let target = CheckpointTarget::new(CheckpointId::from_u128(88), &checkpoint_path);
            assert_eq!(
                storage
                    .create_checkpoint(&target)
                    .expect("RocksDB checkpoint must succeed"),
                target.id()
            );
        }
        {
            let restored = RocksDbWorldStorage::open(&checkpoint_path)
                .expect("checkpoint must open independently");
            assert!(
                restored
                    .load_chunk(commit.key)
                    .expect("checkpoint read must succeed")
                    .is_some()
            );
        }
        fs::remove_dir_all(source_path).expect("source store must be removable");
        fs::remove_dir_all(checkpoint_path).expect("checkpoint store must be removable");
    }

    #[test]
    fn process_kill_at_batch_boundaries_never_exposes_half_a_chunk() {
        run_kill_boundary("before_write", 1);
        run_kill_boundary("after_write", 2);
    }

    #[test]
    fn process_kill_during_consistent_load_leaves_authority_unchanged() {
        let path = temporary_path("during-load");
        let initial = conformance::sample_commit(403);
        {
            let storage =
                RocksDbWorldStorage::open(&path).expect("load kill-test source store must open");
            storage
                .commit_chunk(initial.clone())
                .expect("initial load kill-test commit must succeed");
        }

        let status = run_kill_helper(&path, "during_load");
        assert!(
            !status.success(),
            "injected load child must terminate abruptly"
        );
        {
            let reopened =
                RocksDbWorldStorage::open(&path).expect("load-killed RocksDB store must recover");
            let chunk = reopened
                .load_chunk(initial.key)
                .expect("post-kill chunk read must succeed")
                .expect("authoritative chunk must remain complete");
            assert_eq!(chunk.revision, 1);
            assert_eq!(chunk.snapshot, initial.snapshot);
            assert_eq!(chunk.spatial_entities, initial.spatial_entities);
            assert_eq!(chunk.continuation, initial.continuation);
            assert_eq!(chunk.artifact_receipts, initial.artifact_receipts);
        }
        fs::remove_dir_all(path).expect("load kill-test store must be removable");
    }

    #[test]
    fn process_kill_at_checkpoint_boundaries_yields_absent_or_complete_checkpoint() {
        run_checkpoint_kill_boundary("before_checkpoint", false);
        run_checkpoint_kill_boundary("after_checkpoint", true);
    }

    #[test]
    #[ignore = "subprocess helper; invoked by process_kill_at_batch_boundaries_never_exposes_half_a_chunk"]
    fn kill_helper() {
        let Some(path) = std::env::var_os("LATTICEAXIOM_ROCKSDB_KILL_PATH") else {
            return;
        };
        let storage =
            RocksDbWorldStorage::open(path).expect("kill-test RocksDB store must open in child");
        if std::env::var("LATTICEAXIOM_ROCKSDB_KILL_POINT").as_deref() == Ok("during_load") {
            let key = conformance::sample_commit(403).key;
            let _loaded = storage
                .load_chunk(key)
                .expect("uninjected child load must succeed");
            return;
        }
        if std::env::var("LATTICEAXIOM_ROCKSDB_KILL_POINT")
            .is_ok_and(|point| point.ends_with("checkpoint"))
        {
            let target = std::env::var_os("LATTICEAXIOM_ROCKSDB_CHECKPOINT_PATH")
                .expect("checkpoint kill helper requires a target path");
            storage
                .create_checkpoint(&CheckpointTarget::new(CheckpointId::from_u128(404), target))
                .expect("uninjected child checkpoint must succeed");
            return;
        }
        let mut commit = conformance::sample_commit(402);
        commit.condition = CommitCondition::IfRevision(1);
        commit.snapshot.payload = vec![55, 89, 144];
        storage
            .commit_chunk(commit)
            .expect("uninjected child commit must succeed");
    }

    fn run_kill_boundary(point: &str, expected_revision: u64) {
        let path = temporary_path(point);
        let initial = conformance::sample_commit(401);
        {
            let storage =
                RocksDbWorldStorage::open(&path).expect("kill-test source store must open");
            storage
                .commit_chunk(initial.clone())
                .expect("initial kill-test commit must succeed");
        }

        let status = run_kill_helper(&path, point);
        assert!(!status.success(), "injected child must terminate abruptly");

        {
            let reopened =
                RocksDbWorldStorage::open(&path).expect("killed RocksDB store must recover");
            let chunk = reopened
                .load_chunk(initial.key)
                .expect("recovered chunk read must succeed")
                .expect("old or new complete chunk must exist");
            assert_eq!(chunk.revision, expected_revision);
            if expected_revision == 1 {
                assert_eq!(chunk.snapshot.payload, initial.snapshot.payload);
            } else {
                assert_eq!(chunk.snapshot.payload, vec![55, 89, 144]);
            }
        }
        fs::remove_dir_all(path).expect("recovered kill-test store must be removable");
    }

    fn run_checkpoint_kill_boundary(point: &str, checkpoint_should_exist: bool) {
        let source_path = temporary_path(&format!("{point}-source"));
        let checkpoint_path = temporary_path(&format!("{point}-copy"));
        let initial = conformance::sample_commit(404);
        {
            let storage = RocksDbWorldStorage::open(&source_path)
                .expect("checkpoint kill-test source store must open");
            storage
                .commit_chunk(initial.clone())
                .expect("initial checkpoint kill-test commit must succeed");
        }

        let executable = std::env::current_exe().expect("test executable path must be available");
        let status = Command::new(executable)
            .arg("--exact")
            .arg("tests::kill_helper")
            .arg("--ignored")
            .env("LATTICEAXIOM_ROCKSDB_KILL_PATH", &source_path)
            .env("LATTICEAXIOM_ROCKSDB_KILL_POINT", point)
            .env("LATTICEAXIOM_ROCKSDB_CHECKPOINT_PATH", &checkpoint_path)
            .status()
            .expect("checkpoint kill-test subprocess must start");
        assert!(
            !status.success(),
            "injected checkpoint child must terminate abruptly"
        );

        {
            let reopened = RocksDbWorldStorage::open(&source_path)
                .expect("checkpoint-killed source store must recover");
            assert!(
                reopened
                    .load_chunk(initial.key)
                    .expect("source read after checkpoint kill must succeed")
                    .is_some()
            );
        }
        assert_eq!(checkpoint_path.exists(), checkpoint_should_exist);
        if checkpoint_should_exist {
            let restored = RocksDbWorldStorage::open(&checkpoint_path)
                .expect("acknowledgement-boundary checkpoint must open independently");
            assert!(
                restored
                    .load_chunk(initial.key)
                    .expect("checkpoint read after child kill must succeed")
                    .is_some()
            );
            drop(restored);
            fs::remove_dir_all(&checkpoint_path)
                .expect("complete checkpoint kill-test copy must be removable");
        }
        fs::remove_dir_all(source_path).expect("checkpoint kill-test source must be removable");
    }

    fn run_kill_helper(path: &Path, point: &str) -> std::process::ExitStatus {
        let executable = std::env::current_exe().expect("test executable path must be available");
        Command::new(executable)
            .arg("--exact")
            .arg("tests::kill_helper")
            .arg("--ignored")
            .env("LATTICEAXIOM_ROCKSDB_KILL_PATH", path)
            .env("LATTICEAXIOM_ROCKSDB_KILL_POINT", point)
            .status()
            .expect("kill-test subprocess must start")
    }

    fn temporary_path(label: &str) -> PathBuf {
        let sequence = NEXT_TEMP.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!(
            "latticeaxiom-storage-rocksdb-{label}-{}-{sequence}",
            std::process::id()
        ))
    }
}
